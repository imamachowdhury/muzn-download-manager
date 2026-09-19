import { act, screen, within } from "@testing-library/react";
import { expect, test } from "vitest";
import { fakeRow } from "../api/fake";
import { renderApp } from "../test/render";

const panel = () => screen.getByRole("region", { name: "Download details" });

test("nothing selected: the panel says how to see details", async () => {
  renderApp(undefined, [fakeRow({ id: "a" })]);
  expect(await screen.findByText("Select a download to see its details.")).toBeInTheDocument();
});

test("a completed download opens and shows its folder", async () => {
  const { user, fake } = renderApp(undefined, [
    fakeRow({ id: "c", filename: "done.zip", dir: "/dl", status: "COMPLETED", size: 2048, downloaded: 2048 }),
  ]);
  await user.click(await screen.findByText("done.zip", { selector: "[data-testid=name]" }));
  expect(within(panel()).getByText("/dl/done.zip")).toBeInTheDocument();
  await user.click(within(panel()).getByRole("button", { name: "Open" }));
  await user.click(within(panel()).getByRole("button", { name: "Show in folder" }));
  expect(fake.calls).toEqual(expect.arrayContaining(["open c", "showInFolder c"]));
});

test("double-clicking a completed row opens it", async () => {
  const { user, fake } = renderApp(undefined, [fakeRow({ id: "c", filename: "done.zip", status: "COMPLETED" })]);
  await user.dblClick(await screen.findByText("done.zip", { selector: "[data-testid=name]" }));
  expect(fake.calls).toContain("open c");
});

test("a changed file explains itself and restarts on request", async () => {
  const { user, fake } = renderApp(undefined, [
    fakeRow({ id: "f", filename: "setup.exe", status: "FAILED", errorCode: "SOURCE_CHANGED", errorMessage: "etag" }),
  ]);
  await user.click(await screen.findByText("setup.exe", { selector: "[data-testid=name]" }));
  expect(within(panel()).getByRole("alert")).toHaveTextContent(
    "The file on the server changed. Restart from the beginning?",
  );
  await user.click(within(panel()).getByRole("button", { name: "Restart from the beginning" }));
  expect(fake.calls).toContain("restart f");
});

test("a paused download shows its saved segments; a running one its live segments", async () => {
  const { user, fake } = renderApp(undefined, [
    fakeRow({ id: "p", filename: "paused.iso", status: "PAUSED", size: 1000, downloaded: 400 }),
    fakeRow({ id: "d", filename: "movie.mkv", status: "DOWNLOADING", size: 1000, createdAt: 5 }),
  ]);
  await user.click(await screen.findByText("paused.iso", { selector: "[data-testid=name]" }));
  const saved = await within(panel()).findAllByRole("row");
  expect(saved).toHaveLength(2); // header + one saved segment (the fake saves one)
  expect(within(saved[1]!).getByText("40%")).toBeInTheDocument();

  await user.click(screen.getByText("movie.mkv", { selector: "[data-testid=name]" }));
  act(() =>
    fake.emit({
      type: "progress",
      id: "d",
      total: 1000,
      downloaded: 300,
      speedBps: 1024,
      etaSecs: 3,
      segments: [
        { start: 0, end: 499, downloaded: 250 },
        { start: 500, end: 999, downloaded: 50 },
      ],
    }),
  );
  const rows = within(panel()).getAllByRole("row");
  expect(rows).toHaveLength(3);
  expect(within(rows[1]!).getByText("50%")).toBeInTheDocument();
  expect(within(rows[2]!).getByText("10%")).toBeInTheDocument();
});

test("cancel asks first", async () => {
  const { user, fake } = renderApp(undefined, [fakeRow({ id: "p", filename: "paused.iso", status: "PAUSED" })]);
  await user.click(await screen.findByText("paused.iso", { selector: "[data-testid=name]" }));
  await user.click(within(panel()).getByRole("button", { name: "Cancel" }));
  const dialog = await screen.findByRole("dialog", { name: "Cancel download" });
  await user.click(within(dialog).getByRole("button", { name: "Cancel download" }));
  expect(fake.calls).toContain("cancel p");
});
