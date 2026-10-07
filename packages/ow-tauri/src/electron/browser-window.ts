/**
 * `BrowserWindow` and `webContents` over Tauri windows (`docs/CONTRACT.md`
 * section B.2.2).
 *
 * A `BrowserWindow` is created with `window_create` (one native window
 * `bw-<id>` with one webview of the same label); the preload named in
 * `webPreferences.preload` is injected by the plugin as an initialization
 * script. Operations that `@tauri-apps/api` already exposes go straight to
 * `core:window:*` / `core:webview:*` commands.
 *
 * Synchronous getters read a per-window state cache. Setters update it
 * immediately and send their command afterwards, in call order (B.1.6
 * item 5), so `isVisible()` right after `show()` is `true`, as in Electron.
 * Window events from the plugin overwrite the cache.
 *
 * @packageDocumentation
 */
import type { FacadeKernel } from '../bootstrap/facade-kernel.js';
import type { IpcMain } from '../bootstrap/ipc-main.js';
import { checkChannel, encodeMessageArgs } from '../bootstrap/ipc-renderer.js';
import { EventEmitter, emitFromHost } from '../shared/emitter.js';
import { OwTauriError, OwTauriUnsupportedError } from '../shared/errors.js';
import { decode } from '../shared/otj.js';
import type { Display, Rectangle, WindowMessage } from '../shared/protocol.js';
import { defineUnsupported } from '../shared/unsupported.js';
import { createEvent, kernel, toAssetPath } from './runtime.js';
import type { UnsupportedMethod } from './unsupported-types.js';
import type {
  BrowserWindowConstructorOptions,
  HandlerDetails,
  LoadFileOptions,
  WindowOpenHandlerResponse,
} from './types.js';

/** Brand that makes `instanceof BrowserWindow` work across copies of the package. */
const WINDOW_BRAND = Symbol.for('ow-tauri.BrowserWindow');
/** Brand for `WebContents`. */
const CONTENTS_BRAND = Symbol.for('ow-tauri.WebContents');

/** Options ow-tauri rejects, with the value that is accepted because it is Electron's default. */
const UNSUPPORTED_OPTIONS: Readonly<Record<string, unknown>> = {
  titleBarStyle: 'default',
  trafficLightPosition: undefined,
  vibrancy: undefined,
  visualEffectState: 'followWindow',
  roundedCorners: true,
  thickFrame: true,
  type: undefined,
  tabbingIdentifier: undefined,
  kiosk: false,
  simpleFullscreen: false,
};

/** `webPreferences` that are ignored with a warning (CONTRACT B.2.2). */
const IGNORED_PREFERENCES = [
  'sandbox',
  'webSecurity',
  'partition',
  'session',
  'offscreen',
  'webviewTag',
  'backgroundThrottling',
];

/** Options forwarded to `window_create` as they are. */
const WIRE_OPTIONS = [
  'width',
  'height',
  'x',
  'y',
  'center',
  'minWidth',
  'minHeight',
  'maxWidth',
  'maxHeight',
  'useContentSize',
  'show',
  'title',
  'resizable',
  'movable',
  'minimizable',
  'maximizable',
  'closable',
  'focusable',
  'alwaysOnTop',
  'fullscreen',
  'fullscreenable',
  'skipTaskbar',
  'transparent',
  'backgroundColor',
  'modal',
  'frame',
  'name',
] as const;

/** The per-window state cache. */
interface WindowState {
  visible: boolean;
  focused: boolean;
  minimized: boolean;
  maximized: boolean;
  fullscreen: boolean;
  fullscreenable: boolean;
  bounds: Rectangle;
  minSize: [number, number];
  maxSize: [number, number];
  title: string;
  alwaysOnTop: boolean;
  resizable: boolean;
  movable: boolean;
  minimizable: boolean;
  maximizable: boolean;
  closable: boolean;
  focusable: boolean;
  skipTaskbar: boolean;
  backgroundColor: string;
  url: string;
  loading: boolean;
  zoomFactor: number;
  devToolsOpened: boolean;
}

interface Created {
  hostId: number;
  /** The native window's label (`bw-<id>`). */
  label: string;
  /** The content webview's label: `bw-<id>`, or `bwr-<id>` once the window is remote (A.2.3.1). */
  webviewLabel: string;
}

interface LoadWaiter {
  resolve: () => void;
  reject: (error: unknown) => void;
}

/** The facade's window registry, shared by every copy of the package in a webview. */
class WindowRegistry {
  readonly windows = new Map<number, BrowserWindow>();
  focusedId: number | null = null;
  pendingCreates = 0;
  held: WindowMessage[] = [];
  /** App-level hooks, set by the `app` module. */
  hooks: {
    created?: (win: BrowserWindow) => void;
    focus?: (win: BrowserWindow) => void;
    blur?: (win: BrowserWindow) => void;
    allClosed?: () => void;
  } = {};

  constructor(private readonly k: FacadeKernel) {
    k.on('window', (message) => {
      this.onMessage(message as WindowMessage);
    });
    k.server.senderResolver = (windowId) =>
      this.windows.get(windowId)?.webContents ?? detachedContents(windowId);
    k.onReset(() => {
      this.windows.clear();
      this.focusedId = null;
      this.pendingCreates = 0;
      this.held = [];
    });
  }

  onMessage(message: WindowMessage): void {
    if (typeof message.id !== 'number') return;
    if (
      message.event !== 'created' &&
      !this.k.windowIds.knowsHost(message.id) &&
      this.pendingCreates > 0
    ) {
      // Probably a window whose window_create response has not arrived yet.
      this.held.push(message);
      return;
    }
    const id = this.k.windowIds.fromHost(message.id);
    if (message.event === 'created') {
      if (!this.windows.has(id)) BrowserWindow[ADOPT](id, message.data);
      return;
    }
    const win = this.windows.get(id);
    if (!win) {
      this.k.log('debug', `window event '${message.event}' for unknown window ${String(id)}`);
      if (message.event === 'close' && typeof message.requestId === 'number') {
        // Nobody can prevent it: let the close proceed (A.6).
        this.k
          .command('window_close_reply', {
            id: message.id,
            requestId: message.requestId,
            prevent: false,
          })
          .catch((error: unknown) => {
            this.k.log('warn', `window_close_reply failed: ${(error as Error).message}`);
          });
      }
      return;
    }
    win[ON_EVENT](message);
  }

  /** The last window closed: `window-all-closed`, or quit when the `app` module is absent. */
  allClosed(): void {
    if (this.hooks.allClosed) {
      this.hooks.allClosed();
      return;
    }
    // Electron's default with no `window-all-closed` listener is to quit.
    this.k.command('app_quit').catch((error: unknown) => {
      this.k.log('warn', `app_quit failed: ${(error as Error).message}`);
    });
  }

