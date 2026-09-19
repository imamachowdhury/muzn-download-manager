import type {
  DownloadRow,
  ManagerEvent,
  NewDownload,
  ProbePreview,
  SegmentView,
  Settings,
} from "./types";

/** Everything the UI may ask of the app. The UI talks to nothing else. */
export interface Backend {
  list(): Promise<DownloadRow[]>;
  segments(id: string): Promise<SegmentView[]>;
  add(download: NewDownload): Promise<DownloadRow>;
  probe(url: string, referrer?: string | null): Promise<ProbePreview>;
  pause(id: string): Promise<void>;
  resume(id: string): Promise<void>;
  cancel(id: string): Promise<void>;
  restart(id: string): Promise<void>;
  remove(id: string, deleteFile: boolean): Promise<void>;
  pauseAll(): Promise<void>;
  resumeAll(): Promise<void>;
  getSettings(): Promise<Settings>;
  setSettings(settings: Settings): Promise<Settings>;
  openFile(id: string): Promise<void>;
  showInFolder(id: string): Promise<void>;
  pickFolder(current: string | null): Promise<string | null>;
  clipboardUrl(): Promise<string | null>;
  autostartEnabled(): Promise<boolean>;
  setAutostart(enabled: boolean): Promise<boolean>;
  /** Manager events from now on; `onResync` = events were lost, reload. Resolves to an unsubscribe. */
  subscribe(onEvent: (e: ManagerEvent) => void, onResync: () => void): Promise<() => void>;
}

/** True inside the Tauri window; false in a plain browser (`pnpm dev`) and in tests. */
export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
