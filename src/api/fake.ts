import type { Backend } from "./backend";
import type {
  ApiError,
  DownloadRow,
  ManagerEvent,
  NewDownload,
  ProbePreview,
  Settings,
} from "./types";
import { isApiError } from "./types";

export function fakeRow(over: Partial<DownloadRow> = {}): DownloadRow {
  return {
    id: "row-1",
    kind: "http",
    url: "https://example.com/file.zip",
    finalUrl: null,
    filename: "file.zip",
    dir: "/downloads",
    size: 1000,
    downloaded: 0,
    status: "QUEUED",
    etag: null,
    lastModified: null,
    mime: null,
    referrer: null,
    headers: [],
    cookies: [],
    errorCode: null,
    errorMessage: null,
    createdAt: 0,
    updatedAt: 0,
    completedAt: null,
    ...over,
  };
}

export function fakeSettings(over: Partial<Settings> = {}): Settings {
  return {
    downloadDir: "/downloads",
    maxConnections: 8,
    maxParallel: 3,
    userAgent: null,
    proxy: { mode: "system" },
    closeToTray: true,
    notifyOnComplete: true,
    ...over,
  };
}

export interface FakeBackend extends Backend {
  rows: Map<string, DownloadRow>;
  settings: Settings;
  /** What was asked, in order: "pause a", "remove a true", "add <url>"… */
  calls: string[];
  clipboard: string | null;
  probeResult: ProbePreview | ApiError;
  folderPick: string | null;
  autostart: boolean;
  emit(e: ManagerEvent): void;
  resync(): void;
}

/** An in-memory stand-in for the app: tests and the plain-browser dev mode. */
export function createFakeBackend(
  init: { rows?: DownloadRow[]; settings?: Partial<Settings> } = {},
): FakeBackend {
  const listeners = new Set<(e: ManagerEvent) => void>();
  const resyncers = new Set<() => void>();
  let next = 1;
  const fake: FakeBackend = {
    rows: new Map((init.rows ?? []).map((r) => [r.id, r])),
    settings: fakeSettings(init.settings),
    calls: [],
    clipboard: null,
    probeResult: {
      finalUrl: "https://example.com/y.zip",
      filename: "y.zip",
      size: 5 * 1024 * 1024,
      resumable: true,
      mime: "application/zip",
    },
    folderPick: "/picked",
    autostart: false,
    emit(e) {
      listeners.forEach((l) => l(e));
    },
    resync() {
      resyncers.forEach((r) => r());
    },
    async list() {
      return [...fake.rows.values()];
    },
    async segments(id) {
      const r = fake.rows.get(id);
      return r?.size ? [{ start: 0, end: r.size - 1, downloaded: r.downloaded }] : [];
    },
    async add(d: NewDownload) {
      fake.calls.push(`add ${d.url}`);
      const now = Date.now();
      const row = fakeRow({
        id: `fake-${next++}`,
        url: d.url,
        filename: d.filename ?? d.url.split("/").pop() ?? "download.bin",
        dir: d.dir ?? fake.settings.downloadDir,
        size: null,
        status: d.startPaused ? "PAUSED" : "QUEUED",
        createdAt: now,
        updatedAt: now,
      });
      fake.rows.set(row.id, row);
      fake.emit({ type: "added", download: row });
      return row;
    },
    async probe(url) {
      fake.calls.push(`probe ${url}`);
      if (isApiError(fake.probeResult)) throw fake.probeResult;
      return fake.probeResult;
    },
    async pause(id) {
      fake.calls.push(`pause ${id}`);
      update(id, { status: "PAUSED" });
    },
    async resume(id) {
      fake.calls.push(`resume ${id}`);
      update(id, { status: "QUEUED", errorCode: null, errorMessage: null });
    },
    async cancel(id) {
      fake.calls.push(`cancel ${id}`);
      update(id, { status: "CANCELLED", downloaded: 0 });
    },
    async restart(id) {
      fake.calls.push(`restart ${id}`);
      update(id, { status: "QUEUED", downloaded: 0, errorCode: null, errorMessage: null });
    },
    async remove(id, deleteFile) {
      fake.calls.push(`remove ${id} ${deleteFile}`);
      if (fake.rows.delete(id)) fake.emit({ type: "removed", id });
    },
    async pauseAll() {
      fake.calls.push("pauseAll");
    },
    async resumeAll() {
      fake.calls.push("resumeAll");
    },
    async getSettings() {
      return fake.settings;
    },
    async setSettings(s) {
      fake.calls.push("setSettings");
      fake.settings = s;
      return s;
    },
    async openFile(id) {
      fake.calls.push(`open ${id}`);
    },
    async showInFolder(id) {
      fake.calls.push(`showInFolder ${id}`);
    },
    async pickFolder() {
      fake.calls.push("pickFolder");
      return fake.folderPick;
    },
    async clipboardUrl() {
      return fake.clipboard;
    },
    async autostartEnabled() {
      return fake.autostart;
    },
    async setAutostart(enabled) {
      fake.calls.push(`setAutostart ${enabled}`);
      fake.autostart = enabled;
      return enabled;
    },
    async subscribe(onEvent, onResync) {
      listeners.add(onEvent);
      resyncers.add(onResync);
      return () => {
        listeners.delete(onEvent);
        resyncers.delete(onResync);
      };
    },
  };
  function update(id: string, patch: Partial<DownloadRow>) {
    const r = fake.rows.get(id);
    if (!r) return;
    const row = { ...r, ...patch, updatedAt: Date.now() };
    fake.rows.set(id, row);
    fake.emit({ type: "updated", download: row });
  }
  return fake;
}
