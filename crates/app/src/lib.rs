//! Opit Speech to Text desktop app: Tauri shell, platform layer and the dictation controller.

pub mod controller;
pub mod i18n;
pub mod platform;
pub mod settings;

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
