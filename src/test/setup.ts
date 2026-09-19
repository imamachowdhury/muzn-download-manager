import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";
import { clearToasts } from "../ui/toast";

afterEach(() => {
  cleanup();
  clearToasts();
});

// jsdom has no ResizeObserver; the virtual list only needs it to exist.
class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
globalThis.ResizeObserver ??= ResizeObserverStub as unknown as typeof ResizeObserver;

// jsdom does no layout: every element measures 0 × 0, so the virtual list
// would think its viewport is empty. Give elements a desktop-sized box.
Object.defineProperties(HTMLElement.prototype, {
  offsetHeight: { configurable: true, get: () => 600 },
  offsetWidth: { configurable: true, get: () => 1000 },
});
