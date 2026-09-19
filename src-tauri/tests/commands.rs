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
            url: app_url(),
            body: InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|r| r.deserialize::<Value>().unwrap())
}

fn status_of(w: &tauri::WebviewWindow<tauri::test::MockRuntime>, id: &str) -> Value {
    let list = call(w, "list_downloads", json!({})).unwrap();
    list.as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .map(|r| r["status"].clone())
        .unwrap_or(Value::Null)
}

/// Poll the list until the row reaches `want` (the manager stops a running
/// download asynchronously).
fn wait_for_status(w: &tauri::WebviewWindow<tauri::test::MockRuntime>, id: &str, want: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let now = status_of(w, id);
        if now == want {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{id} never reached {want}; it is {now}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

// Final review M12 (2026-09-19): the name says what it drives - pause and
// resume on the new row included.
#[test]
fn the_ui_contract_add_list_resume_pause_remove() {
    let d = tempfile::tempdir().unwrap();
    let (_app, w) = app(d.path());
    // A server that accepts the connection and never answers: the download
    // stays in flight (QUEUED / PROBING) until it is paused.
    let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/never.bin", silent.local_addr().unwrap());
    let row = call(
        &w,
        "add_download",
        json!({ "download": { "url": url, "startPaused": true } }),
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

    assert_eq!(
        call(&w, "resume_download", json!({ "id": id })).unwrap(),
        Value::Null
    );
    let running = status_of(&w, &id);
    assert!(
        running == "QUEUED" || running == "PROBING",
        "a resumed row is back in flight, got {running}"
    );
    assert_eq!(
        call(&w, "pause_download", json!({ "id": id })).unwrap(),
        Value::Null
    );
    wait_for_status(&w, &id, "PAUSED");
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
