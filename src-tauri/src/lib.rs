pub mod app;
mod commands;
pub mod config;
pub mod context;
pub mod domain;
pub mod secrets;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Starts the Singularity Live desktop runtime.
///
/// # Panics
///
/// Panics when Tauri cannot initialize or run, because the application cannot operate
/// without its desktop runtime.
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::application_status::get_app_status
        ])
        .run(tauri::generate_context!())
        .expect("the Tauri runtime must initialize for the application to start");
}
