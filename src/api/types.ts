// Mirrors of the Rust serde shapes (mdm-core model.rs / events.rs / settings.rs).

export type DownloadStatus =
  | "QUEUED"
  | "PROBING"
  | "DOWNLOADING"
  | "PAUSED"
  | "COMPLETED"
  | "FAILED"
  | "CANCELLED"
  | "SEEDING";

export interface NameValue {
  name: string;
  value: string;
}

export interface DownloadRow {
  id: string;
  kind: "http" | "torrent";
  url: string;
  finalUrl: string | null;
  filename: string | null;
  dir: string;
  size: number | null;
  downloaded: number;
  status: DownloadStatus;
  etag: string | null;
  lastModified: string | null;
  mime: string | null;
  referrer: string | null;
  headers: NameValue[];
  cookies: NameValue[];
  errorCode: string | null;
  errorMessage: string | null;
  createdAt: number;
  updatedAt: number;
  completedAt: number | null;
}

export interface SegmentView {
  start: number;
  /** Inclusive. */
  end: number;
  downloaded: number;
}

export interface ProgressEvent {
  type: "progress";
  id: string;
  total: number | null;
  downloaded: number;
  speedBps: number;
  etaSecs: number | null;
  segments: SegmentView[];
}

export type ManagerEvent =
  | { type: "added"; download: DownloadRow }
  | { type: "updated"; download: DownloadRow }
  | ProgressEvent
  | { type: "removed"; id: string }
  | { type: "notice"; id: string; message: string };

export interface NewDownload {
  url: string;
  dir?: string | null;
  filename?: string | null;
  referrer?: string | null;
  headers?: NameValue[];
  cookies?: NameValue[];
  startPaused?: boolean;
}

export interface ProbePreview {
  finalUrl: string;
  filename: string;
  size: number | null;
  resumable: boolean;
  mime: string | null;
}

export type ProxySetting =
  | { mode: "system" }
  | { mode: "none" }
  | { mode: "manual"; url: string };

export interface Settings {
  downloadDir: string;
  maxConnections: number;
  maxParallel: number;
  userAgent: string | null;
  proxy: ProxySetting;
  closeToTray: boolean;
  notifyOnComplete: boolean;
}

export interface ApiError {
  code: string;
  message: string;
}

export function isApiError(e: unknown): e is ApiError {
  return (
    typeof e === "object" &&
    e !== null &&
    typeof (e as ApiError).code === "string" &&
    typeof (e as ApiError).message === "string"
  );
}
