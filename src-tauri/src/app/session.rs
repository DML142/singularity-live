use std::{
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
};

use thiserror::Error;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    context::{
        ContextPackLoader, MAX_RETAINED_ASSISTANT_BYTES, MAX_SESSION_SUMMARY_BYTES, SessionHistory,
        build_session_system_prompt, select_context, truncate_to_bytes,
    },
    domain::{
        CompletedResponse, ConversationMessage, ModelId, ProviderError, ProviderErrorKind,
        ProviderId, RequestId, SelectedContext, StreamEvent, TextGenerationRequest,
    },
    providers::{StreamSink, TextGenerationRouter},
};

const MAX_MANUAL_TEXT_BYTES: usize = 16 * 1024;

pub struct SessionService {
    runtime: SessionRuntime,
    state: Mutex<SessionState>,
}

#[derive(Clone)]
enum SessionRuntime {
    Configured(ConfiguredRuntime),
    Unconfigured { message: String },
}

#[derive(Clone)]
struct ConfiguredRuntime {
    context_pack_root: PathBuf,
    context_pack_directory: PathBuf,
    context_pack_id: String,
    router: Arc<dyn TextGenerationRouter>,
}

struct SessionState {
    session_id: Uuid,
    lifecycle: SessionLifecycle,
    history: SessionHistory,
    active: Option<ActiveRequest>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum SessionLifecycle {
    Idle,
    Active,
    Processing,
}

struct ActiveRequest {
    request_id: RequestId,
    session_id: Uuid,
    cancellation: CancellationToken,
}

impl SessionService {
    #[must_use]
    pub fn configured(
        context_pack_root: PathBuf,
        context_pack_directory: PathBuf,
        context_pack_id: String,
        router: Arc<dyn TextGenerationRouter>,
    ) -> Self {
        Self::new(SessionRuntime::Configured(ConfiguredRuntime {
            context_pack_root,
            context_pack_directory,
            context_pack_id,
            router,
        }))
    }

    #[must_use]
    pub fn unconfigured(message: String) -> Self {
        Self::new(SessionRuntime::Unconfigured { message })
    }

    fn new(runtime: SessionRuntime) -> Self {
        Self {
            runtime,
            state: Mutex::new(SessionState {
                session_id: Uuid::new_v4(),
                lifecycle: SessionLifecycle::Idle,
                history: SessionHistory::default(),
                active: None,
            }),
        }
    }

    #[must_use]
    pub fn readiness(&self) -> ManualAssistanceReadiness {
        let runtime = match &self.runtime {
            SessionRuntime::Configured(runtime) => runtime,
            SessionRuntime::Unconfigured { message } => {
                return ManualAssistanceReadiness::Unconfigured {
                    message: message.clone(),
                };
            }
        };
        if let Err(error) = runtime.router.check_readiness() {
            return ManualAssistanceReadiness::Unconfigured {
                message: error.message,
            };
        }
        if ContextPackLoader::load_beneath(
            &runtime.context_pack_root,
            &runtime.context_pack_directory,
        )
        .is_err()
        {
            return ManualAssistanceReadiness::Unconfigured {
                message: "The configured context pack is invalid. Check its manifest and referenced Markdown files."
                    .to_owned(),
            };
        }
        ManualAssistanceReadiness::Ready {
            provider: runtime.router.provider(),
            model: runtime.router.model().clone(),
            context_pack: runtime.context_pack_id.clone(),
        }
    }

