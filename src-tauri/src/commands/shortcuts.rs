use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::shortcuts::{
    BindingRegistrationFailure, ShortcutBinding, ShortcutBindingId, ShortcutBindingService,
    ShortcutBindingView, ShortcutServiceError,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateShortcutBindingsPayload {
    bindings: Vec<ShortcutBinding>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutCommandError {
    code: &'static str,
    message: String,
    failed_binding_ids: Vec<ShortcutBindingId>,
    failures: Vec<BindingRegistrationFailure>,
}

#[tauri::command]
pub async fn get_shortcut_bindings(
    service: State<'_, Arc<ShortcutBindingService>>,
) -> Result<Vec<ShortcutBindingView>, ShortcutCommandError> {
    service.initialize().await.map_err(shortcut_command_error)
}

#[tauri::command]
pub async fn update_shortcut_bindings(
    request: UpdateShortcutBindingsPayload,
    service: State<'_, Arc<ShortcutBindingService>>,
) -> Result<Vec<ShortcutBindingView>, ShortcutCommandError> {
    service
        .update_bindings(request.bindings)
        .await
        .map_err(shortcut_command_error)
}

fn shortcut_command_error(error: ShortcutServiceError) -> ShortcutCommandError {
    match error {
        ShortcutServiceError::InvalidBindings(_) => ShortcutCommandError {
            code: "invalidBindings",
            message: "The shortcut bindings are invalid".to_owned(),
            failed_binding_ids: Vec::new(),
            failures: Vec::new(),
        },
        ShortcutServiceError::Configuration => ShortcutCommandError {
            code: "configurationUnavailable",
            message: "Shortcut configuration could not be loaded or saved".to_owned(),
            failed_binding_ids: Vec::new(),
            failures: Vec::new(),
        },
        ShortcutServiceError::RegistrationRejected { failures } => {
            let details = failures
                .iter()
                .map(|failure| failure.message.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            let message = if details.is_empty() {
                "One or more shortcuts could not be registered".to_owned()
            } else {
                format!("Shortcut registration failed: {details}")
            };
            ShortcutCommandError {
                code: "registrationRejected",
                message,
                failed_binding_ids: failures.iter().map(|failure| failure.binding_id).collect(),
                failures,
            }
        }
        ShortcutServiceError::Rollback => ShortcutCommandError {
            code: "rollbackFailed",
            message: "Shortcut bindings could not be restored".to_owned(),
            failed_binding_ids: Vec::new(),
            failures: Vec::new(),
        },
        ShortcutServiceError::InvalidRegistrarState => ShortcutCommandError {
            code: "registrarUnavailable",
            message: "Shortcut registration returned an invalid state".to_owned(),
            failed_binding_ids: Vec::new(),
            failures: Vec::new(),
        },
        ShortcutServiceError::RegistrarUnavailable => ShortcutCommandError {
            code: "registrarUnavailable",
            message: "Shortcut registration is unavailable on this desktop".to_owned(),
            failed_binding_ids: Vec::new(),
            failures: Vec::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::UpdateShortcutBindingsPayload;

    #[test]
    fn update_payload_rejects_unknown_fields() {
        assert!(
            serde_json::from_value::<UpdateShortcutBindingsPayload>(json!({
                "bindings": [],
                "apiKey": "must never be accepted"
            }))
            .is_err()
        );
    }
}
