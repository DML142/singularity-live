use std::sync::Arc;

use serde::Deserialize;
use tauri::State;

use crate::voice::{AudioInputSource, VoiceInputService};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VoiceInputSourcePayload {
    source: AudioInputSource,
}

#[tauri::command]
pub async fn set_voice_input_source(
    request: VoiceInputSourcePayload,
    service: State<'_, Arc<VoiceInputService>>,
) -> Result<(), String> {
    service
        .set_source(request.source)
        .await
        .map_err(|error| error.safe_message().to_owned())
}

#[tauri::command]
pub async fn start_voice_input(
    request: VoiceInputSourcePayload,
    service: State<'_, Arc<VoiceInputService>>,
) -> Result<(), String> {
    service
        .start(request.source)
        .await
        .map_err(|error| error.safe_message().to_owned())
}

#[tauri::command]
pub async fn stop_voice_input(service: State<'_, Arc<VoiceInputService>>) -> Result<(), String> {
    service.stop().await;
    Ok(())
}
