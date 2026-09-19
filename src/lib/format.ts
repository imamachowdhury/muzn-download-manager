const UNITS = ["B", "KB", "MB", "GB", "TB"] as const;

/** 1536 → "1.50 KB"; three significant digits above a kilobyte. */
export function formatBytes(n: number | null | undefined): string {
  if (n == null) return "—";
  if (n < 1024) return `${n} B`;
  let v = n;
  let i = 0;
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i++;
  }
  const digits = v < 10 ? 2 : v < 100 ? 1 : 0;
  return `${v.toFixed(digits)} ${UNITS[i]}`;
}

export function formatSpeed(bps: number): string {
  return bps > 0 ? `${formatBytes(bps)}/s` : "—";
}

const pad = (n: number) => String(n).padStart(2, "0");

export function formatEta(secs: number | null): string {
  if (secs == null) return "—";
  if (secs < 60) return `${secs}s`;
  const m = Math.floor(secs / 60);
  if (m < 60) return `${m}m ${pad(secs % 60)}s`;
  return `${Math.floor(m / 60)}h ${pad(m % 60)}m`;
}

/** Whole percent, capped at 100; `null` when the size is unknown. */
export function percent(downloaded: number, total: number | null): number | null {
  if (!total) return null;
  return Math.min(100, Math.floor((downloaded * 100) / total));
}
