import { useEffect, useId, useState, type FormEvent } from "react";
import type { ProxySetting, Settings } from "../api/types";
import { describeError } from "../lib/errors";
import { forgetDir } from "../lib/lastDir";
import { useBackend } from "../state/context";
import { Dialog } from "../ui/Dialog";
import { toast } from "../ui/toast";

const clamp = (n: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, Math.round(n) || lo));

/** A cleared number field holds NaN; the input shows it as empty, never "NaN" (final review M8). */
const numberValue = (n: number): number | "" => (Number.isNaN(n) ? "" : n);

/** UX clean-up before saving; the core validates again (its checks are the real ones). */
export function clampSettings(s: Settings): Settings {
  const proxy: ProxySetting = s.proxy.mode === "manual" ? { mode: "manual", url: s.proxy.url.trim() } : s.proxy;
  return {
    ...s,
    maxConnections: clamp(s.maxConnections, 1, 32),
    maxParallel: clamp(s.maxParallel, 1, 10),
    userAgent: s.userAgent?.trim() ? s.userAgent.trim() : null,
    proxy,
  };
}

export function SettingsDialog({ onClose }: { onClose(): void }) {
  const backend = useBackend();
  const id = useId();
  const [form, setForm] = useState<Settings | null>(null);
  const [initialDir, setInitialDir] = useState<string | null>(null);
  const [autostart, setAutostart] = useState<boolean | null>(null);
  const [initialAutostart, setInitialAutostart] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let on = true;
    backend
      .getSettings()
      .then((s) => {
        if (!on) return;
        setForm(s);
        setInitialDir(s.downloadDir);
      })
      .catch((e: unknown) => toast(describeError(e), "error"));
    backend
      .autostartEnabled()
      .then((a) => {
        if (!on) return;
        setAutostart(a);
        setInitialAutostart(a);
      })
      .catch(() => {});
    return () => {
      on = false;
    };
  }, [backend]);

  const set = <K extends keyof Settings>(key: K, value: Settings[K]) => setForm((f) => (f ? { ...f, [key]: value } : f));
  const proxyUrlMissing = form?.proxy.mode === "manual" && !form.proxy.url.trim();

  async function save(e: FormEvent) {
    e.preventDefault();
    if (!form || proxyUrlMissing) return;
    setBusy(true);
    try {
      const saved = await backend.setSettings(clampSettings(form));
      // A newly chosen download folder beats the add dialog's remembered one.
      if (saved.downloadDir !== initialDir) forgetDir();
      if (autostart !== null && autostart !== initialAutostart) await backend.setAutostart(autostart);
      toast("Settings saved", "ok");
      onClose();
    } catch (err) {
      toast(describeError(err), "error");
      setBusy(false);
    }
  }

  async function browse() {
    if (!form) return;
    try {
      const picked = await backend.pickFolder(form.downloadDir || null);
      if (picked) set("downloadDir", picked);
    } catch (err) {
      toast(describeError(err), "error");
    }
  }

  return (
    <Dialog
      title="Settings"
      onClose={onClose}
      width={560}
      footer={
        <>
          <button type="button" className="secondary-button" onClick={onClose}>
            Close
          </button>
          <button type="submit" form="settings-form" className="primary-button" disabled={!form || busy || proxyUrlMissing}>
            Save
          </button>
        </>
      }
    >
      {!form ? (
        <p className="hint">Loading…</p>
      ) : (
        <form id="settings-form" onSubmit={save}>
          <div className="field">
            <label htmlFor={`${id}-dir`}>
              <span>Download folder</span>
            </label>
            <div className="field-row">
              <input id={`${id}-dir`} value={form.downloadDir} readOnly />
              <button type="button" className="secondary-button" onClick={() => void browse()}>
                Browse…
              </button>
            </div>
          </div>
          <div className="field-row">
            <label className="field" htmlFor={`${id}-conns`}>
              <span>Connections per download</span>
              <input
                id={`${id}-conns`}
                type="number"
                min={1}
                max={32}
                value={numberValue(form.maxConnections)}
                onChange={(e) => set("maxConnections", e.target.valueAsNumber)}
              />
            </label>
            <label className="field" htmlFor={`${id}-par`}>
              <span>Downloads at once</span>
              <input
                id={`${id}-par`}
                type="number"
                min={1}
                max={10}
                value={numberValue(form.maxParallel)}
                onChange={(e) => set("maxParallel", e.target.valueAsNumber)}
              />
            </label>
          </div>
          <label className="field" htmlFor={`${id}-ua`}>
            <span>User agent</span>
            <input
              id={`${id}-ua`}
              value={form.userAgent ?? ""}
              placeholder="Muzn Download Manager's own"
              onChange={(e) => set("userAgent", e.target.value)}
            />
          </label>
          <div className="field-row">
            <label className="field" htmlFor={`${id}-proxy`}>
              <span>Proxy</span>
              <select
                id={`${id}-proxy`}
                value={form.proxy.mode}
                onChange={(e) => {
                  const mode = e.target.value as ProxySetting["mode"];
                  set("proxy", mode === "manual" ? { mode, url: "" } : { mode });
                }}
              >
                <option value="system">The system's</option>
                <option value="none">None</option>
                <option value="manual">Manual</option>
              </select>
            </label>
            {form.proxy.mode === "manual" && (
              <label className="field" htmlFor={`${id}-purl`}>
                <span>Proxy URL</span>
                <input
                  id={`${id}-purl`}
                  value={form.proxy.url}
                  placeholder="http://127.0.0.1:8080"
                  onChange={(e) => set("proxy", { mode: "manual", url: e.target.value })}
                />
              </label>
            )}
          </div>
          <label className="check">
            <input type="checkbox" checked={form.closeToTray} onChange={(e) => set("closeToTray", e.target.checked)} />
            Close button hides the app to the tray
          </label>
          <label className="check">
            <input
              type="checkbox"
              checked={form.notifyOnComplete}
              onChange={(e) => set("notifyOnComplete", e.target.checked)}
            />
            Notify me when a download completes
          </label>
          {autostart !== null && (
            <label className="check">
              <input type="checkbox" checked={autostart} onChange={(e) => setAutostart(e.target.checked)} />
              Start with the computer
            </label>
          )}
        </form>
      )}
    </Dialog>
  );
}
