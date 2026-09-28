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
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};
use voice::{VoiceInputEvent, VoiceInputEventSink, VoiceInputService};

pub mod app;
pub mod capture;
mod commands;
pub mod config;
pub mod context;
pub mod customization;
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
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                if let Some(coordinator) = window
                    .app_handle()
                    .try_state::<Arc<HotkeyCaptureCoordinator>>()
                {
                    coordinator.cancel_active();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::application_status::get_app_status,
            commands::customization::get_window_opacity,
            commands::customization::set_window_opacity,
            commands::customization::get_screenshot_preferences,
            commands::customization::set_screenshot_preferences,
            commands::manual_assistance::get_manual_assistance_readiness,
            commands::manual_assistance::start_manual_assistance,
            commands::manual_assistance::cancel_manual_assistance,
            commands::manual_assistance::reset_session,
            commands::screen_assistance::get_screen_capture_capabilities,
            commands::screen_assistance::list_screen_capture_targets,
            commands::screen_assistance::start_screen_capture,
            commands::screen_assistance::capture_screen_from_ui,
            commands::screen_assistance::cancel_ui_screen_capture,
            commands::screen_assistance::cancel_screen_capture,
            commands::screen_assistance::crop_screen_capture,
            commands::screen_assistance::discard_screen_capture,
            commands::screen_assistance::start_screenshot_assistance,
            commands::shortcuts::get_shortcut_bindings,
            commands::shortcuts::update_shortcut_bindings,
            commands::voice_input::set_voice_input_source,
            commands::voice_input::list_audio_input_devices,
            commands::voice_input::get_voice_input_settings,
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
    let customization = application
        .path()
        .app_config_dir()
        .or_else(|_| application.path().app_data_dir())
        .map_or_else(
            |_| Arc::new(customization::CustomizationService::unavailable()),
            |directory| {
                Arc::new(customization::CustomizationService::new(
                    directory.join("customization.json"),
                ))
            },
        );
    application.manage(Arc::clone(&customization));
    let voice_input = Arc::new(VoiceInputService::new(
        Arc::new(EnvironmentSecretStore),
        Arc::new(TauriVoiceInputEventSink(application.handle().clone())),
        application
            .path()
            .app_config_dir()
            .or_else(|_| application.path().app_data_dir())
            .ok()
            .map(|directory| directory.join("voice-input.json")),
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
    build_system_tray(application)?;
    let manual_session = Arc::clone(&service);
    let close_preferences = Arc::clone(&customization);
    let target_preferences = Arc::clone(&customization);
    let coordinator = Arc::new(
        HotkeyCaptureCoordinator::new(
            captures,
            Arc::new(TauriCaptureWindow::new(window)),
            Arc::new(TauriHotkeyCaptureEventSink::new(
                application.handle().clone(),
            )),
            Arc::new(move || manual_session.has_active_request()),
        )
        .with_capture_preferences(
            Arc::new(move || {
                close_preferences
                    .screenshot_preferences()
                    .is_ok_and(|preferences| preferences.close_window_on_screenshot)
            }),
            Arc::new(move || {
                target_preferences
                    .screenshot_preferences()
                    .map_or(capture::CaptureTargetKind::Monitor, |preferences| {
                        preferences.target_kind
                    })
            }),
        ),
    );
    application.manage(Arc::clone(&coordinator));
    setup_shortcuts(application, &coordinator, &voice_input);
    Ok(())
}

fn setup_shortcuts(
    application: &tauri::App<tauri::Wry>,
    coordinator: &Arc<HotkeyCaptureCoordinator>,
    voice_input: &Arc<VoiceInputService>,
) {
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
    let shortcut_coordinator = Arc::clone(coordinator);
    let shortcut_voice_input = Arc::clone(voice_input);
    let shortcut_application = application.handle().clone();
    let taskbar_icon_hidden = Arc::new(std::sync::Mutex::new(false));
    let shortcut_taskbar_icon_hidden = Arc::clone(&taskbar_icon_hidden);
    let activation = Arc::new(move |action| match action {
        ShortcutAction::Screenshot => {
            let coordinator = Arc::clone(&shortcut_coordinator);
            tauri::async_runtime::spawn(async move {
                coordinator.capture_from_hotkey().await;
            });
        }
        ShortcutAction::ScreenshotSend => {
            let _ = shortcut_application.emit("singularity:screenshot-send", ());
        }
        ShortcutAction::ToggleTaskbarIcon => {
            toggle_taskbar_icon(&shortcut_application, &shortcut_taskbar_icon_hidden);
        }
        ShortcutAction::VoiceInput => {
            let voice_input = Arc::clone(&shortcut_voice_input);
            tauri::async_runtime::spawn(async move {
                voice_input.toggle().await;
            });
        }
        ShortcutAction::QuickSend => {
            let _ = shortcut_application.emit("singularity:quick-send", ());
        }
        ShortcutAction::MinMode => {
            let _ = shortcut_application.emit("singularity:min-mode-toggle", ());
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
}

fn build_system_tray(application: &tauri::App<tauri::Wry>) -> tauri::Result<()> {
    let show = MenuItem::with_id(
        application,
        "show",
        "Show Singularity Live",
        true,
        None::<&str>,
    )?;
    let hide = MenuItem::with_id(application, "hide", "Hide window", true, None::<&str>)?;
    let quit = MenuItem::with_id(application, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(application, &[&show, &hide, &quit])?;
    let mut tray = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("Singularity Live")
        .show_menu_on_left_click(false)
        .on_menu_event(|application, event| match event.id().as_ref() {
            "show" => show_main_window(application),
            "hide" => {
                if let Some(window) = application.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
            "quit" => application.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = application.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(application)?;
    Ok(())
}

fn toggle_taskbar_icon(application: &tauri::AppHandle, hidden: &std::sync::Mutex<bool>) {
    if let Some(window) = application.get_webview_window("main")
        && let Ok(mut is_hidden) = hidden.lock()
    {
        let next_hidden = !*is_hidden;
        if window.set_skip_taskbar(next_hidden).is_ok() {
            *is_hidden = next_hidden;
        }
    }
}

fn show_main_window(application: &tauri::AppHandle) {
    if let Some(window) = application.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
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
