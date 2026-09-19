import { useBackend, useDownloads } from "../state/context";
import { canPause, canResume, removeWithConfirm, toggle } from "../lib/actions";
import { attempt } from "../ui/toast";

interface Props {
  onAdd(): void;
  onSettings(): void;
}

export function TopBar({ onAdd, onSettings }: Props) {
  const backend = useBackend();
  const row = useDownloads((s) => (s.selected ? s.rows[s.selected] : undefined));
  return (
    <header className="topbar">
      <h1 className="brand">Muzn Download Manager</h1>
      <div className="toolbar" role="toolbar" aria-label="Actions">
        <button type="button" className="primary-button" onClick={onAdd} title="Add a download (Ctrl+N)">
          Add URL
        </button>
        <button
          type="button"
          className="secondary-button"
          disabled={!row || !canResume(row)}
          onClick={() => row && void toggle(backend, row)}
        >
          Resume
        </button>
        <button
          type="button"
          className="secondary-button"
          disabled={!row || !canPause(row)}
          onClick={() => row && void toggle(backend, row)}
        >
          Pause
        </button>
        <button
          type="button"
          className="secondary-button"
          disabled={!row}
          onClick={() => row && void removeWithConfirm(backend, row)}
        >
          Remove
        </button>
        <span className="toolbar-gap" />
        <button type="button" className="secondary-button" onClick={() => void attempt(backend.resumeAll())}>
          Resume all
        </button>
        <button type="button" className="secondary-button" onClick={() => void attempt(backend.pauseAll())}>
          Pause all
        </button>
        <button type="button" className="secondary-button" onClick={onSettings}>
          Settings
        </button>
      </div>
    </header>
  );
}
