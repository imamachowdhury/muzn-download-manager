import { expect, test } from "vitest";
import css from "./styles.css?raw";

// Final review I4 (2026-09-19): every text colour on its background meets
// WCAG AA (4.5:1) in the light AND the dark theme. Read from the real
// stylesheet, so a token change that breaks contrast fails here.

function tokens(block: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const m of block.matchAll(/--([\w-]+):\s*(#[0-9a-fA-F]{6})\s*;/g)) out[m[1]!] = m[2]!.toLowerCase();
  return out;
}

const light = tokens(css.slice(css.indexOf(":root {"), css.indexOf("@media (prefers-color-scheme: dark)")));
const darkStart = css.indexOf("@media (prefers-color-scheme: dark)");
const dark = { ...light, ...tokens(css.slice(darkStart, css.indexOf("\n}\n", darkStart))) };

function luminance(hex: string): number {
  const [r, g, b] = [1, 3, 5].map((i) => {
    const v = parseInt(hex.slice(i, i + 2), 16) / 255;
    return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r! + 0.7152 * g! + 0.0722 * b!;
}

function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi! + 0.05) / (lo! + 0.05);
}

// [what, text token, background token]
const PAIRS: [string, string, string][] = [
  ["filled primary button", "accent-text", "accent-strong"],
  ["filled danger button", "accent-text", "danger"],
  ["link button (detail panel)", "accent-strong", "surface"],
  ["Completed badge", "ok", "surface-2"],
  ["Paused badge", "warn", "surface-2"],
  ["Failed badge", "danger", "surface-2"],
  ["error line in the detail panel / dialogs", "danger", "surface"],
  ["muted text on a badge", "text-muted", "surface-2"],
  ["muted text on the page", "text-muted", "bg"],
  ["running badge", "text", "accent-soft"],
];

test.each(PAIRS)("%s passes 4.5:1 in both themes", (_what, fg, bg) => {
  for (const theme of [light, dark]) {
    expect(theme[fg], fg).toBeDefined();
    expect(theme[bg], bg).toBeDefined();
    expect(contrast(theme[fg]!, theme[bg]!)).toBeGreaterThanOrEqual(4.5);
  }
});

test("the accent itself stays Muzn teal for the focus ring and the segments", () => {
  expect(light.accent).toBe("#0f9b8e");
  expect(dark.accent).toBe("#2cc4b4");
});
