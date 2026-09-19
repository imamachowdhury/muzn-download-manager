const KEY = "mdm.lastDir";

/** The folder the last download went to (this machine's UI convenience). */
export function lastDir(): string | null {
  try {
    return localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

export function rememberDir(dir: string): void {
  try {
    if (dir) localStorage.setItem(KEY, dir);
  } catch {
    // storage unavailable: nothing to remember
  }
}

/**
 * Forget the remembered folder: Settings saved a new download folder, and
 * that choice must win from now on (final review M7).
 */
export function forgetDir(): void {
  try {
    localStorage.removeItem(KEY);
  } catch {
    // storage unavailable: nothing was remembered
  }
}
