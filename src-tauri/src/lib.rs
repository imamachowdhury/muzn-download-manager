//! Muzn Download Manager — the desktop app: Tauri commands and events over `mdm-core`.

mod api;
pub mod commands;
mod events;
mod paths;
mod tray;
mod window;

use std::time::Duration;

use tauri::{AppHandle, Manager as _, RunEvent, Runtime};

use crate::paths::AppPaths;

/// How long quitting waits for running downloads to save their state.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// Start the app. Returns when the app exits.
pub fn run() {
    init_logging();
    let app = tauri::Builder::default()
        // Must be the first plugin: a second launch focuses this one and exits.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            window::show_main(app)
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let paths = AppPaths::resolve(app.handle())?;
            std::fs::create_dir_all(&paths.data_dir)?;
            tracing::info!(db = %paths.db.display(), "opening the download database");
            // The manager captures the runtime it is opened in and spawns
            // every download there; Tauri's own runtime is that runtime.
            let manager = tauri::async_runtime::block_on(async {
                mdm_core::Manager::open(&paths.db, &paths.download_dir)
            })?;
            app.manage(manager);
            let manager = app.state::<mdm_core::Manager>().inner().clone();
            events::spawn_forwarder(app.handle().clone(), manager);
            let has_tray = match tray::build(app) {
                Ok(()) => true,
                Err(e) => {
                    tracing::warn!(error = %e, "no tray icon; the close button will quit");
                    false
                }
            };
            app.manage(tray::TrayState(has_tray));
            Ok(())
        })
        .on_window_event(window::on_window_event)
        .invoke_handler(commands::handler())
        .build(tauri::generate_context!())
        .expect("building the Muzn Download Manager window failed");
    app.run(|app, event| match event {
        RunEvent::Exit => shutdown(app),
        // macOS: clicking the Dock icon of a running app whose window is
        // hidden to the tray brings the window back (final review M11).
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => window::show_main(app),
        _ => {}
    });
}

/// Pause and save every running download so it continues at the next launch.
fn shutdown<R: Runtime>(app: &AppHandle<R>) {
    let Some(m) = app.try_state::<mdm_core::Manager>() else {
        return;
    };
    let m = m.inner().clone();
    let saved = tauri::async_runtime::block_on(async move {
        tokio::time::timeout(SHUTDOWN_GRACE, m.shutdown()).await
    });
    if saved.is_err() {
        tracing::warn!("some downloads did not save their state within {SHUTDOWN_GRACE:?}");
    }
}

fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}
