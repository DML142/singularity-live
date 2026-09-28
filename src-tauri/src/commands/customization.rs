use std::sync::Arc;

use serde::Deserialize;
use tauri::State;

use crate::customization::{CustomizationService, ScreenshotPreferences};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowOpacityPayload {
    percentage: u8,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppScalePayload {
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

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_app_scale(service: State<'_, Arc<CustomizationService>>) -> Result<u8, String> {
    service.app_scale()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn set_app_scale(
    window: tauri::WebviewWindow,
    request: AppScalePayload,
    service: State<'_, Arc<CustomizationService>>,
) -> Result<u8, String> {
    let previous = service.app_scale()?;
    let percentage = service.set_app_scale(request.percentage)?;
    if window.set_zoom(f64::from(percentage) / 100.0).is_err() {
        let _ = service.set_app_scale(previous);
        return Err("Application scale could not be applied".to_owned());
    }
    Ok(percentage)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_screenshot_preferences(
    service: State<'_, Arc<CustomizationService>>,
) -> Result<ScreenshotPreferences, String> {
    service.screenshot_preferences()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn set_screenshot_preferences(
    request: ScreenshotPreferences,
    service: State<'_, Arc<CustomizationService>>,
) -> Result<ScreenshotPreferences, String> {
    service.set_screenshot_preferences(request)
}
