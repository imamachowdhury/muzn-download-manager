import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { describeError } from "../lib/errors";

type Kind = "info" | "ok" | "error";
interface Toast {
  id: number;
  message: string;
  kind: Kind;
}

const toasts = createStore<{ items: Toast[] }>(() => ({ items: [] }));
let nextId = 1;

function dismiss(id: number) {
  toasts.setState((s) => ({ items: s.items.filter((t) => t.id !== id) }));
}

/** The one notice primitive (never alert()). At most four stay on screen. */
export function toast(message: string, kind: Kind = "info"): void {
  const id = nextId++;
  toasts.setState((s) => ({ items: [...s.items, { id, message, kind }].slice(-4) }));
  setTimeout(() => dismiss(id), kind === "error" ? 8000 : 4000);
}

/** Await a command; a failure becomes an error toast instead of an unhandled rejection. */
export async function attempt<T>(p: Promise<T>): Promise<T | undefined> {
  try {
    return await p;
  } catch (e) {
    toast(describeError(e), "error");
    return undefined;
  }
}

export function clearToasts(): void {
  toasts.setState({ items: [] });
}

export function ToastHost() {
  const items = useStore(toasts, (s) => s.items);
  return (
    <div className="toasts" role="status" aria-live="polite">
      {items.map((t) => (
        <div key={t.id} className={`toast toast-${t.kind}`}>
          <span>{t.message}</span>
          <button type="button" className="link-button" aria-label="Dismiss" onClick={() => dismiss(t.id)}>
            ×
          </button>
        </div>
      ))}
    </div>
  );
}
