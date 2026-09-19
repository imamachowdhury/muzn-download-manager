import { act, screen, within } from "@testing-library/react";
import { expect, test } from "vitest";
import { fakeRow } from "./api/fake";
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
  await user.click(screen.getByRole("button", { name: "Pause" }));
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
