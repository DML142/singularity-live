fn main() {
    let manifest = tauri_build::AppManifest::new().commands(&["get_app_status"]);

    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("Tauri build metadata must be generated");
}
