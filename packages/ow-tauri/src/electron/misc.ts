/**
 * The remaining Electron modules of `docs/CONTRACT.md` section B.2.5:
 * `crashReporter`, `nativeTheme`, `process` and the module stand-ins for
 * everything ow-tauri does not provide.
 *
 * @packageDocumentation
 */
import { createProcessShim, type ProcessShim } from '../bootstrap/process-shim.js';
import { EventEmitter, emitFromHost } from '../shared/emitter.js';
import { unsupportedModule, type UnsupportedMethod } from '../shared/unsupported.js';
import type * as U from './unsupported-types.js';
import type { UnsupportedModule } from './unsupported-types.js';
import { kernel } from './runtime.js';

/** Electron's `crashReporter` (CONTRACT B.2.5). */
export interface CrashReporter {
  /**
   * Partial: a no-op with a warning; install a Rust crash handler instead.
   *
   * @param options - ignored
   */
  start(options?: unknown): void;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.5).
   */
  readonly getLastCrashReport: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.5).
   */
  readonly getUploadedReports: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.5).
   */
  readonly getUploadToServer: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.5).
   */
  readonly setUploadToServer: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.5).
   */
  readonly addExtraParameter: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.5).
   */
  readonly removeExtraParameter: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.5).
   */
  readonly getParameters: UnsupportedMethod;
}

// The other members come from the proxy below.
const reporter = {
  start(): void {
    kernel.warnOnce(
      'crashReporter.start',
      'crashReporter.start is a no-op in ow-tauri; install a crash handler in Rust (see docs/PORT-MAP.md)',
    );
  },
} as CrashReporter;

/** Electron's `crashReporter`: `start` warns, every other member throws. */
export const crashReporter: CrashReporter = new Proxy(reporter, {
  get(target, key, receiver) {
    if (key === 'start' || typeof key === 'symbol' || key === 'then' || key === 'toJSON')
      return Reflect.get(target, key, receiver) as unknown;
    return (unsupportedModule('crashReporter') as Record<string, unknown>)[key];
  },
});

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

/**
 * Electron's `Menu`: no application menus; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri: build menus in the UI or with a Tauri menu in Rust.
 */
export const Menu = unsupportedModule(
  'Menu',
  reason('build menus in the UI or with a Tauri menu in Rust'),
) as UnsupportedModule<U.MenuMembers>;
/**
 * Electron's `MenuItem`: no application menus; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri: build menus in the UI or with a Tauri menu in Rust.
 */
export const MenuItem = unsupportedModule(
  'MenuItem',
  reason('build menus in the UI or with a Tauri menu in Rust'),
) as UnsupportedModule<U.MenuItemMembers>;
/**
 * Electron's `Tray`: no tray API in JavaScript; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri: use Tauri's tray icon from Rust.
 */
export const Tray = unsupportedModule(
  'Tray',
  reason("use Tauri's tray icon from Rust"),
) as UnsupportedModule<U.TrayMembers>;
/**
 * Electron's `Notification`: no notification API in the facade; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri: use the Tauri notification plugin.
 */
export const Notification = unsupportedModule(
  'Notification',
  reason('use the Tauri notification plugin'),
) as UnsupportedModule<U.NotificationMembers>;
/**
 * Electron's `session`: no session API; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri.
 */
export const session = unsupportedModule('session') as UnsupportedModule<U.SessionMembers>;
/**
 * Electron's `protocol`: no protocol API in JavaScript; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri: register URI scheme protocols in Rust.
 */
export const protocol = unsupportedModule(
  'protocol',
  reason('register URI scheme protocols in Rust'),
) as UnsupportedModule<U.ProtocolMembers>;
/**
 * Electron's `net`: no net module; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri: use fetch.
 */
export const net = unsupportedModule('net', reason('use fetch')) as UnsupportedModule<U.NetMembers>;
/**
 * Electron's `netLog`: no net log; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri.
 */
export const netLog = unsupportedModule('netLog') as UnsupportedModule<U.NetLogMembers>;
/**
 * Electron's `powerMonitor`: no power monitor; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri.
 */
export const powerMonitor = unsupportedModule(
  'powerMonitor',
) as UnsupportedModule<U.PowerMonitorMembers>;
/**
 * Electron's `powerSaveBlocker`: no power save blocker; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri.
 */
