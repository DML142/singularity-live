use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;

use crate::{
    domain::ProviderId,
    provider_settings::{ProviderSettingsError, ProviderSettingsStore, ProviderSettingsView},
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveProviderProfileRequest {
    provider: ProviderId,
    model: String,
}

#[derive(Debug, Serialize)]
pub struct ProviderSettingsErrorDto {
    code: &'static str,
    message: &'static str,
}

impl From<ProviderSettingsError> for ProviderSettingsErrorDto {
    fn from(error: ProviderSettingsError) -> Self {
        let (code, message) = match error {
            ProviderSettingsError::Read => (
                "settingsReadFailed",
                "Provider settings could not be read. Check the local settings file.",
            ),
            ProviderSettingsError::Write => (
                "settingsWriteFailed",
                "Provider settings could not be saved.",
            ),
            ProviderSettingsError::Invalid => (
                "settingsInvalid",
                "Provider settings are invalid. Check the local settings file.",
            ),
            ProviderSettingsError::Open => (
                "settingsOpenFailed",
                "The provider settings file could not be opened in the file manager.",
            ),
        };
        Self { code, message }
    }
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_provider_settings(
    settings: State<'_, Arc<ProviderSettingsStore>>,
) -> Result<ProviderSettingsView, ProviderSettingsErrorDto> {
    let settings = Arc::clone(settings.inner());
    settings.view().map_err(ProviderSettingsErrorDto::from)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn save_provider_profile(
    request: SaveProviderProfileRequest,
    settings: State<'_, Arc<ProviderSettingsStore>>,
) -> Result<ProviderSettingsView, ProviderSettingsErrorDto> {
    let SaveProviderProfileRequest { provider, model } = request;
    let settings = Arc::clone(settings.inner());
    settings
        .save_profile(provider, &model)
        .map_err(ProviderSettingsErrorDto::from)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn open_provider_settings_file(
    settings: State<'_, Arc<ProviderSettingsStore>>,
) -> Result<(), ProviderSettingsErrorDto> {
    let settings = Arc::clone(settings.inner());
    settings
        .open_in_file_manager()
        .map_err(ProviderSettingsErrorDto::from)
}
