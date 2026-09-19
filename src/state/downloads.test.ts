import { expect, test } from "vitest";
import { fakeRow } from "../api/fake";
import type { ProgressEvent } from "../api/types";
import { createDownloadsStore, filterCounts, visibleRows } from "./downloads";

const progress = (id: string, downloaded: number): ProgressEvent => ({
  type: "progress",
  id,
  total: 100,
  downloaded,
  speedBps: 10,
  etaSecs: 9,
  segments: [{ start: 0, end: 99, downloaded }],
});

test("load replaces the rows and keeps a valid selection", () => {
  const s = createDownloadsStore();
  s.getState().load([fakeRow({ id: "a" }), fakeRow({ id: "b" })]);
  s.getState().select("b");
  s.getState().load([fakeRow({ id: "b" })]);
  expect(Object.keys(s.getState().rows)).toEqual(["b"]);
  expect(s.getState().selected).toBe("b");
  s.getState().load([]);
  expect(s.getState().selected).toBeNull();
  expect(s.getState().loaded).toBe(true);
});

test("progress is kept only for a downloading row and dropped when it stops", () => {
  const s = createDownloadsStore();
  s.getState().load([fakeRow({ id: "a", status: "DOWNLOADING" }), fakeRow({ id: "b", status: "PAUSED" })]);
  s.getState().apply(progress("a", 40));
  s.getState().apply(progress("b", 40)); // a late event for a stopped row
  s.getState().apply(progress("zzz", 1)); // unknown row
  expect(s.getState().live.a?.downloaded).toBe(40);
  expect(s.getState().live.b).toBeUndefined();
  expect(s.getState().live.zzz).toBeUndefined();
  s.getState().apply({ type: "updated", download: fakeRow({ id: "a", status: "PAUSED", downloaded: 40 }) });
  expect(s.getState().live.a).toBeUndefined();
  expect(s.getState().rows.a?.status).toBe("PAUSED");
});

test("added and removed rows, and the selection follows a removal", () => {
  const s = createDownloadsStore();
  s.getState().load([]);
  s.getState().apply({ type: "added", download: fakeRow({ id: "n" }) });
  s.getState().select("n");
  expect(s.getState().rows.n).toBeDefined();
  s.getState().apply({ type: "removed", id: "n" });
  expect(s.getState().rows.n).toBeUndefined();
  expect(s.getState().selected).toBeNull();
});

test("filters and counts, newest first", () => {
  const rows = {
    q: fakeRow({ id: "q", status: "QUEUED", createdAt: 1 }),
    d: fakeRow({ id: "d", status: "DOWNLOADING", createdAt: 3 }),
    p: fakeRow({ id: "p", status: "PAUSED", createdAt: 2 }),
    c: fakeRow({ id: "c", status: "COMPLETED", createdAt: 4 }),
    f: fakeRow({ id: "f", status: "FAILED", createdAt: 5 }),
    x: fakeRow({ id: "x", status: "CANCELLED", createdAt: 6 }),
  };
  expect(visibleRows(rows, "all").map((r) => r.id)).toEqual(["x", "f", "c", "d", "p", "q"]);
  expect(visibleRows(rows, "active").map((r) => r.id)).toEqual(["d", "p", "q"]);
  expect(visibleRows(rows, "completed").map((r) => r.id)).toEqual(["c"]);
  expect(visibleRows(rows, "failed").map((r) => r.id)).toEqual(["f"]);
  expect(filterCounts(rows)).toEqual({ all: 6, active: 3, completed: 1, failed: 1 });
});
