import { expect, test } from "vitest";
import { fakeRow } from "../api/fake";
import { canCancel, canPause, canRestart, canResume, displayName, savedPath } from "./actions";

test("which action fits which status", () => {
  const s = fakeRow;
  expect(canPause(s({ status: "DOWNLOADING" }))).toBe(true);
  expect(canPause(s({ status: "QUEUED" }))).toBe(true);
  expect(canPause(s({ status: "PAUSED" }))).toBe(false);
  expect(canResume(s({ status: "PAUSED" }))).toBe(true);
  expect(canResume(s({ status: "FAILED" }))).toBe(true);
  expect(canResume(s({ status: "COMPLETED" }))).toBe(false);
  expect(canRestart(s({ status: "FAILED" }))).toBe(true);
  expect(canRestart(s({ status: "DOWNLOADING" }))).toBe(false);
  expect(canRestart(s({ status: "COMPLETED" }))).toBe(false);
  expect(canCancel(s({ status: "PAUSED" }))).toBe(true);
  expect(canCancel(s({ status: "COMPLETED" }))).toBe(false);
  expect(canCancel(s({ status: "CANCELLED" }))).toBe(false);
});

test("a SOURCE_CHANGED row cannot resume, only restart (final review M3)", () => {
  const changed = fakeRow({ status: "FAILED", errorCode: "SOURCE_CHANGED" });
  expect(canResume(changed)).toBe(false);
  expect(canRestart(changed)).toBe(true);
  expect(canResume(fakeRow({ status: "FAILED", errorCode: "NETWORK" }))).toBe(true);
});

test("a row's name and where it is saved", () => {
  expect(displayName(fakeRow({ filename: "a.zip" }))).toBe("a.zip");
  expect(displayName(fakeRow({ filename: null, url: "https://x.com/dir/b.iso?x=1" }))).toBe("b.iso");
  expect(displayName(fakeRow({ filename: null, url: "https://x.com/" }))).toBe("https://x.com/");
  expect(savedPath(fakeRow({ dir: "/dl", filename: "a.zip" }))).toBe("/dl/a.zip");
  expect(savedPath(fakeRow({ dir: "C:\\Users\\u\\Downloads", filename: "a.zip" }))).toBe(
    "C:\\Users\\u\\Downloads\\a.zip",
  );
  expect(savedPath(fakeRow({ dir: "/dl/", filename: "a.zip" }))).toBe("/dl/a.zip");
  expect(savedPath(fakeRow({ dir: "/dl", filename: null }))).toBe("/dl");
});
