import { act, screen, waitFor, within } from "@testing-library/react";
import { expect, test } from "vitest";
import { createFakeBackend, fakeRow } from "./api/fake";
import type { DownloadRow } from "./api/types";
import { renderApp } from "./test/render";

const rows = [
  fakeRow({ id: "d", filename: "movie.mkv", status: "DOWNLOADING", size: 1000, createdAt: 3 }),
  fakeRow({ id: "p", filename: "paused.iso", status: "PAUSED", size: 1000, downloaded: 400, createdAt: 2 }),
  fakeRow({ id: "c", filename: "done.zip", status: "COMPLETED", size: 1000, downloaded: 1000, createdAt: 1 }),
];

test("the list shows every download, newest first", async () => {
  renderApp(undefined, rows);
  const options = await screen.findAllByRole("option");
  expect(options.map((o) => within(o).getByTestId("name").textContent)).toEqual([
    "movie.mkv",
    "paused.iso",
    "done.zip",
  ]);
  expect(screen.getByRole("heading", { name: "Muzn Download Manager" })).toBeInTheDocument();
});

test("the filter rail counts and filters", async () => {
  const { user } = renderApp(undefined, rows);
  await screen.findAllByRole("option");
  await user.click(screen.getByRole("button", { name: /Completed 1/ }));
  expect(screen.getAllByRole("option")).toHaveLength(1);
  await user.click(screen.getByRole("button", { name: /Downloading 2/ }));
  expect(screen.getAllByRole("option")).toHaveLength(2);
});

test("select a row, then pause and resume it from the toolbar and with Space", async () => {
  const { user, fake } = renderApp(undefined, rows);
  await user.click(await screen.findByText("movie.mkv"));
  expect(screen.getByRole("option", { name: /movie\.mkv/ })).toHaveAttribute("aria-selected", "true");
  await user.click(within(screen.getByRole("toolbar", { name: "Actions" })).getByRole("button", { name: "Pause" }));
  expect(fake.calls).toContain("pause d");
  await within(screen.getByRole("option", { name: /movie\.mkv/ })).findByText("Paused");
  (document.activeElement as HTMLElement | null)?.blur();
  await user.keyboard(" ");
  expect(fake.calls).toContain("resume d");
});

test("Delete asks first, and can also delete a finished file", async () => {
  const { user, fake } = renderApp(undefined, rows);
  await user.click(await screen.findByText("done.zip"));
  (document.activeElement as HTMLElement | null)?.blur();
  await user.keyboard("{Delete}");
  const dialog = await screen.findByRole("dialog", { name: "Remove download" });
  await user.click(within(dialog).getByRole("checkbox", { name: "Also delete the file from disk" }));
  await user.click(within(dialog).getByRole("button", { name: "Remove" }));
  expect(fake.calls).toContain("remove c true");
  expect(screen.queryByText("done.zip")).not.toBeInTheDocument();
});

test("Escape keeps the download", async () => {
  const { user, fake } = renderApp(undefined, rows);
  await user.click(await screen.findByText("paused.iso"));
  (document.activeElement as HTMLElement | null)?.blur();
  await user.keyboard("{Delete}");
  await screen.findByRole("dialog");
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(fake.calls.some((c) => c.startsWith("remove"))).toBe(false);
});

test("live progress moves the bar and a notice becomes a toast", async () => {
  const { fake } = renderApp(undefined, rows);
  await screen.findAllByRole("option");
  act(() => {
    fake.emit({
      type: "progress",
      id: "d",
      total: 1000,
      downloaded: 250,
      speedBps: 2048,
      etaSecs: 7,
      segments: [
        { start: 0, end: 499, downloaded: 250 },
        { start: 500, end: 999, downloaded: 0 },
      ],
    });
    fake.emit({ type: "notice", id: "d", message: "Starting over from the beginning" });
  });
  const row = screen.getByRole("option", { name: /movie\.mkv/ });
  expect(within(row).getByRole("progressbar")).toHaveAttribute("aria-valuenow", "25");
  expect(within(row).getByText("2.00 KB/s")).toBeInTheDocument();
  expect(await screen.findByText("Starting over from the beginning")).toBeInTheDocument();
});

test("pause all and resume all reach the app", async () => {
  const { user, fake } = renderApp(undefined, rows);
  await screen.findAllByRole("option");
  await user.click(screen.getByRole("button", { name: "Pause all" }));
  await user.click(screen.getByRole("button", { name: "Resume all" }));
  expect(fake.calls).toEqual(expect.arrayContaining(["pauseAll", "resumeAll"]));
});

test("a resync reloads the list", async () => {
  const { fake } = renderApp(undefined, rows);
  await screen.findAllByRole("option");
  fake.rows.set("n", fakeRow({ id: "n", filename: "new.bin", createdAt: 9 }));
  act(() => fake.resync());
  expect(await screen.findByText("new.bin")).toBeInTheDocument();
});