    /// Starts one request in the active session and returns its stable request identifier.
    ///
    /// # Errors
    ///
    /// Returns a typed error for invalid input, unavailable provider configuration, a busy
    /// session, or an unavailable event consumer.
    pub fn start(
        self: &Arc<Self>,
        text: &str,
        sink: Arc<dyn StreamSink>,
    ) -> Result<RequestId, ManualAssistanceError> {
        let text = validate_input(text)?;
        let runtime = match &self.runtime {
            SessionRuntime::Configured(runtime) => runtime,
            SessionRuntime::Unconfigured { message } => {
                return Err(ManualAssistanceError::NotConfigured {
                    message: message.clone(),
                });
            }
        };
        runtime
            .router
            .check_readiness()
            .map_err(|error| ManualAssistanceError::NotConfigured {
                message: error.message,
            })?;

        let request_id = RequestId::new();
        let cancellation = CancellationToken::new();
        let history = self.reserve(request_id, cancellation.clone())?;
        if sink.emit(StreamEvent::Started { request_id }).is_err() {
            self.finish_request(request_id, None);
            return Err(ManualAssistanceError::EventConsumerUnavailable);
        }

        let service = Arc::clone(self);
        let runtime = runtime.clone();
        tokio::spawn(async move {
            let result = service
                .process_request(
                    request_id,
                    text,
                    runtime,
                    history,
                    cancellation,
                    Arc::clone(&sink),
                )
                .await;
            let (event, completed_history) = match result {
                Ok((completed, history)) => (StreamEvent::Completed(completed), Some(history)),
                Err(error) if error.kind == ProviderErrorKind::Cancellation => {
                    (StreamEvent::Cancelled { request_id }, None)
                }
                Err(error) => (StreamEvent::Failed { request_id, error }, None),
            };
            service.finish_request(request_id, completed_history);
            let _ = sink.emit(event);
        });

        Ok(request_id)
    }

    async fn process_request(
        &self,
        request_id: RequestId,
        text: String,
        runtime: ConfiguredRuntime,
        history: SessionHistory,
        cancellation: CancellationToken,
        sink: Arc<dyn StreamSink>,
    ) -> Result<(CompletedResponse, SessionHistory), ProviderError> {
        if cancellation.is_cancelled() {
            return Err(cancellation_error());
        }
        let context_pack_root = runtime.context_pack_root.clone();
        let context_pack_directory = runtime.context_pack_directory.clone();
        let selection_text = text.clone();
        let context_result = tokio::task::spawn_blocking(move || {
            let pack =
                ContextPackLoader::load_beneath(&context_pack_root, &context_pack_directory)?;
            Ok::<_, crate::context::ContextError>(select_context(&pack, &selection_text))
        })
        .await;
        if cancellation.is_cancelled() {
            return Err(cancellation_error());
        }
        let Ok(Ok(selected_context)) = context_result else {
            return Err(context_error());
        };

        let staged_history = compact_history(
            history,
            runtime.router.as_ref(),
            request_id,
            cancellation.clone(),
        )
        .await?;
        if cancellation.is_cancelled() {
            return Err(cancellation_error());
        }
        self.replace_history(request_id, staged_history.clone())?;
        let request = TextGenerationRequest {
            request_id,
            provider: runtime.router.provider(),
            model: runtime.router.model().clone(),
            selected_context: selected_context.clone(),
            system_prompt: build_session_system_prompt(
                &selected_context,
                staged_history.rolling_summary(),
            ),
            messages: staged_history.messages_with_current(&text),
        };
        let answer_sink = Arc::new(RetainingStreamSink::new(sink, MAX_RETAINED_ASSISTANT_BYTES));
        let completed = runtime
            .router
            .stream(&request, cancellation.clone(), answer_sink.as_ref())
            .await?;
        if cancellation.is_cancelled() {
            return Err(cancellation_error());
        }
        let mut completed_history = staged_history;
        completed_history.append_completed(text, answer_sink.retained_text());
        Ok((completed, completed_history))
    }

    /// Cancels the active request when its identifier matches.
    ///
    /// # Errors
    ///
    /// Returns an error when there is no active request with `request_id`.
    pub fn cancel(&self, request_id: RequestId) -> Result<(), ManualAssistanceError> {
        let state = self.state_lock();
        let Some(active) = state.active.as_ref() else {
            return Err(ManualAssistanceError::NoMatchingRequest);
        };
        if active.request_id != request_id {
            return Err(ManualAssistanceError::NoMatchingRequest);
        }
        active.cancellation.cancel();
        Ok(())
    }

    /// Clears the active session when no request is processing.
    ///
    /// # Errors
    ///
    /// Returns `Busy` while a request owns the session.
    pub fn reset(&self) -> Result<(), ManualAssistanceError> {
        let mut state = self.state_lock();
        if state.lifecycle == SessionLifecycle::Processing || state.active.is_some() {
            return Err(ManualAssistanceError::Busy);
        }
        state.session_id = Uuid::new_v4();
        state.history = SessionHistory::default();
        state.lifecycle = SessionLifecycle::Idle;
        Ok(())
    }

    #[must_use]
    pub fn has_active_request(&self) -> bool {
        self.state_lock().active.is_some()
    }

