import { screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { createFakeBackend, fakeSettings } from "../api/fake";
import { renderApp } from "../test/render";
import { clampSettings } from "./SettingsDialog";

async function openSettings(user: ReturnType<typeof renderApp>["user"]) {
  await user.click(screen.getByRole("button", { name: "Settings" }));
  return screen.findByRole("dialog", { name: "Settings" });
}

test("numbers are kept in range and a blank user agent means the default", () => {
  const s = clampSettings(fakeSettings({ maxConnections: 99, maxParallel: 0, userAgent: "  " }));
  expect(s).toMatchObject({ maxConnections: 32, maxParallel: 1, userAgent: null });
  expect(clampSettings(fakeSettings({ proxy: { mode: "manual", url: " http://p:8080 " } })).proxy).toEqual({
    mode: "manual",
    url: "http://p:8080",
  });
});

test("the dialog shows the current settings and saves the changes", async () => {
  const fake = createFakeBackend({ settings: { maxConnections: 8, maxParallel: 3 } });
  const { user } = renderApp(fake);
  const dialog = await openSettings(user);
  const conns = await within(dialog).findByLabelText("Connections per download");
  expect(conns).toHaveValue(8);
  await user.clear(conns);
  await user.type(conns, "16");
  await user.click(within(dialog).getByLabelText("Close button hides the app to the tray"));
  await user.click(within(dialog).getByRole("button", { name: "Save" }));
  expect(fake.settings).toMatchObject({ maxConnections: 16, closeToTray: false });
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(await screen.findByText("Settings saved")).toBeInTheDocument();
});

test("a manual proxy needs its URL", async () => {
  const { user } = renderApp();
  const dialog = await openSettings(user);
  await user.selectOptions(await within(dialog).findByLabelText("Proxy"), "manual");
  expect(within(dialog).getByRole("button", { name: "Save" })).toBeDisabled();
  await user.type(within(dialog).getByLabelText("Proxy URL"), "http://127.0.0.1:8080");
  expect(within(dialog).getByRole("button", { name: "Save" })).toBeEnabled();
});

test("launch at startup is the OS's own switch", async () => {
  const fake = createFakeBackend();
  const { user } = renderApp(fake);
  const dialog = await openSettings(user);
  const box = await within(dialog).findByLabelText("Start with the computer");
  expect(box).not.toBeChecked();
  await user.click(box);
  await user.click(within(dialog).getByRole("button", { name: "Save" }));
  expect(fake.calls).toContain("setAutostart true");
});

test("a refused save keeps the dialog open and says why", async () => {
  const fake = createFakeBackend();
  fake.setSettings = async () => {
    throw { code: "SETTINGS", message: "proxy URL: relative URL without a base" };
  };
  const { user } = renderApp(fake);
  const dialog = await openSettings(user);
  await within(dialog).findByLabelText("Connections per download");
  await user.click(within(dialog).getByRole("button", { name: "Save" }));
  expect(await screen.findByText(/proxy URL: relative URL without a base/)).toBeInTheDocument();
  expect(screen.getByRole("dialog", { name: "Settings" })).toBeInTheDocument();
});

test("the download folder is chosen through the OS dialog", async () => {
  const fake = createFakeBackend();
  const { user } = renderApp(fake);
  const dialog = await openSettings(user);
  await within(dialog).findByLabelText("Download folder");
  await user.click(within(dialog).getByRole("button", { name: "Browse…" }));
  expect(within(dialog).getByLabelText("Download folder")).toHaveValue("/picked");
});

test("a cleared number becomes the minimum when saved (final review M8)", () => {
  const s = clampSettings(fakeSettings({ maxConnections: Number.NaN, maxParallel: Number.NaN }));
  expect(s).toMatchObject({ maxConnections: 1, maxParallel: 1 });
});

test("a cleared number field shows empty, without React's NaN warning (final review M8)", async () => {
  const errors = vi.spyOn(console, "error").mockImplementation(() => {});
  try {
    const { user } = renderApp();
    const dialog = await openSettings(user);
    const conns = await within(dialog).findByLabelText("Connections per download");
    await user.clear(conns);
    expect(conns).toHaveValue(null);
    expect(conns.getAttribute("value")).not.toBe("NaN");
    expect(errors.mock.calls.some((c) => String(c[0]).includes("NaN"))).toBe(false);
  } finally {
    errors.mockRestore();
  }
});

// Final review M7 (2026-09-19): the add dialog's remembered folder must not
// outlive a download folder chosen in Settings.
test("saving a new download folder forgets the add dialog's last folder", async () => {
  localStorage.setItem("mdm.lastDir", "/elsewhere");
  const fake = createFakeBackend();
  const { user } = renderApp(fake);
  const dialog = await openSettings(user);
  await within(dialog).findByLabelText("Download folder");
  await user.click(within(dialog).getByRole("button", { name: "Browse…" }));
  await user.click(within(dialog).getByRole("button", { name: "Save" }));
  expect(fake.settings.downloadDir).toBe("/picked");
  expect(localStorage.getItem("mdm.lastDir")).toBeNull();
});

test("saving without changing the folder keeps the add dialog's last folder", async () => {
  localStorage.setItem("mdm.lastDir", "/elsewhere");
  const { user } = renderApp();
  const dialog = await openSettings(user);
  await within(dialog).findByLabelText("Download folder");
  await user.click(within(dialog).getByRole("button", { name: "Save" }));
  expect(localStorage.getItem("mdm.lastDir")).toBe("/elsewhere");
  localStorage.removeItem("mdm.lastDir");
});
