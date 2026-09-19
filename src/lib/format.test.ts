import { expect, test } from "vitest";
import { formatBytes, formatEta, formatSpeed, percent } from "./format";

test("bytes read like a download manager's", () => {
  expect(formatBytes(null)).toBe("—");
  expect(formatBytes(0)).toBe("0 B");
  expect(formatBytes(1023)).toBe("1023 B");
  expect(formatBytes(1536)).toBe("1.50 KB");
  expect(formatBytes(10 * 1024 * 1024)).toBe("10.0 MB");
  expect(formatBytes(500 * 1024 ** 3)).toBe("500 GB");
});

test("speed and time left", () => {
  expect(formatSpeed(0)).toBe("—");
  expect(formatSpeed(2 * 1024 * 1024)).toBe("2.00 MB/s");
  expect(formatEta(null)).toBe("—");
  expect(formatEta(45)).toBe("45s");
  expect(formatEta(125)).toBe("2m 05s");
  expect(formatEta(3725)).toBe("1h 02m");
});

test("percent never passes 100 and is unknown without a size", () => {
  expect(percent(50, 200)).toBe(25);
  expect(percent(1, 3)).toBe(33);
  expect(percent(10, 5)).toBe(100);
  expect(percent(5, null)).toBeNull();
  expect(percent(5, 0)).toBeNull();
});
