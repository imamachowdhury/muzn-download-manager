import { expect, test } from "vitest";
import type { ManagerEvent } from "./types";
import { createFakeBackend, fakeRow } from "./fake";

test("the fake announces what it does, like the real manager", async () => {
  const fake = createFakeBackend({ rows: [fakeRow({ id: "a", status: "DOWNLOADING" })] });
  const seen: ManagerEvent[] = [];
  const off = await fake.subscribe((e) => seen.push(e), () => {});
  await fake.pause("a");
  const added = await fake.add({ url: "https://x/y.zip", startPaused: true });
  await fake.remove("a", false);
  off();
  await fake.resume(added.id);
  expect(seen.map((e) => e.type)).toEqual(["updated", "added", "removed"]);
  expect(added.status).toBe("PAUSED");
  expect(fake.calls).toEqual(["pause a", `add https://x/y.zip`, "remove a false", `resume ${added.id}`]);
});

test("probe answers the configured preview or rejects with it", async () => {
  const fake = createFakeBackend();
  expect((await fake.probe("https://x/y.zip")).filename).toBe("y.zip");
  fake.probeResult = { code: "HTTP_STATUS", message: "404" };
  await expect(fake.probe("https://x/z")).rejects.toEqual({ code: "HTTP_STATUS", message: "404" });
});
