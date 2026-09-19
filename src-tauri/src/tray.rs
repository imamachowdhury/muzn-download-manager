//! The tray icon: show the window, pause / resume everything, quit.

use mdm_core::Manager;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager as _, Runtime};

use crate::window::show_main;

/// Whether a tray icon exists (some Linux desktops have none).
pub struct TrayState(pub bool);

/// Build the tray icon and its menu.
pub fn build<R: Runtime>(app: &App<R>) -> tauri::Result<()> {
    let show = MenuItemBuilder::with_id("show", "Show Muzn Download Manager").build(app)?;
    let pause = MenuItemBuilder::with_id("pause_all", "Pause all").build(app)?;
    let resume = MenuItemBuilder::with_id("resume_all", "Resume all").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&show, &pause, &resume])
        .separator()
        .item(&quit)
        .build()?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Muzn Download Manager")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main(app),
            "pause_all" => on_manager(app, Manager::pause_all),
            "resume_all" => on_manager(app, Manager::resume_all),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

fn on_manager<R: Runtime>(app: &AppHandle<R>, action: fn(&Manager) -> mdm_core::Result<()>) {
    if let Some(m) = app.try_state::<Manager>() {
        if let Err(e) = action(&m) {
            tracing::warn!(error = %e, "a tray action failed");
        }
    }
}
