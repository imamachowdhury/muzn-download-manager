//! Tauri commands: thin wrappers over `mdm_core::Manager`. Every one is
//! async so it runs on the runtime, never on the window's main thread.

use mdm_core::{
    DownloadId, DownloadRow, Manager, NewDownload, ProbePreview, SegmentView, Settings,
};
use tauri::{AppHandle, Runtime, State};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_clipboard_manager::ClipboardExt as _;
use tauri_plugin_dialog::DialogExt as _;
use tauri_plugin_opener::OpenerExt as _;

use crate::api::{clipboard_link, completed_path, ApiError, ApiResult};

/// Every command the window may call — the one list `run()` registers and the
/// wiring test drives. Names and argument names are the UI's contract
/// (src/api/tauri.ts).
pub fn handler<R: Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        list_downloads,
        download_segments,
        add_download,
        probe_url,
        pause_download,
        resume_download,
        cancel_download,
        restart_download,
        remove_download,
        pause_all,
        resume_all,
        get_settings,
        set_settings,
        open_download,
        show_download_in_folder,
        pick_folder,
        clipboard_url,
        autostart_enabled,
        set_autostart,
    ]
}

fn row_of(m: &Manager, id: &DownloadId) -> ApiResult<DownloadRow> {
    m.get(id)?
        .ok_or_else(|| ApiError::new("NOT_FOUND", format!("not found: {id}")))
}

#[tauri::command]
pub async fn list_downloads(m: State<'_, Manager>) -> ApiResult<Vec<DownloadRow>> {
    Ok(m.list()?)
}

#[tauri::command]
pub async fn download_segments(
    m: State<'_, Manager>,
    id: DownloadId,
) -> ApiResult<Vec<SegmentView>> {
    Ok(m.segments(&id)?.iter().map(SegmentView::from).collect())
}

#[tauri::command]
pub async fn add_download(m: State<'_, Manager>, download: NewDownload) -> ApiResult<DownloadRow> {
    Ok(m.add(download)?)
}

#[tauri::command]
pub async fn probe_url(
    m: State<'_, Manager>,
    url: String,
    referrer: Option<String>,
) -> ApiResult<ProbePreview> {
    Ok(m.probe(&url, referrer.as_deref()).await?)
}

#[tauri::command]
pub async fn pause_download(m: State<'_, Manager>, id: DownloadId) -> ApiResult<()> {
    Ok(m.pause(&id)?)
}

#[tauri::command]
pub async fn resume_download(m: State<'_, Manager>, id: DownloadId) -> ApiResult<()> {
    Ok(m.resume(&id)?)
}

#[tauri::command]
pub async fn cancel_download(m: State<'_, Manager>, id: DownloadId) -> ApiResult<()> {
    Ok(m.cancel(&id)?)
}

#[tauri::command]
pub async fn restart_download(m: State<'_, Manager>, id: DownloadId) -> ApiResult<()> {
    Ok(m.restart(&id)?)
}

#[tauri::command]
pub async fn remove_download(
    m: State<'_, Manager>,
    id: DownloadId,
    delete_file: bool,
) -> ApiResult<()> {
    Ok(m.remove(&id, delete_file)?)
}

#[tauri::command]
pub async fn pause_all(m: State<'_, Manager>) -> ApiResult<()> {
    Ok(m.pause_all()?)
}

#[tauri::command]
pub async fn resume_all(m: State<'_, Manager>) -> ApiResult<()> {
    Ok(m.resume_all()?)
}

#[tauri::command]
pub async fn get_settings(m: State<'_, Manager>) -> ApiResult<Settings> {
    Ok(m.settings())
}

#[tauri::command]
pub async fn set_settings(m: State<'_, Manager>, settings: Settings) -> ApiResult<Settings> {
    Ok(m.set_settings(settings)?)
}

#[tauri::command]
pub async fn open_download<R: Runtime>(
    app: AppHandle<R>,
    m: State<'_, Manager>,
    id: DownloadId,
) -> ApiResult<()> {
    let path = completed_path(&row_of(&m, &id)?)?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| ApiError::internal(e.to_string()))
}

#[tauri::command]
pub async fn show_download_in_folder<R: Runtime>(
    app: AppHandle<R>,
    m: State<'_, Manager>,
    id: DownloadId,
) -> ApiResult<()> {
    let path = completed_path(&row_of(&m, &id)?)?;
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| ApiError::internal(e.to_string()))
}

#[tauri::command]
pub async fn pick_folder<R: Runtime>(
    app: AppHandle<R>,
    current: Option<String>,
) -> ApiResult<Option<String>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = app.dialog().file().set_title("Choose a download folder");
    if let Some(dir) = current.filter(|d| !d.is_empty()) {
        dialog = dialog.set_directory(dir);
    }
    dialog.pick_folder(move |picked| {
        let _ = tx.send(picked);
    });
    let picked = rx
        .await
        .map_err(|_| ApiError::internal("the folder dialog closed unexpectedly"))?;
    Ok(picked
        .and_then(|p| p.into_path().ok())
        .map(|p| p.display().to_string()))
}

#[tauri::command]
pub async fn clipboard_url<R: Runtime>(app: AppHandle<R>) -> ApiResult<Option<String>> {
    // An empty or non-text clipboard is not an error: there is just no link.
    Ok(app
        .clipboard()
        .read_text()
        .ok()
        .and_then(|t| clipboard_link(&t)))
}

#[tauri::command]
pub async fn autostart_enabled<R: Runtime>(app: AppHandle<R>) -> ApiResult<bool> {
    app.autolaunch()
        .is_enabled()
        .map_err(|e| ApiError::internal(e.to_string()))
}

#[tauri::command]
pub async fn set_autostart<R: Runtime>(app: AppHandle<R>, enabled: bool) -> ApiResult<bool> {
    let al = app.autolaunch();
    let r = if enabled { al.enable() } else { al.disable() };
    r.map_err(|e| ApiError::internal(e.to_string()))?;
    al.is_enabled()
        .map_err(|e| ApiError::internal(e.to_string()))
}
