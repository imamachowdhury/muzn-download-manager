import type { FakeBackend } from "./fake";
import { fakeRow } from "./fake";

const MB = 1024 * 1024;

/** Seed a few rows and move the downloading ones, so `pnpm dev` in a browser looks alive. */
export function startDemo(fake: FakeBackend): FakeBackend {
  const now = Date.now();
  const seed = [
    fakeRow({ id: "demo-1", filename: "ubuntu-24.04-desktop-amd64.iso", size: 5800 * MB, downloaded: 1900 * MB, status: "DOWNLOADING", createdAt: now - 1000 }),
    fakeRow({ id: "demo-2", filename: "vacation-video.mp4", size: 820 * MB, downloaded: 300 * MB, status: "PAUSED", createdAt: now - 2000 }),
    fakeRow({ id: "demo-3", filename: "report-final.pdf", size: 3 * MB, downloaded: 3 * MB, status: "COMPLETED", createdAt: now - 3000 }),
    fakeRow({ id: "demo-4", filename: "setup-2.1.exe", size: 96 * MB, downloaded: 40 * MB, status: "FAILED", errorCode: "SOURCE_CHANGED", errorMessage: "the ETag changed", createdAt: now - 4000 }),
    fakeRow({ id: "demo-5", filename: "podcast-episode-12.mp3", size: null, downloaded: 0, status: "QUEUED", createdAt: now - 5000 }),
  ];
  seed.forEach((r) => fake.rows.set(r.id, r));
  const parts = 8;
  const progress = new Map<string, number[]>();
  setInterval(() => {
    for (const row of fake.rows.values()) {
      if (row.status !== "DOWNLOADING" || !row.size) continue;
      const size = row.size;
      const segLen = Math.ceil(size / parts);
      const done = progress.get(row.id) ?? Array.from({ length: parts }, () => 0);
      const next = done.map((d, i) => {
        const len = Math.min(segLen, size - i * segLen);
        return Math.min(len, d + Math.round(len * (0.004 + Math.random() * 0.006)));
      });
      progress.set(row.id, next);
      const downloaded = next.reduce((a, b) => a + b, 0);
      fake.emit({
        type: "progress",
        id: row.id,
        total: size,
        downloaded,
        speedBps: 6 * MB + Math.round(Math.random() * 2 * MB),
        etaSecs: Math.round((size - downloaded) / (7 * MB)),
        segments: next.map((d, i) => ({ start: i * segLen, end: Math.min(size, (i + 1) * segLen) - 1, downloaded: d })),
      });
      if (downloaded >= size) {
        const doneRow = { ...row, status: "COMPLETED" as const, downloaded: size };
        fake.rows.set(row.id, doneRow);
        fake.emit({ type: "updated", download: doneRow });
      }
    }
  }, 250);
  return fake;
}