    fn reserve(
        &self,
        request_id: RequestId,
        cancellation: CancellationToken,
    ) -> Result<SessionHistory, ManualAssistanceError> {
        let mut state = self.state_lock();
        if state.lifecycle == SessionLifecycle::Processing || state.active.is_some() {
            return Err(ManualAssistanceError::Busy);
        }
        let history = state.history.clone();
        state.lifecycle = SessionLifecycle::Processing;
        state.active = Some(ActiveRequest {
            request_id,
            session_id: state.session_id,
            cancellation,
        });
        Ok(history)
    }

    fn replace_history(
        &self,
        request_id: RequestId,
        history: SessionHistory,
    ) -> Result<(), ProviderError> {
        let mut state = self.state_lock();
        let current_session = state.session_id;
        let Some(active) = state.active.as_ref() else {
            return Err(cancellation_error());
        };
        if active.request_id != request_id || active.session_id != current_session {
            return Err(cancellation_error());
        }
        state.history = history;
        Ok(())
    }

    fn finish_request(&self, request_id: RequestId, history: Option<SessionHistory>) {
        let mut state = self.state_lock();
        let current_session = state.session_id;
        let matches = state.active.as_ref().is_some_and(|active| {
            active.request_id == request_id && active.session_id == current_session
        });
        if !matches {
            return;
        }
        if let Some(history) = history {
            state.history = history;
        }
        state.active = None;
        state.lifecycle = if state.history.has_context() {
            SessionLifecycle::Active
        } else {
            SessionLifecycle::Idle
        };
    }

