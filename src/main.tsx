import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { inTauri, type Backend } from "./api/backend";
import { tauriBackend } from "./api/tauri";
import { AppProvider } from "./state/context";
import { createDownloadsStore } from "./state/downloads";
import "./styles.css";

/**
 * Inside the Tauri window: the real app. In a plain browser under `pnpm dev`:
 * the demo. The demo and the fake backend are loaded by a dynamic import that
 * only exists in a dev build (`import.meta.env.DEV` is `false` in `pnpm
 * build`, so the bundler drops it): the shipped app carries no demo rows
 * (final review M9, 2026-09-19).
 */
async function pickBackend(): Promise<Backend | null> {
  if (inTauri()) return tauriBackend;
  if (import.meta.env.DEV) {
    const [{ startDemo }, { createFakeBackend }] = await Promise.all([import("./api/demo"), import("./api/fake")]);
    return startDemo(createFakeBackend());
  }
  return null;
}

const root = createRoot(document.getElementById("root")!);
void pickBackend().then((backend) => {
  if (!backend) {
    root.render(<p className="empty">Muzn Download Manager runs in its own window.</p>);
    return;
  }
  root.render(
    <StrictMode>
      <AppProvider backend={backend} store={createDownloadsStore()}>
        <App />
      </AppProvider>
    </StrictMode>,
  );
});
