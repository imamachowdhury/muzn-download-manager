import { isApiError, type DownloadRow } from "../api/types";

// The one place stable error codes become words (Global Constraints).
const MESSAGES: Record<string, string> = {
  // The detail panel puts a "Restart from the beginning" link right beside it.
  SOURCE_CHANGED: "The file on the server changed.",
  CANCELLED: "The download was cancelled.",
  JSON: "Saved data could not be read.",
  HTTP_STATUS: "The server refused the download — the link may have expired.",
  NETWORK: "Network problem. Check the connection and resume.",
  TLS: "The secure connection failed.",
  DISK_FULL: "Not enough disk space. Free some space and resume.",
  IO: "The file could not be written.",
  PART_IN_USE: "Another download is writing this file.",
  RANGE_NOT_SUPPORTED: "The server does not support resuming this file.",
  INVALID_URL: "That is not a valid http or https link.",
  INVALID_RESUME: "The saved progress no longer fits this file.",
  INVALID_STATE: "That action does not fit this download right now.",
  NOT_FOUND: "That download no longer exists.",
  SETTINGS: "A setting is not valid.",
  INTERNAL: "Something went wrong inside the app.",
  DB: "The download list could not be saved.",
};

export function errorText(code: string | null | undefined, detail?: string | null): string {
  if (code && MESSAGES[code]) return MESSAGES[code];
  return detail || "Something went wrong.";
}

/** A rejected command (or any thrown value) as one line for a toast. */
export function describeError(e: unknown): string {
  if (isApiError(e)) {
    const known = MESSAGES[e.code];
    return known ? `${known} (${e.message})` : e.message;
  }
  if (e instanceof Error) return e.message;
  return String(e);
}

export function rowErrorText(row: DownloadRow): string | null {
  if (!row.errorCode) return null;
  return errorText(row.errorCode, row.errorMessage);
}
