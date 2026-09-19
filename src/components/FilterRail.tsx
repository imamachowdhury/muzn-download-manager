import { useMemo } from "react";
import { useDownloads, useDownloadsStore } from "../state/context";
import { filterCounts, type Filter } from "../state/downloads";

const LABELS: [Filter, string][] = [
  ["all", "All"],
  ["active", "Downloading"],
  ["completed", "Completed"],
  ["failed", "Failed"],
];

export function FilterRail() {
  const store = useDownloadsStore();
  const rows = useDownloads((s) => s.rows);
  const filter = useDownloads((s) => s.filter);
  const counts = useMemo(() => filterCounts(rows), [rows]);
  return (
    <nav className="rail" aria-label="Filters">
      {LABELS.map(([key, label]) => (
        <button
          key={key}
          type="button"
          className={`rail-item${filter === key ? " rail-item-on" : ""}`}
          aria-pressed={filter === key}
          onClick={() => store.getState().setFilter(key)}
        >
          <span>{label}</span> <span className="count">{counts[key]}</span>
        </button>
      ))}
    </nav>
  );
}
