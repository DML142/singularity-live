use std::sync::Arc;

use serde::Deserialize;
use tauri::State;

use crate::customization::CustomizationService;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowOpacityPayload {
    percentage: u8,
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_window_opacity(service: State<'_, Arc<CustomizationService>>) -> Result<u8, String> {
    service.window_opacity()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn set_window_opacity(
    request: WindowOpacityPayload,
    service: State<'_, Arc<CustomizationService>>,
) -> Result<u8, String> {
    service.set_window_opacity(request.percentage)
}
