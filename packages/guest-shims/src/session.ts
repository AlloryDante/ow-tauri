/**
 * `sessionStorage` carry-over for a recreated ad guest (see
 * `session-restore.ts`).
 *
 * @packageDocumentation
 */

/**
 * Most bytes (UTF-16 code units of keys and values) a snapshot may hold;
 * a larger one is not carried and the guest reloads in place instead.
 */
export const MAX_SNAPSHOT_UNITS = 2 * 1024 * 1024;

function isTopFrame(win: Window): boolean {
  try {
    return win.top === win;
  } catch {
    return false;
  }
}

/**
 * Restores a snapshot into the top frame's `sessionStorage` when the frame
 * is on `origin`. Does nothing in subframes, on another origin, or for a
 * snapshot that is not a `{ key: string }` object (or its JSON text).
 *
 * @param win - the frame's window
 * @param origin - the only origin the snapshot belongs to
 * @param snapshot - the snapshot object, or its JSON text
 * @returns the number of keys restored
 */
export function restoreSessionStorage(win: Window, origin: string, snapshot: unknown): number {
  if (!isTopFrame(win)) return 0;
  try {
    if (win.location.origin !== origin) return 0;
    const data: unknown = typeof snapshot === 'string' ? JSON.parse(snapshot) : snapshot;
    if (typeof data !== 'object' || data === null || Array.isArray(data)) return 0;
    let restored = 0;
    for (const [key, value] of Object.entries(data as Record<string, unknown>)) {
      if (typeof value !== 'string') continue;
      win.sessionStorage.setItem(key, value);
      restored++;
    }
    return restored;
  } catch {
    return 0;
  }
}

/**
 * Reads the top frame's `sessionStorage` as a `{ key: value }` object, for
 * the plugin's snapshot before a recreate. Returns `null` in a subframe, on
 * another origin, when storage cannot be read, or when it holds more than
 * {@link MAX_SNAPSHOT_UNITS} (never a truncated snapshot).
 *
 * @param win - the frame's window
 * @param origin - the only origin a snapshot is taken on
 * @param maxUnits - the size limit
 * @returns the snapshot, or `null`
 */
export function snapshotSessionStorage(
  win: Window,
  origin: string,
  maxUnits: number = MAX_SNAPSHOT_UNITS,
): Record<string, string> | null {
  if (!isTopFrame(win)) return null;
  try {
    if (win.location.origin !== origin) return null;
    const storage = win.sessionStorage;
    const out: Record<string, string> = {};
    let units = 0;
    for (let i = 0; i < storage.length; i++) {
      const key = storage.key(i);
      if (key === null) continue;
      const value = storage.getItem(key) ?? '';
      units += key.length + value.length;
      if (units > maxUnits) return null;
      out[key] = value;
    }
    return out;
  } catch {
    return null;
  }
}
