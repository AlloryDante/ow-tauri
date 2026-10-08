/**
 * Keeps the page's own `alert()`, `confirm()` and `prompt()` in the webviews
 * ow-tauri manages (`docs/CONTRACT.md` B.2.6). `tauri-plugin-dialog`, which
 * the plugin registers for Electron's `dialog` module, replaces
 * `window.alert` with a message box that does not block and
 * `window.confirm` with one that returns a promise; Electron's pages keep
 * the blocking functions. The plugin injects this guard before that plugin's
 * script, so the replacement is ignored and the page's functions stay. An
 * app's own assignment still replaces them.
 *
 * @packageDocumentation
 */

/** The functions the guard keeps. */
export const NATIVE_DIALOGS = ['alert', 'confirm', 'prompt'] as const;

/**
 * Whether `label` names a webview ow-tauri manages: `ow-main`, `bw-<id>`,
 * `bwr-<id>`, an ad guest (`owad-*`) or a consent window (`ow-cmp*`).
 *
 * @param label - the webview label
 * @returns whether the guard applies
 */
export function isManagedLabel(label: string): boolean {
  return /^(?:ow-main$|ow-cmp(?:$|-)|bwr?-\d|owad-.)/.test(label);
}

/**
 * Whether `value` is the browser's own function (`[native code]`).
 *
 * @param value - the current `window.alert` (or `confirm`, `prompt`)
 * @returns whether it is native
 */
export function isNativeFunction(value: unknown): boolean {
  return (
    typeof value === 'function' &&
    /\{\s*\[native code\]\s*\}\s*$/.test(Function.prototype.toString.call(value))
  );
}

/**
 * Whether `value` is `tauri-plugin-dialog`'s replacement (it calls the
 * plugin's `message` or `confirm` command).
 *
 * @param value - the value being assigned
 * @returns whether to ignore the assignment
 */
export function isDialogPluginOverride(value: unknown): boolean {
  return (
    typeof value === 'function' &&
    Function.prototype.toString.call(value).includes('plugin:dialog|')
  );
}

/**
 * Turns each native `alert`, `confirm` and `prompt` of `target` into an
 * accessor that returns it and ignores `tauri-plugin-dialog`'s assignment.
 * Any other assignment makes the property an ordinary writable value again.
 *
 * @param target - the window
 * @param isNative - tells a native function (tests pass their own)
 * @returns the names guarded
 */
export function guardNativeDialogs(
  target: object,
  isNative: (value: unknown) => boolean = isNativeFunction,
): string[] {
  const guarded: string[] = [];
  for (const name of NATIVE_DIALOGS) {
    const native: unknown = Reflect.get(target, name);
    if (!isNative(native)) continue;
    const own = Object.getOwnPropertyDescriptor(target, name);
    if (own?.configurable === false) continue;
    const enumerable = own?.enumerable ?? true;
    Object.defineProperty(target, name, {
      configurable: true,
      enumerable,
      get: () => native,
      set(value: unknown) {
        if (isDialogPluginOverride(value)) return;
        Object.defineProperty(target, name, {
          value,
          writable: true,
          configurable: true,
          enumerable,
        });
      },
    });
    guarded.push(name);
  }
  return guarded;
}

/**
 * Guards `target` when it is a webview ow-tauri manages, read from Tauri's
 * webview metadata.
 *
 * @param target - the window
 * @returns the names guarded
 */
export function installNativeDialogGuard(target: object): string[] {
  const internals = Reflect.get(target, '__TAURI_INTERNALS__') as
    { metadata?: { currentWebview?: { label?: unknown } } } | undefined;
  const label = internals?.metadata?.currentWebview?.label;
  return typeof label === 'string' && isManagedLabel(label) ? guardNativeDialogs(target) : [];
}