    fn state_lock(&self) -> MutexGuard<'_, SessionState> {
        match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManualAssistanceReadiness {
    Ready {
        provider: ProviderId,
        model: ModelId,
        context_pack: String,
    },
    Unconfigured {
        message: String,
    },
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ManualAssistanceError {
    #[error("Enter text before sending a request")]
    EmptyInput,
    #[error("Manual input exceeds the 16 KiB limit")]
    InputTooLarge,
    #[error("A manual request is already active")]
    Busy,
    #[error("Manual assistance is not configured: {message}")]
    NotConfigured { message: String },
    #[error("Context is unavailable: {message}")]
    ContextUnavailable { message: String },
    #[error("No active request matches that identifier")]
    NoMatchingRequest,
    #[error("The assistant event consumer is unavailable")]
    EventConsumerUnavailable,
}

struct LimitedTextSink {
    text: Mutex<String>,
    max_bytes: usize,
}

impl LimitedTextSink {
    fn new(max_bytes: usize) -> Self {
        Self {
            text: Mutex::new(String::new()),
            max_bytes,
        }
    }

    fn text(&self) -> String {
        self.text
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl StreamSink for LimitedTextSink {
    fn emit(&self, event: StreamEvent) -> Result<(), ProviderError> {
        if let StreamEvent::TextDelta { delta, .. } = event {
            let mut text = self.text_lock();
            retain_limited(&mut text, &delta, self.max_bytes);
        }
        Ok(())
    }
}

struct RetainingStreamSink {
    downstream: Arc<dyn StreamSink>,
    retained: Mutex<String>,
    max_bytes: usize,
}

impl RetainingStreamSink {
    fn new(downstream: Arc<dyn StreamSink>, max_bytes: usize) -> Self {
        Self {
            downstream,
            retained: Mutex::new(String::new()),
            max_bytes,
        }
    }

    fn retained_text(&self) -> String {
        self.retained
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl StreamSink for RetainingStreamSink {
    fn emit(&self, event: StreamEvent) -> Result<(), ProviderError> {
        if let StreamEvent::TextDelta { delta, .. } = &event {
            let mut retained = self.retained_lock();
            retain_limited(&mut retained, delta, self.max_bytes);
        }
        self.downstream.emit(event)
    }
}

impl LimitedTextSink {
    fn text_lock(&self) -> MutexGuard<'_, String> {
        self.text
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl RetainingStreamSink {
    fn retained_lock(&self) -> MutexGuard<'_, String> {
        self.retained
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn retain_limited(retained: &mut String, text: &str, max_bytes: usize) {
    let remaining = max_bytes.saturating_sub(retained.len());
    retained.push_str(truncate_to_bytes(text, remaining));
}

async fn compact_history(
    mut history: SessionHistory,
    router: &dyn TextGenerationRouter,
    request_id: RequestId,
    cancellation: CancellationToken,
) -> Result<SessionHistory, ProviderError> {
    while let Some(batch) = history.next_summary_batch() {
        if cancellation.is_cancelled() {
            return Err(cancellation_error());
        }
        let request = TextGenerationRequest {
            request_id,
            provider: router.provider(),
            model: router.model().clone(),
            selected_context: SelectedContext::default(),
            system_prompt: batch.system_prompt().to_owned(),
            messages: vec![ConversationMessage::user(batch.request_text())],
        };
        let sink = LimitedTextSink::new(MAX_SESSION_SUMMARY_BYTES);
        router.stream(&request, cancellation.clone(), &sink).await?;
        if cancellation.is_cancelled() {
            return Err(cancellation_error());
        }
        let summary = sink.text();
        if summary.trim().is_empty() {
            return Err(ProviderError {
                kind: ProviderErrorKind::MalformedResponse,
                message: "The session summary could not be created".to_owned(),
            });
        }
        history.apply_summary_batch(&batch, &summary);
    }
    Ok(history)
}

fn cancellation_error() -> ProviderError {
    ProviderError {
        kind: ProviderErrorKind::Cancellation,
        message: "The request was cancelled".to_owned(),
    }
}

fn context_error() -> ProviderError {
    ProviderError {
        kind: ProviderErrorKind::Configuration,
        message: "The configured context pack could not be loaded".to_owned(),
    }
}

fn validate_input(text: &str) -> Result<String, ManualAssistanceError> {
    let text = text.trim().to_owned();
    if text.is_empty() {
        return Err(ManualAssistanceError::EmptyInput);
    }
    if text.len() > MAX_MANUAL_TEXT_BYTES {
        return Err(ManualAssistanceError::InputTooLarge);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Mutex};

    use async_trait::async_trait;
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    use crate::{
        context::{SessionHistory, SessionTurn},
        domain::{
            CompletedResponse, ModelId, ProviderError, ProviderErrorKind, ProviderId, StreamEvent,
            TextGenerationRequest,
        },
        providers::{StreamSink, TextGenerationRouter},
    };

    use super::{SessionLifecycle, SessionService};

    enum RouterStep {
        Complete(String),
        Fail,
    }

    struct ScriptedRouter {
        steps: Mutex<VecDeque<RouterStep>>,
        requests: Mutex<Vec<TextGenerationRequest>>,
    }

    impl ScriptedRouter {
        fn new(steps: Vec<RouterStep>) -> Self {
            Self {
                steps: Mutex::new(steps.into()),
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl TextGenerationRouter for ScriptedRouter {
        fn provider(&self) -> ProviderId {
            ProviderId::OpenRouter
        }

        fn model(&self) -> &ModelId {
            static MODEL: std::sync::OnceLock<ModelId> = std::sync::OnceLock::new();
            MODEL.get_or_init(|| ModelId::new("openrouter/free").expect("test model"))
        }

        fn check_readiness(&self) -> Result<(), ProviderError> {
            Ok(())
        }

        async fn stream(
            &self,
            request: &TextGenerationRequest,
            _cancellation: CancellationToken,
            sink: &dyn StreamSink,
        ) -> Result<CompletedResponse, ProviderError> {
            self.requests
                .lock()
                .expect("captured requests")
                .push(request.clone());
            match self
                .steps
                .lock()
                .expect("router script")
                .pop_front()
                .unwrap_or(RouterStep::Fail)
            {
                RouterStep::Complete(text) => {
                    sink.emit(StreamEvent::TextDelta {
                        request_id: request.request_id,
                        delta: text,
                    })?;
                    Ok(CompletedResponse {
                        request_id: request.request_id,
                        provider: request.provider,
                        model: request.model.clone(),
                        usage: None,
                    })
                }
                RouterStep::Fail => Err(ProviderError {
                    kind: ProviderErrorKind::RateLimit,
                    message: "Safe provider failure".to_owned(),
                }),
            }
        }
    }

    struct EventChannel(mpsc::UnboundedSender<StreamEvent>);

    impl StreamSink for EventChannel {
        fn emit(&self, event: StreamEvent) -> Result<(), ProviderError> {
            self.0.send(event).map_err(|_| ProviderError {
                kind: ProviderErrorKind::Cancellation,
                message: "The event consumer is unavailable".to_owned(),
            })
        }
    }

    #[tokio::test]
    async fn later_summary_batch_failure_keeps_stored_history_unchanged() {
        let context_pack = tempfile::tempdir().expect("temporary context pack");
        std::fs::write(
            context_pack.path().join("manifest.yaml"),
            "schema_version: 1\nid: fixture\nname: Fixture\ndocuments: []\n",
        )
        .expect("write context manifest");
        let router = std::sync::Arc::new(ScriptedRouter::new(vec![
            RouterStep::Complete("A compacted summary".to_owned()),
            RouterStep::Fail,
        ]));
        let service = std::sync::Arc::new(SessionService::configured(
            context_pack.path().to_owned(),
            context_pack.path().to_owned(),
            "fixture".to_owned(),
            router.clone(),
        ));
        let original_history = SessionHistory::from_turns(
            (0..9)
                .map(|index| {
                    SessionTurn::new(
                        format!("Question {index}: {}", "u".repeat(16 * 1024 - 12)),
                        "a".repeat(16 * 1024),
                    )
                })
                .collect(),
        );
        {
            let mut state = service.state_lock();
            state.history = original_history.clone();
            state.lifecycle = SessionLifecycle::Active;
        }
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let sink = std::sync::Arc::new(EventChannel(sender));

        service
            .start("Current question", sink)
            .expect("request starts");
        let terminal = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                match receiver.recv().await.expect("event channel remains open") {
                    event @ (StreamEvent::Completed(_)
                    | StreamEvent::Cancelled { .. }
                    | StreamEvent::Failed { .. }) => break event,
                    StreamEvent::Started { .. } | StreamEvent::TextDelta { .. } => {}
                }
            }
        })
        .await
        .expect("request reaches a terminal event");

        assert!(matches!(terminal, StreamEvent::Failed { .. }));
        let state = service.state_lock();
        assert!(state.history.rolling_summary().is_none());
        assert_eq!(state.history.recent_turn_count(), 9);
        let messages = state.history.messages_with_current("current");
        assert_eq!(messages.len(), 19);
        assert!(messages[0].content.starts_with("Question 0: "));
        assert!(messages[16].content.starts_with("Question 8: "));
        assert_eq!(router.requests.lock().expect("captured requests").len(), 2);
    }

    #[tokio::test]
    async fn successful_summary_remains_after_answer_generation_fails() {
        let context_pack = tempfile::tempdir().expect("temporary context pack");
        std::fs::write(
            context_pack.path().join("manifest.yaml"),
            "schema_version: 1\nid: fixture\nname: Fixture\ndocuments: []\n",
        )
        .expect("write context manifest");
        let router = std::sync::Arc::new(ScriptedRouter::new(vec![
            RouterStep::Complete("The user still wants the code explained.".to_owned()),
            RouterStep::Fail,
        ]));
        let service = std::sync::Arc::new(SessionService::configured(
            context_pack.path().to_owned(),
            context_pack.path().to_owned(),
            "fixture".to_owned(),
            router.clone(),
        ));
        let previous_history = SessionHistory::from_turns(vec![SessionTurn::new(
            format!("Explain this code: {}", "x".repeat(16 * 1024 - 18)),
            "I will explain it once you send it.",
        )]);
        {
            let mut state = service.state_lock();
            state.history = previous_history;
            state.lifecycle = SessionLifecycle::Active;
        }
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let sink = std::sync::Arc::new(EventChannel(sender));

        service
            .start("Here is the code", sink)
            .expect("request starts");
        let terminal = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                match receiver.recv().await.expect("event channel remains open") {
                    event @ (StreamEvent::Completed(_)
                    | StreamEvent::Cancelled { .. }
                    | StreamEvent::Failed { .. }) => break event,
                    StreamEvent::Started { .. } | StreamEvent::TextDelta { .. } => {}
                }
            }
        })
        .await
        .expect("request reaches a terminal event");

        assert!(matches!(terminal, StreamEvent::Failed { .. }));
        let state = service.state_lock();
        assert_eq!(
            state.history.rolling_summary(),
            Some("The user still wants the code explained.")
        );
        assert_eq!(state.history.recent_turn_count(), 0);
        assert_eq!(router.requests.lock().expect("captured requests").len(), 2);
    }
}
