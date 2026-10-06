/**
 * The remaining Electron modules of `docs/CONTRACT.md` section B.2.5:
 * `crashReporter`, `nativeTheme`, `process` and the module stand-ins for
 * everything ow-tauri does not provide.
 *
 * @packageDocumentation
 */
import { createProcessShim, type ProcessShim } from '../bootstrap/process-shim.js';
import { EventEmitter, emitFromHost } from '../shared/emitter.js';
import { unsupportedModule } from '../shared/unsupported.js';
import { kernel } from './runtime.js';

/** Electron's `crashReporter` (CONTRACT B.2.5). */
export interface CrashReporter {
  /**
   * Partial: a no-op with a warning; install a Rust crash handler instead.
   *
   * @param options - ignored
   */
  start(options?: unknown): void;
}

/** Electron's `crashReporter`: `start` warns, every other member throws. */
export const crashReporter: CrashReporter = new Proxy(
  {
    start(): void {
      kernel.warnOnce(
        'crashReporter.start',
        'crashReporter.start is a no-op in ow-tauri; install a crash handler in Rust (see docs/PORT-MAP.md)',
      );
    },
  },
  {
    get(target, key, receiver) {
      if (key === 'start' || typeof key === 'symbol' || key === 'then' || key === 'toJSON')
        return Reflect.get(target, key, receiver) as unknown;
      return (unsupportedModule('crashReporter') as Record<string, unknown>)[key];
    },
  },
);

const DARK_QUERY = '(prefers-color-scheme: dark)';

/**
 * Partial Electron `nativeTheme` (CONTRACT B.2.5): `shouldUseDarkColors`
 * follows the webview's `prefers-color-scheme`, which the webview takes from
 * the window theme; `updated` fires when it changes.
 */
export class NativeTheme extends EventEmitter {
  #query: MediaQueryList | undefined;
  #source: 'system' | 'light' | 'dark' = 'system';

  /** @internal */
  constructor() {
    super();
    if (typeof matchMedia === 'function') {
      this.#query = matchMedia(DARK_QUERY);
      this.#query.addEventListener('change', () => {
        emitFromHost(this, 'updated');
      });
    }
  }

  /** Whether the OS (or {@link NativeTheme.themeSource}) asks for dark colours. */
  get shouldUseDarkColors(): boolean {
    if (this.#source !== 'system') return this.#source === 'dark';
    return this.#query?.matches === true;
  }

  /** Always `false`: high contrast is not reported by the webview. */
  readonly shouldUseHighContrastColors = false;

  /** Always `false`. */
  readonly shouldUseInvertedColorScheme = false;

  /**
   * Partial: `system`, `light` or `dark`. Changes only what this object
   * reports (the webview's CSS media query is not overridden).
   */
  get themeSource(): 'system' | 'light' | 'dark' {
    return this.#source;
  }

  /**
   * Overrides what this object reports; `updated` fires on change.
   *
   * @param value - `system`, `light` or `dark`
   */
  set themeSource(value: 'system' | 'light' | 'dark') {
    if (value === this.#source) return;
    this.#source = value;
    emitFromHost(this, 'updated');
  }
}

/** Electron's `nativeTheme`. */
export const nativeTheme: NativeTheme = kernel.singleton(
  'electron.nativeTheme',
  () => new NativeTheme(),
);

/** The `process` shim (CONTRACT B.2.5), the same object as the global one the bootstrap installs. */
export const process: ProcessShim = kernel.singleton('process', () => createProcessShim(kernel));

const reason = (alternative: string): string => `it has no equivalent in ow-tauri; ${alternative}`;

/** Unsupported: no application menus. Every member throws `OwTauriUnsupportedError`. */
export const Menu: object = unsupportedModule(
  'Menu',
  reason('build menus in the UI or with a Tauri menu in Rust'),
);
/** Unsupported. */
export const MenuItem: object = unsupportedModule(
  'MenuItem',
  reason('build menus in the UI or with a Tauri menu in Rust'),
);
/** Unsupported: use Tauri's tray icon from Rust. */
export const Tray: object = unsupportedModule('Tray', reason("use Tauri's tray icon from Rust"));
/** Unsupported: use the Tauri notification plugin. */
export const Notification: object = unsupportedModule(
  'Notification',
  reason('use the Tauri notification plugin'),
);
/** Unsupported. */
export const session: object = unsupportedModule('session');
/** Unsupported. */
export const protocol: object = unsupportedModule(
  'protocol',
  reason('register URI scheme protocols in Rust'),
);
/** Unsupported: use `fetch`. */
export const net: object = unsupportedModule('net', reason('use fetch'));
/** Unsupported. */
export const netLog: object = unsupportedModule('netLog');
/** Unsupported. */
export const powerMonitor: object = unsupportedModule('powerMonitor');
/** Unsupported. */
export const powerSaveBlocker: object = unsupportedModule('powerSaveBlocker');
/** Unsupported: Electron's updater; use `autoUpdater` from `ow-tauri/main`. */
export const autoUpdater: object = unsupportedModule(
  'autoUpdater',
  reason("use autoUpdater from 'ow-tauri/main'"),
);
/** Unsupported: use `navigator.clipboard` or the Tauri clipboard plugin. */
export const clipboard: object = unsupportedModule(
  'clipboard',
  reason('use navigator.clipboard or the Tauri clipboard plugin'),
);
/** Unsupported. */
export const nativeImage: object = unsupportedModule('nativeImage');
/** Unsupported. */
export const systemPreferences: object = unsupportedModule('systemPreferences');
/** Unsupported. */
export const desktopCapturer: object = unsupportedModule('desktopCapturer');
/** Unsupported. */
export const webFrame: object = unsupportedModule('webFrame');
/** Unsupported. */
export const webFrameMain: object = unsupportedModule('webFrameMain');
/** Unsupported. */
export const utilityProcess: object = unsupportedModule(
  'utilityProcess',
  reason('move the work to Rust or a web worker'),
);
/** Unsupported. */
export const MessageChannelMain: object = unsupportedModule('MessageChannelMain');
/** Unsupported. */
export const BrowserView: object = unsupportedModule('BrowserView');
/** Unsupported. */
export const WebContentsView: object = unsupportedModule('WebContentsView');
/** Unsupported. */
export const BaseWindow: object = unsupportedModule('BaseWindow', reason('use BrowserWindow'));
/** Unsupported. */
export const TouchBar: object = unsupportedModule('TouchBar');
/** Unsupported. */
export const inAppPurchase: object = unsupportedModule('inAppPurchase');
/** Unsupported. */
export const pushNotifications: object = unsupportedModule('pushNotifications');
/** Unsupported. */
export const safeStorage: object = unsupportedModule('safeStorage');
/** Unsupported. */
export const contentTracing: object = unsupportedModule('contentTracing');
