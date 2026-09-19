import type { Backend } from "../api/backend";
import type { DownloadRow } from "../api/types";
import { confirmDialog } from "../ui/confirm";
import { attempt } from "../ui/toast";

export function displayName(row: DownloadRow): string {
  if (row.filename) return row.filename;
  try {
    const last = new URL(row.url).pathname.split("/").filter(Boolean).pop();
    if (last) return decodeURIComponent(last);
  } catch {
    // fall through to the URL itself
  }
  return row.url;
}

/** Folder + file name with the folder's own separator. */
export function savedPath(row: DownloadRow): string {
  if (!row.filename) return row.dir;
  const sep = row.dir.includes("\\") ? "\\" : "/";
  const dir = row.dir.endsWith(sep) ? row.dir.slice(0, -1) : row.dir;
  return `${dir}${sep}${row.filename}`;
}

export const canPause = (r: DownloadRow) => ["QUEUED", "PROBING", "DOWNLOADING"].includes(r.status);
// A SOURCE_CHANGED row would only fail again on resume: the saved part belongs
// to the old file. Its way on is "Restart from the beginning" (final review M3).
export const canResume = (r: DownloadRow) =>
  ["PAUSED", "FAILED", "CANCELLED"].includes(r.status) && r.errorCode !== "SOURCE_CHANGED";
export const canRestart = (r: DownloadRow) => ["PAUSED", "FAILED", "CANCELLED"].includes(r.status);
export const canCancel = (r: DownloadRow) => !["COMPLETED", "CANCELLED"].includes(r.status);

/** Space / the toolbar: pause a running download, resume a stopped one. */
export async function toggle(backend: Backend, row: DownloadRow): Promise<void> {
  if (canPause(row)) await attempt(backend.pause(row.id));
  else if (canResume(row)) await attempt(backend.resume(row.id));
}

export async function removeWithConfirm(backend: Backend, row: DownloadRow): Promise<void> {
  const finished = row.status === "COMPLETED";
  const answer = await confirmDialog({
    title: "Remove download",
    message: finished
      ? `Remove "${displayName(row)}" from the list?`
      : `Remove "${displayName(row)}" from the list? The partly downloaded data is deleted.`,
    confirmLabel: "Remove",
    danger: true,
    checkbox: finished ? "Also delete the file from disk" : undefined,
  });
  if (answer.ok) await attempt(backend.remove(row.id, answer.checked));
}

export async function cancelWithConfirm(backend: Backend, row: DownloadRow): Promise<void> {
  const answer = await confirmDialog({
    title: "Cancel download",
    message: `Cancel "${displayName(row)}"? The partly downloaded data is deleted.`,
    confirmLabel: "Cancel download",
    danger: true,
  });
  if (answer.ok) await attempt(backend.cancel(row.id));
}

/** Restart throws the downloaded part away, so it asks first (final review I3). */
export async function restartWithConfirm(backend: Backend, row: DownloadRow): Promise<void> {
  const answer = await confirmDialog({
    title: "Restart download",
    message: `Restart "${displayName(row)}" from the beginning? The downloaded part is deleted.`,
    confirmLabel: "Restart",
    danger: true,
  });
  if (answer.ok) await attempt(backend.restart(row.id));
}
