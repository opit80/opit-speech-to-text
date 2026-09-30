//! Tray icon: left click toggles dictation, the right-click menu comes from `tray_menu`.

use std::sync::Arc;

use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, MenuItemKind, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};
use tracing::warn;

use crate::app_core::AppCore;
use crate::controller::{DictationState, Msg};
use crate::tray_menu::{self, TrayCommand, TrayItem};
use crate::{events, window};

pub const TRAY_ID: &str = "main";

fn core(app: &AppHandle) -> Option<Arc<AppCore>> {
    app.try_state::<Arc<AppCore>>().map(|state| state.inner().clone())
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu(app)?)
        .show_menu_on_left_click(false)
        .tooltip(tooltip(app))
        .on_menu_event(on_menu_event)
        .on_tray_icon_event(on_icon_event);
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// Rebuilds the menu and tooltip from the current state. Safe to call from any thread.
pub fn refresh(app: &AppHandle) {
    let handle = app.clone();
    let result = app.run_on_main_thread(move || {
        let Some(tray) = handle.tray_by_id(TRAY_ID) else {
            return;
        };
        match menu(&handle) {
            Ok(menu) => {
                let _ = tray.set_menu(Some(menu));
            }
            Err(err) => warn!(error = %err, "could not rebuild the tray menu"),
        }
        let _ = tray.set_tooltip(Some(tooltip(&handle)));
    });
    if let Err(err) = result {
        warn!(error = %err, "could not schedule a tray refresh");
    }
}

fn tooltip(app: &AppHandle) -> String {
    let Some(core) = core(app) else {
        return "Opit Speech to Text".into();
    };
    let recording = core.status().state == DictationState::Recording;
    core.settings.current().lang.tray_tooltip(recording).to_string()
}

fn menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let items = match core(app) {
        Some(core) => {
            let settings = core.settings.current();
            tray_menu::build(&settings.config, settings.lang, core.hotkey_state().paused, core.status().can_retry)
        }
        None => Vec::new(),
    };
    let kinds = items.iter().map(|item| realize(app, item)).collect::<tauri::Result<Vec<_>>>()?;
    let refs: Vec<&dyn IsMenuItem<Wry>> = kinds.iter().map(|kind| kind as &dyn IsMenuItem<Wry>).collect();
    Menu::with_items(app, &refs)
}

fn realize(app: &AppHandle, item: &TrayItem) -> tauri::Result<MenuItemKind<Wry>> {
    Ok(match item {
        TrayItem::Action { id, label, enabled } => {
            MenuItemKind::MenuItem(MenuItem::with_id(app, id, label, *enabled, None::<&str>)?)
        }
        TrayItem::Check { id, label, checked } => {
            MenuItemKind::Check(CheckMenuItem::with_id(app, id, label, true, *checked, None::<&str>)?)
        }
        TrayItem::Submenu { label, items } => {
            let kinds = items.iter().map(|item| realize(app, item)).collect::<tauri::Result<Vec<_>>>()?;
            let refs: Vec<&dyn IsMenuItem<Wry>> = kinds.iter().map(|kind| kind as &dyn IsMenuItem<Wry>).collect();
            MenuItemKind::Submenu(Submenu::with_items(app, label, true, &refs)?)
        }
        TrayItem::Separator => MenuItemKind::Predefined(PredefinedMenuItem::separator(app)?),
    })
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    let Some(core) = core(app) else {
        return;
    };
    match tray_menu::parse(event.id().as_ref()) {
        Some(TrayCommand::Open) => window::show_main(app, None),
        Some(TrayCommand::Profile(id)) => {
            if let Err(err) = core.set_active_profile(&id) {
                warn!(error = %err, "could not switch profile");
            }
            // Also re-syncs the check marks: Windows flips a clicked check item by itself.
            events::config_changed(app, &core.config());
        }
        Some(TrayCommand::TogglePause) => {
            core.set_hotkey_paused(!core.hotkey_state().paused);
            refresh(app);
        }
        Some(TrayCommand::Retry) => core.controller.send(Msg::Retry),
        Some(TrayCommand::Quit) => {
            core.controller.send(Msg::Shutdown);
            app.exit(0);
        }
        None => {}
    }
}

fn on_icon_event(tray: &TrayIcon, event: TrayIconEvent) {
    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event
        && let Some(core) = core(tray.app_handle())
    {
        core.controller.send(Msg::Toggle);
    }
}
