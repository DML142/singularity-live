use std::{path::Path, sync::Arc};

use app::ManualAssistanceService;
use config::{AppConfig, EnvironmentConfigSource};
use providers::ProviderRouter;
use secrets::EnvironmentSecretStore;
use tauri::Manager;

pub mod app;
mod commands;
pub mod config;
pub mod context;
pub mod domain;
pub mod providers;
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
        .setup(|application| {
            let service = match application.path().app_data_dir() {
                Ok(app_data_directory) => manual_assistance_service(&app_data_directory),
                Err(_) => Arc::new(ManualAssistanceService::unconfigured(
                    "The application data directory is unavailable".to_owned(),
                )),
            };
            application.manage(service);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::application_status::get_app_status,
            commands::manual_assistance::get_manual_assistance_readiness,
            commands::manual_assistance::start_manual_assistance,
            commands::manual_assistance::cancel_manual_assistance,
        ])
        .run(tauri::generate_context!())
        .expect("the Tauri runtime must initialize for the application to start");
}

fn manual_assistance_service(app_data_directory: &Path) -> Arc<ManualAssistanceService> {
    let config = match AppConfig::from_source(&EnvironmentConfigSource) {
        Ok(config) => config,
        Err(error) => {
            return Arc::new(ManualAssistanceService::unconfigured(error.to_string()));
        }
    };
    let context_pack_directory = app_data_directory
        .join("context-packs")
        .join(config.context_pack());
    let context_pack_id = config.context_pack().to_owned();
    let router = Arc::new(ProviderRouter::new(
        config,
        Arc::new(EnvironmentSecretStore),
        reqwest::Client::new(),
    ));
    Arc::new(ManualAssistanceService::configured(
        app_data_directory.to_owned(),
        context_pack_directory,
        context_pack_id,
        router,
    ))
}
