use std::{
    collections::VecDeque,
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

use async_trait::async_trait;
use singularity_live::{
    app::{ManualAssistanceError, ManualAssistanceReadiness, SessionService},
    domain::{
        CompletedResponse, ConversationMessage, ModelId, ProviderError, ProviderErrorKind,
        ProviderId, RequestId, StreamEvent, TextGenerationRequest,
    },
    providers::{StreamSink, TextGenerationRouter},
};
use tempfile::TempDir;
use tokio::{sync::mpsc, time::timeout};
use tokio_util::sync::CancellationToken;

const MANIFEST: &str = r"schema_version: 1
id: fictional
name: Fictional
documents:
  - id: style
    title: Style
    path: style.md
    always_include: true
    keywords: []
  - id: rust
    title: Rust
    path: rust.md
    always_include: false
    keywords: [rust]
  - id: sql
    title: SQL
    path: sql.md
    always_include: false
    keywords: [sql]
";

struct PackFixture {
    root: TempDir,
}

impl PackFixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("temporary pack");
        fs::write(root.path().join("manifest.yaml"), MANIFEST).expect("manifest");
        fs::write(root.path().join("style.md"), "Answer briefly.").expect("style");
        fs::write(root.path().join("rust.md"), "Uses Rust daily.").expect("rust");
        fs::write(root.path().join("sql.md"), "Uses SQL daily.").expect("sql");
        Self { root }
    }

    fn path(&self) -> PathBuf {
        self.root.path().to_owned()
    }
}

#[derive(Clone)]
enum RouterBehavior {
    Success,
    Text(String),
    WaitForCancellation,
    PartialThenCancellation(String),
    Failure(ProviderErrorKind),
    PartialThenFailure(String, ProviderErrorKind),
}

struct FakeRouter {
    behavior: RouterBehavior,
    script: Mutex<VecDeque<RouterBehavior>>,
    ready: bool,
    captured: Arc<Mutex<Vec<TextGenerationRequest>>>,
}

