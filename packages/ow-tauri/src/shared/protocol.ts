/**
 * Wire types shared by the JS runtime and the Rust plugin.
 *
 * Every type here mirrors a table in `docs/CONTRACT.md`: host messages (A.3),
 * the bootstrap snapshot (A.2.1, B.1.6) and the IPC protocol (C). Field names
 * are the camelCase JSON keys on the wire.
 *
 * @packageDocumentation
 */
import type { OtjValue } from './otj.js';
import type { OverwolfErrorWire } from './wire-error.js';

/** Contract version this package implements (`docs/CONTRACT.md` header). */
export const CONTRACT_VERSION = 1;

/**
 * Version of the JavaScript interface between the injected bootstrap and the
 * npm facades (`FacadeKernel` in `src/bootstrap/facade-kernel.ts`).
 *
 * The bootstrap ships inside the crate and the facades ship on npm, so the two
 * are released separately. A facade attaches to an installed runtime only when
 * both its contract version and this number match; any incompatible change to
 * `FacadeKernel` increments it.
 */
export const RUNTIME_API_VERSION = 1;

/** Version of the `ow-tauri` npm package. */
export const PACKAGE_VERSION = '0.1.0';

/** Plugin name; commands are invoked as `plugin:overwolf|<name>`. */
export const PLUGIN = 'overwolf';

/** Where a runtime runs, from the calling webview's label (CONTRACT B, "Context check"). */
export type HostContext = 'main' | 'ui' | 'none';

/** A point in DIP. */
export interface Point {
  /** Horizontal coordinate. */
  x: number;
  /** Vertical coordinate. */
  y: number;
}

/** A size in DIP. */
export interface Size {
  /** Width. */
  width: number;
  /** Height. */
  height: number;
}

/** A rectangle in DIP. */
export interface Rectangle {
  /** Left edge. */
  x: number;
  /** Top edge. */
  y: number;
  /** Width. */
  width: number;
  /** Height. */
  height: number;
}

/**
 * A display, in Electron's `Display` shape (CONTRACT B.2.5). Rust always
 * sends `id`, `label`, `bounds`, `workArea`, `scaleFactor`; the rest is
 * optional and filled with Electron's defaults when absent.
 */
export interface Display {
  /** Stable 32-bit hash of the monitor's OS name and position. */
  id: number;
  /** The OS monitor name. */
  label: string;
  /** Bounds in DIP. */
  bounds: Rectangle;
  /** Work area (without task bars and docks) in DIP. */
  workArea: Rectangle;
  /** Device pixels per DIP. */
  scaleFactor: number;
  /** `bounds` size. */
  size?: Size;
  /** `workArea` size. */
  workAreaSize?: Size;
  /** Clockwise rotation in degrees: 0, 90, 180 or 270. */
  rotation?: number;
  /** Whether this is a built-in display. */
  internal?: boolean;
  /** Whether the display is monochrome. */
  monochrome?: boolean;
  /** Accelerometer availability. */
  accelerometerSupport?: 'available' | 'unavailable' | 'unknown';
  /** Touch availability. */
  touchSupport?: 'available' | 'unavailable' | 'unknown';
  /** Refresh rate in Hz. */
  displayFrequency?: number;
  /** Bits per pixel. */
  colorDepth?: number;
  /** Bits per color component. */
  depthPerComponent?: number;
  /** Color space description. */
  colorSpace?: string;
  /** Whether the display is detected. */
  detected?: boolean;
  /** Largest cursor size. */
  maximumCursorSize?: Size;
  /** Origin in physical pixels. */
  nativeOrigin?: Point;
}

/** The embedded `package.json` manifest (CONTRACT G.3). */
export interface EmbeddedManifest {
  /** `name`. */
  name: string;
  /** `build.productName`, else `productName`, else `name`. */
  productName: string;
  /** `version`. */
  version: string;
  /** `author` (string form). */
  author: string;
  /** The `overwolf` block. */
  overwolf: {
    /** `overwolf.packages`: the package names the app uses. */
    packages: string[];
    /** Console-assigned uid override, when set. */
    uid?: string;
  };
  /** The `build.overwolf` block. */
  buildOverwolf: {
    /** `disableAdOptimization`. */
    disableAdOptimization: boolean;
    /** `enablePackageBundling`. */
    enablePackageBundling: boolean;
    /** `overridePackagesUrl`, when set. */
    overridePackagesUrl?: string;
    /** `requireSigning`. */
    requireSigning: boolean;
    /** `enableOWCertSigning`. */
    enableOWCertSigning: boolean;
  };
  /** The whole `package.json` minus `devDependencies` and `scripts`. */
  raw: Record<string, unknown>;
}

