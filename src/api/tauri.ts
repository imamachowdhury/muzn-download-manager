import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Backend } from "./backend";
import type {
  DownloadRow,
  ManagerEvent,
  ProbePreview,
  SegmentView,
  Settings,
} from "./types";

// Command and argument names: src-tauri/src/commands.rs (Tauri camelCases the args).
export const tauriBackend: Backend = {
  list: () => invoke<DownloadRow[]>("list_downloads"),
  segments: (id) => invoke<SegmentView[]>("download_segments", { id }),
  add: (download) => invoke<DownloadRow>("add_download", { download }),
  probe: (url, referrer) => invoke<ProbePreview>("probe_url", { url, referrer: referrer ?? null }),
  pause: (id) => invoke<void>("pause_download", { id }),
  resume: (id) => invoke<void>("resume_download", { id }),
  cancel: (id) => invoke<void>("cancel_download", { id }),
  restart: (id) => invoke<void>("restart_download", { id }),
  remove: (id, deleteFile) => invoke<void>("remove_download", { id, deleteFile }),
  pauseAll: () => invoke<void>("pause_all"),
  resumeAll: () => invoke<void>("resume_all"),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings) => invoke<Settings>("set_settings", { settings }),
  openFile: (id) => invoke<void>("open_download", { id }),
  showInFolder: (id) => invoke<void>("show_download_in_folder", { id }),
  pickFolder: (current) => invoke<string | null>("pick_folder", { current }),
  clipboardUrl: () => invoke<string | null>("clipboard_url"),
  autostartEnabled: () => invoke<boolean>("autostart_enabled"),
  setAutostart: (enabled) => invoke<boolean>("set_autostart", { enabled }),
  async subscribe(onEvent, onResync) {
    const offs = await Promise.all([
      listen<ManagerEvent>("download:progress", (e) => onEvent(e.payload)),
      listen<ManagerEvent>("download:status", (e) => onEvent(e.payload)),
      listen("download:resync", () => onResync()),
    ]);
    return () => offs.forEach((off) => off());
  },
};
