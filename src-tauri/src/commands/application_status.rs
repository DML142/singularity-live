use crate::app::{ApplicationStatus, application_status};

#[tauri::command]
#[must_use]
pub fn get_app_status() -> ApplicationStatus {
    application_status()
}