  createSettled(): void {
    this.pendingCreates = Math.max(0, this.pendingCreates - 1);
    const held = this.held;
    this.held = [];
    for (const message of held) {
      if (this.pendingCreates > 0 && !this.k.windowIds.knowsHost(message.id))
        this.held.push(message);
      else this.onMessage(message);
    }
  }
}

function registry(): WindowRegistry {
  return kernel.singleton('electron.windows', () => new WindowRegistry(kernel));
}

/**
 * The registry hooks the `app` module installs (`browser-window-created`,
 * focus, blur, `window-all-closed`).
 *
 * @returns the mutable hook object
 * @internal
 */
export function windowHooks(): WindowRegistry['hooks'] {
  return registry().hooks;
}

/**
 * A `webContents` stand-in for an IPC sender the registry does not know.
 *
 * @param windowId - the app-visible id
 * @returns an object with `id` and `send`
 */
function detachedContents(windowId: number): unknown {
  return Object.freeze({
    id: windowId,
    send: (channel: string, ...args: unknown[]) => {
      kernel.server.emit(windowId, channel, args);
    },
  });
}

// The registry is shared by every copy of the package in a webview, so the
// members it calls on windows created by another copy are keyed with
// registered symbols. The `v1` suffix is part of the facade API
// (RUNTIME_API_VERSION): change the methods' behaviour, change the suffix.
const ADOPT = Symbol.for('ow-tauri.BrowserWindow.v1.adopt');
const ON_EVENT = Symbol.for('ow-tauri.BrowserWindow.v1.onEvent');
const OP = Symbol.for('ow-tauri.BrowserWindow.v1.op');
const ON_CONTENTS_EVENT = Symbol.for('ow-tauri.WebContents.v1.onEvent');
let adopting: { id: number } | null = null;

/**
 * Parses an Electron colour (`#RGB`, `#RRGGBB`, `#AARRGGBB`) to Tauri's
 * `[r, g, b, a]`.
 *
 * @param color - the colour
 * @returns the RGBA tuple, or `null` when the text is not a hex colour
 */
export function parseColor(color: string): [number, number, number, number] | null {
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/i.exec(color.trim())?.[1];
  if (hex === undefined) return null;
  const n = (s: string): number => parseInt(s, 16);
  if (hex.length === 3) {
    const [r = '0', g = '0', b = '0'] = hex;
    return [n(r + r), n(g + g), n(b + b), 255];
  }
  if (hex.length === 6) return [n(hex.slice(0, 2)), n(hex.slice(2, 4)), n(hex.slice(4, 6)), 255];
  return [n(hex.slice(2, 4)), n(hex.slice(4, 6)), n(hex.slice(6, 8)), n(hex.slice(0, 2))];
}

function primaryWorkArea(): Rectangle {
  const displays = kernel.state.get('displays');
  const primaryId = kernel.state.get('primaryDisplayId');
  if (Array.isArray(displays) && displays.length > 0) {
    const list = displays as Display[];
    const primary = list.find((d) => d.id === primaryId) ?? list[0];
    if (primary?.workArea) return primary.workArea;
  }
  return { x: 0, y: 0, width: 1920, height: 1080 };
}

function num(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
}

function bool(value: unknown, fallback: boolean): boolean {
  return typeof value === 'boolean' ? value : fallback;
}

/**
 * Electron's `BrowserWindow` over a Tauri window (CONTRACT B.2.2). Main
 * webview only.
 *
 * `id` is available synchronously; the native window is created
 * asynchronously and every operation waits for it, in call order.
 *
 * @example
 * ```ts
 * const win = new BrowserWindow({ width: 1280, height: 800, show: false,
 *   webPreferences: { preload: 'preload/preload.js' } });
 * win.once('ready-to-show', () => win.show());
 * await win.loadFile('renderer/index.html');
 * ```
 */
export class BrowserWindow extends EventEmitter {
  /**
   * Brand-aware `instanceof`, so windows created by another copy of the
   * package are recognised.
   *
   * @param value - the value on the left of `instanceof`
   * @returns whether `value` is a `BrowserWindow`
   */
  static override [Symbol.hasInstance](value: unknown): boolean {
    return (
      typeof value === 'object' &&
      value !== null &&
      (value as Record<symbol, unknown>)[WINDOW_BRAND] === true
    );
  }

  /**
   * All windows that are not destroyed, in creation order.
   *
   * @returns the windows
   */
  static getAllWindows(): BrowserWindow[] {
    kernel.require('main', 'BrowserWindow.getAllWindows');
    return [...registry().windows.values()];
  }

  /**
   * The focused window, if it belongs to this app.
   *
   * @returns the window or `null`
   */
  static getFocusedWindow(): BrowserWindow | null {
    kernel.require('main', 'BrowserWindow.getFocusedWindow');
    const reg = registry();
    return reg.focusedId === null ? null : (reg.windows.get(reg.focusedId) ?? null);
  }

  /**
   * The window with the given id.
   *
   * @param id - the window id
   * @returns the window or `null`
   */
  static fromId(id: number): BrowserWindow | null {
    kernel.require('main', 'BrowserWindow.fromId');
    return registry().windows.get(id) ?? null;
  }

  /**
   * The window that owns a `webContents`.
   *
   * @param webContents - the web contents
   * @returns the window or `null`
   */
  static fromWebContents(webContents: unknown): BrowserWindow | null {
    kernel.require('main', 'BrowserWindow.fromWebContents');
    const id = (webContents as { id?: unknown } | null)?.id;
    return typeof id === 'number' ? (registry().windows.get(id) ?? null) : null;
  }

