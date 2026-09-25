use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::{
    app::{ManualAssistanceError, ManualAssistanceReadiness, SessionService},
    domain::{ProviderError, ProviderErrorKind, ProviderId, RequestId, StreamEvent, Usage},
    providers::StreamSink,
};

const EVENT_NAME: &str = "manual-assistance-event";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManualRequestPayload {
    text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelManualRequestPayload {
    request_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartManualAssistanceResponse {
    request_id: String,
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ReadinessDto {
    Ready {
        provider: ProviderId,
        model: String,
        context_pack: String,
    },
    Unconfigured {
        message: String,
    },
}

impl From<ManualAssistanceReadiness> for ReadinessDto {
    fn from(value: ManualAssistanceReadiness) -> Self {
        match value {
            ManualAssistanceReadiness::Ready {
                provider,
                model,
                context_pack,
            } => Self::Ready {
                provider,
                model: model.as_str().to_owned(),
                context_pack,
            },
            ManualAssistanceReadiness::Unconfigured { message } => Self::Unconfigured { message },
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ManualAssistanceEventDto {
    Started {
        request_id: String,
    },
    TextDelta {
        request_id: String,
        delta: String,
    },
    Completed {
        request_id: String,
        provider: ProviderId,
        model: String,
        usage: Option<Usage>,
    },
    Cancelled {
        request_id: String,
    },
    Failed {
        request_id: String,
        code: &'static str,
        message: String,
    },
}

impl From<StreamEvent> for ManualAssistanceEventDto {
    fn from(value: StreamEvent) -> Self {
        match value {
            StreamEvent::Started { request_id } => Self::Started {
                request_id: request_id.to_string(),
            },
            StreamEvent::TextDelta { request_id, delta } => Self::TextDelta {
                request_id: request_id.to_string(),
                delta,
            },
            StreamEvent::Completed(completed) => Self::Completed {
                request_id: completed.request_id.to_string(),
                provider: completed.provider,
                model: completed.model.as_str().to_owned(),
                usage: completed.usage,
            },
            StreamEvent::Cancelled { request_id } => Self::Cancelled {
                request_id: request_id.to_string(),
            },
            StreamEvent::Failed { request_id, error } => Self::Failed {
                request_id: request_id.to_string(),
                code: provider_error_code(error.kind),
                message: error.message,
            },
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CommandError {
    code: &'static str,
    message: String,
}

impl From<ManualAssistanceError> for CommandError {
    fn from(value: ManualAssistanceError) -> Self {
        let code = match value {
            ManualAssistanceError::EmptyInput => "emptyInput",
            ManualAssistanceError::InputTooLarge => "inputTooLarge",
            ManualAssistanceError::Busy => "busy",
            ManualAssistanceError::NotConfigured { .. } => "notConfigured",
            ManualAssistanceError::ContextUnavailable { .. } => "contextUnavailable",
            ManualAssistanceError::NoMatchingRequest => "noMatchingRequest",
            ManualAssistanceError::EventConsumerUnavailable => "eventConsumerUnavailable",
        };
        Self {
            code,
            message: value.to_string(),
        }
    }
}

struct TauriEventSink {
    app: AppHandle,
}

impl StreamSink for TauriEventSink {
    fn emit(&self, event: StreamEvent) -> Result<(), ProviderError> {
        self.app
            .emit_to("main", EVENT_NAME, ManualAssistanceEventDto::from(event))
            .map_err(|_| ProviderError {
                kind: ProviderErrorKind::Cancellation,
                message: "The assistant event consumer is unavailable".to_owned(),
            })
    }
}

#[tauri::command]
pub async fn get_manual_assistance_readiness(
    service: State<'_, Arc<SessionService>>,
) -> Result<ReadinessDto, CommandError> {
    let service = Arc::clone(service.inner());
    tokio::task::spawn_blocking(move || ReadinessDto::from(service.readiness()))
        .await
        .map_err(|_| CommandError {
            code: "readinessUnavailable",
            message: "Manual assistance readiness could not be determined".to_owned(),
        })
}

#[tauri::command]
pub async fn start_manual_assistance(
    request: ManualRequestPayload,
    service: State<'_, Arc<SessionService>>,
    app: AppHandle,
) -> Result<StartManualAssistanceResponse, CommandError> {
    let sink = Arc::new(TauriEventSink { app });
    service
        .start(request.text, sink)
        .await
        .map(|request_id| StartManualAssistanceResponse {
            request_id: request_id.to_string(),
        })
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn cancel_manual_assistance(
    request: CancelManualRequestPayload,
    service: State<'_, Arc<SessionService>>,
) -> Result<(), CommandError> {
    let CancelManualRequestPayload { request_id } = request;
    let request_id = RequestId::parse(&request_id).map_err(|_| CommandError {
        code: "invalidRequestId",
        message: "The request identifier is invalid".to_owned(),
    })?;
    service.cancel(request_id).map_err(CommandError::from)
}

#[tauri::command]
pub async fn reset_session(service: State<'_, Arc<SessionService>>) -> Result<(), CommandError> {
    service.reset().map_err(CommandError::from)
}

const fn provider_error_code(kind: ProviderErrorKind) -> &'static str {
    match kind {
        ProviderErrorKind::Authentication => "authentication",
        ProviderErrorKind::Configuration => "configuration",
        ProviderErrorKind::InvalidRequest => "invalidRequest",
        ProviderErrorKind::RateLimit => "rateLimit",
        ProviderErrorKind::Timeout => "timeout",
        ProviderErrorKind::Transport => "transport",
        ProviderErrorKind::Provider => "provider",
        ProviderErrorKind::Cancellation => "cancellation",
        ProviderErrorKind::MalformedResponse => "malformedResponse",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::{
        app::{ManualAssistanceError, ManualAssistanceReadiness},
        domain::{
            CompletedResponse, ModelId, ProviderError, ProviderErrorKind, ProviderId, RequestId,
            StreamEvent, Usage,
        },
    };

    use super::{CommandError, ManualAssistanceEventDto, ManualRequestPayload, ReadinessDto};

    #[test]
    fn request_payload_rejects_unknown_fields() {
        let payload = serde_json::from_value::<ManualRequestPayload>(json!({
            "text": "Help me prepare",
            "apiKey": "must-not-cross-ipc"
        }));

        assert!(payload.is_err());
    }

    #[test]
    fn readiness_serializes_only_safe_configuration_metadata() {
        let readiness = ReadinessDto::from(ManualAssistanceReadiness::Ready {
            provider: ProviderId::OpenRouter,
            model: ModelId::new("openrouter/free").expect("model"),
            context_pack: "fictional".to_owned(),
        });

        assert_eq!(
            serde_json::to_value(readiness).expect("readiness serializes"),
            json!({
                "status": "ready",
                "provider": "open_router",
                "model": "openrouter/free",
                "contextPack": "fictional"
            })
        );
    }

    #[test]
    fn reset_busy_error_uses_the_stable_busy_code() {
        let error = CommandError::from(ManualAssistanceError::Busy);

        assert_eq!(
            serde_json::to_value(error).expect("command error serializes")["code"],
            "busy"
        );
    }

    #[test]
    fn stream_events_have_a_stable_safe_wire_shape() {
        let request_id = RequestId::new();
        let completed = ManualAssistanceEventDto::from(StreamEvent::Completed(CompletedResponse {
            request_id,
            provider: ProviderId::OpenRouter,
            model: ModelId::new("openrouter/free").expect("model"),
            usage: Some(Usage {
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
            }),
        }));
        let failed = ManualAssistanceEventDto::from(StreamEvent::Failed {
            request_id,
            error: ProviderError {
                kind: ProviderErrorKind::Authentication,
                message: "OpenRouter rejected the configured credential".to_owned(),
            },
        });

        assert_eq!(
            serde_json::to_value(completed).expect("event serializes"),
            json!({
                "type": "completed",
                "requestId": request_id.to_string(),
                "provider": "open_router",
                "model": "openrouter/free",
                "usage": {
                    "inputTokens": 10,
                    "outputTokens": 5,
                    "totalTokens": 15
                }
            })
        );
        assert_eq!(
            serde_json::to_value(failed).expect("event serializes"),
            json!({
                "type": "failed",
                "requestId": request_id.to_string(),
                "code": "authentication",
                "message": "OpenRouter rejected the configured credential"
            })
        );
    }
}
