fn main() {
    let manifest = tauri_build::AppManifest::new().commands(&[
        "get_app_status",
        "get_manual_assistance_readiness",
        "start_manual_assistance",
        "cancel_manual_assistance",
        "reset_session",
        "get_screen_capture_capabilities",
        "list_screen_capture_targets",
        "start_screen_capture",
        "cancel_screen_capture",
        "crop_screen_capture",
        "discard_screen_capture",
        "start_screenshot_assistance",
        "get_shortcut_bindings",
        "update_shortcut_bindings",
        "list_audio_input_devices",
        "set_voice_input_source",
        "start_voice_input",
        "stop_voice_input",
        "get_window_opacity",
        "set_window_opacity",
    ]);

    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("Tauri build metadata must be generated");
}
