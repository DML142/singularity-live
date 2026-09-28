use std::sync::Arc;

use serde::Deserialize;
use tauri::State;

use crate::context::{UserContextFileInfo, UserContextService};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoveUserContextFilePayload {
    id: String,
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_user_context_files(
    service: State<'_, Arc<UserContextService>>,
) -> Result<Vec<UserContextFileInfo>, String> {
    service.list()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn add_user_context_files(
    service: State<'_, Arc<UserContextService>>,
) -> Result<Vec<UserContextFileInfo>, String> {
    let paths = rfd::FileDialog::new()
        .add_filter("Markdown or text files", &["md", "txt"])
        .pick_files()
        .unwrap_or_default();
    service.add_files(&paths)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn remove_user_context_file(
    request: RemoveUserContextFilePayload,
    service: State<'_, Arc<UserContextService>>,
) -> Result<Vec<UserContextFileInfo>, String> {
    service.remove_file(&request.id)
}
