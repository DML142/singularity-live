use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::{
    app::{ManualAssistanceError, SessionService},
    capture::{
        CaptureCapabilities, CaptureError, CaptureErrorKind, CaptureId, CaptureOperationId,
        CapturePreview, CaptureTarget, CaptureTargetKind, CropRect, ScreenCaptureService,
    },
    shortcuts::capture_coordinator::HotkeyCaptureCoordinator,
};

use super::manual_assistance::TauriEventSink;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CaptureTargetsPayload {
    kind: CaptureTargetKind,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartScreenCapturePayload {
    target_id: String,
    operation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelScreenCapturePayload {
    operation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CaptureIdPayload {
    capture_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CropScreenCapturePayload {
    capture_id: String,
    rect: CropRect,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartScreenshotAssistancePayload {
    capture_id: String,
    text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartScreenshotAssistanceResponse {
    request_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    code: &'static str,
    message: String,
}

#[tauri::command]
pub async fn get_screen_capture_capabilities(
    service: State<'_, Arc<ScreenCaptureService>>,
) -> Result<CaptureCapabilities, CommandError> {
    Ok(service.capabilities().await)
}

#[tauri::command]
pub async fn list_screen_capture_targets(
    request: CaptureTargetsPayload,
    service: State<'_, Arc<ScreenCaptureService>>,
) -> Result<Vec<CaptureTarget>, CommandError> {
    service
        .targets(request.kind)
        .await
        .map_err(|error| capture_command_error(&error))
}

#[tauri::command]
pub async fn start_screen_capture(
    request: StartScreenCapturePayload,
    service: State<'_, Arc<ScreenCaptureService>>,
) -> Result<CapturePreview, CommandError> {
    let operation_id = CaptureOperationId::parse(&request.operation_id).map_err(|_| {
        safe_command_error(
            "invalidOperationId",
            "The capture operation identifier is invalid",
        )
    })?;
    service
        .capture(&request.target_id, operation_id)
        .await
        .map_err(|error| capture_command_error(&error))
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn capture_screen_from_ui(
    coordinator: State<'_, Arc<HotkeyCaptureCoordinator>>,
) -> Result<(), String> {
    coordinator.capture_from_hotkey().await;
    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn cancel_ui_screen_capture(coordinator: State<'_, Arc<HotkeyCaptureCoordinator>>) {
    coordinator.cancel_active();
}

#[tauri::command]
pub async fn cancel_screen_capture(
    request: CancelScreenCapturePayload,
    service: State<'_, Arc<ScreenCaptureService>>,
) -> Result<(), CommandError> {
    let operation_id = CaptureOperationId::parse(&request.operation_id).map_err(|_| {
        safe_command_error(
            "invalidOperationId",
            "The capture operation identifier is invalid",
        )
    })?;
    service
        .cancel_capture(operation_id)
        .map_err(|error| capture_command_error(&error))
}

#[tauri::command]
pub async fn crop_screen_capture(
    request: CropScreenCapturePayload,
    service: State<'_, Arc<ScreenCaptureService>>,
) -> Result<CapturePreview, CommandError> {
    let capture_id = CaptureId::parse(&request.capture_id).map_err(|_| {
        safe_command_error(
            "imageExpired",
            "The screenshot expired or is no longer available",
        )
    })?;
    service
        .crop(&capture_id, request.rect)
        .map_err(|error| capture_command_error(&error))
}

#[tauri::command]
pub async fn discard_screen_capture(
    request: CaptureIdPayload,
    service: State<'_, Arc<ScreenCaptureService>>,
) -> Result<(), CommandError> {
    let capture_id = CaptureId::parse(&request.capture_id).map_err(|_| {
        safe_command_error(
            "imageExpired",
            "The screenshot expired or is no longer available",
        )
    })?;
    service
        .discard(&capture_id)
        .map_err(|error| capture_command_error(&error))
}

#[tauri::command]
pub async fn start_screenshot_assistance(
    request: StartScreenshotAssistancePayload,
    captures: State<'_, Arc<ScreenCaptureService>>,
    session: State<'_, Arc<SessionService>>,
    app: AppHandle,
) -> Result<StartScreenshotAssistanceResponse, CommandError> {
    let capture_id = CaptureId::parse(&request.capture_id).map_err(|_| {
        safe_command_error(
            "imageExpired",
            "The screenshot expired or is no longer available",
        )
    })?;
    let image = captures
        .store()
        .take(&capture_id)
        .map_err(|error| capture_command_error(&error))?;
    let sink = Arc::new(TauriEventSink::new(app));
    let request_id = session
        .start_screenshot_with_prompt(image, &request.text, sink)
        .map_err(|error| manual_assistance_command_error(&error))?;
    tokio::task::yield_now().await;
    Ok(StartScreenshotAssistanceResponse {
        request_id: request_id.to_string(),
    })
}

fn capture_command_error(error: &CaptureError) -> CommandError {
    let code = match error.kind {
        CaptureErrorKind::Unsupported => "unsupported",
        CaptureErrorKind::PermissionRequired => "permissionRequired",
        CaptureErrorKind::PermissionDenied => "permissionDenied",
        CaptureErrorKind::InvalidTarget => "invalidTarget",
        CaptureErrorKind::InvalidRegion => "invalidRegion",
        CaptureErrorKind::ImageExpired => "imageExpired",
        CaptureErrorKind::Preparation => "preparation",
        CaptureErrorKind::Cancelled => "cancelled",
        CaptureErrorKind::Busy => "busy",
        CaptureErrorKind::NoMatchingCapture => "noMatchingCapture",
        CaptureErrorKind::Unavailable => "unavailable",
    };
    safe_command_error(code, error.message)
}

fn manual_assistance_command_error(error: &ManualAssistanceError) -> CommandError {
    let code = match error {
        ManualAssistanceError::EmptyInput => "emptyInput",
        ManualAssistanceError::InputTooLarge => "inputTooLarge",
        ManualAssistanceError::NoPreviousRequest => "noPreviousRequest",
        ManualAssistanceError::Busy => "busy",
        ManualAssistanceError::NotConfigured { .. } => "notConfigured",
        ManualAssistanceError::ContextUnavailable { .. } => "contextUnavailable",
        ManualAssistanceError::NoMatchingRequest => "noMatchingRequest",
        ManualAssistanceError::EventConsumerUnavailable => "eventConsumerUnavailable",
    };
    safe_command_error(code, error.to_string())
}

fn safe_command_error(code: &'static str, message: impl Into<String>) -> CommandError {
    CommandError {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        CancelScreenCapturePayload, CaptureIdPayload, CaptureTargetsPayload,
        CropScreenCapturePayload, StartScreenCapturePayload, StartScreenshotAssistancePayload,
        capture_command_error, safe_command_error,
    };
    use crate::capture::{CaptureError, CaptureErrorKind};

    #[test]
    fn payloads_reject_unknown_fields() {
        assert!(
            serde_json::from_value::<CaptureTargetsPayload>(json!({
                "kind": "monitor",
                "path": "/tmp/image.png"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<StartScreenCapturePayload>(json!({
                "targetId": "monitor-1",
                "operationId": "00000000-0000-0000-0000-000000000001",
                "persist": true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<CancelScreenCapturePayload>(json!({
                "operationId": "00000000-0000-0000-0000-000000000001",
                "cancelAll": true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<CropScreenCapturePayload>(json!({
                "captureId": "00000000-0000-0000-0000-000000000001",
                "rect": {"x": 0, "y": 0, "width": 1, "height": 1, "path": "/tmp"}
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<CaptureIdPayload>(json!({
                "captureId": "00000000-0000-0000-0000-000000000001",
                "save": true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<StartScreenshotAssistancePayload>(json!({
                "captureId": "00000000-0000-0000-0000-000000000001",
                "apiKey": "private"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<CaptureTargetsPayload>(json!({ "kind": "region" })).is_err()
        );
    }

    #[test]
    fn capture_errors_map_to_safe_codes_and_messages() {
        let error = capture_command_error(&CaptureError::new(
            CaptureErrorKind::PermissionDenied,
            "Screen capture permission was denied",
        ));
        assert_eq!(error.code, "permissionDenied");
        assert_eq!(error.message, "Screen capture permission was denied");
        assert_eq!(
            safe_command_error(
                "imageExpired",
                "The screenshot expired or is no longer available"
            )
            .code,
            "imageExpired"
        );
    }
}
