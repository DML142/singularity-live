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
    ]);

    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("Tauri build metadata must be generated");
}
