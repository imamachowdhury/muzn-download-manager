import type { DownloadStatus, SegmentView } from "../api/types";
import { percent } from "../lib/format";

interface Props {
  status: DownloadStatus;
  total: number | null;
  downloaded: number;
  /** The live segment map; `null` = draw one plain bar. */
  segments: SegmentView[] | null;
}

/** IDM's look: one cell per segment, each filled as far as it got. */
export function SegmentBar({ status, total, downloaded, segments }: Props) {
  const done = status === "COMPLETED";
  const pct = done ? 100 : percent(downloaded, total);
  const label = pct == null ? "Progress unknown" : `${pct}%`;
  return (
    <div
      className={`segbar${pct == null && status === "DOWNLOADING" ? " segbar-busy" : ""}`}
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={pct ?? undefined}
    >
      {!done && total && segments && segments.length > 0 ? (
        segments.map((s) => {
          const len = s.end - s.start + 1;
          return (
            <div key={s.start} className="seg" style={{ width: `${(len / total) * 100}%` }}>
              <div className="seg-fill" style={{ width: `${Math.min(100, (s.downloaded / len) * 100)}%` }} />
            </div>
          );
        })
      ) : (
        <div className="seg" style={{ width: "100%" }}>
          <div className="seg-fill" style={{ width: `${pct ?? 0}%` }} />
        </div>
      )}
    </div>
  );
}
