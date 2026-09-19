import { expect, test } from "vitest";
import { describeError, errorText, rowErrorText } from "./errors";
import { fakeRow } from "../api/fake";

test("known codes read as plain English", () => {
  expect(errorText("SOURCE_CHANGED")).toBe(
    "The file on the server changed. Restart from the beginning?",
  );
  expect(errorText("DISK_FULL")).toContain("disk space");
});

test("an unknown code falls back to the detail, then a generic line", () => {
  expect(errorText("WHAT", "server said no")).toBe("server said no");
  expect(errorText(null)).toBe("Something went wrong.");
});

test("a command failure keeps its detail for the user", () => {
  expect(describeError({ code: "INVALID_URL", message: "ftp://x: unsupported scheme ftp" })).toBe(
    "That is not a valid http or https link. (ftp://x: unsupported scheme ftp)",
  );
  expect(describeError(new Error("boom"))).toBe("boom");
  expect(describeError("plain")).toBe("plain");
});

test("a row shows its error only when it has one", () => {
  expect(rowErrorText(fakeRow())).toBeNull();
  expect(
    rowErrorText(fakeRow({ status: "FAILED", errorCode: "NETWORK", errorMessage: "reset" })),
  ).toBe("Network problem. Check the connection and resume.");
});
