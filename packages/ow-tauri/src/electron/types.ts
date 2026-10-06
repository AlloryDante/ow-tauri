/**
 * Electron types used by the `ow-tauri/electron` facade, re-declared with
 * the subset ow-tauri supports (`docs/CONTRACT.md` section B.2). Members
 * ow-tauri does not support are kept so code compiles, and marked
 * `@deprecated Unsupported in ow-tauri` so editors flag them.
 *
 * @packageDocumentation
 */
export type { Display, Point, Rectangle, Size } from '../shared/protocol.js';

/**
 * The synthetic `Event` ow-tauri passes where Electron passes one
 * (CONTRACT B.1.2).
 */
export interface Event {
  /** Cancels the default action of actionable events (`close`, `before-quit`, `will-quit`, ...). */
  preventDefault(): void;
  /** Whether {@link Event.preventDefault} was called. */
  readonly defaultPrevented: boolean;
}

/** `webPreferences` of a `BrowserWindow` (CONTRACT B.2.2). */
export interface WebPreferences {
  /** App-asset path of a bundled preload script; injected as an initialization script. */
  preload?: string;
  /** Whether devtools can be opened. */
  devTools?: boolean;
  /** Partial: the preload shares the page's world; `contextBridge` defines frozen globals. */
  contextIsolation?: boolean;
  /** Partial: ignored with a warning; `require('electron')` works through the bundler alias only. */
  nodeIntegration?: boolean;
  /** Initial zoom factor. */
  zoomFactor?: number;
  /** Partial: ignored with a warning. */
  sandbox?: boolean;
  /** Partial: ignored with a warning. */
  webSecurity?: boolean;
  /** Partial: ignored with a warning. */
  partition?: string;
  /** Partial: ignored with a warning. */
  session?: unknown;
  /** Partial: ignored with a warning. */
  offscreen?: boolean;
  /** Partial: ignored with a warning. */
  webviewTag?: boolean;
  /** Partial: ignored with a warning. */
  backgroundThrottling?: boolean;
  /** Other Electron preferences: ignored with a warning. */
  [key: string]: unknown;
}

/** Options of `new BrowserWindow(options)` (CONTRACT B.2.2). */
export interface BrowserWindowConstructorOptions {
  /** Width in logical pixels (default 800). */
  width?: number;
  /** Height in logical pixels (default 600). */
  height?: number;
  /** Left edge; centred on the primary display when absent. */
  x?: number;
  /** Top edge; centred on the primary display when absent. */
  y?: number;
  /** Centre the window. */
  center?: boolean;
  /** Minimum width. */
  minWidth?: number;
  /** Minimum height. */
  minHeight?: number;
  /** Maximum width. */
  maxWidth?: number;
  /** Maximum height. */
  maxHeight?: number;
  /** `width` / `height` describe the web page rather than the window. */
  useContentSize?: boolean;
  /** Show the window when created (default `true`). */
  show?: boolean;
  /** Window title. */
  title?: string;
  /** Resizable (default `true`). */
  resizable?: boolean;
  /** Movable (default `true`). */
  movable?: boolean;
  /** Minimizable (default `true`). */
  minimizable?: boolean;
  /** Maximizable (default `true`). */
  maximizable?: boolean;
  /** Closable (default `true`). */
  closable?: boolean;
  /** Focusable (default `true`). */
  focusable?: boolean;
  /** Always on top. */
  alwaysOnTop?: boolean;
  /** Start in full screen. */
  fullscreen?: boolean;
  /** Partial: `false` makes the facade ignore `setFullScreen(true)`. */
  fullscreenable?: boolean;
  /** Hide from the task bar. */
  skipTaskbar?: boolean;
  /** Transparent window. */
  transparent?: boolean;
  /** Background colour (`#RGB`, `#RRGGBB`, `#AARRGGBB`). */
  backgroundColor?: string;
  /** Owner window. */
  parent?: BrowserWindowLike | null;
  /** Partial: owned by `parent` and kept above it; input modality Windows-only. */
  modal?: boolean;
  /** Partial: `false` = no decorations; an overlay title bar on macOS. */
  frame?: boolean;
  /** ow-electron: window name used for analytics. */
  name?: string;
  /** Partial: an app-asset path only. */
  icon?: unknown;
  /** Web preferences. */
  webPreferences?: WebPreferences;
  /** @deprecated Unsupported in ow-tauri: throws `OwTauriUnsupportedError` unless `'default'`. */
  titleBarStyle?: string;
  /** @deprecated Unsupported in ow-tauri. */
  trafficLightPosition?: unknown;
  /** @deprecated Unsupported in ow-tauri. */
  vibrancy?: string;
  /** @deprecated Unsupported in ow-tauri: throws unless `'followWindow'`. */
  visualEffectState?: string;
  /** @deprecated Unsupported in ow-tauri: throws unless `true`. */
  roundedCorners?: boolean;
  /** @deprecated Unsupported in ow-tauri: throws unless `true`. */
  thickFrame?: boolean;
  /** @deprecated Unsupported in ow-tauri. */
  type?: string;
  /** @deprecated Unsupported in ow-tauri. */
  tabbingIdentifier?: string;
  /** @deprecated Unsupported in ow-tauri: throws unless `false`. */
  kiosk?: boolean;
  /** @deprecated Unsupported in ow-tauri: throws unless `false`. */
  simpleFullscreen?: boolean;
  /** Other Electron options: ignored with a warning. */
  [key: string]: unknown;
}

