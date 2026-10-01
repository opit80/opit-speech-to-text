//! Opit Speech to Text desktop app: Tauri shell, platform layer and the dictation controller.

pub mod app_core;
pub mod commands;
pub mod controller;
pub mod events;
pub mod history_service;
pub mod i18n;
pub mod logging;
pub mod platform;
pub mod providers;
pub mod settings;
pub mod startup;
pub mod tray;
pub mod tray_menu;
pub mod uninstall;
pub mod updates;
pub mod window;

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use opit_core::provider::retry::RETRY_DELAY;
use tauri::{Manager, RunEvent};
use tracing::{error, info, warn};

use crate::app_core::{AppCore, Platform};
use crate::controller::{Env, HistorySink, Msg};
use crate::events::TauriEvents;
use crate::history_service::{DisabledHistory, HistoryService};
use crate::platform::windows::{
    CpalMicrophone, KeyringStore, RegistryAutostart, WinHotkey, WinOverlay, WinPaster, WinSounds,
};
use crate::platform::{Autostart, DebugAutostart, NoAutostart, NoOverlay, Overlay, SecretStore, UnavailableSecrets};
use crate::providers::HttpProviders;
use crate::settings::{Settings, SettingsHandle};
use crate::startup::Paths;

/// Windows passes this when it starts the app at sign-in; the app then starts in the tray.
pub const AUTOSTART_FLAG: &str = "--autostart";

pub fn run() {
    // The uninstaller's clean-up (windows/hooks.nsh): no window, no tray, no log file.
    if std::env::args().any(|arg| arg == uninstall::DELETE_CREDENTIALS_FLAG) {
        std::process::exit(uninstall::run());
    }
    let start_hidden = std::env::args().any(|arg| arg == AUTOSTART_FLAG);
    let Some(paths) = Paths::from_system() else {
        eprintln!("Opit Speech to Text: the AppData folder could not be found");
        std::process::exit(1);
    };
    // Keep the guard alive for the whole run: dropping it flushes the log writer.
    let _log_guard = match logging::init(&paths.logs) {
        Ok(guard) => Some(guard),
        Err(err) => {
            eprintln!("logging disabled: {err}");
            None
        }
    };
    info!(version = env!("CARGO_PKG_VERSION"), autostart = start_hidden, "starting");

    let app = tauri::Builder::default()
        // Must be the first plugin: a second launch focuses this instance and exits.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| window::show_main(app, None)))
        // In-app updates (updates.rs). Its commands are not exposed to the WebView.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(commands::handler())
        .setup(move |app| {
            setup(app, paths, start_hidden);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("the Tauri application could not be built");

    app.run(|app, event| match event {
        // `code: None` = the user closed the last window: stay in the tray.
        // `app.exit(n)` arrives as `Some(n)` and is let through.
        RunEvent::ExitRequested { code: None, api, .. } => api.prevent_exit(),
        RunEvent::Exit => {
            if let Some(core) = app.try_state::<Arc<AppCore>>() {
                core.controller.send(Msg::Shutdown);
            }
            info!("exiting");
        }
        _ => {}
    });
}

fn setup(app: &mut tauri::App, paths: Paths, start_hidden: bool) {
    // Before anything can emit or invoke: `app.state::<UpdateService>()` panics without it.
    app.manage(updates::UpdateService::default());
    let handle = app.handle().clone();
    let loaded = startup::load_config(&paths.config, now_ms());
    let (user_pack, rules_notice) = startup::load_user_pack(&paths.user_rules);
    let locale = i18n::system_locale();
    let config = loaded.config.clone();
    let (settings, warnings) = Settings::build(loaded.config, user_pack.clone(), locale.as_deref());
    for warning in &warnings {
        warn!(pack = %warning.pack_id, message = %warning.message, "rule skipped");
    }
    let settings = SettingsHandle::new(settings);
    // Release builds only: a dev build must never pull an installer over the installed app.
    if !cfg!(debug_assertions) {
        updates::spawn_auto_check(handle.clone(), settings.clone());
    }

    let history = match HistoryService::open(&paths.history_db, paths.audio.clone()) {
        Ok(history) => {
            match history.purge_expired_audio(now_ms(), config.history.audio_retention_days) {
                Ok(0) => {}
                Ok(removed) => info!(removed, "expired audio files removed"),
                Err(err) => warn!(error = %err, "audio clean-up failed"),
            }
            Some(Arc::new(history))
        }
        Err(err) => {
            error!(error = %err, "history database unavailable; dictations will not be saved");
            None
        }
    };

    let (controller, inbox) = controller::channel();
    let overlay: Arc<dyn Overlay> = match WinOverlay::new(config.ui.overlay_position, controller.overlay_sink()) {
        Ok(overlay) => Arc::new(overlay),
        Err(err) => {
            error!(error = %err, "overlay unavailable");
            Arc::new(NoOverlay)
        }
    };
    let secrets: Arc<dyn SecretStore> = match KeyringStore::new() {
        Ok(store) => Arc::new(store),
        Err(err) => {
            error!(error = %err, "credential store unavailable");
            Arc::new(UnavailableSecrets(err.to_string()))
        }
    };
    // Debug builds never touch the Run key, whichever code path asks for it.
    let autostart: Arc<dyn Autostart> = if cfg!(debug_assertions) {
        Arc::new(DebugAutostart)
    } else {
        match RegistryAutostart::for_current_exe() {
            Ok(autostart) => Arc::new(autostart),
            Err(err) => Arc::new(NoAutostart(err)),
        }
    };
    let hotkey = Arc::new(WinHotkey::new());
    let mic = Arc::new(CpalMicrophone::new());
    let sounds = Arc::new(WinSounds::new());
    sounds.warm_up();
    let history_sink: Arc<dyn HistorySink> = match &history {
        Some(history) => history.clone(),
        None => Arc::new(DisabledHistory),
    };

    let env = Env {
        settings: settings.clone(),
        providers: HttpProviders::new(secrets.clone()),
        mic: mic.clone(),
        hotkey: hotkey.clone(),
        paster: Arc::new(WinPaster::new()),
        overlay: overlay.clone(),
        sounds,
        history: history_sink,
        events: Arc::new(TauriEvents::new(handle.clone())),
        retry_delay: RETRY_DELAY,
    };
    tauri::async_runtime::spawn(controller::run(&controller, inbox, env));

    let notices = loaded.notice.into_iter().chain(rules_notice).collect();
    let platform = Platform { secrets, hotkey, mic, overlay: overlay.clone(), autostart };
    let core = Arc::new(AppCore::new(paths, settings, controller, history, platform, user_pack, locale, notices));
    core.apply_hotkey(&config);
    core.apply_autostart(app_core::autostart_wanted(&config));
    if let Some(view) = core.startup_overlay() {
        overlay.show(view);
    }
    app.manage(core);

    if let Err(err) = tray::create(&handle) {
        error!(error = %err, "tray icon unavailable");
    }
    if !(start_hidden || config.ui.start_in_tray)
        && let Err(err) = window::build(&handle, None)
    {
        error!(error = %err, "could not open the main window");
    }
    info!(first_run = loaded.created, "ready");
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}