  /**
   * Adopts a window the plugin created (a `created` window event).
   *
   * @param id - the app-visible id (already bound to the plugin id)
   * @param data - the event data: `{ options }`
   * @returns the window
   * @internal
   */
  static [ADOPT](id: number, data: unknown): BrowserWindow {
    const options = (data as { options?: BrowserWindowConstructorOptions } | null)?.options ?? {};
    adopting = { id };
    try {
      return new BrowserWindow({ ...options, webPreferences: {} });
    } finally {
      adopting = null;
    }
  }

  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.2).
   */
  declare readonly setOpacity: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.2).
   */
  declare readonly setVibrancy: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.2).
   */
  declare readonly setShape: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.2).
   */
  declare readonly capturePage: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.2).
   */
  declare readonly setThumbarButtons: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.2).
   */
  declare readonly setOverlayIcon: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.2).
   */
  declare readonly previewFile: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.2).
   */
  declare readonly setBrowserView: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.2).
   */
  declare readonly addBrowserView: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.2).
   */
  declare readonly setTouchBar: UnsupportedMethod;
  /** The window id. */
  readonly id: number;
  /** The window's web contents. */
  readonly webContents: WebContents;
  readonly #state: WindowState;
  readonly #created: Promise<Created>;
  #chain: Promise<unknown> = Promise.resolve();
  #destroyed = false;
  readonly #loadWaiters: LoadWaiter[] = [];
  #host: Created | null = null;

  /**
   * Creates the window (main webview only).
   *
   * @param options - Electron constructor options; unsupported options
   *   throw `OwTauriUnsupportedError`, ignored ones log a warning
   * @throws OwTauriError `forbidden` outside the main webview
   */
  constructor(options: BrowserWindowConstructorOptions = {}) {
    super();
    kernel.require('main', 'new BrowserWindow');
    const adopted = adopting;
    if (!adopted) checkOptions(options);
    Object.defineProperty(this, WINDOW_BRAND, { value: true });
    this.id = adopted?.id ?? kernel.windowIds.reserve();
    this.#state = initialState(options);
    this.webContents = new WebContents(this, this.#state);
    const reg = registry();
    reg.windows.set(this.id, this);
    if (adopted) {
      const hostId = kernel.windowIds.toHost(adopted.id) ?? adopted.id;
      const label = `bw-${String(hostId)}`;
      this.#host = { hostId, label, webviewLabel: label };
      this.#created = Promise.resolve(this.#host);
    } else {
      this.#created = this.#create(options);
      this.#created.catch(() => undefined);
    }
    reg.hooks.created?.(this);
  }

  /**
   * Resolves once the native window exists (ow-tauri addition; Electron
   * creates windows synchronously).
   *
   * @returns resolves when created, rejects when creation failed
   */
  whenCreated(): Promise<void> {
    return this.#created.then(() => undefined);
  }

  /**
   * Whether the window was closed or destroyed.
   *
   * @returns `true` after `closed`
   */
  isDestroyed(): boolean {
    return this.#destroyed;
  }

  /**
   * Loads a URL. An `http(s)` URL that is not an app asset turns the window
   * into a remote window with no IPC, preload or init scripts (A.2.3.1).
   *
   * @param url - the URL
   * @returns resolves on `did-finish-load`, rejects on `did-fail-load`
   */
  loadURL(url: string): Promise<void> {
    this.#assertAlive('loadURL');
    if (/^file:/i.test(url)) return this.loadFile(url);
    this.#state.url = url;
    return this.#load({ kind: 'url', url }, isRemoteUrl(url));
  }

  /**
   * Loads an app asset. Absolute paths under `app.getAppPath()` and
   * `file://` URLs are accepted and normalised.
   *
   * @param path - the asset path
   * @param options - query and hash
   * @returns resolves on `did-finish-load`, rejects on `did-fail-load`
   */
  loadFile(path: string, options: LoadFileOptions = {}): Promise<void> {
    this.#assertAlive('loadFile');
    const asset = toAssetPath(path, appPath());
    const target: Record<string, unknown> = { kind: 'file', path: asset };
    let query = options.query;
    if (query === undefined && options.search !== undefined) {
      query = Object.fromEntries(new URLSearchParams(options.search.replace(/^\?/, '')));
    }
    if (query !== undefined) target['query'] = query;
    if (options.hash !== undefined) target['hash'] = options.hash.replace(/^#/, '');
    this.#state.url = asset;
    return this.#load(target);
  }

  /** Shows and focuses the window; emits `show` when it was hidden. */
  show(): void {
    const changed = !this.#state.visible;
    this.#set({ visible: true, minimized: false, focused: true });
    this.#native(
      'show',
      undefined,
      changed
        ? () => {
            this.#emitVisibility('show');
          }
        : undefined,
    );
    this.#native('set_focus');
  }

  /**
   * Shows the window without requesting focus (partial: some platforms still
   * activate it); emits `show` when it was hidden.
   */
  showInactive(): void {
    const changed = !this.#state.visible;
    this.#set({ visible: true, minimized: false });
    this.#native(
      'show',
      undefined,
      changed
        ? () => {
            this.#emitVisibility('show');
          }
        : undefined,
    );
  }

  /** Hides the window; emits `hide` when it was visible. */
  hide(): void {
    const changed = this.#state.visible;
    this.#set({ visible: false, focused: false });
    this.#native(
      'hide',
      undefined,
      changed
        ? () => {
            this.#emitVisibility('hide');
          }
        : undefined,
    );
  }

  /** Requests a close: emits `close` (preventable), then `closed`. */
  close(): void {
    if (this.#destroyed) return;
    this.#native('close');
  }

  /** Closes without a `close` event. */
  destroy(): void {
    if (this.#destroyed) return;
    this.#op(({ hostId }) => kernel.command('window_destroy', { id: hostId })).catch(
      (error: unknown) => {
        kernel.log(
          'warn',
          `BrowserWindow ${String(this.id)}: destroy failed: ${(error as Error).message}`,
        );
      },
    );
  }

  /** Focuses the window. */
  focus(): void {
    this.#set({ focused: true });
    this.#native('set_focus');
  }

  /**
   * Removes focus. Tauri has no command for this; only the cached state
   * changes (`isFocused()` returns `false`).
   */
  blur(): void {
    this.#set({ focused: false });
  }

  /**
   * Whether the window is visible. As in Electron, a minimized window counts
   * as not visible on macOS (`NativeWindowMac::IsVisible`) and as visible on
   * Windows and Linux.
   *
   * @returns the cached state
   */
  isVisible(): boolean {
    if (this.#state.minimized && kernel.state.get('platform') === 'darwin') return false;
    return this.#state.visible;
  }

  /**
   * Whether the window has focus.
   *
   * @returns the cached state
   */
  isFocused(): boolean {
    return this.#state.focused;
  }

  /** Minimizes the window. */
  minimize(): void {
    this.#set({ minimized: true, focused: false });
    this.#native('minimize');
  }

  /**
   * Maximizes the window. A hidden window is shown first (not focused), and
   * `show` is emitted, as in Electron. A minimized window is restored first:
   * Electron's `maximize()` leaves no window minimized, while the native
   * zoom alone keeps a minimized window in the Dock.
   */
  maximize(): void {
    const minimized = this.#state.minimized;
    this.#showIfHidden();
    this.#set({ maximized: true, minimized: false });
    if (minimized) this.#native('unminimize');
    this.#native('maximize');
  }

  /** Leaves the maximized state. */
  unmaximize(): void {
    this.#set({ maximized: false });
    this.#native('unmaximize');
  }

  /**
   * Restores the window from the minimized state. A hidden window is shown
   * (not focused), and `show` is emitted.
   */
  restore(): void {
    this.#showIfHidden();
    this.#set({ minimized: false });
    this.#native('unminimize');
  }

  /**
   * Whether the window is minimized.
   *
   * @returns the cached state
   */
  isMinimized(): boolean {
    return this.#state.minimized;
  }

  /**
   * Whether the window is maximized.
   *
   * @returns the cached state
   */
  isMaximized(): boolean {
    return this.#state.maximized;
  }

  /**
   * Enters or leaves full screen; ignored when created with `fullscreenable: false`.
   *
   * @param flag - full screen or not
   */
  setFullScreen(flag: boolean): void {
    if (flag && !this.#state.fullscreenable) return;
    this.#set({ fullscreen: flag });
    this.#native('set_fullscreen', flag);
  }

  /**
   * Whether the window is in full screen.
   *
   * @returns the cached state
   */
  isFullScreen(): boolean {
    return this.#state.fullscreen;
  }

  /**
   * Sets the window bounds; missing fields keep their value.
   *
   * @param bounds - the new bounds in logical pixels
   */
  setBounds(bounds: Partial<Rectangle>): void {
    const next = { ...this.#state.bounds, ...bounds };
    this.#set({ bounds: next });
    this.#native('set_position', { Logical: { x: next.x, y: next.y } });
    this.#native('set_size', { Logical: { width: next.width, height: next.height } });
  }

  /**
   * The window bounds.
   *
   * @returns a copy of the cached bounds
   */
  getBounds(): Rectangle {
    return { ...this.#state.bounds };
  }

  /**
   * The content bounds (the cached window bounds; Tauri windows report inner sizes).
   *
   * @returns a copy of the cached bounds
   */
  getContentBounds(): Rectangle {
    return { ...this.#state.bounds };
  }

  /**
   * Resizes the window.
   *
   * @param width - logical width
   * @param height - logical height
   */
  setSize(width: number, height: number): void {
    this.#set({ bounds: { ...this.#state.bounds, width, height } });
    this.#native('set_size', { Logical: { width, height } });
  }

  /**
   * The window size.
   *
   * @returns `[width, height]`
   */
  getSize(): [number, number] {
    return [this.#state.bounds.width, this.#state.bounds.height];
  }

  /**
   * Same as {@link BrowserWindow.setSize}.
   *
   * @param width - logical width
   * @param height - logical height
   */
  setContentSize(width: number, height: number): void {
    this.setSize(width, height);
  }

  /**
   * Same as {@link BrowserWindow.getSize}.
   *
   * @returns `[width, height]`
   */
  getContentSize(): [number, number] {
    return this.getSize();
  }

  /**
   * Moves the window.
   *
   * @param x - logical left edge
   * @param y - logical top edge
   */
  setPosition(x: number, y: number): void {
    this.#set({ bounds: { ...this.#state.bounds, x, y } });
    this.#native('set_position', { Logical: { x, y } });
  }

  /**
   * The window position.
   *
   * @returns `[x, y]`
   */
  getPosition(): [number, number] {
    return [this.#state.bounds.x, this.#state.bounds.y];
  }

  /**
   * Sets the minimum size (`0, 0` removes it).
   *
   * @param width - logical width
   * @param height - logical height
   */
  setMinimumSize(width: number, height: number): void {
    this.#set({ minSize: [width, height] });
    this.#native('set_min_size', width > 0 || height > 0 ? { Logical: { width, height } } : null);
  }

  /**
   * The minimum size.
   *
   * @returns `[width, height]`
   */
  getMinimumSize(): [number, number] {
    return [...this.#state.minSize];
  }

  /**
   * Sets the maximum size (`0, 0` removes it).
   *
   * @param width - logical width
   * @param height - logical height
   */
  setMaximumSize(width: number, height: number): void {
    this.#set({ maxSize: [width, height] });
    this.#native('set_max_size', width > 0 || height > 0 ? { Logical: { width, height } } : null);
  }

  /**
   * The maximum size.
   *
   * @returns `[width, height]`
   */
  getMaximumSize(): [number, number] {
    return [...this.#state.maxSize];
  }

  /** Centres the window on its display's work area. */
  center(): void {
    const area = primaryWorkArea();
    const { width, height } = this.#state.bounds;
    this.#set({
      bounds: {
        x: Math.round(area.x + (area.width - width) / 2),
        y: Math.round(area.y + (area.height - height) / 2),
        width,
        height,
      },
    });
    this.#native('center');
  }

  /**
   * Allows or forbids resizing.
   *
   * @param resizable - the flag
   */
  setResizable(resizable: boolean): void {
    this.#set({ resizable });
    this.#native('set_resizable', resizable);
  }

  /**
   * Whether the window can be resized.
   *
   * @returns the cached state
   */
  isResizable(): boolean {
    return this.#state.resizable;
  }

  /**
   * Allows or forbids moving. Tauri has no such command; the flag is cached
   * and passed to `window_create` only.
   *
   * @param movable - the flag
   */
  setMovable(movable: boolean): void {
    this.#set({ movable });
  }

  /**
   * Whether the window can be moved.
   *
   * @returns the cached state
   */
  isMovable(): boolean {
    return this.#state.movable;
  }

  /**
   * Allows or forbids minimizing.
   *
   * @param minimizable - the flag
   */
  setMinimizable(minimizable: boolean): void {
    this.#set({ minimizable });
    this.#native('set_minimizable', minimizable);
  }

  /**
   * Whether the window can be minimized.
   *
   * @returns the cached state
   */
  isMinimizable(): boolean {
    return this.#state.minimizable;
  }

  /**
   * Allows or forbids maximizing.
   *
   * @param maximizable - the flag
   */
  setMaximizable(maximizable: boolean): void {
    this.#set({ maximizable });
    this.#native('set_maximizable', maximizable);
  }

  /**
   * Whether the window can be maximized.
   *
   * @returns the cached state
   */
  isMaximizable(): boolean {
    return this.#state.maximizable;
  }

  /**
   * Allows or forbids closing.
   *
   * @param closable - the flag
   */
  setClosable(closable: boolean): void {
    this.#set({ closable });
    this.#native('set_closable', closable);
  }

  /**
   * Whether the window can be closed by the user.
   *
   * @returns the cached state
   */
  isClosable(): boolean {
    return this.#state.closable;
  }

  /**
   * Keeps the window above others.
   *
   * @param flag - the flag (Electron's `level` and `relativeLevel` are ignored)
   */
  setAlwaysOnTop(flag: boolean): void {
    this.#set({ alwaysOnTop: flag });
    this.#native('set_always_on_top', flag);
  }

  /**
   * Whether the window is always on top.
   *
   * @returns the cached state
   */
  isAlwaysOnTop(): boolean {
    return this.#state.alwaysOnTop;
  }

  /** Partial: brings the window to the front by toggling always-on-top. */
  moveTop(): void {
    const keep = this.#state.alwaysOnTop;
    this.#native('set_always_on_top', true);
    if (!keep) this.#native('set_always_on_top', false);
  }

  /**
   * Hides or shows the window in the task bar.
   *
   * @param skip - the flag
   */
  setSkipTaskbar(skip: boolean): void {
    this.#set({ skipTaskbar: skip });
    this.#native('set_skip_taskbar', skip);
  }

  /**
   * Allows or forbids focusing.
   *
   * @param focusable - the flag
   */
  setFocusable(focusable: boolean): void {
    this.#set({ focusable });
    this.#native('set_focusable', focusable);
  }

  /**
   * Whether the window can take focus.
   *
   * @returns the cached state
   */
  isFocusable(): boolean {
    return this.#state.focusable;
  }

  /**
   * Lets mouse events pass through the window.
   *
   * @param ignore - the flag
   * @param _options - Electron's `{ forward }`, ignored
   */
  setIgnoreMouseEvents(ignore: boolean, _options?: { forward?: boolean }): void {
    this.#native('set_ignore_cursor_events', ignore);
  }

  /**
   * Sets the title.
   *
   * @param title - the title
   */
  setTitle(title: string): void {
    this.#set({ title });
    this.#native('set_title', title);
  }

  /**
   * The title.
   *
   * @returns the cached title
   */
  getTitle(): string {
    return this.#state.title;
  }

  /**
   * Sets the background colour.
   *
   * @param color - `#RGB`, `#RRGGBB` or `#AARRGGBB`
   */
  setBackgroundColor(color: string): void {
    const rgba = parseColor(color);
    if (!rgba) {
      kernel.warnOnce(
        `BrowserWindow#setBackgroundColor:${color}`,
        `setBackgroundColor: '${color}' is not a hex colour; ignored`,
      );
      return;
    }
    this.#set({ backgroundColor: color });
    this.#native('set_background_color', rgba);
  }

  /**
   * The background colour.
   *
   * @returns the cached colour
   */
  getBackgroundColor(): string {
    return this.#state.backgroundColor;
  }

  /**
   * Sets the task bar progress (`< 0` removes it, `> 1` is indeterminate).
   *
   * @param progress - 0 to 1
   * @param options - Electron's `{ mode }`: `none`, `normal`, `indeterminate`, `error`, `paused`
   */
  setProgressBar(progress: number, options?: { mode?: string }): void {
    let status =
      options?.mode ?? (progress < 0 ? 'none' : progress > 1 ? 'indeterminate' : 'normal');
    if (!['none', 'normal', 'indeterminate', 'paused', 'error'].includes(status)) status = 'normal';
    const value: Record<string, unknown> = { status };
    if (status === 'normal' || status === 'paused' || status === 'error') {
      value['progress'] = Math.round(Math.min(Math.max(progress, 0), 1) * 100);
    }
    this.#native('set_progress_bar', value);
  }

  /**
   * Requests (or stops requesting) the user's attention.
   *
   * @param flag - the flag
   */
  flashFrame(flag: boolean): void {
    this.#native('request_user_attention', flag ? 2 : null);
  }

  /**
   * Shows the window on every workspace (macOS, Linux).
   *
   * @param visible - the flag
   */
  setVisibleOnAllWorkspaces(visible: boolean): void {
    this.#native('set_visible_on_all_workspaces', visible);
  }

  /**
   * Excludes the window from screen capture.
   *
   * @param enable - the flag
   */
  setContentProtection(enable: boolean): void {
    this.#native('set_content_protected', enable);
  }

  /** Partial: no-op (Tauri windows have no menu unless the app adds one in Rust). */
  setMenu(): void {
    // no menu
  }

  /** Partial: no-op. */
  removeMenu(): void {
    // no menu
  }

  /** Partial: no-op. */
  setMenuBarVisibility(): void {
    // no menu
  }

  /** Partial: no-op. */
  setAutoHideMenuBar(): void {
    // no menu
  }

  /**
   * Starts a native drag of the window (ow-tauri addition, used by the
   * `app-region` emulation and overlay windows).
   */
  startDragging(): void {
    this.#native('start_dragging');
  }

  /**
   * Handles a `window` host message.
   *
   * @param message - the message
   * @internal
   */
  [ON_EVENT](message: WindowMessage): void {
    const data = (
      typeof message.data === 'object' && message.data !== null ? message.data : {}
    ) as Record<string, unknown>;
    const bounds = data['bounds'] as Partial<Rectangle> | undefined;
    if (bounds && typeof bounds === 'object')
      this.#set({ bounds: { ...this.#state.bounds, ...bounds } });
    const reg = registry();
    switch (message.event) {
      case 'close': {
        const event = createEvent();
        emitFromHost(this, 'close', event);
        if (typeof message.requestId === 'number') {
          kernel
            .command('window_close_reply', {
              id: message.id,
              requestId: message.requestId,
              prevent: event.defaultPrevented,
            })
            .catch((error: unknown) => {
              kernel.log('warn', `window_close_reply failed: ${(error as Error).message}`);
            });
        }
        return;
      }
      case 'closed':
        this.#closed();
        return;
      case 'focus':
        this.#set({ focused: true });
        reg.focusedId = this.id;
        emitFromHost(this, 'focus', createEvent());
        reg.hooks.focus?.(this);
        return;
      case 'blur':
        this.#set({ focused: false });
        if (reg.focusedId === this.id) reg.focusedId = null;
        emitFromHost(this, 'blur', createEvent());
        reg.hooks.blur?.(this);
        return;
      case 'show':
        this.#set({ visible: true });
        break;
      case 'hide':
        this.#set({ visible: false });
        break;
      case 'minimize':
        this.#set({ minimized: true });
        break;
      case 'restore':
        this.#set({ minimized: false });
        break;
      case 'maximize':
        this.#set({ maximized: true });
        break;
      case 'unmaximize':
        this.#set({ maximized: false });
        break;
      case 'enter-full-screen':
        this.#set({ fullscreen: true });
        break;
      case 'leave-full-screen':
        this.#set({ fullscreen: false });
        break;
      case 'resize':
      case 'move':
      case 'ready-to-show':
        break;
      default:
        this.webContents[ON_CONTENTS_EVENT](message.event, data, this.#loadWaiters);
        return;
    }
    emitFromHost(this, message.event, createEvent());
  }

  #closed(): void {
    if (this.#destroyed) return;
    this.#destroyed = true;
    const reg = registry();
    reg.windows.delete(this.id);
    if (reg.focusedId === this.id) reg.focusedId = null;
    for (const waiter of this.#loadWaiters.splice(0))
      waiter.reject(new OwTauriError('not-found', 'the window was closed before the page loaded'));
    emitFromHost(this, 'closed');
    this.webContents[ON_CONTENTS_EVENT]('destroyed', {}, []);
    kernel.server.dropScope(this.id);
    kernel.windowIds.forget(this.id);
    if (reg.windows.size === 0) reg.allClosed();
  }

  /** `window_create` failed: the window never existed, so no app event fires. */
  #failed(error: unknown): void {
    if (this.#destroyed) return;
    this.#destroyed = true;
    const reg = registry();
    reg.windows.delete(this.id);
    if (reg.focusedId === this.id) reg.focusedId = null;
    for (const waiter of this.#loadWaiters.splice(0)) waiter.reject(error);
    this.webContents[ON_CONTENTS_EVENT]('destroyed', {}, []);
    kernel.server.dropScope(this.id);
    kernel.windowIds.forget(this.id);
  }

  async #create(options: BrowserWindowConstructorOptions): Promise<Created> {
    const reg = registry();
    reg.pendingCreates++;
    try {
      // `new BrowserWindow({ parent })` right after the parent's constructor:
      // the parent's plugin id arrives with its own window_create response.
      const parent = options.parent as { whenCreated?: () => Promise<void> } | null | undefined;
      if (typeof parent?.whenCreated === 'function') {
        await parent.whenCreated().catch(() => undefined);
      }
      const response = (await kernel.command('window_create', {
        options: wireOptions(options),
        preload:
          typeof options.webPreferences?.preload === 'string'
            ? toAssetPath(options.webPreferences.preload, appPath())
            : null,
        windowClass: 'ui',
      })) as { id?: unknown; label?: unknown } | null;
      const hostId = response?.id;
      if (typeof hostId !== 'number')
        throw new OwTauriError('backend', 'window_create returned no window id');
      kernel.windowIds.bind(this.id, hostId);
      const label = typeof response?.label === 'string' ? response.label : `bw-${String(hostId)}`;
      this.#host = { hostId, label, webviewLabel: label };
      return this.#host;
    } catch (error) {
      kernel.log(
        'error',
        `new BrowserWindow: the native window could not be created: ${(error as Error).message}`,
      );
      this.#failed(error);
      throw error;
    } finally {
      reg.createSettled();
    }
  }

  #load(target: Record<string, unknown>, remote = false): Promise<void> {
    this.#state.loading = true;
    const loaded = new Promise<void>((resolve, reject) => {
      this.#loadWaiters.push({ resolve, reject });
    });
    this.#op(async (host) => {
      await kernel.command('window_load', { id: host.hostId, target });
      // A remote page lives in a fresh `bwr-<id>` webview from now on.
      if (remote) host.webviewLabel = `bwr-${String(host.hostId)}`;
    }).catch((error: unknown) => {
      this.#state.loading = false;
      for (const waiter of this.#loadWaiters.splice(0)) waiter.reject(error);
    });
    return loaded;
  }

  /**
   * Runs `fn` once the native window exists, after every earlier operation.
   *
   * @param fn - the operation
   * @returns the operation's result
   * @internal
   */
  [OP]<T>(fn: (host: Created) => Promise<T>): Promise<T> {
    return this.#op(fn);
  }

  #op<T>(fn: (host: Created) => Promise<T>): Promise<T> {
    const run = this.#chain.then(async () => fn(await this.#created));
    this.#chain = run.catch(() => undefined);
    return run;
  }

  /**
   * Tauri reports no visibility change, so `show()`, `showInactive()` and
   * `hide()` emit `show` / `hide` themselves after the native call succeeded
   * (CONTRACT A.3, B.2.2).
   */
  /** Shows a hidden window without focus; `show` follows the native call. */
  #showIfHidden(): void {
    if (this.#state.visible) return;
    this.#set({ visible: true });
    this.#native('show', undefined, () => {
      this.#emitVisibility('show');
    });
  }

  #emitVisibility(event: 'show' | 'hide'): void {
    if (this.#destroyed) return;
    emitFromHost(this, event, createEvent());
  }

  #native(command: string, value?: unknown, then?: () => void): void {
    if (this.#destroyed) return;
    this.#op(async ({ label }) => {
      await kernel.raw(
        `plugin:window|${command}`,
        value === undefined ? { label } : { label, value },
      );
      then?.();
    }).catch((error: unknown) => {
      kernel.log(
        'warn',
        `BrowserWindow ${String(this.id)}: ${command} failed: ${(error as Error).message}`,
      );
    });
  }

  #set(patch: Partial<WindowState>): void {
    Object.assign(this.#state, patch);
  }

  #assertAlive(api: string): void {
    kernel.require('main', `BrowserWindow#${api}`);
    if (this.#destroyed) throw new TypeError('Object has been destroyed');
  }
}

defineUnsupported(BrowserWindow.prototype, 'BrowserWindow#', [
  'setOpacity',
  'setVibrancy',
  'setShape',
  'capturePage',
  'setThumbarButtons',
  'setOverlayIcon',
  'previewFile',
  'setBrowserView',
  'addBrowserView',
  'setTouchBar',
]);

/**
 * Electron's `webContents` of a `BrowserWindow` (CONTRACT B.2.2).
 */
export class WebContents extends EventEmitter {
  /**
   * Brand-aware `instanceof`.
   *
   * @param value - the value on the left of `instanceof`
   * @returns whether `value` is a `WebContents`
   */
  static override [Symbol.hasInstance](value: unknown): boolean {
    return (
      typeof value === 'object' &&
      value !== null &&
      (value as Record<symbol, unknown>)[CONTENTS_BRAND] === true
    );
  }

  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.2).
   */
  declare readonly print: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.2).
   */
  declare readonly printToPDF: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.2).
   */
  declare readonly capturePage: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.2).
   */
  declare readonly setAudioMuted: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.2).
   */
  declare readonly startDrag: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.2).
   */
  declare readonly insertCSS: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.2).
   */
  declare readonly savePage: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.2).
   */
  declare readonly sendInputEvent: UnsupportedMethod;
  /**
   * Unsupported: throws `OwTauriUnsupportedError`.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.2).
   */
  declare readonly postMessage: UnsupportedMethod;
  /**
   * Unsupported: reads as `undefined` and logs a warning once.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.2).
   */
  declare readonly session: undefined;
  /**
   * Unsupported: reads as `undefined` and logs a warning once.
   *
   * @deprecated Unsupported in ow-tauri (CONTRACT B.2.2).
   */
  declare readonly debugger: undefined;
  /** Same as the window id. */
  readonly id: number;
  readonly #window: BrowserWindow;
  readonly #state: WindowState;
  #openHandler: ((details: HandlerDetails) => WindowOpenHandlerResponse) | null = null;
  #destroyed = false;

  /**
   * @param window - the owning window
   * @param state - the window's state cache
   * @internal
   */
  constructor(window: BrowserWindow, state: WindowState) {
    super();
    Object.defineProperty(this, CONTENTS_BRAND, { value: true });
    this.id = window.id;
    this.#window = window;
    this.#state = state;
  }

  /** The window's `ipcMain`-like scope, consulted before `ipcMain` (C.2, C.3). */
  get ipc(): IpcMain {
    return kernel.server.scoped(this.id);
  }

  /**
   * Whether the window was destroyed.
   *
   * @returns `true` after `destroyed`
   */
  isDestroyed(): boolean {
    return this.#destroyed;
  }

  /**
   * Sends a message to `ipcRenderer` listeners of this window (C.5).
   *
   * @param channel - the channel
   * @param args - the arguments (OTJ-encodable)
   * @throws OwTauriError `ipc-serialization` synchronously; `TypeError` when destroyed
   */
  send(channel: string, ...args: unknown[]): void {
    kernel.require('main', 'webContents.send');
    if (this.#destroyed) throw new TypeError('Object has been destroyed');
    if (kernel.windowIds.toHost(this.id) !== undefined) {
      kernel.server.emit(this.id, channel, args);
      return;
    }
    // The native window is still being created: encode now (errors are
    // synchronous, as in Electron) and send once it exists, in call order.
    checkChannel(channel, 'webContents.send');
    const encoded = encodeMessageArgs(args, kernel.server.maxMessageBytes);
    this.#window
      [OP](() => {
        if (!this.#destroyed) kernel.server.emitEncoded(this.id, channel, encoded);
        return Promise.resolve();
      })
      .catch(() => undefined);
  }

  /**
   * The URL of the loaded page.
   *
   * @returns the cached URL
   */
  getURL(): string {
    return this.#state.url;
  }

  /**
   * The window title.
   *
   * @returns the cached title
   */
  getTitle(): string {
    return this.#state.title;
  }

  /**
   * Whether a load is in progress.
   *
   * @returns the cached state
   */
  isLoading(): boolean {
    return this.#state.loading;
  }

  /**
   * See {@link BrowserWindow.loadURL}.
   *
   * @param url - the URL
   * @returns resolves on `did-finish-load`
   */
  loadURL(url: string): Promise<void> {
    return this.#window.loadURL(url);
  }

  /**
   * See {@link BrowserWindow.loadFile}.
   *
   * @param path - the asset path
   * @param options - query and hash
   * @returns resolves on `did-finish-load`
   */
  loadFile(path: string, options?: LoadFileOptions): Promise<void> {
    return this.#window.loadFile(path, options);
  }

  /** Reloads the page. */
  reload(): void {
    this.#state.loading = true;
    void this.#eval('location.reload()', false).catch((error: unknown) => {
      kernel.log('warn', `webContents.reload failed: ${(error as Error).message}`);
    });
  }

  /**
   * Partial: runs `code` through native evaluation (A.2.3 `window_eval`).
   * Resolves the value of expression code in UI windows; statement code and
   * remote windows resolve `undefined`. `userGesture` is ignored.
   *
   * @param code - the code
   * @param _userGesture - ignored
   * @returns the decoded result
   */
  executeJavaScript(code: string, _userGesture?: boolean): Promise<unknown> {
    kernel.require('main', 'webContents.executeJavaScript');
    return this.#eval(code, true);
  }

  /**
   * Partial: opens devtools (debug builds, or release builds with Tauri's
   * `devtools` feature); `mode` is ignored.
   *
   * @param _options - ignored
   */
  openDevTools(_options?: unknown): void {
    this.#devtools(true);
  }

  /** Closes devtools. */
  closeDevTools(): void {
    this.#devtools(false);
  }

  /** Toggles devtools. */
  toggleDevTools(): void {
    this.#devtools(!this.#state.devToolsOpened);
  }

  /**
   * Whether devtools are open (as last requested).
   *
   * @returns the cached state
   */
  isDevToolsOpened(): boolean {
    return this.#state.devToolsOpened;
  }

  /**
   * Sets the zoom factor.
   *
   * @param factor - 1 = 100 %
   */
  setZoomFactor(factor: number): void {
    this.#state.zoomFactor = factor;
    void this.#window
      [OP](({ webviewLabel }) =>
        kernel.raw('plugin:webview|set_webview_zoom', { label: webviewLabel, value: factor }),
      )
      .catch((error: unknown) => {
        kernel.log('warn', `setZoomFactor failed: ${(error as Error).message}`);
      });
  }

  /**
   * The zoom factor.
   *
   * @returns the cached factor
   */
  getZoomFactor(): number {
    return this.#state.zoomFactor;
  }

  /**
   * Partial: the handler is called for window-open requests;
   * `{ action: 'allow' }` opens the URL in the system browser (`http`, `https` only)
   * instead of a new app window.
   *
   * @param handler - the handler
   */
  setWindowOpenHandler(handler: (details: HandlerDetails) => WindowOpenHandlerResponse): void {
    this.#openHandler = handler;
  }

  /**
   * Handles window events that belong to the web contents.
   *
   * @param event - the event name
   * @param data - the event data
   * @param waiters - pending load promises
   * @internal
   */
  [ON_CONTENTS_EVENT](event: string, data: Record<string, unknown>, waiters: LoadWaiter[]): void {
    switch (event) {
      case 'did-finish-load':
        this.#state.loading = false;
        if (typeof data['url'] === 'string') this.#state.url = data['url'];
        for (const waiter of waiters.splice(0)) waiter.resolve();
        emitFromHost(this, 'did-finish-load', createEvent());
        return;
      case 'dom-ready':
        emitFromHost(this, 'dom-ready', createEvent());
        return;
      case 'did-fail-load': {
        this.#state.loading = false;
        const code = typeof data['errorCode'] === 'number' ? data['errorCode'] : -2;
        const description =
          typeof data['errorDescription'] === 'string' ? data['errorDescription'] : 'ERR_FAILED';
        const url =
          typeof data['validatedURL'] === 'string' ? data['validatedURL'] : this.#state.url;
        for (const waiter of waiters.splice(0)) {
          waiter.reject(
            new OwTauriError('io', `${description} (${String(code)}) loading '${url}'`, {
              data: { errorCode: code, errorDescription: description, url },
            }),
          );
        }
        emitFromHost(this, 'did-fail-load', createEvent(), code, description, url, true);
        return;
      }
      case 'render-process-gone':
        emitFromHost(this, 'render-process-gone', createEvent(), {
          reason: 'crashed',
          exitCode: num(data['exitCode'], 0),
        });
        return;
      case 'did-navigate-in-page': {
        // A fragment or History API change: `getURL()` follows it (B.2).
        const url = typeof data['url'] === 'string' ? data['url'] : this.#state.url;
        this.#state.url = url;
        emitFromHost(
          this,
          'did-navigate-in-page',
          createEvent(),
          url,
          data['isMainFrame'] !== false,
        );
        return;
      }
      case 'will-navigate':
        emitFromHost(
          this,
          'will-navigate',
          createEvent(),
          typeof data['url'] === 'string' ? data['url'] : '',
        );
        return;
      case 'new-window':
        this.#windowOpen(data);
        return;
      case 'destroyed':
        this.#destroyed = true;
        emitFromHost(this, 'destroyed');
        return;
      default:
        kernel.log('debug', `unknown window event '${event}' for window ${String(this.id)}`);
    }
  }

  #windowOpen(data: Record<string, unknown>): void {
    const details: HandlerDetails = {
      url: typeof data['url'] === 'string' ? data['url'] : '',
      frameName: typeof data['frameName'] === 'string' ? data['frameName'] : '',
      features: typeof data['features'] === 'string' ? data['features'] : '',
      disposition: typeof data['disposition'] === 'string' ? data['disposition'] : 'new-window',
    };
    let response: WindowOpenHandlerResponse = { action: 'deny' };
    if (this.#openHandler) {
      try {
        response = this.#openHandler(details);
      } catch (error) {
        kernel.log('warn', `setWindowOpenHandler handler threw: ${(error as Error).message}`);
      }
    }
    if (response.action === 'allow' && /^https?:/i.test(details.url)) {
      kernel.command('shell_open_external', { url: details.url }).catch((error: unknown) => {
        kernel.log('warn', `opening ${details.url} failed: ${(error as Error).message}`);
      });
    }
  }

  #devtools(open: boolean): void {
    this.#state.devToolsOpened = open;
    void this.#window
      [OP](({ hostId }) => kernel.command('window_devtools', { id: hostId, open }))
      .catch((error: unknown) => {
        this.#state.devToolsOpened = false;
        kernel.log('warn', `devtools: ${(error as Error).message}`);
      });
  }

  #eval(code: string, wantResult: boolean): Promise<unknown> {
    return this.#window
      [OP](({ hostId }) => kernel.command('window_eval', { id: hostId, code, wantResult }))
      .then(decode);
  }
}

