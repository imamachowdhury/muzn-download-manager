import { createContext, useContext, useEffect, useMemo, type ReactNode } from "react";
import { useStore } from "zustand";
import type { Backend } from "../api/backend";
import { describeError } from "../lib/errors";
import type { DownloadsState, DownloadsStore } from "./downloads";

interface AppCtx {
  backend: Backend;
  store: DownloadsStore;
}

const Ctx = createContext<AppCtx | null>(null);

export function AppProvider(props: { backend: Backend; store: DownloadsStore; children: ReactNode }) {
  const { backend, store, children } = props;
  const value = useMemo(() => ({ backend, store }), [backend, store]);
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

function useCtx(): AppCtx {
  const ctx = useContext(Ctx);
  if (!ctx) throw new Error("AppProvider is missing");
  return ctx;
}

export const useBackend = (): Backend => useCtx().backend;
export const useDownloadsStore = (): DownloadsStore => useCtx().store;

/** Select from the downloads state. The selector must return a stable value (no new arrays). */
export function useDownloads<T>(selector: (s: DownloadsState) => T): T {
  return useStore(useCtx().store, selector);
}

/**
 * Keep the store in step with the manager: subscribe FIRST, then load the
 * list, so nothing that happens in between is missed. `onNotice` must be a
 * stable function (a module-level one such as `toast`).
 */
export function useManagerSync(onNotice: (message: string) => void): void {
  const { backend, store } = useCtx();
  useEffect(() => {
    let cancelled = false;
    let off: (() => void) | undefined;
    const reload = () =>
      backend
        .list()
        .then((rows) => {
          if (!cancelled) store.getState().load(rows);
        })
        .catch((e: unknown) => onNotice(describeError(e)));
    backend
      .subscribe(
        (e) => {
          store.getState().apply(e);
          if (e.type === "notice") onNotice(e.message);
        },
        () => void reload(),
      )
      .then((unsubscribe) => {
        if (cancelled) unsubscribe();
        else {
          off = unsubscribe;
          void reload();
        }
      })
      .catch((e: unknown) => onNotice(describeError(e)));
    return () => {
      cancelled = true;
      off?.();
    };
  }, [backend, store, onNotice]);
}
