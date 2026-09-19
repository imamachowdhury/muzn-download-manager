//! The main window.

use tauri::{AppHandle, Manager as _, Runtime, Window, WindowEvent};

use crate::tray::TrayState;

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

/// The close button hides the window (instead of quitting) only when there is
/// a tray icon to bring it back and the user wants it (Plan 3 decision).
pub fn hides_on_close(has_tray: bool, close_to_tray: bool) -> bool {
    has_tray && close_to_tray
}

/// The close button: hide to the tray when `hides_on_close` says so; otherwise
/// let the window close, which quits the app (and `run` saves the downloads).
pub fn on_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    let WindowEvent::CloseRequested { api, .. } = event else {
        return;
    };
    if window.label() != MAIN {
        return;
    }
    let app = window.app_handle();
    let has_tray = app.try_state::<TrayState>().is_some_and(|t| t.0);
    let close_to_tray = app
        .try_state::<mdm_core::Manager>()
        .is_some_and(|m| m.settings().close_to_tray);
    if hides_on_close(has_tray, close_to_tray) {
        api.prevent_close();
        let _ = window.hide();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_tray_the_close_button_quits() {
        assert!(hides_on_close(true, true));
        assert!(
            !hides_on_close(false, true),
            "no tray: never hide into nothing"
        );
        assert!(!hides_on_close(true, false), "the user turned it off");
    }
}
