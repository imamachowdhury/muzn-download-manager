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
// clientHeight/clientWidth (the scroll container's own viewport) and
// scrollHeight/scrollWidth (its content size, used to clamp scrollToIndex's
// target offset — see getMaxScrollOffset() in @tanstack/virtual-core) get
// the same treatment: without them jsdom's default 0 × 0 clamps every
// scrollToIndex() call down to offset 0, no matter the index.
Object.defineProperties(HTMLElement.prototype, {
  offsetHeight: { configurable: true, get: () => 600 },
  offsetWidth: { configurable: true, get: () => 1000 },
  clientHeight: { configurable: true, get: () => 600 },
  clientWidth: { configurable: true, get: () => 1000 },
  scrollHeight: { configurable: true, get: () => 100_000 },
  scrollWidth: { configurable: true, get: () => 100_000 },
});

// jsdom has no Element.scrollTo: the virtual list's scrollToIndex() (used to
// keep the keyboard-moved selection in view) writes scrollTop/scrollLeft
// through it and relies on the resulting "scroll" event to tell the
// virtualizer where it ended up. Give it a minimal real implementation
// instead of leaving every scrollToIndex() call a silent no-op.
if (!("scrollTo" in Element.prototype)) {
  Object.defineProperty(Element.prototype, "scrollTo", {
    configurable: true,
    writable: true,
    value: function scrollTo(this: Element, options?: ScrollToOptions | number, y?: number) {
      if (typeof options === "object" && options !== null) {
        if (typeof options.top === "number") this.scrollTop = options.top;
        if (typeof options.left === "number") this.scrollLeft = options.left;
      } else if (typeof options === "number" && typeof y === "number") {
        this.scrollLeft = options;
        this.scrollTop = y;
      }
      this.dispatchEvent(new Event("scroll"));
    },
  });
}
