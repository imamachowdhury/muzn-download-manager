//! The window's permissions, checked through Tauri's own ACL: this builds the
//! app's REAL context (`generate_context!` resolves `capabilities/default.json`
//! at compile time) on the mock runtime and invokes the way the page does.
//!
//! Final review M10 (2026-09-19): the page may listen to events (what
//! `src/api/tauri.ts` `subscribe` does - `plugin:event|listen` / `unlisten`,
//! granted by `core:event:default`) and call the app's own commands, and
//! nothing else of core - `core:default` used to hand it app / window / path /
//! image / menu / tray / webview APIs it never uses.

use mdm_app::commands::handler;
use serde_json::{json, Value};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{get_ipc_response, mock_builder, MockRuntime, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{Manager as _, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

fn window(dir: &std::path::Path) -> (tauri::App<MockRuntime>, WebviewWindow<MockRuntime>) {
    let app = mock_builder()
        .invoke_handler(handler())
        .build(tauri::generate_context!(test = true))
        .unwrap();
    let manager =
        tauri::async_runtime::block_on(async { mdm_core::Manager::open(&dir.join("mdm.db"), dir) })
            .unwrap();
    app.manage(manager);
    let w = match app.get_webview_window("main") {
        Some(w) => w,
        None => WebviewWindowBuilder::new(&app, "main", WebviewUrl::default())
            .build()
            .unwrap(),
    };
    (app, w)
}

// Tauri's ACL treats the page's request URL as "local" only when it matches how the
// webview actually serves the app: `http://tauri.localhost` on Windows/Android (their
// webviews can't route a custom `tauri://` scheme), `tauri://localhost` everywhere else.
// Hard-coding the Windows form made Linux/macOS CI see a "remote" origin and refuse.
fn app_url() -> url::Url {
    if cfg!(windows) {
        "http://tauri.localhost".parse().unwrap()
    } else {
        "tauri://localhost".parse().unwrap()
    }
}

fn invoke(w: &WebviewWindow<MockRuntime>, cmd: &str, body: Value) -> Result<Value, Value> {
    get_ipc_response(
        w,
        InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: app_url(),
            body: InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|r| r.deserialize::<Value>().unwrap())
}

#[test]
fn the_page_may_listen_to_events_and_call_the_apps_commands_only() {
    let d = tempfile::tempdir().unwrap();
    let (_app, w) = window(d.path());

    // What `subscribe` does for download:progress / download:status / download:resync.
    let id = invoke(
        &w,
        "plugin:event|listen",
        json!({ "event": "download:status", "target": { "kind": "Any" }, "handler": 7 }),
    )
    .expect("listening to the app's events is allowed");
    invoke(
        &w,
        "plugin:event|unlisten",
        json!({ "event": "download:status", "eventId": id }),
    )
    .expect("the unsubscribe is allowed");

    // The app's own commands stay callable.
    assert_eq!(invoke(&w, "list_downloads", json!({})).unwrap(), json!([]));

    // The rest of core is not the page's business any more.
    for cmd in [
        "plugin:app|version",
        "plugin:window|close",
        "plugin:path|resolve_directory",
    ] {
        let refused = invoke(&w, cmd, json!({})).unwrap_err();
        assert!(
            refused.to_string().contains("not allowed"),
            "{cmd} must be refused by the ACL, got {refused}"
        );
    }
}
