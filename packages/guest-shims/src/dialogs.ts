/**
 * Silent page dialogs in ad guests: `alert()`, `confirm()` and `prompt()`
 * return at once without showing anything (`alert` → `undefined`,
 * `confirm` → `false`, `prompt` → `null`), so an ad page can never block the
 * app with a modal dialog. Installed in every frame the guest shim reaches.
 *
 * @packageDocumentation
 */
import { hostFunction } from './outbox.js';

/** The answers of the silenced dialogs. */
export const SILENT_DIALOGS = Object.freeze({
  alert: undefined,
  confirm: false,
  prompt: null,
});

/**
 * Replaces `alert`, `confirm` and `prompt` of `win` with silent functions.
 * A property the page made non-configurable is left alone.
 *
 * @param win - the frame's window
 * @returns the names replaced
 */
export function silenceDialogs(win: Window): string[] {
  const replaced: string[] = [];
  for (const [name, answer] of Object.entries(SILENT_DIALOGS)) {
    const own = Object.getOwnPropertyDescriptor(win, name);
    if (own?.configurable === false) continue;
    try {
      Object.defineProperty(win, name, {
        value: hostFunction(() => answer),
        writable: true,
        configurable: true,
        enumerable: own?.enumerable ?? true,
      });
      replaced.push(name);
    } catch {
      // A frame that refuses the property keeps its dialogs.
    }
  }
  return replaced;
}
