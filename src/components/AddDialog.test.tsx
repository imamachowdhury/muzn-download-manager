import { screen, within } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { createFakeBackend } from "../api/fake";
import { renderApp } from "../test/render";

afterEach(() => localStorage.clear());

async function openAdd(user: ReturnType<typeof renderApp>["user"]) {
  await user.keyboard("{Control>}n{/Control}");
  return screen.findByRole("dialog", { name: "Add a download" });
}

test("a link on the clipboard fills the URL and the probe previews it", async () => {
  const fake = createFakeBackend();
  fake.clipboard = "https://example.com/y.zip";
  const { user } = renderApp(fake);
  const dialog = await openAdd(user);
  expect(await within(dialog).findByDisplayValue("https://example.com/y.zip")).toBeInTheDocument();
  expect(await within(dialog).findByText("5.00 MB · resumable")).toBeInTheDocument();
  expect(within(dialog).getByLabelText("File name")).toHaveValue("y.zip");
  expect(within(dialog).getByLabelText("Save to")).toHaveValue("/downloads");
});

test("a refused probe says why but still lets the user add", async () => {
  const fake = createFakeBackend();
  fake.probeResult = { code: "HTTP_STATUS", message: "404 Not Found" };
  const { user } = renderApp(fake);
  const dialog = await openAdd(user);
  await user.type(within(dialog).getByLabelText("URL"), "https://example.com/gone.zip");
  expect(await within(dialog).findByText(/link may have expired/)).toBeInTheDocument();
  expect(within(dialog).getByRole("button", { name: "Start download" })).toBeEnabled();
});

test("not a web link: no probe, no add", async () => {
  const fake = createFakeBackend();
  const { user } = renderApp(fake);
  const dialog = await openAdd(user);
  await user.type(within(dialog).getByLabelText("URL"), "ftp://example.com/x");
  expect(within(dialog).getByText("Paste an http or https link.")).toBeInTheDocument();
  expect(within(dialog).getByRole("button", { name: "Start download" })).toBeDisabled();
  await new Promise((r) => setTimeout(r, 500));
  expect(fake.calls.some((c) => c.startsWith("probe"))).toBe(false);
});

test("start: the typed name and the chosen folder are sent, the row is selected, the folder remembered", async () => {
  const fake = createFakeBackend();
  const { user, store } = renderApp(fake);
  const dialog = await openAdd(user);
  await user.type(within(dialog).getByLabelText("URL"), "https://example.com/y.zip");
  await within(dialog).findByText("5.00 MB · resumable");
  const name = within(dialog).getByLabelText("File name");
  await user.clear(name);
  await user.type(name, "mine.zip");
  await user.click(within(dialog).getByRole("button", { name: "Browse…" }));
  expect(within(dialog).getByLabelText("Save to")).toHaveValue("/picked");
  await user.click(within(dialog).getByRole("button", { name: "Start download" }));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  const row = [...fake.rows.values()][0]!;
  expect(row).toMatchObject({ url: "https://example.com/y.zip", filename: "mine.zip", dir: "/picked", status: "QUEUED" });
  expect(store.getState().selected).toBe(row.id);
  expect(localStorage.getItem("mdm.lastDir")).toBe("/picked");
});

test("add paused, and an untouched name lets the server's name win", async () => {
  const fake = createFakeBackend();
  const { user } = renderApp(fake);
  const dialog = await openAdd(user);
  await user.type(within(dialog).getByLabelText("URL"), "https://example.com/y.zip");
  await within(dialog).findByText("5.00 MB · resumable");
  await user.click(within(dialog).getByRole("button", { name: "Add paused" }));
  const row = [...fake.rows.values()][0]!;
  expect(row.status).toBe("PAUSED");
  // fake.add names the row after the URL when no name is sent
  expect(row.filename).toBe("y.zip");
});

test("the last folder is offered next time", async () => {
  localStorage.setItem("mdm.lastDir", "/elsewhere");
  const { user } = renderApp();
  const dialog = await openAdd(user);
  expect(within(dialog).getByLabelText("Save to")).toHaveValue("/elsewhere");
});