export const powerSaveBlocker = unsupportedModule(
  'powerSaveBlocker',
) as UnsupportedModule<U.PowerSaveBlockerMembers>;
/**
 * Electron's `autoUpdater`: Electron's updater is not provided; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri: use the electron-updater compatible `autoUpdater` from `ow-tauri/main` (CONTRACT I.5).
 */
export const autoUpdater = unsupportedModule(
  'autoUpdater',
  reason("use the electron-updater compatible autoUpdater from 'ow-tauri/main'"),
) as UnsupportedModule<U.AutoUpdaterMembers>;
/**
 * Electron's `clipboard`: no clipboard module; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri: use navigator.clipboard or the Tauri clipboard plugin.
 */
export const clipboard = unsupportedModule(
  'clipboard',
  reason('use navigator.clipboard or the Tauri clipboard plugin'),
) as UnsupportedModule<U.ClipboardMembers>;
/**
 * Electron's `nativeImage`: no native images; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri.
 */
export const nativeImage = unsupportedModule(
  'nativeImage',
) as UnsupportedModule<U.NativeImageMembers>;
/**
 * Electron's `systemPreferences`: no system preferences; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri.
 */
export const systemPreferences = unsupportedModule(
  'systemPreferences',
) as UnsupportedModule<U.SystemPreferencesMembers>;
/**
 * Electron's `desktopCapturer`: no desktop capturer; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri.
 */
export const desktopCapturer = unsupportedModule(
  'desktopCapturer',
) as UnsupportedModule<U.DesktopCapturerMembers>;
/**
 * Electron's `webFrame`: no webFrame; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri.
 */
export const webFrame = unsupportedModule('webFrame') as UnsupportedModule<U.WebFrameMembers>;
/**
 * Electron's `webFrameMain`: no webFrameMain; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri.
 */
export const webFrameMain = unsupportedModule(
  'webFrameMain',
) as UnsupportedModule<U.WebFrameMainMembers>;
/**
 * Electron's `utilityProcess`: no utility processes; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri: move the work to Rust or a web worker.
 */
export const utilityProcess = unsupportedModule(
  'utilityProcess',
  reason('move the work to Rust or a web worker'),
) as UnsupportedModule<U.UtilityProcessMembers>;
/**
 * Electron's `MessageChannelMain`: no MessagePorts; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri.
 */
export const MessageChannelMain = unsupportedModule(
  'MessageChannelMain',
) as UnsupportedModule<U.MessageChannelMainMembers>;
/**
 * Electron's `BrowserView`: no BrowserView; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri.
 */
export const BrowserView = unsupportedModule(
  'BrowserView',
) as UnsupportedModule<U.BrowserViewMembers>;
/**
 * Electron's `WebContentsView`: no WebContentsView; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri.
 */
export const WebContentsView = unsupportedModule(
  'WebContentsView',
) as UnsupportedModule<U.WebContentsViewMembers>;
/**
 * Electron's `BaseWindow`: no BaseWindow; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri: use BrowserWindow.
 */
export const BaseWindow = unsupportedModule(
  'BaseWindow',
  reason('use BrowserWindow'),
) as UnsupportedModule<U.BaseWindowMembers>;
/**
 * Electron's `TouchBar`: no Touch Bar; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri.
 */
export const TouchBar = unsupportedModule('TouchBar') as UnsupportedModule<U.TouchBarMembers>;
/**
 * Electron's `inAppPurchase`: no in-app purchases; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri.
 */
export const inAppPurchase = unsupportedModule(
  'inAppPurchase',
) as UnsupportedModule<U.InAppPurchaseMembers>;
/**
 * Electron's `pushNotifications`: no push notifications; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri.
 */
export const pushNotifications = unsupportedModule(
  'pushNotifications',
) as UnsupportedModule<U.PushNotificationsMembers>;
/**
 * Electron's `safeStorage`: no safe storage; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri.
 */
export const safeStorage = unsupportedModule(
  'safeStorage',
) as UnsupportedModule<U.SafeStorageMembers>;
/**
 * Electron's `contentTracing`: no content tracing; every member throws `OwTauriUnsupportedError`.
 *
 * @deprecated Unsupported in ow-tauri.
 */
export const contentTracing = unsupportedModule(
  'contentTracing',
) as UnsupportedModule<U.ContentTracingMembers>;
