use std::{path::Path, sync::Arc};

use app::SessionService;
use capture::{ScreenCaptureService, TransientImageStore, platform_capture_backend};
use config::{AppConfig, EnvironmentConfigSource};
use providers::ProviderRouter;
use secrets::EnvironmentSecretStore;
use shortcuts::capture_coordinator::{
    HotkeyCaptureCoordinator, TauriCaptureWindow, TauriHotkeyCaptureEventSink,
};
use shortcuts::{
    ShortcutAction, ShortcutBindingService, ShortcutConfigStore, ShortcutPlatform,
    platform_shortcut_registrar,
};
use tauri::Manager;
use voice::{VoiceInputEvent, VoiceInputEventSink, VoiceInputService};

pub mod app;
pub mod capture;
mod commands;
pub mod config;
pub mod context;
pub mod domain;
pub mod providers;
pub mod secrets;
pub mod shortcuts;
pub mod voice;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Starts the Singularity Live desktop runtime.
///
/// # Panics
///
/// Panics when Tauri cannot initialize or run, because the application cannot operate
/// without its desktop runtime.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(setup_application)
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::CloseRequested { .. })
                && let Some(coordinator) = window
                    .app_handle()
                    .try_state::<Arc<HotkeyCaptureCoordinator>>()
            {
                coordinator.cancel_active();
            }
            if matches!(event, tauri::WindowEvent::CloseRequested { .. })
                && let Some(voice_input) = window.app_handle().try_state::<Arc<VoiceInputService>>()
            {
                let service = Arc::clone(&voice_input);
                tauri::async_runtime::spawn(async move {
                    service.stop().await;
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::application_status::get_app_status,
            commands::manual_assistance::get_manual_assistance_readiness,
            commands::manual_assistance::start_manual_assistance,
            commands::manual_assistance::cancel_manual_assistance,
            commands::manual_assistance::reset_session,
            commands::screen_assistance::get_screen_capture_capabilities,
            commands::screen_assistance::list_screen_capture_targets,
            commands::screen_assistance::start_screen_capture,
            commands::screen_assistance::cancel_screen_capture,
            commands::screen_assistance::crop_screen_capture,
            commands::screen_assistance::discard_screen_capture,
            commands::screen_assistance::start_screenshot_assistance,
            commands::shortcuts::get_shortcut_bindings,
            commands::shortcuts::update_shortcut_bindings,
            commands::voice_input::set_voice_input_source,
            commands::voice_input::start_voice_input,
            commands::voice_input::stop_voice_input,
        ])
        .run(tauri::generate_context!())
        .expect("the Tauri runtime must initialize for the application to start");
}

fn setup_application(
    application: &mut tauri::App<tauri::Wry>,
) -> Result<(), Box<dyn std::error::Error>> {
    let service = match application.path().app_data_dir() {
        Ok(app_data_directory) => session_service(&app_data_directory),
        Err(_) => Arc::new(SessionService::unconfigured(
            "The application data directory is unavailable".to_owned(),
        )),
    };
    application.manage(Arc::clone(&service));
    let voice_input = Arc::new(VoiceInputService::new(
        Arc::new(EnvironmentSecretStore),
        Arc::new(TauriVoiceInputEventSink(application.handle().clone())),
    ));
    application.manage(Arc::clone(&voice_input));
    let captures = Arc::new(ScreenCaptureService::new(
        platform_capture_backend(),
        Arc::new(TransientImageStore::default()),
    ));
    application.manage(Arc::clone(&captures));
    let window = application
        .get_webview_window("main")
        .ok_or_else(|| std::io::Error::other("The main application window is unavailable"))?;
    window.set_always_on_top(true)?;
    let manual_session = Arc::clone(&service);
    let coordinator = Arc::new(HotkeyCaptureCoordinator::new(
        captures,
        Arc::new(TauriCaptureWindow::new(window)),
        Arc::new(TauriHotkeyCaptureEventSink::new(
            application.handle().clone(),
        )),
        Arc::new(move || manual_session.has_active_request()),
    ));
    application.manage(Arc::clone(&coordinator));
    let shortcut_store = application
        .path()
        .app_config_dir()
        .or_else(|_| application.path().app_data_dir())
        .map_or_else(
            |_| ShortcutConfigStore::unavailable(ShortcutPlatform::current()),
            |directory| {
                ShortcutConfigStore::new(
                    directory.join("shortcut-bindings.json"),
                    ShortcutPlatform::current(),
                )
            },
        );
    let shortcut_coordinator = Arc::clone(&coordinator);
    let shortcut_voice_input = Arc::clone(&voice_input);
    let activation = Arc::new(move |action| match action {
        ShortcutAction::Screenshot => {
            let coordinator = Arc::clone(&shortcut_coordinator);
            tauri::async_runtime::spawn(async move {
                coordinator.capture_from_hotkey().await;
            });
        }
        ShortcutAction::VoiceInput => {
            let voice_input = Arc::clone(&shortcut_voice_input);
            tauri::async_runtime::spawn(async move {
                voice_input.toggle().await;
            });
        }
    });
    let registrar = platform_shortcut_registrar(application.handle().clone(), activation);
    let bindings = Arc::new(ShortcutBindingService::new(
        Arc::new(shortcut_store),
        registrar,
    ));
    application.manage(Arc::clone(&bindings));
    tauri::async_runtime::spawn(async move {
        let _ = bindings.initialize().await;
    });
    Ok(())
}

struct TauriVoiceInputEventSink(tauri::AppHandle);

impl VoiceInputEventSink for TauriVoiceInputEventSink {
    fn emit(&self, event: VoiceInputEvent) {
        let _ = tauri::Emitter::emit(&self.0, "singularity:voice-input", &event);
    }
}

fn session_service(app_data_directory: &Path) -> Arc<SessionService> {
    let config = match AppConfig::from_source(&EnvironmentConfigSource) {
        Ok(config) => config,
        Err(error) => {
            return Arc::new(SessionService::unconfigured(error.to_string()));
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
    Arc::new(SessionService::configured(
        app_data_directory.to_owned(),
        context_pack_directory,
        context_pack_id,
        router,
    ))
}
