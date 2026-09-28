use std::sync::Arc;

use serde::Deserialize;
use tauri::State;

use crate::voice::{
    AudioInputSource, VoiceInputService, VoiceInputSettings,
    list_audio_input_devices as available_audio_inputs,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VoiceInputSourcePayload {
    source: AudioInputSource,
    microphone_device_id: Option<String>,
}

#[tauri::command]
pub async fn list_audio_input_devices() -> Result<Vec<crate::voice::AudioInputDevice>, String> {
    tauri::async_runtime::spawn_blocking(available_audio_inputs)
        .await
        .map_err(|_| "Audio input devices could not be listed".to_owned())?
        .map_err(|error| error.safe_message().to_owned())
}

#[tauri::command]
pub async fn get_voice_input_settings(
    service: State<'_, Arc<VoiceInputService>>,
) -> Result<VoiceInputSettings, String> {
    Ok(service.settings().await)
}

#[tauri::command]
pub async fn set_voice_input_source(
    request: VoiceInputSourcePayload,
    service: State<'_, Arc<VoiceInputService>>,
) -> Result<(), String> {
    service
        .set_source(request.source, request.microphone_device_id)
        .await
        .map_err(|error| error.safe_message().to_owned())
}

#[tauri::command]
pub async fn start_voice_input(
    request: VoiceInputSourcePayload,
    service: State<'_, Arc<VoiceInputService>>,
) -> Result<(), String> {
    service
        .start(request.source, request.microphone_device_id)
        .await
        .map_err(|error| error.safe_message().to_owned())
}

#[tauri::command]
pub async fn stop_voice_input(service: State<'_, Arc<VoiceInputService>>) -> Result<(), String> {
    service.stop().await;
    Ok(())
}
