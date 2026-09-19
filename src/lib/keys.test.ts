import { expect, test } from "vitest";
import { shortcutFor, step } from "./keys";

const key = (k: string, mods: Partial<{ ctrlKey: boolean; metaKey: boolean; altKey: boolean }> = {}) => ({
  key: k,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  ...mods,
});
const body = { tagName: "BODY" };

test("IDM-style shortcuts", () => {
  expect(shortcutFor(key("n", { ctrlKey: true }), body)).toBe("add");
  expect(shortcutFor(key("N", { metaKey: true }), body)).toBe("add");
  expect(shortcutFor(key(" "), body)).toBe("toggle");
  expect(shortcutFor(key("Delete"), body)).toBe("remove");
  expect(shortcutFor(key("ArrowDown"), body)).toBe("down");
  expect(shortcutFor(key("ArrowUp"), body)).toBe("up");
  expect(shortcutFor(key("x"), body)).toBeNull();
});

test("typing in a field or pressing a button is never a shortcut, except Ctrl+N", () => {
  for (const tagName of ["INPUT", "TEXTAREA", "SELECT", "BUTTON"]) {
    expect(shortcutFor(key(" "), { tagName })).toBeNull();
    expect(shortcutFor(key("Delete"), { tagName })).toBeNull();
  }
  expect(shortcutFor(key("n", { ctrlKey: true }), { tagName: "INPUT" })).toBe("add");
  expect(shortcutFor(key(" ", { ctrlKey: true }), body)).toBeNull();
});

test("arrow steps stay inside the list", () => {
  const list = [{ id: "a" }, { id: "b" }, { id: "c" }];
  expect(step(list, null, 1)).toBe("a");
  expect(step(list, "a", 1)).toBe("b");
  expect(step(list, "c", 1)).toBe("c");
  expect(step(list, "a", -1)).toBe("a");
  expect(step(list, "gone", 1)).toBe("a");
  expect(step([], null, 1)).toBeNull();
});
