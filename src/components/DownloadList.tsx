import { useVirtualizer } from "@tanstack/react-virtual";
import { useCallback, useEffect, useMemo, useRef } from "react";
import type { DownloadRow } from "../api/types";
import { useBackend, useDownloads, useDownloadsStore } from "../state/context";
import { visibleRows } from "../state/downloads";
import { attempt } from "../ui/toast";
import { DownloadRowView } from "./DownloadRowView";

const ROW_HEIGHT = 48;

export function DownloadList() {
  const backend = useBackend();
  const store = useDownloadsStore();
  const rowsById = useDownloads((s) => s.rows);
  const filter = useDownloads((s) => s.filter);
  const live = useDownloads((s) => s.live);
  const selected = useDownloads((s) => s.selected);
  const loaded = useDownloads((s) => s.loaded);
  const rows = useMemo(() => visibleRows(rowsById, filter), [rowsById, filter]);
  const scrollRef = useRef<HTMLDivElement>(null);
  const virtual = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 8,
  });
  const onSelect = useCallback((id: string) => store.getState().select(id), [store]);
  const onOpen = useCallback(
    (row: DownloadRow) => {
      if (row.status === "COMPLETED") void attempt(backend.openFile(row.id));
    },
    [backend],
  );

  // The keyboard (ArrowUp/ArrowDown) can move the selection past what is
  // currently rendered; keep the selected row scrolled into view so
  // Space/Delete never act on a row the user cannot see.
  useEffect(() => {
    if (!selected) return;
    const index = rows.findIndex((r) => r.id === selected);
    if (index >= 0) virtual.scrollToIndex(index, { align: "auto" });
  }, [selected, rows, virtual]);

  return (
    <div className="list">
      <div className="list-head" aria-hidden="true">
        <div className="cell cell-name">Name</div>
        <div className="cell cell-size">Size</div>
        <div className="cell cell-progress">Progress</div>
        <div className="cell cell-speed">Speed</div>
        <div className="cell cell-eta">Time left</div>
        <div className="cell cell-status">Status</div>
      </div>
      <div ref={scrollRef} className="list-scroll" role="listbox" aria-label="Downloads" tabIndex={0}>
        {loaded && rows.length === 0 && (
          <p className="empty">No downloads here. Press Ctrl+N to add one.</p>
        )}
        <div style={{ height: virtual.getTotalSize(), position: "relative" }}>
          {virtual.getVirtualItems().map((item) => {
            const row = rows[item.index]!;
            return (
              <DownloadRowView
                key={row.id}
                row={row}
                live={live[row.id]}
                selected={row.id === selected}
                onSelect={onSelect}
                onOpen={onOpen}
                style={{ position: "absolute", top: 0, left: 0, right: 0, height: ROW_HEIGHT, transform: `translateY(${item.start}px)` }}
              />
            );
          })}
        </div>
      </div>
    </div>
  );
}
