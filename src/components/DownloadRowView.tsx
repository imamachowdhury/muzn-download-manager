import { memo, type CSSProperties } from "react";
import type { DownloadRow } from "../api/types";
import { displayName } from "../lib/actions";
import { formatBytes, formatEta, formatSpeed, percent } from "../lib/format";
import type { Live } from "../state/downloads";
import { SegmentBar } from "./SegmentBar";

export const STATUS_LABEL: Record<DownloadRow["status"], string> = {
  QUEUED: "Queued",
  PROBING: "Connecting",
  DOWNLOADING: "Downloading",
  PAUSED: "Paused",
  COMPLETED: "Completed",
  FAILED: "Failed",
  CANCELLED: "Cancelled",
  SEEDING: "Seeding",
};

interface Props {
  row: DownloadRow;
  live: Live | undefined;
  selected: boolean;
  onSelect(id: string): void;
  onOpen(row: DownloadRow): void;
  style: CSSProperties;
}

export const DownloadRowView = memo(function DownloadRowView({ row, live, selected, onSelect, onOpen, style }: Props) {
  const total = live?.total ?? row.size;
  const downloaded = live?.downloaded ?? row.downloaded;
  const pct = row.status === "COMPLETED" ? 100 : percent(downloaded, total);
  const name = displayName(row);
  return (
    <div
      role="option"
      aria-selected={selected}
      aria-label={name}
      className={`row${selected ? " row-selected" : ""}`}
      style={style}
      onClick={() => onSelect(row.id)}
      onDoubleClick={() => onOpen(row)}
    >
      <div className="cell cell-name">
        <span className="name" data-testid="name" title={name}>
          {name}
        </span>
        <small className="sub">
          {total ? `${formatBytes(downloaded)} of ${formatBytes(total)}` : formatBytes(downloaded)}
        </small>
      </div>
      <div className="cell cell-size">{formatBytes(total)}</div>
      <div className="cell cell-progress">
        <SegmentBar status={row.status} total={total} downloaded={downloaded} segments={live?.segments ?? null} />
        <small className="pct">{pct == null ? "—" : `${pct}%`}</small>
      </div>
      <div className="cell cell-speed">{live ? formatSpeed(live.speedBps) : "—"}</div>
      <div className="cell cell-eta">{live ? formatEta(live.etaSecs) : "—"}</div>
      <div className="cell cell-status">
        <span className={`badge badge-${row.status.toLowerCase()}`}>{STATUS_LABEL[row.status]}</span>
      </div>
    </div>
  );
});