test("arrow keys scroll the moving selection into view", async () => {
  // 40 rows is well past one 600px screen (≈12 rows at 48px) plus the
  // virtualizer's 8-row overscan on each side.
  const many = Array.from({ length: 40 }, (_, i) =>
    fakeRow({ id: `r${i}`, filename: `f${i}.bin`, createdAt: i }),
  );
  const { user } = renderApp(undefined, many);
  await screen.findAllByRole("option");
  // Newest first: r39 (createdAt 39) is the topmost row.
  await user.click(screen.getByText("f39.bin"));
  (document.activeElement as HTMLElement | null)?.blur();
  for (let i = 0; i < 30; i++) {
    await user.keyboard("{ArrowDown}");
  }
  // 30 steps down from the top lands on r9 (createdAt 39 - 30), far past
  // the first screen — it must still be selected AND actually rendered.
  const selected = screen.getByRole("option", { name: "f9.bin" });
  expect(selected).toHaveAttribute("aria-selected", "true");
});

// Final review I1 (2026-09-19): only a change of the selection scrolls the
// list; another row's status update must not yank it back.
test("an update to another row leaves the scroll position alone", async () => {
  const many = Array.from({ length: 40 }, (_, i) =>
    fakeRow({ id: `r${i}`, filename: `f${i}.bin`, createdAt: i, updatedAt: 1 }),
  );
  const { user, fake } = renderApp(undefined, many);
  await screen.findAllByRole("option");
  await user.click(screen.getByText("f39.bin")); // the topmost row
  const list = screen.getByRole("listbox", { name: "Downloads" });
  act(() => {
    list.scrollTop = 1200;
    list.dispatchEvent(new Event("scroll"));
  });
  act(() => fake.emit({ type: "updated", download: { ...many[5]!, status: "PAUSED", updatedAt: 2 } }));
  act(() => fake.emit({ type: "updated", download: { ...many[39]!, status: "PROBING", updatedAt: 2 } }));
  expect(list.scrollTop).toBe(1200);
});

// Final review M1 (2026-09-19): assistive tech follows the selection.
test("the listbox points aria-activedescendant at the selected row", async () => {
  const { user } = renderApp(undefined, rows);
  const list = await screen.findByRole("listbox", { name: "Downloads" });
  expect(list).not.toHaveAttribute("aria-activedescendant");
  await user.click(screen.getByText("movie.mkv"));
  const first = screen.getByRole("option", { name: "movie.mkv" });
  expect(first.id).not.toBe("");
  expect(list).toHaveAttribute("aria-activedescendant", first.id);
  (document.activeElement as HTMLElement | null)?.blur();
  await user.keyboard("{ArrowDown}");
  expect(list).toHaveAttribute("aria-activedescendant", screen.getByRole("option", { name: "paused.iso" }).id);
});

test("the empty message is not an item of the listbox (final review M1)", async () => {
  renderApp(undefined, []);
  const empty = await screen.findByText("No downloads here. Press Ctrl+N to add one.");
  expect(within(screen.getByRole("listbox", { name: "Downloads" })).queryByText(/No downloads here/)).toBeNull();
  expect(empty).toBeInTheDocument();
});

// Final review M4 (2026-09-19): focus starts on "Keep", so Enter is safe.
test("Enter in a remove confirmation keeps the download", async () => {
  const { user, fake } = renderApp(undefined, rows);
  await user.click(await screen.findByText("paused.iso"));
  (document.activeElement as HTMLElement | null)?.blur();
  await user.keyboard("{Delete}");
  const dialog = await screen.findByRole("dialog", { name: "Remove download" });
  expect(within(dialog).getByRole("button", { name: "Keep" })).toHaveFocus();
  await user.keyboard("{Enter}");
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(fake.calls.some((c) => c.startsWith("remove"))).toBe(false);
  expect(screen.getByRole("option", { name: "paused.iso" })).toBeInTheDocument();
});

// Final review M5 (2026-09-19): a filter that hides the selection clears it.
test("switching to a filter that hides the selected row clears the selection", async () => {
  const { user, store } = renderApp(undefined, rows);
  await user.click(await screen.findByText("movie.mkv"));
  await user.click(screen.getByRole("button", { name: /Completed 1/ }));
  expect(store.getState().selected).toBeNull();
  expect(screen.getByText("Select a download to see its details.")).toBeInTheDocument();
});

// Final review I2 (2026-09-19): each reload is numbered; a slower, older reply loses.
test("a late reply to an older list request is ignored", async () => {
  const fake = createFakeBackend({ rows });
  const replies: ((r: DownloadRow[]) => void)[] = [];
  fake.list = () => new Promise<DownloadRow[]>((resolve) => replies.push(resolve));
  renderApp(fake);
  await waitFor(() => expect(replies).toHaveLength(1));
  act(() => fake.resync());
  expect(replies).toHaveLength(2);
  const fresh = [...rows, fakeRow({ id: "n", filename: "new.bin", createdAt: 9 })];
  await act(async () => replies[1]!(fresh));
  expect(await screen.findByText("new.bin")).toBeInTheDocument();
  await act(async () => replies[0]!(rows)); // the first request answers last, without "n"
  expect(screen.getByText("new.bin")).toBeInTheDocument();
});
