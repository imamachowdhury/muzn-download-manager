import { useCallback, useEffect, useState } from "react";
import { removeWithConfirm, toggle } from "./lib/actions";
import { shortcutFor, step } from "./lib/keys";
import { AddDialog } from "./components/AddDialog";
import { DetailPanel } from "./components/DetailPanel";
import { DownloadList } from "./components/DownloadList";
import { FilterRail } from "./components/FilterRail";
import { TopBar } from "./components/TopBar";
import { useBackend, useDownloadsStore, useManagerSync } from "./state/context";
import { visibleRows } from "./state/downloads";
import { ConfirmHost } from "./ui/confirm";
import { ToastHost, toast } from "./ui/toast";

function useShortcuts(onAdd: () => void) {
  const backend = useBackend();
  const store = useDownloadsStore();
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (document.querySelector('[role="dialog"]')) return;
      const target = e.target instanceof Element ? e.target : null;
      const sc = shortcutFor(e, target);
      if (!sc) return;
      e.preventDefault();
      const s = store.getState();
      const row = s.selected ? s.rows[s.selected] : undefined;
      switch (sc) {
        case "add":
          onAdd();
          break;
        case "toggle":
          if (row) void toggle(backend, row);
          break;
        case "remove":
          if (row) void removeWithConfirm(backend, row);
          break;
        case "up":
        case "down":
          s.select(step(visibleRows(s.rows, s.filter), s.selected, sc === "down" ? 1 : -1));
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [backend, store, onAdd]);
}

export function App() {
  useManagerSync(toast);
  const [adding, setAdding] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const openAdd = useCallback(() => setAdding(true), []);
  useShortcuts(openAdd);
  return (
    <div className="app">
      <TopBar onAdd={openAdd} onSettings={() => setSettingsOpen(true)} />
      <div className="main">
        <FilterRail />
        <div className="content">
          <DownloadList />
          <DetailPanel />
        </div>
      </div>
      {/* Task 9 renders the settings dialog while `settingsOpen`. */}
      {adding && <AddDialog onClose={() => setAdding(false)} />}
      {settingsOpen && null}
      <ConfirmHost />
      <ToastHost />
    </div>
  );
}
