//! Main window lifecycle. Closing the window destroys it (and its WebView); the app keeps
//! running in the tray and builds a fresh window when asked.

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tracing::warn;

use crate::events::NAVIGATE;

pub const MAIN_WINDOW: &str = "main";

/// Focuses the main window or creates it. `route` (e.g. `"settings"`) is sent to an open
/// window as a `navigate` event, or becomes the URL hash of a new one.
pub fn show_main(app: &AppHandle, route: Option<&str>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        if let Some(route) = route {
            let _ = app.emit_to(MAIN_WINDOW, NAVIGATE, route);
        }
        return;
    }
    // Building a WebView2 window inside an event handler can deadlock on Windows, so the
    // build runs on its own thread (it posts to the event loop and waits there).
    let app = app.clone();
    let route = route.map(str::to_string);
    std::thread::spawn(move || {
        if let Err(err) = build(&app, route.as_deref()) {
            warn!(error = %err, "could not open the main window");
        }
    });
}

pub fn build(app: &AppHandle, route: Option<&str>) -> tauri::Result<WebviewWindow> {
    let url = match route {
        Some(route) => format!("index.html#/{route}"),
        None => "index.html".to_string(),
    };
    WebviewWindowBuilder::new(app, MAIN_WINDOW, WebviewUrl::App(url.into()))
        .title("Opit Speech to Text")
        .inner_size(900.0, 640.0)
        .min_inner_size(640.0, 480.0)
        .center()
        .build()
}
