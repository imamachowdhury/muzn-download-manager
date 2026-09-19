import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { inTauri } from "./api/backend";
import { startDemo } from "./api/demo";
import { createFakeBackend } from "./api/fake";
import { tauriBackend } from "./api/tauri";
import { AppProvider } from "./state/context";
import { createDownloadsStore } from "./state/downloads";
import "./styles.css";

// Inside the Tauri window: the real app. In a plain browser (`pnpm dev`): the demo.
const backend = inTauri() ? tauriBackend : startDemo(createFakeBackend());
const store = createDownloadsStore();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <AppProvider backend={backend} store={store}>
      <App />
    </AppProvider>
  </StrictMode>,
);