defineUnsupported(
  WebContents.prototype,
  'webContents.',
  [
    'print',
    'printToPDF',
    'capturePage',
    'setAudioMuted',
    'startDrag',
    'insertCSS',
    'savePage',
    'sendInputEvent',
    'postMessage',
  ],
  ['session', 'debugger'],
  (key, message) => {
    kernel.warnOnce(key, message);
  },
);

/**
 * Whether `window_load` turns the window remote for this URL: an `http(s)` URL
 * outside the app origin (the main webview's own origin), A.2.3.1.
 *
 * @param url - the URL passed to `loadURL`
 * @returns `true` for a remote page
 */
function isRemoteUrl(url: string): boolean {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return false;
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return false;
  const origin = typeof location === 'object' ? location.origin : undefined;
  return parsed.origin !== origin;
}

function appPath(): string | undefined {
  const value = kernel.state.get('paths.appPath');
  return typeof value === 'string' ? value : undefined;
}

function checkOptions(options: BrowserWindowConstructorOptions): void {
  for (const [name, accepted] of Object.entries(UNSUPPORTED_OPTIONS)) {
    const value = options[name];
    if (value !== undefined && value !== accepted) {
      throw new OwTauriUnsupportedError(
        `BrowserWindow option ${name}`,
        'it has no equivalent in Tauri (see docs/CONTRACT.md section B.2.2)',
      );
    }
  }
  const prefs = options.webPreferences ?? {};
  if (prefs.nodeIntegration === true) {
    kernel.warnOnce(
      'webPreferences.nodeIntegration',
      'webPreferences.nodeIntegration is ignored: renderer code reaches electron only through the bundler alias',
    );
  }
  for (const name of IGNORED_PREFERENCES) {
    if (prefs[name] !== undefined)
      kernel.warnOnce(`webPreferences.${name}`, `webPreferences.${name} is ignored by ow-tauri`);
  }
  if (options.icon !== undefined && typeof options.icon !== 'string') {
    kernel.warnOnce(
      'BrowserWindow.icon',
      'BrowserWindow option icon must be an app-asset path; a NativeImage is ignored',
    );
  }
}

