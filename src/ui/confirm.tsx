import { useState } from "react";
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { Dialog } from "./Dialog";

interface ConfirmOptions {
  title: string;
  message: string;
  confirmLabel: string;
  danger?: boolean;
  /** Offer a checkbox (e.g. "Also delete the file from disk"). */
  checkbox?: string;
}
interface ConfirmAnswer {
  ok: boolean;
  checked: boolean;
}
interface Request extends ConfirmOptions {
  resolve(a: ConfirmAnswer): void;
}

const requests = createStore<{ current: Request | null }>(() => ({ current: null }));

/** The one confirmation primitive (never window.confirm). */
export function confirmDialog(opts: ConfirmOptions): Promise<ConfirmAnswer> {
  return new Promise((resolve) => requests.setState({ current: { ...opts, resolve } }));
}

export function ConfirmHost() {
  const req = useStore(requests, (s) => s.current);
  const [checked, setChecked] = useState(false);
  if (!req) return null;
  const close = (ok: boolean) => {
    requests.setState({ current: null });
    setChecked(false);
    req.resolve({ ok, checked: ok && checked });
  };
  return (
    <Dialog
      title={req.title}
      onClose={() => close(false)}
      width={420}
      footer={
        <>
          <button type="button" className="secondary-button" onClick={() => close(false)}>
            Keep
          </button>
          <button
            type="button"
            className={req.danger ? "danger-button" : "primary-button"}
            data-autofocus
            onClick={() => close(true)}
          >
            {req.confirmLabel}
          </button>
        </>
      }
    >
      <p>{req.message}</p>
      {req.checkbox && (
        <label className="check">
          <input type="checkbox" checked={checked} onChange={(e) => setChecked(e.target.checked)} />
          {req.checkbox}
        </label>
      )}
    </Dialog>
  );
}
