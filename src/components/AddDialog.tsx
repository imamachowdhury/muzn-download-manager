import { useEffect, useId, useState, type FormEvent, type ReactNode } from "react";
import type { ProbePreview } from "../api/types";
import { describeError } from "../lib/errors";
import { formatBytes } from "../lib/format";
import { lastDir, rememberDir } from "../lib/lastDir";
import { isWebUrl } from "../lib/url";
import { useBackend, useDownloadsStore } from "../state/context";
import { Dialog } from "../ui/Dialog";
import { toast } from "../ui/toast";

export const PROBE_DELAY_MS = 400;

type Probe =
  | { url: string; kind: "loading" }
  | { url: string; kind: "ok"; preview: ProbePreview }
  | { url: string; kind: "error"; message: string };

export function AddDialog({ onClose, initialUrl }: { onClose(): void; initialUrl?: string }) {
  const backend = useBackend();
  const store = useDownloadsStore();
  const ids = { url: useId(), name: useId(), dir: useId() };
  const [url, setUrl] = useState(initialUrl ?? "");
  const [name, setName] = useState("");
  const [nameTouched, setNameTouched] = useState(false);
  const [dir, setDir] = useState(() => lastDir() ?? "");
  const [probe, setProbe] = useState<Probe | null>(null);
  const [busy, setBusy] = useState(false);

  const target = url.trim();
  const valid = isWebUrl(target);
  // A probe answer belongs to the URL it was asked for; typing on makes it stale.
  const current = probe && probe.url === target ? probe : null;
  const preview = current?.kind === "ok" ? current.preview : null;
  const shownName = nameTouched ? name : (preview?.filename ?? "");

  useEffect(() => {
    let live = true;
    if (!lastDir()) {
      backend
        .getSettings()
        .then((s) => live && setDir((d) => d || s.downloadDir))
        .catch(() => {});
    }
    return () => {
      live = false;
    };
  }, [backend]);

  useEffect(() => {
    if (initialUrl) return;
    let live = true;
    backend
      .clipboardUrl()
      .then((link) => live && link && setUrl((u) => u || link))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [backend, initialUrl]);

  useEffect(() => {
    if (!isWebUrl(target)) return;
    const timer = setTimeout(() => {
      setProbe({ url: target, kind: "loading" });
      backend
        .probe(target)
        .then((p) => setProbe((cur) => (cur?.url === target ? { url: target, kind: "ok", preview: p } : cur)))
        .catch((e: unknown) =>
          setProbe((cur) => (cur?.url === target ? { url: target, kind: "error", message: describeError(e) } : cur)),
        );
    }, PROBE_DELAY_MS);
    return () => clearTimeout(timer);
  }, [backend, target]);

  async function submit(startPaused: boolean) {
    if (!valid || busy) return;
    setBusy(true);
    try {
      const typed = nameTouched ? name.trim() : "";
      const row = await backend.add({ url: target, dir: dir || null, filename: typed || null, startPaused });
      rememberDir(dir);
      store.getState().select(row.id);
      onClose();
    } catch (e) {
      toast(describeError(e), "error");
      setBusy(false);
    }
  }

  async function browse() {
    try {
      const picked = await backend.pickFolder(dir || null);
      if (picked) setDir(picked);
    } catch (e) {
      toast(describeError(e), "error");
    }
  }

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    void submit(false);
  }

  let hint: ReactNode = " ";
  if (target && !valid) hint = "Paste an http or https link.";
  else if (current?.kind === "loading") hint = "Checking the link…";
  else if (current?.kind === "error") hint = <span className="error-text">{current.message}</span>;
  else if (preview)
    hint = `${preview.size == null ? "Size unknown" : formatBytes(preview.size)} · ${preview.resumable ? "resumable" : "not resumable (one connection)"}`;

  return (
    <Dialog
      title="Add a download"
      onClose={onClose}
      width={560}
      footer={
        <>
          <button type="button" className="secondary-button" onClick={onClose}>
            Close
          </button>
          <button type="button" className="secondary-button" disabled={!valid || busy} onClick={() => void submit(true)}>
            Add paused
          </button>
          <button type="submit" form="add-form" className="primary-button" disabled={!valid || busy}>
            Start download
          </button>
        </>
      }
    >
      <form id="add-form" onSubmit={onSubmit}>
        <label className="field" htmlFor={ids.url}>
          <span>URL</span>
          <input
            id={ids.url}
            data-autofocus
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            placeholder="https://"
            spellCheck={false}
            autoComplete="off"
          />
        </label>
        <p className="hint" aria-live="polite">
          {hint}
        </p>
        <label className="field" htmlFor={ids.name}>
          <span>File name</span>
          <input
            id={ids.name}
            value={shownName}
            onChange={(e) => {
              setName(e.target.value);
              setNameTouched(true);
            }}
            placeholder="From the server"
            spellCheck={false}
          />
        </label>
        <div className="field">
          <label htmlFor={ids.dir}>
            <span>Save to</span>
          </label>
          <div className="field-row">
            <input id={ids.dir} value={dir} readOnly />
            <button type="button" className="secondary-button" onClick={() => void browse()}>
              Browse…
            </button>
          </div>
        </div>
      </form>
    </Dialog>
  );
}
