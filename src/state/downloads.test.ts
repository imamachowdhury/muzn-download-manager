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

// Final review I2 (2026-09-19): a list reply is a snapshot that can be older
// than the events that arrived while it was on its way.
test("a stale list reply never overwrites newer events", () => {
  const s = createDownloadsStore();
  // subscribe, then ask for the list
  const seq = s.getState().beginLoad();
  s.getState().apply({ type: "updated", download: fakeRow({ id: "a", status: "PROBING", updatedAt: 2 }) });
  // the reply was taken before that event: it still says QUEUED
  s.getState().load([fakeRow({ id: "a", status: "QUEUED", updatedAt: 1 })], seq);
  expect(s.getState().rows.a?.status).toBe("PROBING");
  s.getState().apply({ type: "updated", download: fakeRow({ id: "a", status: "DOWNLOADING", updatedAt: 3 }) });
  s.getState().apply(progress("a", 40));
  expect(s.getState().rows.a?.status).toBe("DOWNLOADING");
  expect(s.getState().live.a?.downloaded).toBe(40);
});

test("a stale reply arriving after DOWNLOADING keeps the row running and its progress", () => {
  const s = createDownloadsStore();
  const seq = s.getState().beginLoad();
  s.getState().apply({ type: "updated", download: fakeRow({ id: "a", status: "PROBING", updatedAt: 2 }) });
  s.getState().apply({ type: "updated", download: fakeRow({ id: "a", status: "DOWNLOADING", updatedAt: 3 }) });
  s.getState().load([fakeRow({ id: "a", status: "QUEUED", updatedAt: 1 })], seq);
  s.getState().apply(progress("a", 40));
  expect(s.getState().rows.a?.status).toBe("DOWNLOADING");
  expect(s.getState().live.a?.downloaded).toBe(40);
});

test("a newer listed row still replaces an older stored one", () => {
  const s = createDownloadsStore();
  s.getState().load([fakeRow({ id: "a", status: "QUEUED", updatedAt: 1 })]);
  const seq = s.getState().beginLoad();
  s.getState().load([fakeRow({ id: "a", status: "PAUSED", updatedAt: 5 })], seq);
  expect(s.getState().rows.a?.status).toBe("PAUSED");
});

test("a row removed while the list was loading stays gone; one added meanwhile stays", () => {
  const s = createDownloadsStore();
  s.getState().load([fakeRow({ id: "r", updatedAt: 1 })]);
  const seq = s.getState().beginLoad();
  s.getState().apply({ type: "removed", id: "r" });
  s.getState().apply({ type: "added", download: fakeRow({ id: "n", updatedAt: 9 }) });
  s.getState().load([fakeRow({ id: "r", updatedAt: 1 })], seq);
  expect(s.getState().rows.r).toBeUndefined();
  expect(s.getState().rows.n).toBeDefined();
});

test("a reply to an older request is ignored", () => {
  const s = createDownloadsStore();
  const first = s.getState().beginLoad();
  const second = s.getState().beginLoad();
  s.getState().load([fakeRow({ id: "new", updatedAt: 2 })], second);
  s.getState().load([fakeRow({ id: "old", updatedAt: 1 })], first);
  expect(Object.keys(s.getState().rows)).toEqual(["new"]);
});

// Final review M5 (2026-09-19): a filter that hides the selected row clears it.
test("changing the filter clears a selection the new filter hides", () => {
  const s = createDownloadsStore();
  s.getState().load([fakeRow({ id: "c", status: "COMPLETED" }), fakeRow({ id: "d", status: "DOWNLOADING" })]);
  s.getState().select("d");
  s.getState().setFilter("active");
  expect(s.getState().selected).toBe("d");
  s.getState().setFilter("completed");
  expect(s.getState().filter).toBe("completed");
  expect(s.getState().selected).toBeNull();
});