/**
 * Names accepted by `app.getPath()` (CONTRACT B.2.1) plus `appPath`, the
 * virtual app root behind `app.getAppPath()`.
 */
export type ElectronPathName =
  | 'appData'
  | 'userData'
  | 'sessionData'
  | 'temp'
  | 'home'
  | 'desktop'
  | 'documents'
  | 'downloads'
  | 'music'
  | 'pictures'
  | 'videos'
  | 'logs'
  | 'exe'
  | 'crashDumps'
  | 'appPath';

/**
 * The snapshot Rust injects as `window.__OW_TAURI_BOOTSTRAP__` into `ow-main`
 * and returns from `bootstrap` (CONTRACT A.2.1). `platform` and `arch` are
 * ow-tauri additions used by the `process` shim (Node spelling); when absent
 * they are derived from the user agent.
 */
export interface HostSnapshot {
  /** Last applied state sequence number (C.6). */
  seq: number;
  /** Version strings. */
  versions: {
    /** The ow-tauri plugin version. */
    owTauri: string;
    /** The Tauri version. */
    tauri: string;
    /** The app version (manifest). */
    app: string;
    /** The webview engine version. */
    webview: string;
    /** The OS version. */
    os: string;
  };
  /** The embedded manifest. */
  manifest: EmbeddedManifest;
  /** App identity. */
  identity: {
    /** App uid (G.2). */
    uid: string;
    /** Console uid. */
    cuid: string;
    /** Machine id. */
    muid: string;
    /** Machine id, version 2. */
    muidV2: string;
    /** Rollout bucket, 0 to 99. */
    phasePercent: number;
  };
  /** `ow-electron.json` `utmParams`, or `null`. */
  utmParams: unknown;
  /** Process arguments and the ad mode. */
  switches: {
    /** The process arguments. */
    argv: string[];
    /** Whether test ads are shown (`--test-ad`). */
    testAd: boolean;
  };
  /** Paths for `app.getPath()`. */
  paths: Partial<Record<ElectronPathName, string>>;
  /** Whether this is a release build. */
  isPackaged: boolean;
  /** App locale. */
  locale: string;
  /** Display list. */
  displays: Display[];
  /** `id` of the primary display. */
  primaryDisplayId: number;
  /** Package manager state (CONTRACT A.2.4); opaque to the core runtime. */
  packages: unknown;
  /** Session switches. */
  flags: {
    /** `disableAnonymousAnalytics()` was called. */
    anonymousAnalyticsDisabled: boolean;
    /** `disableAdsOptimization()` was called. */
    adsOptimizationDisabled: boolean;
    /** `disableAdsFPD()` was called. */
    adsFpdDisabled: boolean;
  };
  /** Node-style platform (`win32`, `darwin`, `linux`); ow-tauri addition. */
  platform?: string;
  /** Node-style architecture (`x64`, `arm64`, ...); ow-tauri addition. */
  arch?: string;
  /** Cursor position in DIP when the snapshot was taken; ow-tauri addition. */
  cursor?: Point;
}

/**
 * What the bootstrap reads from `window.__OW_TAURI_BOOTSTRAP__` in a UI
 * window: the subset of {@link HostSnapshot} the `process` shim needs.
 */
export type RendererBootstrap = Partial<
  Pick<HostSnapshot, 'versions' | 'switches' | 'platform' | 'arch'>
>;

/** The sender of an IPC request, stamped by Rust (CONTRACT A.3). */
export interface IpcSender {
  /** Host window id of the sending webview. */
  windowId: number;
  /** Webview label. */
  label: string;
  /** Document URL. */
  url: string;
  /** Always 0 (main frame). */
  frameId: 0;
}

/** Window lifecycle events Rust reports to `ow-main` (CONTRACT A.3). */
export type WindowEventName =
  | 'created'
  | 'close'
  | 'closed'
  | 'focus'
  | 'blur'
  | 'show'
  | 'hide'
  | 'minimize'
  | 'maximize'
  | 'unmaximize'
  | 'restore'
  | 'resize'
  | 'move'
  | 'enter-full-screen'
  | 'leave-full-screen'
  | 'ready-to-show'
  | 'did-finish-load'
  | 'dom-ready'
  | 'did-fail-load'
  | 'render-process-gone';

