export type Shortcut = "add" | "toggle" | "remove" | "up" | "down";

interface KeyLike {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
}

const TYPING = ["INPUT", "TEXTAREA", "SELECT", "BUTTON"];

/** Ctrl/Cmd+N add; Space pause/resume; Delete remove; arrows move the selection. */
export function shortcutFor(e: KeyLike, target: { tagName: string } | null): Shortcut | null {
  const mod = e.ctrlKey || e.metaKey;
  if (mod && !e.altKey && e.key.toLowerCase() === "n") return "add";
  if (TYPING.includes(target?.tagName ?? "")) return null;
  if (mod || e.altKey) return null;
  switch (e.key) {
    case " ":
      return "toggle";
    case "Delete":
      return "remove";
    case "ArrowUp":
      return "up";
    case "ArrowDown":
      return "down";
    default:
      return null;
  }
}

/** The id one step up (-1) or down (+1) from `selected`, clamped to the list. */
export function step(list: { id: string }[], selected: string | null, dir: 1 | -1): string | null {
  if (list.length === 0) return null;
  const at = list.findIndex((r) => r.id === selected);
  if (at < 0) return list[0]!.id;
  const next = Math.min(list.length - 1, Math.max(0, at + dir));
  return list[next]!.id;
}
