//! The main window.

use tauri::{AppHandle, Manager as _, Runtime};

/// The main window's label (tauri.conf.json).
pub const MAIN: &str = "main";

/// Bring the main window back: unminimise, show, focus.
pub fn show_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}