function wireOptions(options: BrowserWindowConstructorOptions): Record<string, unknown> {
  const wire: Record<string, unknown> = {};
  for (const name of WIRE_OPTIONS) {
    if (options[name] !== undefined) wire[name] = options[name];
  }
  if (typeof options.icon === 'string') wire['icon'] = toAssetPath(options.icon, appPath());
  const parent = options.parent;
  if (parent) {
    const parentId = kernel.windowIds.toHost(parent.id);
    if (parentId !== undefined) wire['parentId'] = parentId;
    else
      kernel.log(
        'warn',
        'BrowserWindow option parent refers to a window that does not exist yet; ignored',
      );
  }
  const prefs = options.webPreferences ?? {};
  const webPreferences: Record<string, unknown> = {};
  if (prefs.devTools !== undefined) webPreferences['devTools'] = prefs.devTools;
  if (prefs.zoomFactor !== undefined) webPreferences['zoomFactor'] = prefs.zoomFactor;
  wire['webPreferences'] = webPreferences;
  return wire;
}

function initialState(options: BrowserWindowConstructorOptions): WindowState {
  const area = primaryWorkArea();
  const width = num(options.width, 800);
  const height = num(options.height, 600);
  const show = bool(options.show, true);
  return {
    visible: show,
    focused: show && bool(options.focusable, true),
    minimized: false,
    maximized: false,
    fullscreen: bool(options.fullscreen, false),
    fullscreenable: bool(options.fullscreenable, true),
    bounds: {
      x: num(options.x, Math.round(area.x + (area.width - width) / 2)),
      y: num(options.y, Math.round(area.y + (area.height - height) / 2)),
      width,
      height,
    },
    minSize: [num(options.minWidth, 0), num(options.minHeight, 0)],
    maxSize: [num(options.maxWidth, 0), num(options.maxHeight, 0)],
    title: typeof options.title === 'string' ? options.title : '',
    alwaysOnTop: bool(options.alwaysOnTop, false),
    resizable: bool(options.resizable, true),
    movable: bool(options.movable, true),
    minimizable: bool(options.minimizable, true),
    maximizable: bool(options.maximizable, true),
    closable: bool(options.closable, true),
    focusable: bool(options.focusable, true),
    skipTaskbar: bool(options.skipTaskbar, false),
    backgroundColor:
      typeof options.backgroundColor === 'string' ? options.backgroundColor : '#FFFFFF',
    url: '',
    loading: false,
    zoomFactor: num(options.webPreferences?.zoomFactor, 1),
    devToolsOpened: false,
  };
}