/** Fields every `ipc` host message has. */
export interface IpcHostMessageBase {
  /** Message type. */
  type: 'ipc';
  /** The channel. */
  channel: string;
  /** OTJ-encoded arguments. */
  args: OtjValue[];
}

/** `ipc { kind: 'invoke' }`: an `ipcRenderer.invoke` for `ow-main` (C.2). */
export interface IpcInvokeMessage extends IpcHostMessageBase {
  /** The kind. */
  kind: 'invoke';
  /** The request id to answer with `ipc_reply`. */
  id: number;
  /** The sending window, stamped by Rust. */
  sender: IpcSender;
}

/** `ipc { kind: 'send' }`: an `ipcRenderer.send` for `ow-main` (C.3). */
export interface IpcSendMessage extends IpcHostMessageBase {
  /** The kind. */
  kind: 'send';
  /** The sending window, stamped by Rust. */
  sender: IpcSender;
}

/** `ipc { kind: 'message' }`: a `webContents.send` for a UI window (C.5). */
export interface IpcMessageMessage extends IpcHostMessageBase {
  /** The kind. */
  kind: 'message';
}

/** `ipc` host message (CONTRACT A.3). */
export type IpcHostMessage = IpcInvokeMessage | IpcSendMessage | IpcMessageMessage;

/** `ipc-result` host message: the reply to an `ipc_invoke` (C.2). */
export interface IpcResultMessage {
  /** Message type. */
  type: 'ipc-result';
  /** The request id `ipc_invoke` returned. */
  id: number;
  /** Whether the handler succeeded. */
  ok: boolean;
  /** OTJ-encoded value; absent decodes to `undefined`. */
  value?: OtjValue;
  /** The error when `ok` is false. */
  error?: OverwolfErrorWire;
}

/** One state-cache patch. */
export interface StatePatch {
  /** Dot path into the snapshot, e.g. `displays`. */
  path: string;
  /** The new value. */
  value: unknown;
}

/** `state` host message: sync-cache patches (B.1.6). */
export interface StateMessage {
  /** Message type. */
  type: 'state';
  /** State sequence number. */
  seq: number;
  /** Dot-path patches into the snapshot, applied in order. */
  patches: StatePatch[];
}

/** `window` host message (A.3). */
export interface WindowMessage {
  /** Message type. */
  type: 'window';
  /** Host window id. */
  id: number;
  /** The event. */
  event: WindowEventName | (string & {});
  /** Present on `close`: answer with `window_close_reply`. */
  requestId?: number;
  /** Event details. */
  data?: unknown;
}

/** `lifecycle` host message (A.3, A.6). */
export interface LifecycleMessage {
  /** Message type. */
  type: 'lifecycle';
  /** The lifecycle step. */
  event: 'before-quit' | 'will-quit' | 'quit' | (string & {});
  /** Present on `before-quit` / `will-quit`: answer with `app_quit_reply`. */
  requestId?: number;
  /** Present on `quit`. */
  exitCode?: number;
  /** Further fields of ow-tauri lifecycle events (`second-instance`, `activate`). */
  [key: string]: unknown;
}

/** `global-shortcut` host message (A.3). */
export interface GlobalShortcutMessage {
  /** Message type. */
  type: 'global-shortcut';
  /** The id passed to `global_shortcut_register`. */
  id: number;
  /** The accelerator. */
  accelerator: string;
  /** Key state. */
  state: 'pressed' | 'released';
}

/** Any message Rust sends over a webview's channel. */
export type HostMessage =
  | IpcHostMessage
  | IpcResultMessage
  | StateMessage
  | WindowMessage
  | LifecycleMessage
  | GlobalShortcutMessage
  | {
      /** The message type. */
      type: string;
      [key: string]: unknown;
    };

/** Shape of the global `window.__TAURI_INTERNALS__` the runtime reads. */
export interface TauriInternals {
  /** The IPC entry point. */
  invoke?: (cmd: string, args?: unknown, options?: unknown) => Promise<unknown>;
  /** Window metadata. */
  metadata?: {
    currentWindow?: { label?: string };
    currentWebview?: { label?: string; windowLabel?: string };
  };
}
