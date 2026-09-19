import { render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "../App";
import { createFakeBackend, type FakeBackend } from "../api/fake";
import type { DownloadRow } from "../api/types";
import { AppProvider } from "../state/context";
import { createDownloadsStore } from "../state/downloads";

/** The whole app over a fake backend (seeded with `rows` when given). */
export function renderApp(fake?: FakeBackend, rows: DownloadRow[] = []) {
  const backend = fake ?? createFakeBackend({ rows });
  const store = createDownloadsStore();
  const user = userEvent.setup();
  const result = render(
    <AppProvider backend={backend} store={store}>
      <App />
    </AppProvider>,
  );
  return { ...result, fake: backend, store, user };
}
