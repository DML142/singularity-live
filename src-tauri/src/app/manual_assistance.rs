use std::{
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
};

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::{
    context::{ContextPackLoader, build_system_prompt, select_context},
    domain::{
        ModelId, ProviderErrorKind, ProviderId, RequestId, StreamEvent, TextGenerationRequest,
    },
    providers::{StreamSink, TextGenerationRouter},
};

const MAX_MANUAL_TEXT_BYTES: usize = 16 * 1024;

pub struct ManualAssistanceService {
    runtime: ConfiguredRuntime,
    active: Mutex<Option<ActiveRequest>>,
}

struct ConfiguredRuntime {
    context_pack_directory: PathBuf,
    context_pack_id: String,
    router: Arc<dyn TextGenerationRouter>,
}

struct ActiveRequest {
    request_id: RequestId,
    cancellation: CancellationToken,
}

impl ManualAssistanceService {
    #[must_use]
    pub fn configured(
        context_pack_directory: PathBuf,
        context_pack_id: String,
        router: Arc<dyn TextGenerationRouter>,
    ) -> Self {
        Self {
            runtime: ConfiguredRuntime {
                context_pack_directory,
                context_pack_id,
                router,
            },
            active: Mutex::new(None),
        }
    }

    #[must_use]
    pub fn readiness(&self) -> ManualAssistanceReadiness {
        if let Err(error) = self.runtime.router.check_readiness() {
            return ManualAssistanceReadiness::Unconfigured {
                message: error.message,
            };
        }
        if let Err(error) = ContextPackLoader::load(&self.runtime.context_pack_directory) {
            return ManualAssistanceReadiness::Unconfigured {
                message: format!(
                    "Context pack {} is unavailable: {error}",
                    self.runtime.context_pack_id
                ),
            };
        }
        ManualAssistanceReadiness::Ready {
            provider: self.runtime.router.provider(),
            model: self.runtime.router.model().clone(),
            context_pack: self.runtime.context_pack_id.clone(),
        }
    }

    /// Starts one manual assistance request and returns its stable identifier.
    ///
    /// # Errors
    ///
    /// Returns a typed error for invalid input, unavailable configuration/context, a busy
    /// coordinator, or an unavailable event consumer.
    pub async fn start(
        self: &Arc<Self>,
        text: String,
        sink: Arc<dyn StreamSink>,
    ) -> Result<RequestId, ManualAssistanceError> {
        let text = validate_input(&text)?;
        self.runtime.router.check_readiness().map_err(|error| {
            ManualAssistanceError::NotConfigured {
                message: error.message,
            }
        })?;

        let request_id = RequestId::new();
        let cancellation = CancellationToken::new();
        self.reserve(request_id, cancellation.clone())?;

        let pack_path = self.runtime.context_pack_directory.clone();
        let selection_text = text.clone();
        let context_result = tokio::task::spawn_blocking(move || {
            let pack = ContextPackLoader::load(&pack_path)?;
            Ok::<_, crate::context::ContextError>(select_context(&pack, &selection_text))
        })
        .await;
        let selected_context = match context_result {
            Ok(Ok(context)) => context,
            Ok(Err(error)) => {
                self.clear_active(request_id);
                return Err(ManualAssistanceError::ContextUnavailable {
                    message: error.to_string(),
                });
            }
            Err(_) => {
                self.clear_active(request_id);
                return Err(ManualAssistanceError::ContextUnavailable {
                    message: "Context loading could not complete".to_owned(),
                });
            }
        };
        let system_prompt = build_system_prompt(&selected_context);
        let request = TextGenerationRequest {
            request_id,
            provider: self.runtime.router.provider(),
            model: self.runtime.router.model().clone(),
            selected_context,
            system_prompt,
            user_text: text,
        };
        if sink.emit(StreamEvent::Started { request_id }).is_err() {
            self.clear_active(request_id);
            return Err(ManualAssistanceError::EventConsumerUnavailable);
        }

        let service = Arc::clone(self);
        tokio::spawn(async move {
            let result = service
                .runtime
                .router
                .stream(&request, cancellation, sink.as_ref())
                .await;
            let terminal_event = match result {
                Ok(completed) => StreamEvent::Completed(completed),
                Err(error) if error.kind == ProviderErrorKind::Cancellation => {
                    StreamEvent::Cancelled { request_id }
                }
                Err(error) => StreamEvent::Failed { request_id, error },
            };
            let _ = sink.emit(terminal_event);
            service.clear_active(request_id);
        });

        Ok(request_id)
    }

    /// Cancels the active request when its identifier matches.
    ///
    /// # Errors
    ///
    /// Returns an error when there is no active request with `request_id`.
    pub fn cancel(&self, request_id: RequestId) -> Result<(), ManualAssistanceError> {
        let active = self.active_lock();
        let Some(active) = active.as_ref() else {
            return Err(ManualAssistanceError::NoMatchingRequest);
        };
        if active.request_id != request_id {
            return Err(ManualAssistanceError::NoMatchingRequest);
        }
        active.cancellation.cancel();
        Ok(())
    }

    #[must_use]
    pub fn has_active_request(&self) -> bool {
        self.active_lock().is_some()
    }

    fn reserve(
        &self,
        request_id: RequestId,
        cancellation: CancellationToken,
    ) -> Result<(), ManualAssistanceError> {
        let mut active = self.active_lock();
        if active.is_some() {
            return Err(ManualAssistanceError::Busy);
        }
        *active = Some(ActiveRequest {
            request_id,
            cancellation,
        });
        Ok(())
    }

    fn clear_active(&self, request_id: RequestId) {
        let mut active = self.active_lock();
        if active.as_ref().map(|value| value.request_id) == Some(request_id) {
            *active = None;
        }
    }

    fn active_lock(&self) -> MutexGuard<'_, Option<ActiveRequest>> {
        match self.active.lock() {
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