impl FakeRouter {
    fn new(behavior: RouterBehavior) -> Self {
        Self {
            behavior,
            script: Mutex::new(VecDeque::new()),
            ready: true,
            captured: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn unconfigured() -> Self {
        Self {
            behavior: RouterBehavior::Success,
            script: Mutex::new(VecDeque::new()),
            ready: false,
            captured: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn with_script(behaviors: Vec<RouterBehavior>) -> Self {
        let mut router = Self::new(RouterBehavior::Success);
        router.script = Mutex::new(behaviors.into());
        router
    }
}

#[async_trait]
impl TextGenerationRouter for FakeRouter {
    fn provider(&self) -> ProviderId {
        ProviderId::OpenRouter
    }

    fn model(&self) -> &ModelId {
        static MODEL: std::sync::OnceLock<ModelId> = std::sync::OnceLock::new();
        MODEL.get_or_init(|| ModelId::new("openrouter/free").expect("model"))
    }

    fn check_readiness(&self) -> Result<(), ProviderError> {
        if self.ready {
            Ok(())
        } else {
            Err(ProviderError {
                kind: ProviderErrorKind::Configuration,
                message: "OpenRouter credential is not configured".to_owned(),
            })
        }
    }

    async fn stream(
        &self,
        request: &TextGenerationRequest,
        cancellation: CancellationToken,
        sink: &dyn StreamSink,
    ) -> Result<CompletedResponse, ProviderError> {
        self.captured
            .lock()
            .expect("captured request lock")
            .push(request.clone());
        let behavior = self
            .script
            .lock()
            .expect("router script lock")
            .pop_front()
            .unwrap_or_else(|| self.behavior.clone());
        match behavior {
            RouterBehavior::Success => {
                sink.emit(StreamEvent::TextDelta {
                    request_id: request.request_id,
                    delta: "A streamed answer".to_owned(),
                })?;
                Ok(CompletedResponse {
                    request_id: request.request_id,
                    provider: request.provider,
                    model: request.model.clone(),
                    usage: None,
                })
            }
            RouterBehavior::Text(text) => {
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
            RouterBehavior::WaitForCancellation => {
                cancellation.cancelled().await;
                Err(ProviderError {
                    kind: ProviderErrorKind::Cancellation,
                    message: "The request was cancelled".to_owned(),
                })
            }
            RouterBehavior::PartialThenCancellation(text) => {
                sink.emit(StreamEvent::TextDelta {
                    request_id: request.request_id,
                    delta: text,
                })?;
                cancellation.cancelled().await;
                Err(ProviderError {
                    kind: ProviderErrorKind::Cancellation,
                    message: "The request was cancelled".to_owned(),
                })
            }
            RouterBehavior::Failure(kind) => Err(ProviderError {
                kind,
                message: "Safe provider failure".to_owned(),
            }),
            RouterBehavior::PartialThenFailure(text, kind) => {
                sink.emit(StreamEvent::TextDelta {
                    request_id: request.request_id,
                    delta: text,
                })?;
                Err(ProviderError {
                    kind,
                    message: "Safe provider failure".to_owned(),
                })
            }
        }
    }
}

struct ChannelSink {
    sender: mpsc::UnboundedSender<StreamEvent>,
}

impl StreamSink for ChannelSink {
    fn emit(&self, event: StreamEvent) -> Result<(), ProviderError> {
        self.sender.send(event).map_err(|_| ProviderError {
            kind: ProviderErrorKind::Cancellation,
            message: "Event receiver is unavailable".to_owned(),
        })
    }
}

struct TerminalObserverSink {
    service: Weak<SessionService>,
    was_active_at_terminal: Arc<AtomicBool>,
}

impl StreamSink for TerminalObserverSink {
    fn emit(&self, event: StreamEvent) -> Result<(), ProviderError> {
        if matches!(
            event,
            StreamEvent::Completed(_) | StreamEvent::Cancelled { .. } | StreamEvent::Failed { .. }
        ) {
            let was_active = self
                .service
                .upgrade()
                .is_some_and(|service| service.has_active_request());
            self.was_active_at_terminal
                .store(was_active, Ordering::SeqCst);
        }
        Ok(())
    }
}

fn service(pack: &PackFixture, router: Arc<dyn TextGenerationRouter>) -> Arc<SessionService> {
    let context_pack_root = pack
        .path()
        .parent()
        .expect("pack parent directory")
        .to_owned();
    Arc::new(SessionService::configured(
        context_pack_root,
        pack.path(),
        "fictional".to_owned(),
        router,
    ))
}

fn channel_sink() -> (Arc<ChannelSink>, mpsc::UnboundedReceiver<StreamEvent>) {
    let (sender, receiver) = mpsc::unbounded_channel();
    (Arc::new(ChannelSink { sender }), receiver)
}

async fn next_event(receiver: &mut mpsc::UnboundedReceiver<StreamEvent>) -> StreamEvent {
    timeout(std::time::Duration::from_secs(1), receiver.recv())
        .await
        .expect("event timeout")
        .expect("event channel open")
}

async fn complete_request(service: &Arc<SessionService>, text: &str) {
    let (sink, mut receiver) = channel_sink();
    service.start(text, sink).expect("request starts");

    loop {
        match next_event(&mut receiver).await {
            StreamEvent::Completed(_) => return,
            StreamEvent::Failed { error, .. } => panic!("request failed: {error}"),
            StreamEvent::Cancelled { .. } => panic!("request was cancelled"),
            StreamEvent::Started { .. } | StreamEvent::TextDelta { .. } => {}
        }
    }
}

#[test]
fn readiness_reports_configuration_and_context_failures_safely() {
    let pack = PackFixture::new();
    let ready = service(&pack, Arc::new(FakeRouter::new(RouterBehavior::Success)));
    assert!(matches!(
        ready.readiness(),
        ManualAssistanceReadiness::Ready {
            provider: ProviderId::OpenRouter,
            ..
        }
    ));

    let unconfigured = service(&pack, Arc::new(FakeRouter::unconfigured()));
    assert_eq!(
        unconfigured.readiness(),
        ManualAssistanceReadiness::Unconfigured {
            message: "OpenRouter credential is not configured".to_owned(),
        }
    );

    let missing_context = Arc::new(SessionService::configured(
        pack.path()
            .parent()
            .expect("pack parent directory")
            .to_owned(),
        pack.path().join("missing"),
        "missing".to_owned(),
        Arc::new(FakeRouter::new(RouterBehavior::Success)),
    ));
    assert!(matches!(
        missing_context.readiness(),
        ManualAssistanceReadiness::Unconfigured { .. }
    ));
}

#[test]
fn readiness_does_not_echo_values_from_a_malformed_context_manifest() {
    let pack = PackFixture::new();
    fs::write(
        pack.path().join("manifest.yaml"),
        "schema_version: \"PRIVATE_SENTINEL\"\n",
    )
    .expect("write malformed manifest");
    let service = service(&pack, Arc::new(FakeRouter::new(RouterBehavior::Success)));

    let ManualAssistanceReadiness::Unconfigured { message } = service.readiness() else {
        panic!("malformed context should be unavailable");
    };

    assert!(!message.contains("PRIVATE_SENTINEL"));
}

#[test]
fn readiness_does_not_echo_manifest_validation_values() {
    let pack = PackFixture::new();
    let manifest = MANIFEST.replace(
        "keywords: [rust]",
        "keywords: [privatecredential, privatecredential]",
    );
    fs::write(pack.path().join("manifest.yaml"), manifest).expect("write invalid manifest");
    let service = service(&pack, Arc::new(FakeRouter::new(RouterBehavior::Success)));

    let ManualAssistanceReadiness::Unconfigured { message } = service.readiness() else {
        panic!("invalid context should be unavailable");
    };

    assert!(!message.contains("privatecredential"));
}

#[tokio::test]
async fn request_context_errors_do_not_echo_manifest_validation_values() {
    let pack = PackFixture::new();
    let manifest = MANIFEST.replace(
        "keywords: [rust]",
        "keywords: [privatecredential, privatecredential]",
    );
    fs::write(pack.path().join("manifest.yaml"), manifest).expect("write invalid manifest");
    let service = service(&pack, Arc::new(FakeRouter::new(RouterBehavior::Success)));
    let (sink, mut receiver) = channel_sink();

    let request_id = service
        .start("Help me", sink)
        .expect("request is reserved before context is loaded");
    assert_eq!(
        next_event(&mut receiver).await,
        StreamEvent::Started { request_id }
    );
    let StreamEvent::Failed { error, .. } = next_event(&mut receiver).await else {
        panic!("invalid context must emit a failure event");
    };

    assert!(!error.to_string().contains("privatecredential"));
}

#[cfg(unix)]
#[test]
fn readiness_rejects_a_context_packs_directory_that_escapes_app_data() {
    use std::os::unix::fs::symlink;

    let pack = PackFixture::new();
    let app_data = tempfile::tempdir().expect("application data directory");
    let linked_packs = app_data.path().join("context-packs");
    symlink(
        pack.path().parent().expect("pack parent directory"),
        &linked_packs,
    )
    .expect("create escaping context-pack parent");
    let linked_pack = linked_packs.join(pack.path().file_name().expect("pack directory name"));
    let service = SessionService::configured(
        app_data.path().to_owned(),
        linked_pack,
        "fictional".to_owned(),
        Arc::new(FakeRouter::new(RouterBehavior::Success)),
    );

    assert!(matches!(
        service.readiness(),
        ManualAssistanceReadiness::Unconfigured { .. }
    ));
}

#[tokio::test]
async fn unconfigured_service_starts_safely_and_rejects_requests() {
    let service = Arc::new(SessionService::unconfigured(
        "Required setting SINGULARITY_LIVE_PROVIDER is not configured".to_owned(),
    ));
    let (sink, _) = channel_sink();

    assert_eq!(
        service.readiness(),
        ManualAssistanceReadiness::Unconfigured {
            message: "Required setting SINGULARITY_LIVE_PROVIDER is not configured".to_owned(),
        }
    );
    assert_eq!(
        service.start("Hello", sink),
        Err(ManualAssistanceError::NotConfigured {
            message: "Required setting SINGULARITY_LIVE_PROVIDER is not configured".to_owned(),
        })
    );
}

#[tokio::test]
async fn validates_manual_text_before_starting() {
    let pack = PackFixture::new();
    let service = service(&pack, Arc::new(FakeRouter::new(RouterBehavior::Success)));
    let (sink, _) = channel_sink();

    assert_eq!(
        service.start("   ", sink.clone()),
        Err(ManualAssistanceError::EmptyInput)
    );
    assert_eq!(
        service.start(&"x".repeat(16 * 1024 + 1), sink),
        Err(ManualAssistanceError::InputTooLarge)
    );
}

#[tokio::test]
async fn loads_selected_context_and_forwards_a_successful_stream() {
    let pack = PackFixture::new();
    let router = Arc::new(FakeRouter::new(RouterBehavior::Success));
    let captured = router.captured.clone();
    let service = service(&pack, router);
    let (sink, mut receiver) = channel_sink();

    let request_id = service
        .start("Explain my Rust work.", sink)
        .expect("request starts");

    assert_eq!(
        next_event(&mut receiver).await,
        StreamEvent::Started { request_id }
    );
    assert_eq!(
        next_event(&mut receiver).await,
        StreamEvent::TextDelta {
            request_id,
            delta: "A streamed answer".to_owned(),
        }
    );
    assert!(matches!(
        next_event(&mut receiver).await,
        StreamEvent::Completed(CompletedResponse {
            request_id: completed_id,
            ..
        }) if completed_id == request_id
    ));

    let requests = captured.lock().expect("captured requests");
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].messages,
        vec![ConversationMessage::user("Explain my Rust work.")]
    );
    assert_eq!(requests[0].selected_context.documents.len(), 2);
    assert_eq!(
        requests[0]
            .selected_context
            .documents
            .iter()
            .map(|document| document.id.as_str())
            .collect::<Vec<_>>(),
        ["style", "rust"]
    );
    assert!(requests[0].system_prompt.contains("Uses Rust daily."));
}

#[tokio::test]
async fn releases_the_active_slot_before_publishing_a_terminal_event() {
    let pack = PackFixture::new();
    let service = service(&pack, Arc::new(FakeRouter::new(RouterBehavior::Success)));
    let was_active_at_terminal = Arc::new(AtomicBool::new(true));
    let sink = Arc::new(TerminalObserverSink {
        service: Arc::downgrade(&service),
        was_active_at_terminal: Arc::clone(&was_active_at_terminal),
    });

    service
        .start("Check terminal ordering", sink)
        .expect("request starts");

    timeout(std::time::Duration::from_secs(1), async {
        loop {
            if !service.has_active_request() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("active slot released");
    assert!(!was_active_at_terminal.load(Ordering::SeqCst));
}

#[tokio::test]
async fn prevents_duplicate_submissions_and_cancels_only_the_current_request() {
    let pack = PackFixture::new();
    let service = service(
        &pack,
        Arc::new(FakeRouter::new(RouterBehavior::WaitForCancellation)),
    );
    let (sink, mut receiver) = channel_sink();

    let request_id = service
        .start("First", sink.clone())
        .expect("first request starts");
    assert_eq!(
        service.start("Second", sink),
        Err(ManualAssistanceError::Busy)
    );
    assert_eq!(service.reset(), Err(ManualAssistanceError::Busy));
    assert_eq!(
        service.cancel(RequestId::new()),
        Err(ManualAssistanceError::NoMatchingRequest)
    );
    service.cancel(request_id).expect("current request cancels");

    assert_eq!(
        next_event(&mut receiver).await,
        StreamEvent::Started { request_id }
    );
    assert_eq!(
        next_event(&mut receiver).await,
        StreamEvent::Cancelled { request_id }
    );
}

#[tokio::test]
async fn provider_failures_emit_safe_terminal_events_and_release_the_active_slot() {
    let pack = PackFixture::new();
    let service = service(
        &pack,
        Arc::new(FakeRouter::new(RouterBehavior::Failure(
            ProviderErrorKind::RateLimit,
        ))),
    );
    let (sink, mut receiver) = channel_sink();

    let request_id = service
        .start("First", sink.clone())
        .expect("request starts");
    let _ = next_event(&mut receiver).await;
    assert_eq!(
        next_event(&mut receiver).await,
        StreamEvent::Failed {
            request_id,
            error: ProviderError {
                kind: ProviderErrorKind::RateLimit,
                message: "Safe provider failure".to_owned(),
            },
        }
    );

    timeout(std::time::Duration::from_secs(1), async {
        loop {
            if !service.has_active_request() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("active slot released");
    service
        .start("Recovery", sink)
        .expect("new request starts after failure");
}

#[tokio::test]
async fn follow_up_request_includes_only_prior_completed_turns() {
    let pack = PackFixture::new();
    let router = Arc::new(FakeRouter::new(RouterBehavior::Success));
    let requests = Arc::clone(&router.captured);
    let service = service(&pack, router);
    complete_request(&service, "What does this code do?").await;
    complete_request(&service, "Here is the function.").await;

    let requests = requests.lock().expect("captured requests");
    assert_eq!(
        requests[1].messages,
        vec![
            ConversationMessage::user("What does this code do?"),
            ConversationMessage::assistant("A streamed answer"),
            ConversationMessage::user("Here is the function."),
        ],
    );
}

#[tokio::test]
async fn reset_removes_prior_turns_and_summary_from_the_next_request() {
    let pack = PackFixture::new();
    let router = Arc::new(FakeRouter::with_script(vec![
        RouterBehavior::Success,
        RouterBehavior::Text("Old session summary".to_owned()),
        RouterBehavior::Success,
        RouterBehavior::Success,
    ]));
    let requests = Arc::clone(&router.captured);
    let service = service(&pack, router);
    let long_goal = "Old session question".to_owned() + &"x".repeat(16 * 1024 - 20);
    complete_request(&service, &long_goal).await;
    complete_request(&service, "Another old question").await;
    service.reset().expect("idle session resets");
    complete_request(&service, "New session question").await;

    let requests = requests.lock().expect("captured requests");
    assert_eq!(
        requests[3].messages,
        vec![ConversationMessage::user("New session question")],
    );
    assert!(requests[2].system_prompt.contains("Old session summary"));
    assert!(!requests[3].system_prompt.contains("Old session question"));
    assert!(!requests[3].system_prompt.contains("Old session summary"));
}

#[tokio::test]
async fn summary_keeps_the_older_goal_and_the_answer_keeps_the_newest_eight_turns() {
    let pack = PackFixture::new();
    let mut script = (0..9).map(|_| RouterBehavior::Success).collect::<Vec<_>>();
    script.push(RouterBehavior::Text(
        "The user asked for an explanation and is waiting to provide code.".to_owned(),
    ));
    script.push(RouterBehavior::Success);
    let router = Arc::new(FakeRouter::with_script(script));
    let requests = Arc::clone(&router.captured);
    let service = service(&pack, router);

    for index in 0..9 {
        complete_request(&service, &format!("Question {index}")).await;
    }
    complete_request(&service, "Question 9").await;

    let requests = requests.lock().expect("captured requests");
    assert_eq!(requests.len(), 11);
    assert!(requests[9].system_prompt.contains("pending requests"));
    assert!(requests[9].messages[0].content.contains("Question 0"));
    assert!(
        requests[10]
            .system_prompt
            .contains("waiting to provide code")
    );
    assert_eq!(requests[10].messages.len(), 17);
    assert_eq!(
        requests[10].messages[0],
        ConversationMessage::user("Question 1")
    );
    assert_eq!(
        requests[10].messages[16],
        ConversationMessage::user("Question 9")
    );
    let combined_bytes = requests[10].system_prompt.len()
        + requests[10]
            .messages
            .iter()
            .map(|message| message.content.len())
            .sum::<usize>();
    assert!(combined_bytes <= 64 * 1024);
}

#[tokio::test]
async fn empty_summary_fails_safely_without_discarding_prior_history() {
    let pack = PackFixture::new();
    let user_goal = "Explain this code".to_owned() + &"g".repeat(16 * 1024 - 17);
    let router = Arc::new(FakeRouter::with_script(vec![
        RouterBehavior::Success,
        RouterBehavior::Text("  \n".to_owned()),
        RouterBehavior::Text("The user still wants an explanation.".to_owned()),
        RouterBehavior::Success,
    ]));
    let requests = Arc::clone(&router.captured);
    let service = service(&pack, router);
    complete_request(&service, &user_goal).await;

    let (sink, mut receiver) = channel_sink();
    let request_id = service
        .start("Follow up", sink)
        .expect("summary request starts");
    assert_eq!(
        next_event(&mut receiver).await,
        StreamEvent::Started { request_id }
    );
    assert!(matches!(
        next_event(&mut receiver).await,
        StreamEvent::Failed { request_id: failed_id, .. } if failed_id == request_id
    ));

    complete_request(&service, "Try again").await;
    let requests = requests.lock().expect("captured requests");
    assert!(
        requests[2].messages[0]
            .content
            .contains("Explain this code")
    );
    assert!(
        requests[3]
            .system_prompt
            .contains("still wants an explanation")
    );
}

#[tokio::test]
async fn cancellation_during_summary_preserves_the_old_request_context() {
    let pack = PackFixture::new();
    let user_goal = "Explain this code".to_owned() + &"g".repeat(16 * 1024 - 17);
    let router = Arc::new(FakeRouter::with_script(vec![
        RouterBehavior::Success,
        RouterBehavior::WaitForCancellation,
        RouterBehavior::Success,
        RouterBehavior::Success,
    ]));
    let requests = Arc::clone(&router.captured);
    let service = service(&pack, router);
    complete_request(&service, &user_goal).await;

    let (sink, mut receiver) = channel_sink();
    let request_id = service
        .start("Waiting follow-up", sink)
        .expect("summary request starts");
    assert_eq!(
        next_event(&mut receiver).await,
        StreamEvent::Started { request_id }
    );
    timeout(std::time::Duration::from_secs(1), async {
        loop {
            if requests.lock().expect("captured requests").len() == 2 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("summary provider starts");
    service.cancel(request_id).expect("active summary cancels");
    assert_eq!(
        next_event(&mut receiver).await,
        StreamEvent::Cancelled { request_id }
    );

    complete_request(&service, "Recovery question").await;
    let requests = requests.lock().expect("captured requests");
    assert!(
        requests[2].messages[0]
            .content
            .contains("Explain this code")
    );
    assert!(
        !requests[2].messages[0]
            .content
            .contains("Waiting follow-up")
    );
}

#[tokio::test]
async fn cancellation_after_partial_answer_does_not_enter_future_context() {
    let pack = PackFixture::new();
    let router = Arc::new(FakeRouter::with_script(vec![
        RouterBehavior::Success,
        RouterBehavior::PartialThenCancellation("partial answer".to_owned()),
        RouterBehavior::Success,
    ]));
    let requests = Arc::clone(&router.captured);
    let service = service(&pack, router);
    complete_request(&service, "Completed question").await;

    let (sink, mut receiver) = channel_sink();
    let request_id = service
        .start("Cancelled question", sink)
        .expect("answer starts");
    assert_eq!(
        next_event(&mut receiver).await,
        StreamEvent::Started { request_id }
    );
    assert_eq!(
        next_event(&mut receiver).await,
        StreamEvent::TextDelta {
            request_id,
            delta: "partial answer".to_owned(),
        }
    );
    service.cancel(request_id).expect("answer cancels");
    assert_eq!(
        next_event(&mut receiver).await,
        StreamEvent::Cancelled { request_id }
    );

    complete_request(&service, "Recovery question").await;
    let requests = requests.lock().expect("captured requests");
    assert_eq!(
        requests[2].messages,
        vec![
            ConversationMessage::user("Completed question"),
            ConversationMessage::assistant("A streamed answer"),
            ConversationMessage::user("Recovery question"),
        ]
    );
}

#[tokio::test]
async fn provider_failure_after_partial_answer_does_not_enter_future_context() {
    let pack = PackFixture::new();
    let router = Arc::new(FakeRouter::with_script(vec![
        RouterBehavior::Success,
        RouterBehavior::PartialThenFailure(
            "partial answer".to_owned(),
            ProviderErrorKind::RateLimit,
        ),
        RouterBehavior::Success,
    ]));
    let requests = Arc::clone(&router.captured);
    let service = service(&pack, router);
    complete_request(&service, "Completed question").await;

    let (sink, mut receiver) = channel_sink();
    let request_id = service
        .start("Failed question", sink)
        .expect("answer starts");
    assert_eq!(
        next_event(&mut receiver).await,
        StreamEvent::Started { request_id }
    );
    assert_eq!(
        next_event(&mut receiver).await,
        StreamEvent::TextDelta {
            request_id,
            delta: "partial answer".to_owned(),
        }
    );
    assert!(matches!(
        next_event(&mut receiver).await,
        StreamEvent::Failed { request_id: failed_id, error }
            if failed_id == request_id && error.kind == ProviderErrorKind::RateLimit
    ));

    complete_request(&service, "Recovery question").await;
    let requests = requests.lock().expect("captured requests");
    assert_eq!(
        requests[2].messages,
        vec![
            ConversationMessage::user("Completed question"),
            ConversationMessage::assistant("A streamed answer"),
            ConversationMessage::user("Recovery question"),
        ]
    );
}
