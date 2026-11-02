//! Opit Speech to Text desktop app: Tauri shell, platform layer and the dictation controller.

pub mod app_core;
pub mod ask;
pub mod commands;
pub mod controller;
pub mod file_jobs;
pub mod history_service;
pub mod ipc;
pub mod local;
pub mod platform;
pub mod providers;
pub mod settings;
pub mod shell;
#[cfg(windows)]
pub use platform::windows::uninstall;

pub(crate) use shell::now_ms;
pub use shell::{AUTOSTART_FLAG, events, i18n, logging, run, startup, tray, tray_menu, updates, window};
