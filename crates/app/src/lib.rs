//! Opit Speech to Text desktop app: Tauri shell, platform layer and the dictation controller.

pub mod app_core;
pub mod controller;
pub mod history_service;
pub mod i18n;
pub mod logging;
pub mod platform;
pub mod providers;
pub mod settings;
pub mod startup;
pub mod tray_menu;

use tauri::{WebviewUrl, WebviewWindowBuilder};

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Opit Speech to Text")
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("the Tauri application could not be built");
}
