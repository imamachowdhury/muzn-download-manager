import { useEffect, useState } from "react";
import type { DownloadRow, SegmentView } from "../api/types";
import {
  canCancel,
  canPause,
  canRestart,
  canResume,
  cancelWithConfirm,
  displayName,
  removeWithConfirm,
  savedPath,
  toggle,
} from "../lib/actions";
import { rowErrorText } from "../lib/errors";
import { formatBytes, formatEta, formatSpeed, percent } from "../lib/format";
import { useBackend, useDownloads } from "../state/context";
import type { Live } from "../state/downloads";
import { attempt } from "../ui/toast";
import { STATUS_LABEL } from "./DownloadRowView";

export function DetailPanel() {
  const row = useDownloads((s) => (s.selected ? s.rows[s.selected] : undefined));
  const live = useDownloads((s) => (s.selected ? s.live[s.selected] : undefined));
  if (!row) {
    return (
      <section className="detail detail-empty" aria-label="Download details">
        Select a download to see its details.
      </section>
    );
  }
  return <DetailBody key={row.id} row={row} live={live} />;
}

function DetailBody({ row, live }: { row: DownloadRow; live: Live | undefined }) {
  const backend = useBackend();
  const running = live !== undefined;
  const [saved, setSaved] = useState<SegmentView[]>([]);

  // Stopped downloads show what the store saved; running ones their live map.
  useEffect(() => {
    if (running) return;
    let on = true;
    backend
      .segments(row.id)
      .then((s) => on && setSaved(s))
      .catch(() => {});
    return () => {
      on = false;
    };
  }, [backend, row.id, row.updatedAt, running]);

  const segments = live?.segments ?? saved;
  const error = rowErrorText(row);
  const done = row.status === "COMPLETED";
  const total = live?.total ?? row.size;
  const downloaded = live?.downloaded ?? row.downloaded;

  return (
    <section className="detail" aria-label="Download details">
      <header className="detail-head">
        <strong className="detail-name" title={displayName(row)}>
          {displayName(row)}
        </strong>
        <span className={`badge badge-${row.status.toLowerCase()}`}>{STATUS_LABEL[row.status]}</span>
        <span className="toolbar-gap" />
        {done && (
          <>
            <button type="button" className="secondary-button" onClick={() => void attempt(backend.openFile(row.id))}>
              Open
            </button>
            <button type="button" className="secondary-button" onClick={() => void attempt(backend.showInFolder(row.id))}>
              Show in folder
            </button>
          </>
        )}
        {(canPause(row) || canResume(row)) && (
          <button type="button" className="secondary-button" onClick={() => void toggle(backend, row)}>
            {canPause(row) ? "Pause" : "Resume"}
          </button>
        )}
        {canRestart(row) && row.errorCode !== "SOURCE_CHANGED" && (
          <button type="button" className="secondary-button" onClick={() => void attempt(backend.restart(row.id))}>
            Restart
          </button>
        )}
        {canCancel(row) && (
          <button type="button" className="secondary-button" onClick={() => void cancelWithConfirm(backend, row)}>
            Cancel
          </button>
        )}
        <button type="button" className="secondary-button" onClick={() => void removeWithConfirm(backend, row)}>
          Remove
        </button>
      </header>

      {error && (
        <p className="detail-error" role="alert">
          {error}{" "}
          {row.errorCode === "SOURCE_CHANGED" && (
            <button type="button" className="link-button" onClick={() => void attempt(backend.restart(row.id))}>
              Restart from the beginning
            </button>
          )}
        </p>
      )}

      <dl className="facts">
        <dt>URL</dt>
        <dd title={row.finalUrl ?? row.url}>{row.finalUrl ?? row.url}</dd>
        <dt>Saved to</dt>
        <dd title={savedPath(row)}>{savedPath(row)}</dd>
        <dt>Size</dt>
        <dd>{formatBytes(total)}</dd>
        <dt>Downloaded</dt>
        <dd>
          {formatBytes(done ? total : downloaded)}
          {running && ` · ${formatSpeed(live.speedBps)} · ${formatEta(live.etaSecs)} left`}
        </dd>
      </dl>

      {segments.length > 0 && (
        <table className="segments">
          <thead>
            <tr>
              <th>#</th>
              <th>Range</th>
              <th>Done</th>
            </tr>
          </thead>
          <tbody>
            {segments.map((s, i) => (
              <tr key={s.start}>
                <td>{i + 1}</td>
                <td>
                  {formatBytes(s.start)} – {formatBytes(s.end + 1)}
                </td>
                <td>{`${percent(s.downloaded, s.end - s.start + 1) ?? 0}%`}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
