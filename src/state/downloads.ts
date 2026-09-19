import { createStore, type StoreApi } from "zustand/vanilla";
import type { DownloadRow, DownloadStatus, ManagerEvent, SegmentView } from "../api/types";

export type Filter = "all" | "active" | "completed" | "failed";

/** Live figures of a running download (from progress events). */
export interface Live {
  total: number | null;
  downloaded: number;
  speedBps: number;
  etaSecs: number | null;
  segments: SegmentView[];
}

export interface DownloadsState {
  rows: Record<string, DownloadRow>;
  live: Record<string, Live>;
  filter: Filter;
  selected: string | null;
  loaded: boolean;
  /**
   * Mark the start of a list request; returns its sequence number. Events
   * that arrive between this call and the matching `load` win over the reply.
   */
  beginLoad(): number;
  /**
   * The list reply. `seq` (from `beginLoad`) drops a reply older than the
   * latest request; without it the reply counts as the latest.
   */
  load(rows: DownloadRow[], seq?: number): void;
  apply(e: ManagerEvent): void;
  setFilter(f: Filter): void;
  select(id: string | null): void;
}

export type DownloadsStore = StoreApi<DownloadsState>;

export const ACTIVE_STATUSES: readonly DownloadStatus[] = ["QUEUED", "PROBING", "DOWNLOADING", "PAUSED"];

const FILTERS: Record<Filter, (r: DownloadRow) => boolean> = {
  all: () => true,
  active: (r) => ACTIVE_STATUSES.includes(r.status),
  completed: (r) => r.status === "COMPLETED",
  failed: (r) => r.status === "FAILED",
};

function without<T>(obj: Record<string, T>, key: string): Record<string, T> {
  if (!(key in obj)) return obj;
  const copy = { ...obj };
  delete copy[key];
  return copy;
}

/** How one manager event changes the state (pure; the store's `apply`). */
export function reduce(s: DownloadsState, e: ManagerEvent): Partial<DownloadsState> {
  switch (e.type) {
    case "added":
    case "updated": {
      const rows = { ...s.rows, [e.download.id]: e.download };
      return e.download.status === "DOWNLOADING" ? { rows } : { rows, live: without(s.live, e.download.id) };
    }
    case "progress": {
      // A progress event that arrives after its row stopped is late: ignore it.
      if (s.rows[e.id]?.status !== "DOWNLOADING") return {};
      const { total, downloaded, speedBps, etaSecs, segments } = e;
      return { live: { ...s.live, [e.id]: { total, downloaded, speedBps, etaSecs, segments } } };
    }
    case "removed":
      return {
        rows: without(s.rows, e.id),
        live: without(s.live, e.id),
        selected: s.selected === e.id ? null : s.selected,
      };
    case "notice":
      return {};
  }
}

/**
 * Merge a list reply with what events already told us (2026-09-19 final
 * review, I2): the reply is a snapshot taken some time after the request, so
 * a row an event brought in or changed since the request is at least as new
 * as the reply's copy — keep the stored one when its `updatedAt` is not
 * older; never bring back a row removed since the request; keep a row added
 * since the request that the snapshot missed.
 */
export function mergeList(
  stored: Record<string, DownloadRow>,
  listed: DownloadRow[],
  touched: ReadonlySet<string>,
  removed: ReadonlySet<string>,
): Record<string, DownloadRow> {
  const rows: Record<string, DownloadRow> = {};
  for (const r of listed) {
    if (removed.has(r.id)) continue;
    const mine = stored[r.id];
    rows[r.id] = mine && mine.updatedAt >= r.updatedAt ? mine : r;
  }
  for (const id of touched) {
    const mine = stored[id];
    if (mine && !rows[id] && !removed.has(id)) rows[id] = mine;
  }
  return rows;
}

export function createDownloadsStore(): DownloadsStore {
  // Bookkeeping for the list request in flight (not rendered, so not state).
  let latestSeq = 0;
  let touched = new Set<string>();
  let removed = new Set<string>();
  return createStore<DownloadsState>()((set) => ({
    rows: {},
    live: {},
    filter: "all",
    selected: null,
    loaded: false,
    beginLoad: () => {
      touched = new Set();
      removed = new Set();
      return ++latestSeq;
    },
    load: (listed, seq) => {
      if (seq !== undefined && seq < latestSeq) return; // an older request's late reply
      set((s) => {
        const byId = mergeList(s.rows, listed, touched, removed);
        const live = Object.fromEntries(
          Object.entries(s.live).filter(([id]) => byId[id]?.status === "DOWNLOADING"),
        );
        const selected = s.selected && byId[s.selected] ? s.selected : null;
        return { rows: byId, live, selected, loaded: true };
      });
      touched = new Set();
      removed = new Set();
    },
    apply: (e) => {
      if (e.type === "added" || e.type === "updated") touched.add(e.download.id);
      if (e.type === "removed") {
        touched.delete(e.id);
        removed.add(e.id);
      }
      set((s) => reduce(s, e));
    },
    // A selection the new filter hides is cleared (final review M5), so
    // Space / Delete never act on a row the user cannot see.
    setFilter: (filter) =>
      set((s) => {
        const row = s.selected ? s.rows[s.selected] : undefined;
        return { filter, selected: row && FILTERS[filter](row) ? s.selected : null };
      }),
    select: (selected) => set({ selected }),
  }));
}

/** The rows a filter shows, newest first. Call inside `useMemo` — it builds a new array. */
export function visibleRows(rows: Record<string, DownloadRow>, filter: Filter): DownloadRow[] {
  return Object.values(rows)
    .filter(FILTERS[filter])
    .sort((a, b) => b.createdAt - a.createdAt || a.id.localeCompare(b.id));
}

export function filterCounts(rows: Record<string, DownloadRow>): Record<Filter, number> {
  const all = Object.values(rows);
  return {
    all: all.length,
    active: all.filter(FILTERS.active).length,
    completed: all.filter(FILTERS.completed).length,
    failed: all.filter(FILTERS.failed).length,
  };
}