/** Anything with a window `id` (a `BrowserWindow` from any copy of the facade). */
export interface BrowserWindowLike {
  /** The app-visible window id. */
  readonly id: number;
}

/** A file filter of the open and save dialogs. */
export interface FileFilter {
  /** Display name. */
  name: string;
  /** Extensions without dots; `*` for all. */
  extensions: string[];
}

/** Options of `dialog.showOpenDialog` (Electron shape). */
export interface OpenDialogOptions {
  /** Title. */
  title?: string;
  /** Initial path. */
  defaultPath?: string;
  /** Confirm button label. */
  buttonLabel?: string;
  /** Filters. */
  filters?: FileFilter[];
  /** `openFile`, `openDirectory`, `multiSelections`, `showHiddenFiles`, ... */
  properties?: string[];
  /** macOS message. */
  message?: string;
}

/** Result of `dialog.showOpenDialog`. */
export interface OpenDialogReturnValue {
  /** Whether the user cancelled. */
  canceled: boolean;
  /** Chosen paths. */
  filePaths: string[];
}

/** Options of `dialog.showSaveDialog` (Electron shape). */
export interface SaveDialogOptions {
  /** Title. */
  title?: string;
  /** Initial path. */
  defaultPath?: string;
  /** Confirm button label. */
  buttonLabel?: string;
  /** Filters. */
  filters?: FileFilter[];
  /** macOS message. */
  message?: string;
  /** macOS name field label. */
  nameFieldLabel?: string;
  /** Show the tags field (macOS). */
  showsTagField?: boolean;
  /** `showHiddenFiles`, `createDirectory`, ... */
  properties?: string[];
}

/** Result of `dialog.showSaveDialog`. */
export interface SaveDialogReturnValue {
  /** Whether the user cancelled. */
  canceled: boolean;
  /** Chosen path; empty when cancelled. */
  filePath: string;
}

/** Options of `dialog.showMessageBox` (Electron shape). */
export interface MessageBoxOptions {
  /** The message. */
  message: string;
  /** `none`, `info`, `error`, `question` or `warning`. */
  type?: 'none' | 'info' | 'error' | 'question' | 'warning';
  /** Up to three button labels (partial: more is rejected). */
  buttons?: string[];
  /** Index of the default button. */
  defaultId?: number;
  /** Title. */
  title?: string;
  /** Extra text. */
  detail?: string;
  /** Check box label. */
  checkboxLabel?: string;
  /** Initial check box state. */
  checkboxChecked?: boolean;
  /** Index of the cancel button. */
  cancelId?: number;
  /** Windows: no command links. */
  noLink?: boolean;
}

/** Result of `dialog.showMessageBox`. */
export interface MessageBoxReturnValue {
  /** Index of the clicked button. */
  response: number;
  /** Check box state. */
  checkboxChecked: boolean;
}

/** Options of `webContents.loadFile` / `BrowserWindow.loadFile`. */
export interface LoadFileOptions {
  /** Query parameters. */
  query?: Record<string, string>;
  /** A raw query string (`a=1&b=2`); used when `query` is absent. */
  search?: string;
  /** Fragment. */
  hash?: string;
}

/** `setWindowOpenHandler` details. */
export interface HandlerDetails {
  /** Requested URL. */
  url: string;
  /** Requested frame name. */
  frameName: string;
  /** Window features. */
  features: string;
  /** Disposition. */
  disposition: string;
}

/** `setWindowOpenHandler` result that blocks the request. */
export interface WindowOpenDeny {
  /** Block the window. */
  action: 'deny';
}

/** `setWindowOpenHandler` result that opens the URL in the system browser. */
export interface WindowOpenAllow {
  /** Open `http(s)` URLs in the system browser; other Electron fields are ignored. */
  action: 'allow';
  /** Electron's `overrideBrowserWindowOptions` and similar fields (ignored). */
  [key: string]: unknown;
}

/** `setWindowOpenHandler` result. */
export type WindowOpenHandlerResponse = WindowOpenDeny | WindowOpenAllow;
