//! Drives the real command list (`mdm_app::commands::handler()`) through Tauri's mock
//! runtime with the exact JSON the UI sends (`src/api/tauri.ts`), so a renamed command
//! or argument fails here, not in the owner's hands.

use mdm_app::commands::handler;
use serde_json::{json, Value};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{Manager as _, WebviewWindowBuilder};

fn app(
    dir: &std::path::Path,
) -> (
    tauri::App<tauri::test::MockRuntime>,
    tauri::WebviewWindow<tauri::test::MockRuntime>,
) {
    let app = mock_builder()
        .invoke_handler(handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let db = dir.join("mdm.db");
    let manager =
        tauri::async_runtime::block_on(async { mdm_core::Manager::open(&db, dir) }).unwrap();
    app.manage(manager);
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    (app, webview)
}

fn call(
    w: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    cmd: &str,
    body: Value,
) -> Result<Value, Value> {
    get_ipc_response(
        w,
        InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: "http://tauri.localhost".parse().unwrap(),
            body: InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|r| r.deserialize::<Value>().unwrap())
}

#[test]
fn the_ui_contract_add_list_pause_remove() {
    let d = tempfile::tempdir().unwrap();
    let (_app, w) = app(d.path());
    let row = call(
        &w,
        "add_download",
        json!({ "download": { "url": "http://127.0.0.1:9/never.bin", "startPaused": true } }),
    )
    .unwrap();
    let id = row["id"].as_str().unwrap().to_owned();
    assert_eq!(row["status"], "PAUSED");
    let list = call(&w, "list_downloads", json!({})).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(
        call(&w, "download_segments", json!({ "id": id })).unwrap(),
        json!([])
    );
    call(
        &w,
        "remove_download",
        json!({ "id": id, "deleteFile": false }),
    )
    .unwrap();
    assert_eq!(call(&w, "list_downloads", json!({})).unwrap(), json!([]));
}

#[test]
fn failures_arrive_as_code_and_message() {
    let d = tempfile::tempdir().unwrap();
    let (_app, w) = app(d.path());
    let e = call(
        &w,
        "add_download",
        json!({ "download": { "url": "ftp://x/y" } }),
    )
    .unwrap_err();
    assert_eq!(e["code"], "INVALID_URL");
    assert!(e["message"].as_str().unwrap().contains("ftp"));
    let e = call(&w, "pause_download", json!({ "id": "no-such-id" })).unwrap_err();
    assert_eq!(e["code"], "NOT_FOUND");
    let e = call(&w, "open_download", json!({ "id": "no-such-id" })).unwrap_err();
    assert_eq!(e["code"], "NOT_FOUND");
}

#[test]
fn settings_round_trip_in_camel_case() {
    let d = tempfile::tempdir().unwrap();
    let (_app, w) = app(d.path());
    let mut s = call(&w, "get_settings", json!({})).unwrap();
    assert_eq!(s["closeToTray"], true);
    s["maxConnections"] = json!(99);
    let saved = call(&w, "set_settings", json!({ "settings": s })).unwrap();
    assert_eq!(saved["maxConnections"], 32, "the core clamps");
}
