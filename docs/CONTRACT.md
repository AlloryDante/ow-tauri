# ow-tauri contract

This is the specification that the Rust plugin (`tauri-plugin-overwolf`) and the
npm package (`ow-tauri`) are both implemented against. When code and this
document disagree, the code is wrong until this document is changed in the
same pull request.

Contract version: **1** (ow-tauri 0.1.0). Reference versions: ow-electron
42.11.4, `@overwolf/ow-electron-packages-types` 1.1.12, Tauri 2.12.1.

| Section | Covers |
|---|---|
| [0. Conventions](#0-conventions) | naming, casing, labels, sources |
| [A. Rust plugin](#a-rust-plugin) | configuration, every command, every host message, errors, the Rust API, main-webview liveness and lifecycle |
| [B. JavaScript API](#b-javascript-api) | `ow-tauri/main`, `ow-tauri/electron`, `ow-tauri/renderer`, typings |
| [C. IPC routing protocol](#c-ipc-routing-protocol) | invoke, send, reply, ids, ordering, back-pressure, serialisation, errors |
| [D. Guest shim contract](#d-guest-shim-contract) | `window.__overwolf__` in the ad page, host and guest messages, the consent page shim |
| [E. Analytics](#e-analytics) | events, endpoints, cadence, fields, opt-outs, options |
| [F. Per-app state file](#f-per-app-state-file) | path, shape, migration from ow-electron |
| [G. Manifest](#g-manifest) | `package.json` `overwolf` and `build.overwolf`, and how each field is honoured |
| [H. Package runtime interface](#h-package-runtime-interface) | Rust trait, JSON-RPC sidecar, C ABI, simulated backends |
| [I. Updater](#i-updater) | electron-updater generic feeds, verification, install per OS |

---

## 0. Conventions

- **Plugin name** `overwolf`. Commands are invoked as `plugin:overwolf|<name>`;
  permissions are `overwolf:<set>` and `overwolf:allow-<command-kebab>`.
- **Host messages.** Rust never uses Tauri events. Everything it sends to a
  webview travels over that webview's own ordered `tauri::ipc::Channel`
  (A.3, [ADR 0010](adr/0010-per-webview-ipc-channels.md)) as a `HostMessage`
  with a kebab-case `type`.
- **Casing.** Command arguments and all JSON payloads use `camelCase` keys
  (`#[serde(rename_all = "camelCase")]`). String enums use the exact spelling of
  the ow-electron typings (`'noPassThrough'`, `'failed-to-initialize'`, ...).
- **Labels** (see [ARCHITECTURE section 3.1](ARCHITECTURE.md#31-window-classes)):
  `ow-main`, `bw-<id>` (local UI and overlay webviews), `bwr-<id>` (a remote
  page shown in window `bw-<id>`), `owad-<embedderLabel>-<n>`, `ow-cmp`. App
  code never sees labels; it sees Electron-style integer ids. Capabilities
  match webview labels only (ARCHITECTURE section 5.2).
- **Time** values are milliseconds (`u64` in Rust, `number` in JS) unless a
  field name says otherwise. Timestamps are Unix epoch milliseconds.
- **Optional** fields may be absent; `null` is accepted wherever a field is
  optional.
- **Sources.** Behaviour copied from ow-electron is taken from Overwolf's
  documentation, the published typings, the official sample, the published
  builder JavaScript, or observation of a running ow-electron app. Where
  ow-tauri had to choose a behaviour that those sources do not pin down, the
  choice is marked **Interim** and listed in [OPEN-QUESTIONS.md](OPEN-QUESTIONS.md)
  with its question id (for example OQ-07). A behaviour that is not pinned
  down by those sources at all and that ow-tauri still offers is an
  **ow-tauri option**: off by default, configurable, and listed as "needs
  Overwolf confirmation".
- **Reference implementation.** Several modules (ads host, guest scripts,
  consent, analytics) are extracted from an earlier proof of concept that
  rendered Overwolf's test ads in Tauri child webviews and was verified
  headlessly. Where this document says "as the reference implementation does",
  that code and its tests are the source.

---

## A. Rust plugin

### A.1 Configuration

Configuration is read from `tauri.conf.json > plugins > overwolf`, then
overridden by `Builder` calls, then by environment variables, then by command
line switches (last wins). Every field is optional.

```jsonc
{
  "plugins": {
    "overwolf": {
      "main": {
        "url": "main.html",          // app asset loaded into the hidden main webview
        "devtools": false,           // open devtools for ow-main in debug builds
        "crashRestartLimit": 3       // ow-main crashes per 60 s before the app exits (A.6)
      },
      "webview": {                    // one browser-argument set for every webview (A.1.1)
        "disableGpu": false,          // --disable-gpu (Windows)
        "remoteDebuggingPort": null,  // --remote-debugging-port=<n> (Windows, debug builds)
        "additionalBrowserArgs": []   // appended verbatim (Windows)
      },
      "uid": null,                    // console-assigned uid; overrides the computed uid
      "packagesBackend": "auto",      // "auto" | "native" | "simulated" | "none"
      "ads": {
        "testAd": false,              // true = test inventory (same as --test-ad)
        "gestureWindowMs": 1500,      // user-gesture window for guest top-level navigation
        "maxRecoveries": 10,          // guest reloads after crashes, per element
        "loadErrorRetryMs": 5000,     // reload delay after a failed guest load
        "exposeEmailHashesToGuest": false,  // OQ-11
        "requestShaping": false,      // extra guest request headers, OQ-05
        "experimentalElementApi": false,    // pageUrl, setPageUrl(), sendCommand() (OQ-32)
        "guestLimits": {              // per guest webview (D.4)
          "eventsPerSecond": 50, "eventBurst": 100,
          "bytesPerSecond": 262144,
          "externalOpensPerMinute": 5
        }
      },
      "analytics": {
        "hostFields": false,          // append host, hostVersion, platform (OQ-03)
        "muidStrategy": "per-install",// "per-install" | "machine-id" (OQ-02)
        "userSwitch": false           // expose analytics_set_user_enabled
      },
      "consent": {
        "cmpRequired": "always",      // "always" | "never" | { "url": "https://..." } (OQ-06)
        "cmpUrl": null,               // default consent page URL override (https://content.overwolf.com only, D.6)
        "gateAdsOnConsent": true,     // first ad mount waits for consent readiness
        "readyTimeoutMs": 30000       // consent page must send `ready` within this time (D.6)
      },
      "emailHashes": { "encoding": "hex" },   // "hex" | "base64" (OQ-10)
      "ipc": {
        "invokeTimeoutMs": 0,         // 0 = no timeout (Electron behaviour)
        "startupQueueMax": 1024,      // requests buffered before ow-main is ready
        "startupTimeoutMs": 30000,    // a buffered request fails with not-ready after this
        "maxMessageBytes": 8388608,   // encoded size cap per message
        "maxInFlightInvokes": 256,    // per sender webview; more reject with ipc-overloaded
        "maxQueuedMessages": 4096     // per receiving webview; more reject the sender with ipc-overloaded
      },
      "updater": {
        "enabled": true,
        "installerArgs": null,
        "publisherNames": null,       // Windows Authenticode subjects; null = the running exe's signer (I.3)
        "pubkey": null                // minisign public key; required on Linux (I.3)
      },
      "shell": { "openPathAllowExecutables": false },  // A.2.3 shell_open_path
      "fs": { "scope": [] },          // extra read-write directories for fs_* (A.2.3)
      "state": { "appDataDir": null } // override for tests; default is the OS config dir
    }
  }
}
```

Environment variables:

| Variable | Effect |
|---|---|
| `OW_TAURI_TEST_AD=1` | same as `ads.testAd: true` |
| `OW_TAURI_PACKAGES_BACKEND=<value>` | overrides `packagesBackend` |
| `OW_TAURI_PACKAGE_RUNTIME=<path>` | registers a native runtime: a sidecar executable, or a shared library exporting `ow_runtime_v1_entry` (section H) |
| `OW_TAURI_REMOTE_DEBUGGING_PORT=<n>` | same as `webview.remoteDebuggingPort` (debug builds only) |
| `OW_CLI_EMAIL`, `OW_CLI_API_KEY`, `OW_DEV_KEY` | passed to the native runtime as dev-mode credentials; never logged, never sent anywhere by ow-tauri itself |

Command line switches (read from the process arguments at setup):

| Switch | Source of the name | Effect |
|---|---|---|
| `--test-ad` | ow-electron documentation | test ad inventory |
| `--owepm-package-channel=<pkg>:<channel>[,...]` | ow-electron documentation | single-run channel override, forwarded to the package runtime |
| `--force-phased-package[=<pkg>,...]` | ow-electron documentation | forwarded to the package runtime |
| `--owepm-packages-url=<url>` | ow-electron documentation (QA) | forwarded to the package runtime |
| `--ow-tauri-packages-backend=<value>` | ow-tauri | overrides `packagesBackend` |

`app.commandLine.hasSwitch()` / `getSwitchValue()` in `ow-tauri/electron` see
the same argument list.

#### A.1.1 Webview environment

WebView2 requires every webview that shares a user-data directory to use the
same browser arguments, and passing custom arguments replaces wry's default
`--disable-features=...` value. The plugin therefore computes **one** argument
string at setup, from `webview.*`, the environment and the command line, and
applies it to every webview it creates: `ow-main`, UI and overlay windows,
remote pages, ad guests and the consent window. On Windows the string is:

```
--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection
--disable-background-timer-throttling --disable-renderer-backgrounding
--disable-backgrounding-occluded-windows
[--disable-gpu] [--remote-debugging-port=<n>] [<additionalBrowserArgs>...]
```

The first value is wry's default, kept on purpose; the next three keep the
hidden main webview's timers running (A.6). `app.commandLine.appendSwitch()`
and `app.disableHardwareAcceleration()` called from app code at runtime cannot
change arguments of webviews that already exist: the call is recorded in
`ow-tauri.json` (`pendingBrowserArgs`), applied on the next launch, and logs a
warning (partial, B.2.1). Each session replaces the stored set with the calls
it made before `main_ready`, so an app that stops calling them gets the
defaults back on the following launch. macOS and Linux have no browser
arguments; the same settings are ignored there with a warning.

### A.2 Commands

Each table lists the permission set that grants the command. Argument and
return types are TypeScript notation for the JSON on the wire; Rust types are
the `camelCase` serde equivalents. "Errors" lists the `code` values from
[A.4](#a4-errors) a command can return beyond `forbidden` (wrong caller class,
possible for every command) and `invalid-argument` (malformed arguments,
possible for every command with arguments).

#### A.2.1 Main webview: bootstrap and lifecycle (`overwolf:main`)

| Command | Arguments | Returns | Errors | Behaviour |
|---|---|---|---|---|
| `ipc_subscribe` | `{ onMessage: Channel<HostMessage[]> }` | `{ epoch: string }` | none | Registers the calling document's host-message channel (A.3, C.1). Called once per document load, before any other command except `bootstrap`. Returns a fresh random `epoch` that tags this document's IPC sequence numbers (C.3). A second call from the same webview replaces the first channel and resets that webview's IPC state. |
| `bootstrap` | none | `HostSnapshot` | none | Returns the current snapshot, the same shape as the injected `window.__OW_TAURI_BOOTSTRAP__`. Used to resynchronise the cache after a state sequence gap (B.1.6). |
| `main_ready` | none | `void` | none | The app's top-level module code has run and `app.whenReady()` is about to resolve. Seals pre-ready switches; starts analytics and package loading. Idempotent. If it never arrives, both start after 10 s with a warning. |
| `ipc_main_ready` | none | `void` | none | `ipcMain` is installed; flushes the startup queue (section C.4). |
| `app_quit_reply` | `{ requestId: number; prevent: boolean }` | `void` | `not-found` (unknown or expired request) | Answer to a `lifecycle` `before-quit` message (A.6). |
| `app_relaunch` | `{ args?: string[], execPath?: null }` | `void` | `unsupported` (non-null `execPath`) | Schedules a relaunch on the next `app_exit` / `app_quit`, like Electron `app.relaunch()`. |
| `app_quit` | none | `void` | none | Graceful exit: the A.6 quit sequence (`before-quit`, window `close` events, `will-quit`), analytics drain, then exit. |
| `app_exit` | `{ code?: number }` | `void` | none | Immediate exit with `code` (default 0) after an analytics drain of at most 1.5 s. |
| `app_focus` | `{ steal?: boolean }` | `void` | none | Focuses the most recent visible UI window. |
| `log` | `{ level: 'debug'\|'info'\|'warn'\|'error', message: string }` | `void` | none | Appends to the ow-tauri log file (section F.4). |
| `ipc_reply` | `{ id: number; ok: boolean; value?: OtjValue; error?: IpcErrorWire; seq: number }` | `void` | none (unknown ids are ignored) | Answers an `invoke` request (C.2). `seq` is the main runtime's per-target counter shared with `ipc_emit`, so a reply is delivered after every message the handler sent to the same window before returning (C.5). An absent `value` decodes to `undefined`. |
| `ipc_emit` | `{ target: number; channel: string; args: OtjValue[]; seq: number }` | `void` | `ipc-serialization`, `ipc-overloaded` | `webContents.send` / `event.reply` to window id `target` (C.5). |

`HostSnapshot`:

```ts
interface HostSnapshot {
  seq: number;                              // last applied state sequence number (C.6)
  versions: { owTauri: string; tauri: string; app: string; webview: string; os: string };
  manifest: EmbeddedManifest;               // section G
  identity: { uid: string; cuid: string; muid: string; muidV2: string; phasePercent: number };
  utmParams: unknown;                       // ow-electron.json utmParams, or null
  switches: { argv: string[]; testAd: boolean };
  paths: Record<ElectronPathName, string>;  // see B.2 app.getPath
  isPackaged: boolean;
  locale: string;
  displays: ElectronDisplay[];              // see B.2 screen
  primaryDisplayId: number;
  packages: PackagesSnapshot;               // A.2.4
  flags: { anonymousAnalyticsDisabled: boolean; adsOptimizationDisabled: boolean; adsFpdDisabled: boolean };
}
```

#### A.2.2 Main webview: Overwolf API (`overwolf:main`)

| Command | Arguments | Returns | Errors | Mirrors |
|---|---|---|---|---|
| `disable_anonymous_analytics` | none | `void` | none | `app.overwolf.disableAnonymousAnalytics()`. Before `main_ready`: the session sends only the mandatory subset (E.3). After: applies to later events and logs a warning. Per session; not persisted. |
| `disable_ads_optimization` | none | `void` | none | `disableAdsOptimization()`. Sets `settings.disableOptimization: true` for guests mounted afterwards and delivers nothing to running guests. Per session. |
| `disable_ads_fpd` | none | `void` | none | `disableAdsFPD()`. Clears stored hashes and stops hash delivery. Per session. |
| `is_cmp_required` | none | `boolean` | none (never fails) | `isCMPRequired()`. **Interim (OQ-06):** `consent.cmpRequired` decides; default `"always"` returns `true`, the documented default value. A configured URL is fetched once per session (5 s timeout); the body must be JSON `true` / `false` or an object with a boolean `required`; anything else, or any failure, returns `true`. |
| `open_cmp_window` | `{ options?: CmpWindowOptions }` | `void` | `invalid-argument` (`cmpURL` outside the allowed scope, D.6), `io` | `openCMPWindow(options)`. Resolves when the consent window closes, including the D.6 timeout. If one is open it is focused and the call resolves when it closes. |
| `open_ad_privacy_settings_window` | `{ options?: CmpWindowOptions }` | `void` | `io` | `openAdPrivacySettingsWindow(options)`. Same window and lifecycle as `open_cmp_window`, `tabName` from `options.tab` (default `purposes`). |
| `set_user_email_hashes` | `{ hashes?: EmailHashes \| null }` | `void` | none | `setUserEmailHashes()`. `null`, absent, or all fields empty clears. Ignored (with a warning) after `disable_ads_fpd`. |
| `set_external_payment_user_id` | `{ options: ExternalPaymentUserIdOptions }` | `void` | `invalid-argument` (missing `userId`), `not-ready` (before `main_ready`, message exactly `ow-electron is not ready yet!`) | `setExternalPaymentUserId()`. **Interim (OQ-12):** validated and stored for the session; no analytics report is sent until Overwolf specifies the payload. Never rejects because of reporting. |
| `analytics_set_user_enabled` | `{ enabled: boolean }` | `void` | `unsupported` unless `analytics.userSwitch` | App-level analytics switch (no ow-electron equivalent). Off sends nothing, including the mandatory subset. Persisted in `ow-tauri.json`. |

`CmpWindowOptions` is the ow-electron type: `{ tab?: 'purposes'|'features'|'vendors'; modal?: boolean; parentId?: number; center?: boolean; backgroundColor?: string; preLoaderSpinnerColor?: string; width?: number; height?: number; x?: number; y?: number; cmpURL?: string; language?: string }`. On the wire `parent` (a `BrowserWindow`) is replaced by its integer `parentId`. Defaults: 800 x 800, centred, `language` from the app locale when it is one of `en, de, pt, es, fr, it, pl`, else `en`. `modal: true` makes the window owned by the parent and keeps it on top of it; true input modality is Windows-only.

`EmailHashes` is `{ sha1?: string; sha256?: string; md5?: string }`.
`ExternalPaymentUserIdOptions` is `{ providerName: string; userId: string; paymentId?: string }` with `providerName` defaulting to `'tebex'` when empty, as the typings document.

Email hash generation is a pure function implemented identically in Rust
(`identity::email_hashes`) and in `ow-tauri/main` (synchronous, see B.1.2),
checked against the shared test vectors in
`crates/tauri-plugin-overwolf/tests/fixtures/email-hashes.json`. Normalisation
follows the UID2 rules the typings link to: trim, lower-case, and for
`gmail.com` addresses remove `.` and any `+suffix` from the local part. Output
encoding follows `emailHashes.encoding` (**Interim**, OQ-10).

#### A.2.3 Main webview: windows, screen, shell, dialogs, files (`overwolf:main`)

These back the `ow-tauri/electron` facade. Operations that `@tauri-apps/api`
already exposes with equivalent semantics (show, hide, focus, size, position,
minimize, maximize, title, always-on-top, ignore cursor events, ...) are called
directly by the facade through `core:window:*` / `core:webview:*` permissions
and are not repeated here.

The opener, dialog and global-shortcut plugins are dependencies of
`tauri-plugin-overwolf`; the plugin registers them itself (`AppHandle::plugin`
in its setup hook) unless the app has already registered them, and calls them
from Rust. No webview, including `ow-main`, is granted their permissions.

| Command | Arguments | Returns | Errors | Behaviour |
|---|---|---|---|---|
| `window_create` | `WindowCreateRequest` | `{ id: number; label: string }` | `invalid-argument`, `io` | Creates the window behind `new BrowserWindow(options)` (B.2): native window `bw-<id>` with one webview `bw-<id>`. Injects the renderer bootstrap and the preload script as initialization scripts, both wrapped in an app-origin guard (A.2.3.1). Installs the navigation policy (A.2.3.1). Registers the window class. |
| `window_load` | `{ id: number; target: LoadTarget }` | `void` | `not-found`, `io` | `loadURL` / `loadFile`. A target that is an app asset loads in the existing webview. An `http(s)` URL that is not an app asset switches the window to class `remote` for good: the `bw-<id>` webview is closed and a fresh child webview `bwr-<id>` filling the window is created for the URL, with no initialization scripts and no capability (A.2.3.1). The window keeps its id, bounds and native handle. |
| `window_close_reply` | `{ id: number; requestId: number; prevent: boolean }` | `void` | `not-found` | Answer to a `close` window event (A.3); `prevent: false` lets the close proceed. |
| `window_destroy` | `{ id: number }` | `void` | `not-found` | `destroy()`: closes without a `close` event. |
| `window_eval` | `{ id: number; code: string; wantResult: boolean }` | `unknown` | `not-found`, `ipc-remote-error`, `ipc-timeout` | `webContents.executeJavaScript(code)`. The code always runs through the platform's native script evaluation (`Webview::eval`), never through page-level `eval`, so the page's CSP needs no `unsafe-eval`. For `ui` / `overlay` windows with `wantResult`, Rust evaluates `__OW_TAURI_RUNTIME__.evalBegin(<n>, () => (<code>\n))` and then `__OW_TAURI_RUNTIME__.evalFallback(<n>, () => { <code>\n})`; the second runs only when the first did not parse (statement code), and resolves `undefined`. The runtime reports through `eval_result` (A.2.5), 30 s timeout. For `remote` windows the code runs and the call resolves `undefined` (partial, B.2). |
| `window_devtools` | `{ id: number; open: boolean }` | `void` | `unsupported` (release build without the `devtools` feature) | `webContents.openDevTools()` / `closeDevTools()`. |
| `window_set_name` | `{ id: number; name: string }` | `void` | `not-found` | ow-electron `BrowserWindow` `name` option; normalised (whitespace and special characters removed) and used for analytics (E.2). |
| `screen_snapshot` | none | `{ displays: ElectronDisplay[]; primaryDisplayId: number; cursor: { x: number; y: number } }` | none | Fresh screen state; the cache is also pushed (C.6). |
| `shell_open_external` | `{ url: string }` | `void` | `invalid-argument` (not an absolute URL by the WHATWG parser, scheme not `http`, `https` or `mailto`, or credentials in the URL), `io` | via `tauri-plugin-opener`. |
| `shell_open_path` | `{ path: string }` | `string` (empty on success, Electron semantics) | none | Checked by the plugin before opener runs (A.2.3.2); a refused or failed open returns the error string, never throws. |
| `shell_show_item_in_folder` | `{ path: string }` | `void` | `io` | via opener `reveal_item_in_dir`; the path is canonicalised first. Revealing never executes anything, so no scope applies. |
| `dialog_open` | `OpenDialogOptions` (Electron shape, `windowId` instead of `BrowserWindow`) | `{ canceled: boolean; filePaths: string[] }` | `io` | via `tauri-plugin-dialog`. |
| `dialog_save` | `SaveDialogOptions` | `{ canceled: boolean; filePath: string }` | `io` | via `tauri-plugin-dialog`. |
| `dialog_message` | `MessageBoxOptions` | `{ response: number; checkboxChecked: boolean }` | `io` | Up to 3 buttons (partial, B.2). |
| `global_shortcut_register` | `{ accelerator: string; id: number }` | `boolean` | none | Electron accelerator syntax; presses arrive as `global-shortcut` host messages (A.3). |
| `global_shortcut_unregister` | `{ accelerator?: string }` | `void` | none | Absent `accelerator` unregisters all. |
| `fs_read_text` | `{ path: string }` | `string \| null` | `forbidden` (out of scope), `io` | `null` when the file does not exist. |
| `fs_write_text` | `{ path: string; data: string }` | `void` | `forbidden`, `io` | Atomic (temp file + rename); creates parent directories. |
| `fs_exists` | `{ path: string }` | `boolean` | `forbidden` | |
| `fs_mkdir` | `{ path: string; recursive?: boolean }` | `void` | `forbidden`, `io` | succeeds if the directory exists |

File-system scope for `fs_*` (the replacement for the `fs` calls in
main-process code): `paths.userData` and below (read-write), the per-app state
directory (read-only, F.1), the app's own embedded `package.json` (read-only,
path `paths.appPath + '/package.json'`, served from the embedded manifest), and
each directory in `fs.scope` (read-write). `fs.scope` entries are templates
that may start with `$USERDATA`, `$PICTURES`, `$VIDEOS`, `$DOCUMENTS`,
`$DOWNLOADS` or `$TEMP` and may contain `$APPNAME` (`productName`), for example
`"$PICTURES/Overwolf/$APPNAME"`. Paths are canonicalised before the check;
`..` segments and symlinks that leave the scope are `forbidden`.

`WindowCreateRequest`:

```ts
interface WindowCreateRequest {
  options: BrowserWindowOptionsWire;  // supported subset of BrowserWindowConstructorOptions (B.2), parent as parentId
  preload: string | null;             // app-asset path of the preload bundle, e.g. "preload/preload.js"
  windowClass: 'ui' | 'overlay';      // 'overlay' only from the overlay package backend
  overlayOptions?: OverlayOptions;    // when windowClass is 'overlay'
}
type LoadTarget = { kind: 'file'; path: string; query?: Record<string, string>; hash?: string }
                | { kind: 'url'; url: string };
```

The preload file is read from the embedded app assets (never from disk) and
wrapped so that it runs once per document, in the main frame, before page
scripts.

##### A.2.3.1 Navigation and remote pages

Tauri cannot remove an initialization script from a webview, and it re-runs
initialization scripts on every top-level navigation. ow-tauri therefore never
lets a webview that carries app scripts show a remote document:

- **Origin guard.** The renderer bootstrap and every preload are wrapped in
  `if (location.origin === <app origin>) { ... }`, where the app origin is the
  platform's asset origin (`tauri://localhost`, `http://tauri.localhost`, or the
  dev server origin in debug builds). They do nothing in any other document.
- **Navigation policy.** Every `bw-*` webview gets an `on_navigation` handler
  that allows top-level navigations to the app origin only. Any other target is
  cancelled: an `http(s)` URL is opened in the system browser (as
  `shell_open_external` validates it), anything else is dropped and logged.
  `window_load` with a remote URL does not navigate; it uses the recreate path
  above.
- **Remote class.** `bwr-<id>` webviews have no initialization scripts, no
  capability and no host-message channel; `webContents.send` to them is
  dropped (C.5), `executeJavaScript` runs through native `eval` and resolves
  `undefined` (B.2.2). A `loadFile` or app-asset `loadURL` on a remote window
  is `invalid-argument`: the window stays remote, as in the table above.

##### A.2.3.2 `shell_open_path` checks

`shell.openPath` hands the path to the OS shell, which runs executables,
scripts and shortcuts. Main-process handlers often receive the path from a
renderer, so the plugin checks it first:

1. The path is canonicalised (symlinks resolved); a path that does not exist
   returns `"path does not exist"`.
2. It must lie inside the `fs_*` scope below (read or read-write); otherwise
   `"path is outside the allowed scope"`.
3. Unless `shell.openPathAllowExecutables` is `true`, it must not be an
   executable or a launcher: on Windows any extension in `PATHEXT` plus `.lnk`,
   `.url`, `.scf`, `.ps1`, `.msi`, `.msp`, `.reg`, `.hta`, `.cpl`, `.jar`; on
   macOS any `.app`, `.command`, `.tool`, `.terminal`, `.workflow`, `.pkg`,
   `.mpkg` or a file with an execute bit; on Linux any `.desktop`, `.AppImage`
   or file with an execute bit. Directories are always allowed. A refused path
   returns `"opening executables is disabled"`.

#### A.2.4 Main webview: packages (`overwolf:main`)

| Command | Arguments | Returns | Errors | Mirrors |
|---|---|---|---|---|
| `packages_snapshot` | none | `PackagesSnapshot` | none | refresh of the cached state |
| `packages_relaunch` | none | `void` | `backend` | `packages.relaunch()` |
| `packages_set_channel` | `{ name: string; channel?: string \| null; ready?: RemoteCallback }` | `SetChannelResult` | `invalid-argument` (name not in `overwolf.packages`; **Interim**, OQ-37: JS rejects, B.1.3) | `packages.setChannel(name, channel, ready?)`. `null`, `''`, absent and `'public'` reset to public. The choice is persisted (`ow-tauri.json` `packageChannels`) and the runtime starts downloading that channel's version. `ready` is a callback reference (H.2.1): the runtime invokes it with `{ name, version }` once that download completes and a restart is required, and never when the package is already on that channel's version. A later `setChannel` for the same package releases the earlier callback. |
| `packages_get_available_channels` | `{ names: string[] }` | `Record<string, string[]>` | `invalid-argument` (a name not in `overwolf.packages`; **Interim**, OQ-37) | `getAvailableChannels(...names)`; empty `names` = all listed packages; packages without channels map to `[]`. |
| `packages_get_channel` | `{ names: string[] }` | `Record<string, string>` | none | `getChannel(...names)`. Unknown names are omitted. Empty `names` = every listed package plus any package with a stored non-public channel, even if it is no longer listed. `'public'` means the default release. |
| `package_call` | `{ package: string; method: string; args: unknown[] }` | `unknown` | `not-ready` (package not ready), `unsupported` (member not provided by the runtime), `backend` | Generic call into the package runtime (H). `method` is the typed member path, for example `setRequiredFeatures` or `hotkeys.register`. `args` and the result use the remote-value encoding of H.2.1 (callbacks, windows, handles). |
| `package_handle_call` | `{ package: string; handle: number; method: string; args: unknown[] }` | `unknown` | `not-found` (handle released), `backend` | A method on a remote object returned by the runtime (H.2.1), for example `ActiveReplay.stop`, `OverlayBrowserWindow.startDragging` or `OverlayBrowserWindow.setOverlayOptions`. |
| `package_handle_release` | `{ package: string; handles: number[] }` | `void` | none (unknown handles ignored) | JS no longer references these handles (H.2.1). |
| `package_event_action` | `{ eventId: number; action: PackageEventAction; args?: unknown[] }` | `void` | `not-found` (event already settled) | The methods on event objects: `enable` (gep `game-detected`), `inject` / `dismiss` (overlay `game-launched`), `prevent-default` (manager `crashed`), `abort` (crn `before-notification`). |
| `package_event_settled` | `{ eventId: number }` | `void` | none | All listeners for an actionable event have returned (and their returned promises settled). Unanswered actions take their default (H.5). |
| `sim_inject` | `{ package: string; event: string; args: unknown[] }` | `void` | `unsupported` (backend is not `simulated`) | Development only: emit a package event from the simulated backend. |
| `sim_load_scenario` | `{ scenario: SimScenario }` | `{ steps: number }` | `unsupported`, `invalid-argument` | Development only: queue a recorded scenario (H.7). |

```ts
interface PackagesSnapshot {
  backend: 'native' | 'simulated' | 'none';
  runtime: { name: string; version: string } | null;
  logsFolderPath: string;
  phasePercent: number;
  listed: string[];                                  // manifest overwolf.packages
  packages: Record<string, {
    state: 'pending' | 'loading' | 'ready' | 'failed';
    version: string | null;
    failure: { reason: string; version?: string } | null;
  }>;
  pendingUpdates: { hasPendingUpdate: boolean; details: { name: string; version: string }[] };
  channels: Record<string, string>;
  members: Record<string, string[]>;                 // per package: member paths the runtime provides (H.3 packages/load)
  packageState: Record<string, unknown>;             // per-package sync caches, H.4
}
type PackageEventAction = 'enable' | 'inject' | 'dismiss' | 'prevent-default' | 'abort';
type SetChannelResult = { success: boolean; error?: 'invalid-package' | 'invalid-channel' | string };
type RemoteCallback = { $cb: number };              // H.2.1
```

#### A.2.5 UI windows (`overwolf:renderer`)

| Command | Arguments | Returns | Errors | Behaviour |
|---|---|---|---|---|
| `ipc_subscribe` | `{ onMessage: Channel<HostMessage[]> }` | `{ epoch: string }` | none | As in A.2.1; also flushes messages buffered for this window (C.5). Once per document load. |
| `ipc_invoke` | `{ channel: string; args: OtjValue[]; epoch: string; seq: number }` | `{ id: number }` | `ipc-serialization`, `ipc-overloaded`, `not-ready` (stale `epoch`) | section C.2. Resolves as soon as the request is accepted; the result arrives later as an `ipc-result` host message. |
| `ipc_send` | `{ channel: string; args: OtjValue[]; epoch: string; seq: number }` | `void` | `ipc-serialization`, `ipc-overloaded`, `not-ready` (stale `epoch`) | section C.3 |
| `ipc_skip` | `{ epoch: string; seq: number }` | `void` | none | The runtime reports that the call carrying `seq` was rejected before it reached the plugin (C.3), so the reorder buffer does not wait for it. |
| `eval_result` | `{ id: number; ok: boolean; value?: OtjValue; error?: IpcErrorWire }` | `void` | `not-found` | Result of a `window_eval` with `wantResult` targeted at the calling window. |
| `adview_mount` | `AdviewMount` | `{ guestLabel: string }` | `invalid-argument`, `not-ready` (consent gate timed out), `io` | Creates the guest for one `<owadview>` (B.3, D). |
| `adview_update` | `{ elementId: string; rect?: AdviewRect; visible?: boolean; attributes?: Partial<AdviewAttributes> }` | `void` | `not-found` | Moves, resizes, shows or hides the guest; attribute changes per B.3.4. |
| `adview_unmount` | `{ elementId: string }` | `void` | none (idempotent) | Closes the guest. |
| `adview_command` | `{ elementId: string; command: 'setAudioMuted' \| 'reload' \| 'setPageUrl' \| 'sendCommand'; args: unknown[] }` | `void` | `not-found`, `unsupported` (`setPageUrl` / `sendCommand` without `ads.experimentalElementApi`) | Element methods (B.3.3). |

```ts
interface AdviewMount {
  elementId: string;            // runtime-assigned, unique per embedder webview ("e1", "e2", ...)
  attributes: AdviewAttributes;
  rect: AdviewRect;
  visible: boolean;
}
interface AdviewAttributes {
  cid: string;                  // trimmed, at most 20 characters
  slotsize: string;             // "WxH"
  adstyle: string;              // "" or e.g. "high-impact-ad;"
  customTracking: unknown;      // parsed JSON object, or null
  performance: boolean;
  unit: string | null;
  pageUrl: string | null;       // only with ads.experimentalElementApi (OQ-32); otherwise always null
}
interface AdviewRect { x: number; y: number; width: number; height: number; devicePixelRatio: number }
```

`rect` is the element's `getBoundingClientRect()` in CSS pixels relative to the
embedder viewport; for a `performance` element it is the embedder viewport
itself (B.3.4). Rust converts to window-logical pixels:
`logical = css * devicePixelRatio / window.scaleFactor` plus the embedder
webview's own position in the window.

#### A.2.6 Ad guests (`overwolf:adview-guest`)

| Command | Arguments | Returns | Behaviour |
|---|---|---|---|
| `adview_event` | `{ slotId?: string; name: string; data?: unknown }` | `void` | Guest-to-host message (D.4). The caller label is authoritative; a disagreeing `slotId` is recorded as `claimedSlotId`. `name` is 1 to 64 characters of `[A-Za-z0-9_:.-]`. `data` whose JSON encoding exceeds 16 KiB is replaced by `{ truncated: true, bytes }`. Rate-limited per guest (`ads.guestLimits`, D.4); calls over the limit are dropped and counted. Every value is untrusted ([ADR 0011](adr/0011-remote-guest-ipc.md)). |

#### A.2.7 Consent window (`overwolf:cmp-window`)

| Command | Arguments | Returns | Behaviour |
|---|---|---|---|
| `cmp_event` | `{ name: 'ready' \| 'saveConsent' \| 'saveUnifiedConsent' \| 'enableAdOptimization' \| 'close'; data?: { consent?: string; enabled?: boolean } }` | `void` | Consent page to host (D.6). Consent strings must be printable ASCII (0x21 to 0x7E) and at most 16 KiB. |

#### A.2.8 Updater (`overwolf:main`)

| Command | Arguments | Returns | Errors | Behaviour |
|---|---|---|---|---|
| `updater_configure` | `UpdaterConfig` | `void` | `invalid-argument` | `setFeedURL` plus the electron-updater properties (I.1). |
| `updater_check` | none | `UpdateCheckResult \| null` | `network`, `invalid-argument` (bad feed) | `checkForUpdates()`; `null` when updates are disabled for this build. |
| `updater_download` | none | `string[]` (downloaded file paths) | `network`, `io`, `backend` (verification failed) | `downloadUpdate()`. |
| `updater_quit_and_install` | `{ isSilent?: boolean; isForceRunAfter?: boolean }` | `void` | `not-found` (nothing downloaded), `io` | `quitAndInstall()` (I.4). |

### A.3 Host messages

Rust delivers everything it sends to a webview over the `Channel` that the
webview's runtime registered with `ipc_subscribe`
([ADR 0010](adr/0010-per-webview-ipc-channels.md)). A channel belongs to one
webview, so no other webview can observe its traffic; Tauri events are not
used at all, and no capability grants `core:event:*`. Messages for one webview
are delivered in the order Rust queued them. Rust coalesces the messages queued
for a webview during one event-loop turn into one channel send, so the payload
is always an array, `HostMessage[]`, processed in order.

| `type` | Receiver | Fields | When |
|---|---|---|---|
| `ipc` | `ow-main` | `{ kind: 'invoke', id: number, channel, args, sender: IpcSender }` or `{ kind: 'send', channel, args, sender: IpcSender }` | C.2, C.3 |
| `ipc` | a `bw-*` webview | `{ kind: 'message', channel, args }` | `webContents.send` (C.5) |
| `ipc-result` | a `bw-*` webview | `{ id: number, ok: boolean, value?: OtjValue, error?: IpcErrorWire }` | the reply to that webview's `ipc_invoke` `id` (C.2) |
| `state` | `ow-main` | `{ seq: number, patches: { path: string, value: unknown }[] }` | sync-cache update (B.1.6) |
| `window` | `ow-main` | `{ id: number, event: WindowEventName, requestId?: number, data?: unknown }` | window lifecycle (B.2); `close` carries `requestId` and waits for `window_close_reply` (5 s, then closes) |
| `lifecycle` | `ow-main` | `{ event: 'before-quit' \| 'will-quit', requestId: number }` or `{ event: 'quit', exitCode: number }` | the quit sequence (A.6) |
| `packages` | `ow-main` | `{ type: 'loading' \| 'ready' \| 'failed-to-initialize' \| 'crashed' \| 'package-update-pending' \| 'updated', name?, version?, details?, info?, canRecover?, eventId? }` | package manager lifecycle (B.1.3) |
| `package-event` | `ow-main` | `{ package: string, event: string, args: unknown[], eventId?: number, actions?: PackageEventAction[] }` | an event emitted by a package object (B.1.4); `args` use the remote-value encoding (H.2.1) |
| `package-callback` | `ow-main` | `{ cbId: number, args: unknown[] }` | the runtime invokes a callback reference (H.2.1) |
| `package-callback-release` | `ow-main` | `{ cbIds: number[] }` | the runtime will never invoke these callbacks again |
| `adview-event` | the embedder webview | `{ elementId: string, name: string, data?: unknown, source: 'guest' \| 'host' }` | B.3.5 |
| `updater` | `ow-main` | `{ type: 'checking-for-update' \| 'update-available' \| 'update-not-available' \| 'download-progress' \| 'update-downloaded' \| 'error', info?, progress?, error? }` | I.3 |
| `global-shortcut` | `ow-main` | `{ id: number, accelerator: string, state: 'pressed' \| 'released' }` | registered accelerator |

Messages for a webview that has not subscribed yet are buffered (C.4, C.5).
When a webview is destroyed (`WebviewEvent::Destroyed` or its window's
`WindowEvent::Destroyed`), Rust drops its channel, its buffers, its reorder
state, its pending invokes (rejected with `not-ready`) and every ad guest it
embeds.

`IpcSender` is `{ windowId: number, label: string, url: string, frameId: 0 }`.
`WindowEventName` is one of `created`, `close`, `closed`, `focus`, `blur`,
`show`, `hide`, `minimize`, `maximize`, `unmaximize`, `restore`, `resize`,
`move`, `enter-full-screen`, `leave-full-screen`, `ready-to-show`,
`did-finish-load`, `dom-ready`, `did-fail-load`, `render-process-gone`.
`created` announces a window the JS side did not create itself (an overlay
window made by a package runtime, H.2) with its options, so
`BrowserWindow.fromId` finds it. Tauri has no minimize event: Rust derives
`minimize` and `restore` from `WindowEvent::Resized` plus `is_minimized()`,
with a 2 s poll of `is_minimized()` as a fallback for platforms that do not
report a resize on minimize.

### A.4 Errors

Every command error serialises as:

```ts
interface OverwolfErrorWire {
  code: 'unsupported' | 'not-ready' | 'invalid-argument' | 'not-found' | 'forbidden'
      | 'ipc-no-handler' | 'ipc-timeout' | 'ipc-serialization' | 'ipc-remote-error'
      | 'ipc-overloaded' | 'io' | 'network' | 'backend';
  message: string;              // English, single sentence, no secrets or user data
  data?: unknown;               // code-specific details, e.g. { channel } or { reason }
}
```

In Rust this is `tauri_plugin_overwolf::Error` (`thiserror`), one variant per
code; `impl serde::Serialize` produces the shape above. In JS it becomes
`OwTauriError` with the same `code` (`OwTauriUnsupportedError` for
`unsupported`).

A call that Tauri rejects before the command runs (a capability denial, an
argument that does not deserialise) is not an `OverwolfErrorWire`; the
runtime wraps it as `OwTauriError('forbidden')` or
`OwTauriError('invalid-argument')` with the original text in `data.raw`.

### A.5 Rust API

For Rust-first apps and for the example's `src-tauri`:

```rust
use tauri_plugin_overwolf::{Builder, OverwolfExt, PackagesBackend};

fn main() {
    tauri::Builder::default()
        .plugin(
            Builder::new()
                .manifest_json(tauri_plugin_overwolf::embedded_manifest!())
                .packages_backend(PackagesBackend::Auto)
                .build(),
        )
        .setup(|app| {
            let ow = app.overwolf();          // &Overwolf<R>
            log::info!("uid {}", ow.uid());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

| Item | Signature (abridged) | Notes |
|---|---|---|
| `Builder::new()` | `-> Builder` | defaults from A.1 |
| `Builder::manifest_json` | `(self, &'static str) -> Self` | required; output of `embedded_manifest!()` |
| `Builder::packages_backend` | `(self, PackagesBackend) -> Self` | |
| `Builder::package_runtime` | `(self, Arc<dyn PackageRuntime>) -> Self` | registers a native runtime (H.2) |
| `Builder::package_runtime_sidecar` | `(self, impl Into<PathBuf>) -> Self` | H.3 |
| `Builder::test_ad` | `(self, bool) -> Self` | |
| `Builder::analytics_transport` | `(self, Arc<dyn Transport>) -> Self` | tests: capture requests instead of sending |
| `Builder::build` | `<R: Runtime>(self) -> TauriPlugin<R, Config>` | |
| `OverwolfExt::overwolf` | `(&self) -> &Overwolf<R>` | on `App`, `AppHandle`, `Window`, `Webview`, `WebviewWindow` |
| `Overwolf::uid`, `cuid`, `muid`, `muid_v2`, `phase_percent`, `utm_params` | getters | |
| `Overwolf::disable_anonymous_analytics`, `disable_ads_optimization`, `disable_ads_fpd` | `(&self)` | same semantics as A.2.2 |
| `Overwolf::is_cmp_required` | `async (&self) -> bool` | |
| `Overwolf::open_cmp_window`, `open_ad_privacy_settings_window` | `async (&self, CmpWindowOptions) -> Result<()>` | |
| `Overwolf::generate_user_email_hashes` | `(&self, &str) -> EmailHashes` | |
| `Overwolf::set_user_email_hashes` | `(&self, Option<EmailHashes>)` | |
| `Overwolf::packages` | `(&self) -> &Packages<R>` | `snapshot`, `set_channel`, `get_available_channels`, `get_channel`, `relaunch`, `call`, `subscribe` |
| `Overwolf::updater` | `(&self) -> &Updater<R>` | `configure`, `check`, `download`, `quit_and_install` |
| `Overwolf::emit_second_instance` | `(&self, argv: Vec<String>, cwd: String)` | call from the app's `tauri-plugin-single-instance` callback; fires `app.on('second-instance')` in `ow-main` (B.2.1) |
| `build::embed_manifest` | `(path: impl AsRef<Path>) -> Result<(), BuildError>` | in the app's `build.rs` (G.3) |

`tauri-plugin-single-instance` must be the first plugin an app registers, so
the app registers it, not ow-tauri:

```rust
tauri::Builder::default()
    .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
        app.overwolf().emit_second_instance(argv, cwd);
    }))
    .plugin(tauri_plugin_overwolf::Builder::new() /* ... */ .build())
```

### A.6 Main webview liveness and lifecycle

`ow-main` runs the app's main-process code, so it must keep running while it
is hidden and must not silently disappear
([ADR 0009](adr/0009-main-webview-liveness-and-lifecycle.md)).

**Timers keep running.** Hidden webviews are throttled by every engine, and
Tauri's `background_throttling` setting works only on macOS 14 and newer.

| Platform | Measure |
|---|---|
| Windows (WebView2) | the shared browser arguments of A.1.1 (`--disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows`) |
| macOS 14+ | `ow-main` is created hidden with `BackgroundThrottlingPolicy::Disabled` |
| macOS 12 and 13, Linux | `ow-main` is a *technically visible* window: 1 x 1 logical pixel, fully transparent, ignores the cursor, skips the taskbar and window switcher, never focused, placed at the origin of the primary display (Wayland ignores positions; the window is still 1 x 1 and transparent) |

The requirement is measurable: over a 10-minute run with every app window
hidden or minimized, a 1 s `setInterval` in `ow-main` fires with a median
period under 1.5 s. The scheduled CI soak job checks it on all three
platforms (`tests/liveness_soak.rs`, test ads only).

**Navigation and reloads.** After its first load, `ow-main` may navigate only
in debug builds. A reload in a debug build (manual or a dev-server hot
reload) is a *soft restart*: Rust closes every `bw-*`, `bwr-*`, guest and
consent window, rejects pending invokes with `not-ready`, clears the IPC,
package-callback and handle registries, and lets the reloaded page run the
app's main code again from a fresh snapshot. Package runtimes keep running;
their state is replayed through the snapshot. In release builds every
navigation of `ow-main` is cancelled.

**Crashes.** When the `ow-main` render process dies (`render-process-gone`,
WebView2 `ProcessFailed`, WKWebView web-content termination, WebKitGTK
`web-process-terminated`), Rust logs it, drains analytics for at most 1.5 s
and relaunches the app with its original arguments. If `ow-main` crashes
`main.crashRestartLimit` times within 60 s, the app exits with code 1 instead
and the log names the cause. There is no in-place rehydration: the JS-side
registries (windows, `ipcMain` handlers, hotkeys, package listeners) cannot be
rebuilt without the app's own code.

**Quitting.** Every exit path runs the same sequence: `app.quit()`, the last
window closing with no `window-all-closed` listener, an OS request
(`RunEvent::ExitRequested`: Cmd+Q, logoff, `WM_QUERYENDSESSION`, Ctrl+C), and
`autoInstallOnAppQuit` (I.4).

1. Rust calls `prevent_exit()` on the request (when it came from the OS) and
   sends `lifecycle { event: 'before-quit', requestId }`.
2. The main runtime emits `before-quit` on `app` with a synthetic event and
   answers `app_quit_reply { requestId, prevent }`. `prevent: true` cancels the
   quit (an OS logoff cannot be cancelled; it continues after the timeout).
3. Each UI window receives its `close` event (`window`, `requestId`), which
   may also be prevented, as in Electron.
4. `lifecycle { event: 'will-quit', requestId }`, answered the same way.
5. Analytics drain (at most 1.5 s), `lifecycle { event: 'quit', exitCode }`,
   pending update install (I.4), then exit.

A step that gets no answer within 5 s proceeds as if not prevented. `app.exit()`
skips steps 1 to 4, as in Electron.

---

## B. JavaScript API

The npm package `ow-tauri` has three entry points. All of them export
`OwTauriError`, `OwTauriUnsupportedError` and the `OwTauriErrorCode` type.
Overwolf package types are re-used from `@overwolf/ow-electron-packages-types`
(latest); the core `overwolf` namespace types are re-declared from the
ow-electron 42.11.4 typings with `BrowserWindow` pointing at the facade. How
the `electron` module and the global namespaces resolve is in B.4.

**One runtime per webview** ([ADR 0012](adr/0012-js-runtime-singleton.md)).
The plugin injects the runtime itself as an initialization script into
`ow-main` and every `bw-*` webview: the *bootstrap*, built from
`packages/ow-tauri/src/bootstrap/` and embedded in the crate. It installs
`globalThis.__OW_TAURI_RUNTIME__`, a non-writable, non-configurable object
`{ version, contract, ... }`, before any page script runs. The npm entry
points that an app bundles (`ow-tauri/main`, `/electron`, `/renderer`) are thin
facades: on first use they attach to that object and throw
`OwTauriError('not-ready')` with "ow-tauri runtime <a> does not match package
<b>" when its `contract` differs from their own. So there is exactly one
`ipcRenderer`, one IPC sequence counter, one listener registry and one
`<owadview>` observer per webview, however many bundles import the package.
Errors are branded with `Symbol.for('ow-tauri.error.brands')`, so `instanceof
OwTauriError` holds across copies.

**Context check.** Each entry point decides where it runs with an injectable
host detector (`__OW_TAURI_RUNTIME__.context`: `'main' | 'ui' | 'none'`).
Members that are wrong for the context throw `OwTauriError('forbidden')` when
used, not at import, so unit tests can import every module; tests replace the
detector through the `ow-tauri/testing` entry point (`setHostContext()`),
which only test code imports.

### B.1 `ow-tauri/main`

Runs only in the main webview. Using it anywhere else throws
`OwTauriError('forbidden')` (see "Context check" above).

Exports:

| Export | Kind | Description |
|---|---|---|
| `overwolf` | `overwolf.OverwolfApi` | the `app.overwolf` object; `ow-tauri/electron`'s `app.overwolf` is the same instance |
| `autoUpdater` | `AppUpdater` subset | electron-updater compatible (I.5) |
| `files` | `{ readText, writeText, exists, mkdir }` | scoped file access replacing Node `fs` in main-process code (A.2.3) |
| `whenHostReady()` | `() => Promise<void>` | resolves after `ipc_main_ready` and `main_ready` were acknowledged |
| `RecorderError` | class | runtime class for `instanceof RecorderError` checks (B.1.5) |
| `UpdateCheckResult`, `UpdateInfo`, `ProgressInfo`, `UpdaterConfig` | types | for `autoUpdater` code (I.5) |

#### B.1.1 `app.overwolf` (OverwolfApi)

Every member of the ow-electron 42.11.4 typings, with the same signature.

| Member | Signature | Status | Implementation |
|---|---|---|---|
| `disableAnonymousAnalytics` | `(): void` | supported | fire-and-forget `disable_anonymous_analytics`; recorded synchronously in the cache so a later `main_ready` carries it |
| `disableAdsOptimization` | `(): void` | supported | `disable_ads_optimization` |
| `disableAdsFPD` | `(): void` | supported | `disable_ads_fpd` |
| `isCMPRequired` | `(): Promise<boolean>` | supported | `is_cmp_required`; never rejects; resolves `true` on any failure |
| `openCMPWindow` | `(options?: CMPWindowOptions): Promise<void>` | supported | `open_cmp_window` |
| `openAdPrivacySettingsWindow` | `(options?: CMPWindowOptions): Promise<void>` | supported | `open_ad_privacy_settings_window` |
| `packages` | `overwolf.packages.OverwolfPackageManager` | supported | B.1.3 |
| `generateUserEmailHashes` | `(email: string): EmailHashes` (**sync**) | supported | computed in JS (pure TypeScript md5, sha1, sha256); empty or whitespace email returns `{}` |
| `setUserEmailHashes` | `(emailHashes?: EmailHashes): void` | supported | `set_user_email_hashes` |
| `setExternalPaymentUserId` | `(options: ExternalPaymentUserIdOptions): Promise<void>` | partial | validation and pre-ready rejection exact; no report sent yet (OQ-12) |
| `phasePercent` | `readonly number` | supported | cache `identity.phasePercent` |
| `utmParams` | `readonly any` | supported | cache `utmParams` (F.2) |
| `muid` | `readonly string` | supported | cache `identity.muid` (E.4) |
| `uid` | `readonly string` | supported | cache `identity.uid` (G.2) |

`process.env.OVERWOLF_APP_UID` (documented by Overwolf as set after app ready)
is provided on the `process.env` shim of `ow-tauri/electron` once `main_ready`
is acknowledged; the Rust process also sets the real environment variable.

#### B.1.2 Event emitters and the synthetic `Event`

`overwolf.packages` and every package object are Node-style event emitters:
`on`, `once`, `off`, `addListener`, `prependListener`, `prependOnceListener`,
`removeListener`, `removeAllListeners`, `emit`, `listeners`, `rawListeners`,
`listenerCount`, `eventNames`, `setMaxListeners`, `getMaxListeners`, with
Node's ordering and `'error'` semantics for app-initiated `emit` (an
`'error'` emit with no listener throws; `errorMonitor` listeners run first).
Listeners run synchronously in registration order when the host message
arrives. An `'error'` that a *package* emits while no `'error'` listener is
registered is logged at warn level and not thrown, because there is no
caller to throw to (the sample listens for gep `error` but not overlay
`error`).

Where ow-electron passes an Electron `Event` as the first listener argument
(package manager events and all `gep` events), ow-tauri passes:

```ts
interface SyntheticEvent {
  preventDefault(): void;          // only meaningful on actionable events
  readonly defaultPrevented: boolean;
}
```

`overlay`, `utility`, `recorder` and `crn` events have no leading `Event`,
exactly as typed upstream. Actionable events (H.5) add their methods to the
object passed as the first argument: `enable()` on gep `game-detected`,
`inject(options?)` and `dismiss()` on overlay `game-launched`, `abort()` on crn
`before-notification`. After synchronous dispatch, and after every promise
returned by a listener settles, the runtime sends `package_event_settled`.

#### B.1.3 `overwolf.packages` (OverwolfPackageManager)

| Member | Signature | Status | Notes |
|---|---|---|---|
| `on('loading')` | `(event, packageName)` | supported | |
| `on('ready')` | `(event, packageName, version)` | supported | the package object's own `ready` fires first (see below) |
| `on('failed-to-initialize')` | `(event, packageName, details?: { reason?: string; version?: string })` | supported | the third argument is untyped upstream and read by the sample; reasons in H.1 |
| `on('crashed')` | `(event, canRecover)` | supported | `event.preventDefault()` suppresses the automatic relaunch |
| `on('package-update-pending')` | `(event, info: PackageInfo[])` | supported | |
| `on('updated')` | `(event, packageName, version)` | supported | |
| `relaunch` | `(): void` | supported | `packages_relaunch` |
| `hasPendingUpdates` | `(): PendingUpdatesResult` (**sync**) | supported | cache `packages.pendingUpdates`; Rust applies that patch before it dispatches `package-update-pending`, and a new process starts with the runtime's value (H.4) |
| `setChannel` | `(name, channel?, ready?): Promise<SetChannelResult>` | supported | `packages_set_channel` with `ready` passed as a callback reference. `ready({ name, version })` fires once the channel's download completes and a restart is required; it never fires when the package is already on that channel's version. A `name` not listed in `overwolf.packages` rejects with `invalid-argument` (**Interim**, OQ-37: upstream says "throws" on a promise-returning method) |
| `getAvailableChannels` | `(...names): Promise<AvailableChannelsResult>` | supported | rejects with `invalid-argument` for a name not listed (OQ-37) |
| `getChannel` | `(...names): Promise<CurrentChannelsResult>` | supported | unknown names omitted; no names = all listed packages plus any package with a stored non-public channel |
| `logsFolderPath` | `readonly string` | supported | F.4 |
| `phasePercent` | `readonly number` | supported | |
| `gep`, `overlay`, `recorder`, `utility`, `crn` | package objects | backend-dependent | lifecycle below; only names listed in the manifest are ever defined |

Package object lifecycle (**Interim**, OQ-36; the sample relies on the object
existing before `ready`: `UtilityService` reads `packages.utility` in its
constructor and `registerListeners` subscribes to `packages[name].on('ready')`):

| Phase | Object | Members |
|---|---|---|
| before `loading` | `undefined` | |
| from `loading` (defined before the `loading` listeners run) | defined, `version` `''` | event subscription works; async members reject `OwTauriError('not-ready')`; sync members return their empty value (`undefined`, `false`, `[]`, `{}`) |
| `ready` | same object, `version` set | the object emits its own `ready(version)` immediately before the manager's `ready`; members the runtime does not provide (H.3 `packages/load` `members`) are absent if optional upstream, else reject `unsupported` |
| `failed-to-initialize` | stays defined | async members reject `not-ready` with `data.reason`; listeners are kept |
| `crashed` | same object | async members reject `not-ready` until the package's next `ready`; listeners are kept |
| `updated` (hot update) | same object, `version` updated | listeners and remote handles are kept unless the runtime releases them |

#### B.1.4 Package objects

Every member below keeps the exact upstream signature from
`@overwolf/ow-electron-packages-types` 1.1.12. "Native" means the call is
forwarded to a native runtime with `package_call`; behaviour is the runtime's.
"Simulated" describes the development backend (H.7). Members that are
synchronous upstream are served from the package's cache (H.4). Callbacks,
windows and objects with methods cross the boundary with the remote-value
encoding of H.2.1.

**gep** (`OverwolfGameEventPackage`, leading `Event` on every event)

| Member | Simulated behaviour |
|---|---|
| `getFeatures(gameId)` | feature names from the cached game-events-status JSON for that game |
| `setRequiredFeatures(gameId, features \| null \| undefined)` | records the set; `null` and `undefined` mean all features; later events outside the set are not emitted |
| `getSupportedGames()` | games from the cached status list where `disabled_electron` is false, mapped to `{ name, id }` |
| `getInfo(gameId)` | accumulated `{ [category]: { [key]: value } }` from emitted `new-info-update` events |
| events `game-detected(e{enable}, gameId, name, gameInfo)`, `game-exit(e, gameId, gameName, pid, processName, processPath, commandLine)`, `elevated-privileges-required(e, gameId, name, pid)`, `new-info-update(e, gameId, data)`, `new-game-event(e, gameId, data)`, `error(e, gameId, error, ...args)` | emitted by scenarios and `sim_inject`; `value` is passed as the raw string (OQ-15) |

**overlay** (`IOverwolfOverlayApi`, no leading `Event`)

| Member | Simulated behaviour |
|---|---|
| `createWindow(options)` | a real Tauri window of class `overlay`, always on top; honours `frame`, `transparent`, `show`, `focusable`, `resizable`, `width`, `height`, `x`, `y`, `minWidth`, `minHeight`, `maxWidth`, `maxHeight`, `webPreferences.preload` and the `OverlayOptions` fields; ignores, with a debug log, `dpiAware`, `useSharedTexture`, `enableIsolation` and `disableHardwareAcceleration` (no game surface to render into, OQ-33). Resolves an `OverlayBrowserWindow` (H.2.1) whose `window` is the `BrowserWindow` facade |
| `registerGames(filter)` | recorded; returns `undefined` |
| `getActiveGameInfo()` (sync) | the simulated active game, or `undefined` |
| `getAllWindows()`, `fromWebContents(wc)`, `fromBrowserWindow(bw)` (sync) | from the JS registry of overlay windows |
| `requestGameInjection(classId)` | as typed upstream: if a simulated game with that `classId` is running, emits `game-launched(event{inject, dismiss}, gameInfo)` so the app's own handler decides, then resolves; otherwise rejects with `Error('Game <classId> is not running')` |
| `hotkeys.register(hotkey, cb)` | validates `keyCode` synchronously (VK number or `KeyboardEvent.code`; unknown string throws `Error('Unknown hotkey code: "<value>". ...')`), then registers a desktop global shortcut; `cb` travels as a callback reference and is called `cb(hotkey, 'pressed' \| 'released')`; it is released on `unregister` / `unregisterAll`; `passthrough` cannot be honoured (desktop shortcuts are consumed) and is logged once |
| `hotkeys.update(hotkey)` (sync `boolean`), `unregister(name)` (sync `boolean`), `unregisterAll()`, `all()` | answered from the JS registry, then synced to Rust |
| `version` | `0.0.0-simulated` |
| `enterExclusiveMode(options?)` / `exitExclusiveMode()` | toggles a simulated `exclusiveMode` flag and emits `game-input-exclusive-mode-changed` |
| `takeScreenshot(filePath, format?)` | typed `Promise<string>` (resolves to the written path, as the docs and sample use it); the simulated backend has no game surface to capture and rejects with `'no active graphics device'` |
| `setGpuPreference(p)` / `getGpuPreference()` | stored per session / returned |
| `installHighElevationHelper?`, `isHighElevationHelperInstalled?` | omitted (optional upstream) |
| events `game-launched(e{inject, dismiss}, gameInfo)`, `game-exit(gameInfo, wasInjected)`, `game-injected(gameInfo)`, `game-injection-error(gameInfo, error, ...)`, `game-focus-changed(window, gameInfo, focus)`, `game-window-changed(window, gameInfo, reason?)`, `game-window-destroyed(gameInfo)`, `game-input-interception-changed(info)`, `game-input-exclusive-mode-changed(info)`, `shared-texture-unavailable(reason)`, `error(...)` | from scenarios; `inject()` emits `game-injected` |

`OverlayBrowserWindow` (H.2.1 kind `OverlayBrowserWindow`): `window` is the
`BrowserWindow` facade for the same window id; `name`, `id` and `scaleFactor`
are read from the handle data and updated by `state` patches.
`overlayOptions` is a live object: assigning one of its fields (the sample
assigns `passthrough` and `zOrder` at runtime) updates the JS value at once
and sends `package_handle_call setOverlayOptions({ <field>: value })` in call
order. In the simulated backend `zOrder: 'topMost'` maps to always-on-top,
`'bottomMost'` to always-on-bottom, `passthrough: 'passThrough'` to
ignore-cursor-events. `startDragging()` is `package_handle_call
startDragging`; the simulated backend calls the window's `startDragging`.

**recorder** (`IOverwolfRecordingApi`, no leading `Event`)

| Member | Simulated behaviour |
|---|---|
| `options` (mutable), `version` (`0.0.0-simulated`), `ffmpegPath`, `ffprobePath`, `binFolderPath` (`''`) | `options` is a `Proxy` over the cached value: a property write updates the cache at once and is sent (`package_call options.set { path, value }`) in call order, so `recorder.options[prop] = value` (the sample's `set-recording-app-options`) behaves synchronously |
| `isActive`, `isRecordingActive`, `isReplayActive`, `getRecordingState`, `getReplayState` | from the state machine `idle -> starting -> active -> idle` |
| `queryInformation(override?)` | a fixed, clearly synthetic `RecordingInformation` (one monitor per display, encoder list `['obs_x264']`) |
| `isXboxDVREnabled()` | resolves `{ enabled: false, appCaptureEnabled: false, gameDVREnabled: false }` (every `XboxDVRInfo` field) |
| `disableXboxDVR()` | resolves; no-op |
| `createSettingsBuilder(options?)` | resolves a `CaptureSettingsBuilder` (H.2.1 value kind): `CaptureSettings` defaults from the runtime plus synchronous `add*Source` / `add*Capture` methods that record each call, and `build()` that returns the settings with the recorded calls (`$builderOps`) for the runtime to apply |
| `startRecording(options, settings?, listener?)`, `stopRecording(listener?)`, `splitRecording(listener?)`, `startReplays`, `stopReplays` | drive the state machine and emit `recording-started`, `recording-stopped`, `recording-split`, `replays-started`, `replays-stopped` with `filePath` values that name the requested path; listeners travel as callback references and are released after their call; **no media file is written** |
| `captureReplay(options, callback?)` | resolves an `ActiveReplay` (H.2.1 handle kind) whose `stop(cb?)` / `stopAfter(ms, cb?)` are handle calls; emits `replay-captured` when it ends |
| `registerGames`, `updateAudioDevice`, `setSourceTransform` | recorded |
| events `game-launched`, `game-exit`, `stats` | from scenarios; `stats` every 2 s while active |

Recorder failures reject with a `RecorderError` instance (B.1.5) using the
upstream `ErrorCode` values (for example `AlreadyRunning` -997,
`NoActiveRecording` -10).

**utility** (`IOverwolfUtilityApi`, no leading `Event`)

| Member | Simulated behaviour |
|---|---|
| `trackGames(filter & { classIds?, includeApplication? })` | recorded |
| `scan(filter?)` | the scenario's installed-games list, default `[]` |
| `installHighElevationHelper?`, `isHighElevationHelperInstalled?`, `canInjectElevated?`, `installElevationBroker?`, `uninstallElevationBroker?` | omitted (optional upstream; the sample checks for presence) |
| events `game-launched(gameInfo)`, `game-exit(gameInfo)` | from scenarios |

**crn** (`IOverwolfCRNApi`, no leading `Event`)

| Member | Simulated behaviour |
|---|---|
| `isNotificationVisible()`, `getNotificationStatus()` | `false` |
| `closeNotificationWindow()` (sync), `allowNotifications(enable)` (sync) | recorded |
| events `before-notification(e{abort}, args)`, `notification-action(action)` | from scenarios only |

#### B.1.5 Errors from package calls

A runtime reports a package error as `data.error` with a `$kind` tag (H.3).
`ow-tauri/main` rebuilds the value the typings promise:

| `$kind` | Upstream type | Rejects with |
|---|---|---|
| `RecorderError` | `class RecorderError extends Error` (`code`, `codeStr`, `internalError?`) | an instance of the `RecorderError` class exported by `ow-tauri/main`: `name` `'RecorderError'`, `message`, `code`, `codeStr`, and `internalError` rebuilt as an `Error` from `{ name, message }` when present. `instanceof RecorderError` and `instanceof Error` both hold |
| `UtilityApiError` | `interface UtilityApiError` (`message`, `exitCode?`) | a plain frozen object `{ message, exitCode? }`, not an `Error`, as typed |
| `Error` or absent | | `Error` with the reported `name` and `message` |

Host-side failures reject with `OwTauriError` (`backend`, `not-ready`,
`unsupported`).

#### B.1.6 Synchronous members and the state cache

Electron and ow-electron expose synchronous members that need host state
(`uid`, `hasPendingUpdates()`, `getActiveGameInfo()`, `screen.getAllDisplays()`,
`app.getPath()`, `hotkeys.all()`, ...). Webviews cannot block on IPC, so:

1. At window creation Rust injects `window.__OW_TAURI_BOOTSTRAP__ = <HostSnapshot>`
   into `ow-main` as an initialization script that runs before any page script
   (and again after a soft restart, A.6).
2. `ow-tauri/main` builds its cache from the snapshot at import time. Every
   synchronous member reads the cache.
3. Rust pushes changes as `state` host messages `{ seq, patches }`. `path`
   is a dot path into `HostSnapshot` (for example `packages.pendingUpdates`,
   `displays`, `packages.packageState.overlay.activeGameInfo`). Patches with
   `seq <= cache.seq` are ignored; a gap (`seq > cache.seq + 1`) triggers a
   `bootstrap` call that replaces the cache.
4. A patch is applied before the event that caused it is dispatched to app
   listeners, so a listener for `ready` sees the package defined and
   `hasPendingUpdates()` current.
5. Writes that are synchronous upstream update the JS cache immediately and
   are sent to Rust in call order; Rust's acknowledgement does not change the
   return value. This covers `hotkeys.update`, `overlayOptions.<field> = ...`,
   `recorder.options[prop] = ...`, `crn.allowNotifications`, and the
   `BrowserWindow` setters `show`, `showInactive`, `hide`, `minimize`,
   `restore`, `maximize`, `unmaximize`, `setFullScreen`, `setBounds`,
   `setSize`, `setPosition`, `setAlwaysOnTop` and `setTitle`, so
   `isVisible()`, `isMinimized()`, `getBounds()` and the other getters reflect
   the call at once, as in Electron (the sample toggles windows with
   `isVisible()` right after `show()` / `hide()`). A window event that
   disagrees later (the OS refused a move) overwrites the cache.

#### B.1.7 `files`

```ts
files.readText(path: string): Promise<string | null>;
files.writeText(path: string, data: string): Promise<void>;
files.exists(path: string): Promise<boolean>;
files.mkdir(path: string, options?: { recursive?: boolean }): Promise<void>;
```

Scope and atomicity as `fs_*` in A.2.3. These replace `fs.readFileSync` /
`fs.writeFileSync` in main-process code; the port turns synchronous reads into
awaited reads ([PORT-MAP.md](PORT-MAP.md)).

### B.2 `ow-tauri/electron`

Imported through a bundler alias `electron -> ow-tauri/electron` by main-process
code (in `ow-main`) and by preload and renderer code (in UI windows). Which
members exist depends on the context, exactly as in Electron: main-process
modules throw `OwTauriError('forbidden')` when touched from a UI window and
vice versa.

Legend: **S** supported with Electron semantics. **P** partial (the note says
what differs). **U** unsupported: calling or constructing it throws
`OwTauriUnsupportedError` with the `api` string shown in the first column;
reading a property returns `undefined` and logs a warning once.

#### B.2.1 `app` (main)

| Member | Status | Notes |
|---|---|---|
| `app.overwolf` | S | B.1.1 |
| `whenReady()`, `isReady()`, `on('ready')` | S | ready after `main_ready` |
| `on/once('window-all-closed')` | S | when the last UI window closes; with no listener the app quits (Electron default; the sample keeps macOS alive itself) |
| `on('before-quit' \| 'will-quit' \| 'quit')` | S | the A.6 quit sequence; `before-quit` and `will-quit` support `preventDefault()` (answered within 5 s) |
| `on('activate' \| 'browser-window-created' \| 'browser-window-focus' \| 'browser-window-blur')` | S | `activate` on macOS dock click |
| `on('second-instance')`, `requestSingleInstanceLock()`, `hasSingleInstanceLock()`, `releaseSingleInstanceLock()` | P | lock always granted in JS; real single-instance behaviour comes from `tauri-plugin-single-instance`, registered by the app, whose callback calls `emit_second_instance` (A.5) |
| `quit()`, `exit(code?)`, `relaunch(options?)`, `focus(options?)` | S | `relaunch({ execPath })` is U |
| `getAppPath()` | S | virtual app root; `getAppPath() + '/package.json'` is readable through `files` |
| `getPath(name)` | S | `appData`, `userData`, `sessionData`, `temp`, `home`, `desktop`, `documents`, `downloads`, `music`, `pictures`, `videos`, `logs`, `exe`, `crashDumps`; `userData` is `<appData>/<productName>` like Electron, so migrated prefs files are found; `module` and `recent` are U |
| `setPath(name, path)` | P | affects only ow-tauri lookups (`userData`, `logs`) |
| `getName()`, `name`, `getVersion()`, `isPackaged`, `getLocale()`, `getSystemLocale()` | S | from the manifest and the OS |
| `setName(name)` | P | changes `app.name` for the session only; never changes the uid |
| `commandLine.hasSwitch()`, `getSwitchValue()` | S | process arguments |
| `commandLine.appendSwitch()`, `appendArgument()` | P | recorded; `--disable-gpu` and `--remote-debugging-port` take effect from the next launch (A.1.1), because `ow-main` already exists when app code runs; other switches are ignored with a warning. For the current launch use `plugins.overwolf.webview` |
| `disableHardwareAcceleration()` | P | Windows: `--disable-gpu` from the next launch (A.1.1); set `webview.disableGpu` for the first launch; other platforms no-op with a warning |
| `setAppUserModelId(id)` | P | no-op; Tauri sets the AUMID from the bundle identifier |
| `getGPUInfo`, `getAppMetrics`, `setLoginItemSettings`, `getLoginItemSettings`, `dock`, `setBadgeCount`, `setJumpList`, `setUserTasks`, `showAboutPanel`, `setAsDefaultProtocolClient`, `importCertificate`, `moveToApplicationsFolder` | U | |

#### B.2.2 `BrowserWindow` (main)

Constructor options:

| Option | Status | Notes |
|---|---|---|
| `width`, `height`, `x`, `y`, `center`, `minWidth`, `minHeight`, `maxWidth`, `maxHeight`, `useContentSize` | S | logical pixels |
| `show`, `title`, `resizable`, `movable`, `minimizable`, `maximizable`, `closable`, `focusable`, `alwaysOnTop`, `fullscreen`, `skipTaskbar`, `transparent`, `backgroundColor`, `parent` | S | |
| `frame` | P | `false` = no decorations; on macOS mapped to an overlay title bar with a hidden title so native dragging works (ARCHITECTURE section 6) |
| `fullscreenable` | P | `false` is honoured by the facade (ignores `setFullScreen(true)`); no native flag |
| `modal` | P | owned by `parent` and kept above it; input modality Windows-only |
| `name` | S | ow-electron option; normalised; analytics window name |
| `icon` | P | app asset path only |
| `webPreferences.preload` | S | app-asset path of a bundled preload script; injected as an initialization script |
| `webPreferences.devTools` | S | |
| `webPreferences.contextIsolation` | P | preload and page share one JavaScript world in Tauri; `contextBridge.exposeInMainWorld` defines frozen globals, so code written for isolation works; `false` changes nothing |
| `webPreferences.nodeIntegration` | P | ignored with a warning; renderer `require('electron')` works only through the bundler alias, other Node modules are unavailable |
| `webPreferences.sandbox`, `webSecurity`, `partition`, `session`, `offscreen`, `webviewTag`, `zoomFactor`, `backgroundThrottling` | P | ignored with a warning, except `zoomFactor` (applied) |
| `titleBarStyle`, `trafficLightPosition`, `vibrancy`, `visualEffectState`, `roundedCorners`, `thickFrame`, `type`, `tabbingIdentifier`, `kiosk`, `simpleFullscreen` | U | |

Static members: `getAllWindows()`, `getFocusedWindow()`, `fromId(id)`,
`fromWebContents(wc)` are S (from the JS registry).

Instance members:

| Member | Status | Notes |
|---|---|---|
| `id`, `webContents`, `isDestroyed()` | S | |
| `loadURL(url)`, `loadFile(path, { query, hash })` | S | returns a promise that resolves on `did-finish-load`; a remote URL switches the window to class `remote`: its content becomes a fresh `bwr-<id>` webview with no IPC, no preload and no init scripts (A.2.3.1) |
| `show()`, `hide()`, `close()`, `destroy()`, `focus()`, `blur()`, `isVisible()`, `isFocused()` | S | |
| `showInactive()` | P | shows without requesting focus; some platforms still activate the window |
| `minimize()`, `maximize()`, `unmaximize()`, `restore()`, `isMinimized()`, `isMaximized()`, `setFullScreen()`, `isFullScreen()` | S | state reads use the cache, refreshed on every window event |
| `setBounds()`, `getBounds()`, `getContentBounds()`, `setSize()`, `getSize()`, `setPosition()`, `getPosition()`, `setMinimumSize()`, `setMaximumSize()`, `center()` | S | getters are synchronous from the cache |
| `setResizable()`, `setMovable()`, `setAlwaysOnTop()`, `setSkipTaskbar()`, `setFocusable()`, `setIgnoreMouseEvents(ignore, { forward })`, `setTitle()`, `getTitle()`, `setBackgroundColor()`, `setProgressBar()`, `flashFrame()`, `setVisibleOnAllWorkspaces()`, `setContentProtection()` | S | `forward` is ignored |
| `moveTop()` | P | brings the window to the front by toggling always-on-top |
| `setMenu()`, `removeMenu()`, `setMenuBarVisibility()`, `setAutoHideMenuBar()` | P | no-op (Tauri windows have no menu unless the app adds one in Rust) |
| `setOpacity()`, `setVibrancy()`, `setShape()`, `capturePage()`, `setThumbarButtons()`, `setOverlayIcon()`, `previewFile()`, `setBrowserView()`, `addBrowserView()`, `setTouchBar()` | U | |
| events `close` (preventable), `closed`, `focus`, `blur`, `show`, `hide`, `ready-to-show`, `minimize`, `maximize`, `unmaximize`, `restore`, `resize`, `move`, `enter-full-screen`, `leave-full-screen` | S | from `window` host messages |

`webContents` (per window):

| Member | Status | Notes |
|---|---|---|
| `id`, `getURL()`, `getTitle()`, `isLoading()`, `reload()`, `loadURL()`, `loadFile()` | S | |
| `send(channel, ...args)` | S | C.5 |
| `ipc.on/once/removeListener/removeAllListeners`, `ipc.handle/handleOnce/removeHandler` | S | per-window scope, consulted before `ipcMain` (C.2, C.3) |
| `executeJavaScript(code, userGesture?)` | P | runs through native evaluation, not page `eval` (A.2.3 `window_eval`); result returned for `ui` / `overlay` windows (statement code resolves `undefined`); for `remote` windows the code runs and the promise resolves `undefined`; `userGesture` ignored |
| `openDevTools(options?)`, `closeDevTools()`, `isDevToolsOpened()`, `toggleDevTools()` | P | debug builds, or release builds with Tauri's `devtools` feature; `mode` ignored |
| `setZoomFactor()`, `getZoomFactor()` | S | |
| `on('did-finish-load' \| 'dom-ready')` | S | |
| `on('did-fail-load')` | P | Windows only (WebView2 navigation status) |
| `on('render-process-gone')` | P | reason is always `'crashed'` |
| `setWindowOpenHandler(handler)` | P | the handler is called; `{ action: 'allow' }` is treated as deny + open in the system browser (`http`, `https` only) |
| `on('will-navigate')` | P | emitted for top-level navigations the A.2.3.1 policy cancels; `preventDefault()` has no further effect |
| `session`, `debugger`, `print()`, `printToPDF()`, `capturePage()`, `setAudioMuted()`, `startDrag()`, `insertCSS()`, `savePage()`, `sendInputEvent()`, `postMessage()` | U | |

#### B.2.3 `ipcMain` (main) and `ipcRenderer` (preload and renderer)

| Member | Status | Notes |
|---|---|---|
| `ipcMain.on/once/addListener/removeListener/off/removeAllListeners` | S | listener gets `(event: IpcMainEvent, ...args)` |
| `ipcMain.handle/handleOnce/removeHandler` | S | registering a second handler for a channel throws `Error("Attempted to register a second handler for '<channel>'")` like Electron |
| `IpcMainEvent.sender` (WebContents facade), `.reply(channel, ...args)`, `.frameId` (`0`), `.processId` (`0`), `.senderFrame` (`{ url }`) | P | listeners of `ipcMain.on`; `senderFrame` has `url` only |
| `IpcMainInvokeEvent.sender`, `.frameId` (`0`), `.processId` (`0`), `.senderFrame` (`{ url }`) | P | handlers of `ipcMain.handle`; no `reply`, as in Electron; the handler's return value is the reply (`undefined` when it returns nothing) |
| `IpcMainEvent.returnValue`, `.ports` | U | (`sendSync`, MessagePorts) |
| `ipcRenderer.invoke(channel, ...args)` | S | C.2 |
| `ipcRenderer.send(channel, ...args)` | S | C.3 |
| `ipcRenderer.on/once/addListener/removeListener/off/removeAllListeners` | S | listener gets `(event: IpcRendererEvent, ...args)`; `event.sender` is `ipcRenderer` |
| `ipcRenderer.sendSync`, `sendTo`, `sendToHost`, `postMessage` | U | |

#### B.2.4 `contextBridge` (preload)

| Member | Status | Notes |
|---|---|---|
| `exposeInMainWorld(apiKey, api)` | S | defines a non-writable, non-configurable `window[apiKey]`; objects are deep-frozen; functions are called directly (no cloning) |
| `exposeInIsolatedWorld`, `executeInMainWorld` | U | |

#### B.2.5 Other modules

| Member | Status | Notes |
|---|---|---|
| `screen.getAllDisplays()`, `getPrimaryDisplay()`, `getDisplayNearestPoint()`, `getDisplayMatching()` | S | synchronous from the cache; `id` is a stable 32-bit hash of the monitor's OS name and position; `bounds`, `workArea` in DIP; `scaleFactor`; `label` is the OS monitor name |
| `screen.getCursorScreenPoint()` | P | cached, refreshed at most every 100 ms |
| `screen.on('display-added' \| 'display-removed' \| 'display-metrics-changed')` | P | detected by a 2 s poll |
| `screen.dipToScreenPoint()`, `screenToDipPoint()`, `dipToScreenRect()`, `screenToDipRect()` | P | computed from the cached display list |
| `shell.openExternal(url)` | S | `http`, `https`, `mailto` only |
| `shell.openPath(path)`, `shell.showItemInFolder(path)` | S | opener plugin, no shell |
| `shell.trashItem`, `beep`, `writeShortcutLink`, `readShortcutLink` | U | |
| `dialog.showOpenDialog`, `showSaveDialog`, `showMessageBox`, `showErrorBox` | S / S / P / S | `showMessageBox`: up to three buttons |
| `dialog.showOpenDialogSync`, `showSaveDialogSync`, `showMessageBoxSync`, `showCertificateTrustDialog` | U | |
| `globalShortcut.register(accelerator, cb)` | P | returns `true` synchronously; a later registration failure is logged and `isRegistered` turns false |
| `globalShortcut.unregister`, `unregisterAll`, `isRegistered`, `registerAll` | S | |
| `crashReporter.start(options)` | P | no-op with a warning; use a Rust crash handler (see PORT-MAP) |
| `crashReporter.*` (other members) | U | |
| `nativeTheme.shouldUseDarkColors`, `on('updated')` | P | cached from the main window's theme |
| `Menu`, `MenuItem`, `Tray`, `Notification`, `session`, `protocol`, `net`, `netLog`, `powerMonitor`, `powerSaveBlocker`, `autoUpdater` (Electron's), `clipboard`, `nativeImage`, `systemPreferences`, `desktopCapturer`, `webFrame`, `webFrameMain`, `utilityProcess`, `MessageChannelMain`, `BrowserView`, `WebContentsView`, `BaseWindow`, `TouchBar`, `inAppPurchase`, `pushNotifications`, `safeStorage`, `contentTracing` | U | module objects exist so imports compile; every member throws |
| `process.platform`, `process.arch`, `process.argv`, `process.env`, `process.versions` | P | the bootstrap installs a frozen `globalThis.process` shim in `ow-main` and every `bw-*` webview before any app script runs, so code that uses the global `process` without importing it works (the sample's `index.ts` and preload); `ow-tauri/electron` also exports it. `platform` and `arch` Node-style (`win32`, `darwin`, `linux`; `x64`, `arm64`), `argv` the process arguments, `versions` has `owTauri`, `tauri`, `chrome` (WebView2 only) and no `electron`. `env` holds only `OVERWOLF_APP_UID` (after `main_ready`); `process.env.NODE_ENV` is a build-time constant the bundler defines (webpack 5 does it from `mode`; other bundlers: define it explicitly, see MIGRATION.md) |

### B.3 `ow-tauri/renderer`

The renderer runtime is part of the bootstrap the plugin injects into every
`bw-*` webview (section B, "One runtime per webview"), so `<owadview>` works
even if the app never imports the package. The `ow-tauri/renderer` entry point
is a facade over it; its explicit exports are `ipcRenderer`, `contextBridge`
(the same objects as B.2.3, B.2.4) and `owadview` (`{ upgrade(el), elements() }`
for tests).

#### B.3.1 Element model

`owadview` has no hyphen, so it cannot be a custom element
([ADR 0003](adr/0003-owadview-native-child-webviews.md)). The runtime:

1. **Default style.** At document start it adds a constructed style sheet to
   `document.adoptedStyleSheets` with
   `:where(owadview) { display: block; width: 100%; height: 100%; }`.
   `:where()` has zero specificity, so any app rule wins. Without it an
   unknown element is `display: inline` with no content and a 0 x 0 box, and
   the sample's ads (an unstyled `owadview` appended to a sized
   `div.ad-container`) would never mount.
2. **Upgrade at creation.** It wraps `Document.prototype.createElement` and
   `createElementNS` so that an element created with the local name
   `owadview` (any case) is upgraded before it is returned. Upgrading defines
   the element's properties and methods with `Object.defineProperties` on the
   instance and assigns it an `elementId`.
3. **Upgrade on insertion.** A `MutationObserver` on the document
   (`childList`, `subtree`, and `attributes` filtered to the lower-case names
   in B.3.2) upgrades every `OWADVIEW` element that was not created through the
   wrapped functions (parsed HTML, `innerHTML`, elements from before the
   observer ran). If such an element already has an own `customTracking`
   data property (an app set the typed property before the upgrade), the
   upgrade moves that value into the attribute before it defines the accessor,
   so the value is not lost.
4. **Mounting.** An element mounts when it is connected and either its box is
   non-empty or it has the `performance` attribute (B.3.4).

The element stays an `HTMLUnknownElement` for the DOM: CSS selectors, React
rendering and `removeChild` behave as for any element. The
`document.createElement('owadview')` typing comes from the re-declared global
`Document.createElement(tagName: 'owadview'): overwolf.AdviewTag` (B.4).

Not supported: elements inside shadow roots and inside iframes. The observer
watches the top document only; an `owadview` in a shadow root or an iframe
stays inert and the runtime logs one warning per document when it finds one
through `createElement`.

#### B.3.2 Attributes

HTML lower-cases attribute names, so `setAttribute('customTracking', ...)`
stores `customtracking`. The runtime reads and observes the lower-case names
only: `cid`, `slotsize`, `adstyle`, `customtracking`, `performance`, `unit`,
and `pageurl` (with `ads.experimentalElementApi`).

| Attribute (as written / as stored) | Meaning | Change after mount |
|---|---|---|
| `cid` | container id reported with the ad; trimmed to 20 characters | remount |
| `slotsize` | requested inventory `"WxH"`; sizes the sample uses: `400x60`, `400x300`, `400x600`, `300x250`, `160x600`, `728x90`, `970x90`; other values are passed through with a console warning | remount |
| `adstyle` | `"high-impact-ad;"` and other style tokens | remount |
| `customTracking` / `customtracking` (also the `customTracking` property) | JSON string; invalid JSON clears it | delivered live (`{ type: 'customTracking' }`, D.5), no remount |
| `performance` (boolean) | performance ad; **Interim** (OQ-29): at most one per document, a second is ignored with a warning | remount |
| `unit` | ad unit override (the sample shows it commented out on its performance ad) | remount |
| `pageUrl` / `pageurl` | **ow-tauri option** (`ads.experimentalElementApi`, OQ-32): page URL reported to the guest | delivered live (`setPageUrl`, D.5) |
| `id` | ordinary DOM id; not used by the runtime | none |

Any other attribute is ignored.

#### B.3.3 Properties and methods

| Member | Behaviour |
|---|---|
| `customTracking: string` | getter returns the `customtracking` attribute; setter sets it (same as `setAttribute`) |
| `setAudioMuted(muted: boolean): void` | `adview_command setAudioMuted`; guests start muted |
| `reload(): void` | reloads the guest (counts against `ads.maxRecoveries`) |
| `setPageUrl(url: string): void` | **ow-tauri option** (OQ-32), defined only with `ads.experimentalElementApi`: updates `pageurl` and delivers it |
| `sendCommand(command: string, ...args: unknown[]): void` | **ow-tauri option** (OQ-32), defined only with `ads.experimentalElementApi`: `adview_command sendCommand` |

#### B.3.4 Lifecycle and geometry

| Trigger | Action |
|---|---|
| element connected, and box non-empty or `performance` set | `adview_mount` |
| `ResizeObserver` change, `scroll` / `resize` on window or any scrollable ancestor (coalesced per animation frame) | `adview_update { rect }` |
| visibility changes | `adview_update { visible }` |
| attribute in the "remount" column changes | `adview_unmount` + `adview_mount` |
| element disconnected (including via an ancestor) | `adview_unmount` |
| document `pagehide` / unload | Rust closes every guest owned by the webview |

Visibility is `true` only when all hold: `IntersectionObserver` ratio at least
0.5; `el.checkVisibility({ opacityProperty: true, visibilityProperty: true })`
(polled every 500 ms, because ancestor style changes do not fire observers);
`document.visibilityState === 'visible'`. A hidden guest is hidden, not
destroyed; Rust also hides guests when their window is minimized or hidden and
tells them (`window-minimized`, `window-hidden`, D.5). For a `performance`
element only the last two conditions apply.

Native child webviews always paint above the page. Any HTML that must cover an
ad (menus, modals) needs the app to hide the element; the runtime does that
automatically when an ancestor is hidden. CSS transforms and `clip-path` on
ancestors are not reflected in the guest's geometry.

High-impact ads: the app grows the container (the sample sets it to the zone's
size after `high-impact-ad-loaded`); `ResizeObserver` reports the new rect.
Performance ads (**Interim**, OQ-29): the element's own box is ignored; the
guest covers the embedder webview's full viewport (the sample appends a bare
`<owadview performance>` to `document.body`) and follows its size.

Conformance tests use the sample's exact DOM: an unstyled `owadview` in a
400 x 600 `div`, and a bare `performance` element appended to `body`; both
must mount.

#### B.3.5 DOM events

Each guest message `adview_event { name, data }` that is not internal (D.4) is
dispatched on the element as
`new CustomEvent(name, { bubbles: false, cancelable: false, detail: data })`
(**Interim**, OQ-35: a `CustomEvent` is an `Event`, so listeners written for a
plain `Event` keep working; `detail` is extra). Spelling variants are
dispatched in both forms, the received spelling first:

| Received | Also dispatched |
|---|---|
| `ad_clicked` | `ad-clicked` |
| `ad-clicked` | `ad_clicked` |
| `house_ad_action` | `house-ad-action` |
| `house-ad-action` | `house_ad_action` |

Names the sample or the documentation use include `impression`,
`display_ad_loaded`, `player_loaded`, `play`, `complete`,
`high-impact-ad-loaded`, `high-impact-ad-removed`, `shutdown`,
`performance_ad_no_fill`, `performance_ad_dismiss`, `performance_ad_loaded`,
`performance_ad_clicked`, `performance_ad_video_complete` and
`performance_ad_video_skipped`. Every name that passes the A.2.6 checks is
forwarded unchanged; the runtime keeps no list of names.

Host-originated `ad-clicked` (a popup or gesture navigation opened the system
browser, `source: 'host'`) is dispatched only if no guest-originated click
spelling was dispatched for that element in the previous 1000 ms, so one click
yields one pair of events.

#### B.3.6 Window dragging (`app-region`)

Frameless Electron windows drag where CSS says `-webkit-app-region: drag`
(the sample's header sets it through a React inline style). The renderer
runtime emulates this on engines that expose the property in computed style
(WebView2): on a primary-button `mousedown`, it walks from the target up to the
root; if the nearest element with an `app-region` / `-webkit-app-region`
value has `drag`, and no closer element has `no-drag` or is an interactive
control (`button`, `input`, `select`, `textarea`, `a[href]`), it calls
`startDragging()` on the current window. A double-click on a drag region
toggles maximize. On WebKit engines the property is not exposed; there
`frame: false` maps to an overlay title bar (B.2.2) so the native title area
drags.

### B.4 Typings

A port removes `@overwolf/ow-electron`, which is what supplied Electron's
`electron.d.ts` and the global `overwolf` namespace. Code still needs those
names: `@overwolf/ow-electron-packages-types` itself starts with
`import '@overwolf/ow-electron'` and `import type { BrowserWindow,
BrowserWindowConstructorOptions, Size, WebContents, Display, Rectangle } from
'electron'`, and the sample uses `Electron.Display`,
`overwolf.packages.PackageName`, `overwolf.EmailHashes` and the deprecated
`overwolf.OverwolfApp`. `ow-tauri` ships these declarations:

| Declaration | File | Contents |
|---|---|---|
| `declare module 'electron'` | `dist/types/electron.d.ts` | the B.2 surface: every member the facade exports, with unsupported members marked `@deprecated Unsupported in ow-tauri` so editors flag them; plus the types `BrowserWindow`, `BrowserWindowConstructorOptions`, `WebContents`, `Display`, `Rectangle`, `Size`, `Point`, `Event`, `IpcMainEvent`, `IpcMainInvokeEvent`, `IpcRendererEvent`, `OpenDialogOptions`, `SaveDialogOptions`, `MessageBoxOptions` |
| global `namespace Electron` | same file | the same types under `Electron.*` |
| `declare module '@overwolf/ow-electron'` | `dist/types/ow-electron.d.ts` | an empty module, so the packages-types import resolves |
| global `namespace overwolf` | same file | `OverwolfApi`, `OverwolfApp` (deprecated alias of `OverwolfApi`), `EmailHashes`, `CMPWindowOptions`, `ExternalPaymentUserIdOptions`, `AdviewTag`, `Renderer.AdviewTag`, and `packages.*` (`PackageName`, `PackageInfo`, `PendingUpdatesResult`, `SetChannelResult`, `ChannelPackageInfo`, `AvailableChannelsResult`, `CurrentChannelsResult`, `OverwolfPackageManager`) re-declared from the ow-electron 42.11.4 typings |
| `interface Document { createElement(tagName: 'owadview'): overwolf.AdviewTag }` | same file | the global overload |
| `interface ElectronApp { overwolf: overwolf.OverwolfApi }` | `dist/types/electron.d.ts` | the `app.overwolf` augmentation |

An app points TypeScript at them in `tsconfig.json`; the alias in the bundler
and the path in TypeScript must agree:

```jsonc
{
  "compilerOptions": {
    "types": ["node", "ow-tauri/types"],
    "paths": {
      "electron": ["./node_modules/ow-tauri/dist/types/electron.d.ts"],
      "@overwolf/ow-electron": ["./node_modules/ow-tauri/dist/types/ow-electron.d.ts"]
    }
  }
}
```

`@types/node` stays installed: the typings refer to `NodeJS.EventEmitter`, and
main-process code still uses `events` and `path` through polyfills. `autoUpdater`
code imports `UpdateCheckResult` and `UpdateInfo` from `ow-tauri/main` (B.1)
instead of `electron-updater`.

---

## C. IPC routing protocol

### C.1 Roles

| Electron call | Runs in | Wire path |
|---|---|---|
| `ipcRenderer.invoke` -> `ipcMain.handle` / `webContents.ipc.handle` | UI window -> main webview | `ipc_invoke` -> `ipc {kind: invoke}` to `ow-main` -> `ipc_reply` -> `ipc-result` to the UI window |
| `ipcRenderer.send` -> `ipcMain.on` / `webContents.ipc.on` | UI window -> main webview | `ipc_send` -> `ipc {kind: send}` to `ow-main` |
| `webContents.send`, `IpcMainEvent.reply` -> `ipcRenderer.on` | main webview -> UI window | `ipc_emit` -> `ipc {kind: message}` to the UI window |

Rust is the only router. Webviews talk to Rust with commands; Rust talks to a
webview only through that webview's subscribed channel (A.3). No webview can
send to, or listen to, another webview directly.

Protocol commands: `ipc_subscribe`, `ipc_invoke`, `ipc_send`, `ipc_skip`
(`overwolf:renderer`, A.2.5) and `ipc_subscribe`, `ipc_main_ready`,
`ipc_reply`, `ipc_emit` (`overwolf:main`, A.2.1). `IpcErrorWire` is the
`OverwolfErrorWire` shape of A.4.

### C.2 `invoke`

1. The renderer runtime encodes `args` with the OTJ codec (C.7), assigns the
   next sequence number `seq` of its current `epoch` (C.3), records a pending
   entry, and calls `plugin:overwolf|ipc_invoke { channel, args, epoch, seq }`.
2. Rust checks the caller class (`ui` or `overlay`), the epoch, the encoded
   size (`ipc.maxMessageBytes`), the sender's in-flight count
   (`ipc.maxInFlightInvokes`, else `ipc-overloaded`) and that `channel` is a
   non-empty string of at most 256 UTF-16 code units. It allocates a request
   `id` (a `u64` counter starting at 1, never reused in a process, always
   below 2^53), stamps `sender = { windowId, label, url, frameId: 0 }` from the
   calling webview, and returns `{ id }`. The runtime keys the pending entry by
   `id`.
3. Rust delivers the request to `ow-main` in `seq` order per sender (C.3).
4. The main runtime looks for a handler: first `webContents.ipc.handle` of
   the sender's window, then `ipcMain.handle`. Neither: it replies with
   `{ code: 'ipc-no-handler', message: "No handler registered for '<channel>'" }`.
5. The handler is called as `handler(event, ...decodedArgs)` with an
   `IpcMainInvokeEvent` (B.2.3). Its return value (awaited if it is a
   promise) is encoded and sent with `ipc_reply { id, ok: true, value, seq }`;
   a handler that returns nothing sends no `value`, which decodes to
   `undefined`. A throw or rejection is sent as `ipc_reply { id, ok: false,
   error, seq }` with `error = { code: 'ipc-remote-error', message, data: {
   name, message } }`.
6. Rust queues `ipc-result { id, ok, value | error }` on the sender's channel
   in the main runtime's per-target `seq` order (C.5), so every message the
   handler sent to that window before returning arrives first, as Electron's
   single pipe guarantees. The renderer runtime resolves or rejects the
   pending entry. A remote error rejects with `OwTauriError('ipc-remote-error',
   "Error invoking remote method '<channel>': <name>: <message>")`, the same
   text Electron produces, with `data.name` and `data.message` attached.

Timeouts and failures:

- `ipc.invokeTimeoutMs = 0` (default) means no timeout, as in Electron. A
  positive value rejects with `ipc-timeout` after that many milliseconds; a
  late `ipc_reply` is dropped and logged at debug level.
- If `ow-main` restarts (A.6) or the sender is destroyed, every pending request
  involving it is rejected with `not-ready` ("main webview restarted") or
  dropped.
- An `ipc_reply` for an unknown id is ignored (debug log). A second reply for
  the same id is ignored.

### C.3 `send`, epochs and ordering

`ipc_send { channel, args, epoch, seq }` returns as soon as Rust has queued
the message. The main runtime dispatches it first to the sender window's
`webContents.ipc` listeners and then to `ipcMain` listeners, each with
`(event, ...args)`, where `event.reply(channel, ...args)` sends back to the
sender.

Ordering guarantee: messages (`invoke` and `send`) from one document reach the
main runtime in the order the document issued them. Tauri may run commands
concurrently, so Rust reorders by `seq`:

- **Epochs.** `ipc_subscribe` returns a random `epoch` per document load.
  Every `ipc_invoke` / `ipc_send` carries it, and `seq` starts at 1 in each
  epoch. A call with an epoch other than the webview's current one is
  rejected with `not-ready` and never reordered, so a reload never mixes with
  the previous document's sequence. Subscribing again discards the old
  epoch's reorder state.
- **Gaps.** A call that Tauri rejects before the command runs (A.4) never
  reaches the reorder buffer. When the runtime sees such a rejection it calls
  `ipc_skip { epoch, seq }`. As a fallback, an out-of-order message waits for
  a missing number for at most 1 s, after which the gap is skipped and a
  warning logged.
- No ordering is guaranteed between different senders.

The reorder buffer is covered by property tests (random interleavings,
duplicates, gaps, stale epochs) in the plugin's test suite.

### C.4 Startup queue and back-pressure

Requests that arrive before `ipc_main_ready` are held in a FIFO of at most
`ipc.startupQueueMax` entries. Overflow rejects the newest request with
`not-ready`; an entry older than `ipc.startupTimeoutMs` is rejected with
`not-ready`. `send` messages are held the same way and dropped (with a
warning) when they expire.

Bounds that always apply:

| Bound | Default | Over the bound |
|---|---|---|
| invokes in flight per sender webview (`ipc.maxInFlightInvokes`) | 256 | `ipc_invoke` rejects with `ipc-overloaded` |
| host messages queued for one webview that has not drained them (`ipc.maxQueuedMessages`) | 4096 | the `ipc_emit` or `ipc_send` that would exceed it rejects with `ipc-overloaded`; package and state messages are never dropped (they are bounded by their producers) |
| encoded size of one message (`ipc.maxMessageBytes`) | 8 MiB | `ipc-serialization` at the sender |

### C.5 `webContents.send` and `reply`

The main runtime calls `ipc_emit { target, channel, args, seq }`, where `seq`
is a per-target counter it shares with `ipc_reply` for invokes from that
target. Rust:

- drops the message with a warning if the target is `remote`, destroyed or not
  a `bw-*` webview (Electron would deliver to a remote page; ow-tauri never
  gives remote pages IPC);
- applies the target's `seq` order to `ipc_emit` and `ipc_reply` together;
- buffers messages until that webview has called `ipc_subscribe` (bounded by
  `ipc.maxQueuedMessages`); a reload empties the buffer, as Electron's
  renderer reload does;
- queues `ipc { kind: 'message', channel, args }` on the target's channel.

The renderer runtime calls each `ipcRenderer` listener for `channel` with
`(event, ...decodedArgs)`, where `event = { sender: ipcRenderer, senderId: 0, ports: [] }`.

### C.6 State sequence

`state` messages use their own sequence (B.1.6). It is independent of IPC
`seq` values.

### C.7 Serialisation: the OTJ codec

Electron serialises IPC values with the structured clone algorithm; Tauri
IPC carries JSON. ow-tauri uses a tagged JSON encoding, **OTJ** (ow-tauri JSON),
implemented once in `packages/ow-tauri/src/shared/otj.ts`. Rust treats OTJ
values as opaque `serde_json::Value` and never decodes tags.
`OtjValue` in the tables above means a JSON value produced by this encoding.

| JavaScript value | Encoded as | Decoded as |
|---|---|---|
| `null`, `boolean`, string, finite number other than `-0` | itself | itself |
| `undefined` as an argument or array element | `{ "$otj": "undefined" }` | `undefined` |
| `undefined` as an object property | property omitted | property absent (structured clone keeps the key; documented difference) |
| `NaN`, `Infinity`, `-Infinity`, `-0` | `{ "$otj": "number", "v": "NaN" \| "Infinity" \| "-Infinity" \| "-0" }` | the number |
| `bigint` | `{ "$otj": "bigint", "v": "<decimal>" }` | `bigint` |
| `Date` | `{ "$otj": "date", "v": "<ISO 8601>" }` (invalid date: `"v": null`) | `Date` |
| `RegExp` | `{ "$otj": "regexp", "source": "...", "flags": "..." }` | `RegExp` |
| `Error` and subclasses | `{ "$otj": "error", "name", "message", "stack"? }` | `Error` with `name` set |
| `ArrayBuffer`, typed arrays, `DataView`, Node-style `Buffer` | `{ "$otj": "bytes", "type": "<constructor name>", "b64": "..." }` | the same typed array type (`Buffer` decodes as `Uint8Array`, as Electron delivers it to renderers) |
| `Map` | `{ "$otj": "map", "entries": [[k, v], ...] }` | `Map` |
| `Set` | `{ "$otj": "set", "values": [...] }` | `Set` |
| plain object or class instance | object of own enumerable string-keyed properties | plain object |
| object that has an own `"$otj"` key | `{ "$otj": "object", "v": { ... } }` | the original object |
| array | array (holes become `{ "$otj": "undefined" }`) | array |

Values that throw `OwTauriError('ipc-serialization')` at the sender, naming
the path of the offending value (for example `args[1].handler`): functions,
symbols, DOM nodes, `Window`, promises, `WeakMap`/`WeakSet`, and cyclic or
shared references (structured clone supports cycles; OTJ does not). An encoded
message larger than `ipc.maxMessageBytes` also throws `ipc-serialization`.

### C.8 Error mapping

| Situation | Renderer sees |
|---|---|
| no handler | `OwTauriError('ipc-no-handler', "Error invoking remote method '<ch>': Error: No handler registered for '<ch>'")` |
| handler threw | `OwTauriError('ipc-remote-error', "Error invoking remote method '<ch>': <name>: <message>")` |
| timeout | `OwTauriError('ipc-timeout', ...)` |
| unencodable argument | `OwTauriError('ipc-serialization', ...)`, thrown synchronously by `invoke` and `send` |
| main webview not ready or restarted, or a stale epoch | `OwTauriError('not-ready', ...)` |
| too many invokes in flight or messages queued | `OwTauriError('ipc-overloaded', ...)` |
| caller is not a `ui` / `overlay` window | `OwTauriError('forbidden', ...)` |

All are `instanceof Error`, so existing `try/catch` and `.catch()` code keeps
working; only code that parses Electron's message text sees the same prefix.

---

## D. Guest shim contract

The ad page is `https://www.overwolf.com/monsdk/electron/latest/adview.html`;
the consent pages are under `https://content.overwolf.com/monsdk/electron/latest/cmp/`.
ow-tauri never modifies those pages. It injects one script into each guest
webview as an initialization script: `adview-host.js` for ads, `cmp.js` for
consent. Both are written in TypeScript in `packages/ow-tauri/src/guest/`,
linted and unit-tested with the rest of the package, built into
`crates/tauri-plugin-overwolf/js/` (committed, with a CI drift check) and
embedded with `include_str!` ([ADR 0012](adr/0012-js-runtime-singleton.md)).

Guests are remote content and get exactly one command each (`adview_event`,
`cmp_event`), scoped to the page's own path; everything they send is
untrusted ([ADR 0011](adr/0011-remote-guest-ipc.md)). Third-party scripts in
the ad page's main frame can call the command just as the shim can; the limits
in D.4 and D.7 are what bounds them, not the shim.

### D.1 Injection rules

- Main frame only, at document start, before page scripts.
- The script does nothing unless `location.origin` is `https://www.overwolf.com`
  (ads) or `https://content.overwolf.com` (consent).
- Idempotent: a second run in the same document is a no-op.
- Configuration is spliced in by replacing the token
  `/*__OW_TAURI_ADVIEW_CONFIG__*/null` (ads) or `/*__OW_TAURI_CMP_CONFIG__*/null`
  (consent) with JSON in which `<`, U+2028 and U+2029 are escaped.
- The guest's own transport is `window.__TAURI_INTERNALS__.invoke`, kept in a
  closure at startup so page scripts cannot redirect it later. Messages are
  queued (at most 200, retried every 250 ms) until it is available.

### D.2 `window.__overwolf__` data

Defined with `Object.defineProperty(window, '__overwolf__', { writable: false, configurable: false, enumerable: true })`
and deep-frozen. Keys whose value would be `undefined` are omitted.

| Key | Type | Value |
|---|---|---|
| `uid` | string | app uid (G.2) |
| `name` | string | `productName`, else `name` (G.1) |
| `owVersion` | string | `tauri-<tauri crate version>`, for example `tauri-2.12.1` (OQ-03) |
| `version` | string | manifest `version` |
| `windowName` | string | analytics name of the embedder window (E.2) |
| `windowTitle` | string | embedder window title |
| `testAd` | boolean | `true` unless the host runs live (D.7) |
| `consent` | string | effective consent string (D.6) |
| `consentFull` | string | full consent string when distinct from `consent`, else equal |
| `slotSize` | string | element `slotsize` |
| `containerId` | string | element `cid` |
| `systemInfo` | object | `{ os, arch, cpu, displays: [{ name, isMain, resolution: [w, h], position: [x, y], dpi, scaleFactor }] }`; `os` and `arch` in Node spelling; `cpu` is `"<arch> (<n> logical cores)"` (OQ-19) |
| `runTimeInfo` | object | `{}` |
| `emailHashes` | object | only when `ads.exposeEmailHashesToGuest` and hashes are set and FPD is enabled (OQ-11) |
| `settings` | object | `{ disableOptimization: boolean, anonymous: boolean }`: `disableOptimization` is true when `disableAdsOptimization()` was called or the manifest's `build.overwolf.disableAdOptimization` is true; `anonymous` is true when anonymous analytics are disabled |
| `muid` | string | E.4 |
| `muidV2` | string | equal to `muid` (OQ-02) |
| `phasePercent` | number | identity phase percent |
| `pageUrl` | string | element `pageurl`, only with `ads.experimentalElementApi` (OQ-32); otherwise omitted |
| `performanceAd` | boolean | element has `performance` |
| `adStyle` | string | element `adstyle` |
| `unit` | string | element `unit`; in test mode a non-empty value is replaced by `"testAd"` so a performance ad cannot turn live |
| `customTracking` | object or null | parsed element `customTracking` |

### D.3 `window.__overwolf__` functions and `window.gc`

| Function | Behaviour |
|---|---|
| `setMute(muted)` | `adview_event '__host:setMute' { muted }`; Rust mutes or unmutes the guest |
| `triggerEvent(name, ...args)` | `adview_event name` with `data` = the single argument, or the argument array when there are several; `message` and `messageerror` are dropped |
| `applySetting(setting)` | `adview_event '__host:applySetting'`; recorded only (ow-tauri implements no setting-driven behaviour, and never scans user data) |
| `crash()` | `adview_event '__host:crash'`; Rust treats it as a crash for recovery purposes |
| `reload()` | `adview_event '__host:reload'`, then `location.reload()` |
| `getSystemInformation()` | a copy of `systemInfo` |
| `getCustomTracking()` | a copy of the current `customTracking` |
| `onmessage(handler)` | registers `handler` (at most 16); host messages are passed to every handler as a fresh copy; handler exceptions are swallowed |

`window.gc` is defined as a no-op function when the page has none; the ad page
forwards its events only when it exists (verified with the reference
implementation in test mode).

### D.4 Guest to host

`adview_event { slotId, name, data }`. The shim sanitises `data` before
sending: functions, DOM nodes and `Window` objects are dropped, cycles are
cut, `Event` objects are flattened to `{ type }`, and a value whose JSON
encoding exceeds 16 KiB is replaced by `{ truncated: true, bytes }`. Rust
applies the same 16 KiB rule (A.2.6), because page scripts can call the
command without the shim.

Rust limits each guest with a token bucket (`ads.guestLimits`): at most
`eventsPerSecond` messages per second with bursts up to `eventBurst`, and at
most `bytesPerSecond` of encoded `data`. Messages over the limit are dropped;
the count is logged once per minute per guest. A guest that stays over the
limit for 10 s is reloaded once, then closed (counting as a recovery, D.7).

Internal names (handled by Rust, never dispatched on the element):

| Name | Data | Effect |
|---|---|---|
| `__host:ready` | `{ href, testAd, visibilityState }` | guest initialised |
| `__host:gesture` | `{ kind: 'pointerdown' \| 'keydown' \| 'iframe-focus' }` | user gesture reported by the shim (debounced 100 ms); opens the navigation window (D.7). Page scripts can forge it, so it grants at most one external open (D.7) |
| `__host:focus` | `{ focused }` | guest focus changes |
| `__host:setMute`, `__host:applySetting`, `__host:crash`, `__host:reload` | see D.3 | |

Every other name is forwarded to the embedder as an `adview-event` host message
(B.3.5).

### D.5 Host to guest

Rust calls `webview.eval("window.__owTauriHost && window.__owTauriHost.deliver(<json>)")`;
`deliver` validates `{ type: string, data? }` and passes it to the
`onmessage` handlers.

| `type` | `data` | Sent when |
|---|---|---|
| `ad-clicked` | URL string | a popup or gesture navigation was opened in the system browser |
| `window-minimized` | none | the embedder window was minimized |
| `window-hidden` | none | the embedder window was hidden |
| `consent` | consent string | consent changed (D.6); the shim also rewrites its cookies |
| `customTracking` | object or `null` | the element's `customTracking` changed |
| `setPageUrl` | URL string (**Interim** payload) | **ow-tauri option** (`ads.experimentalElementApi`, OQ-32): `setPageUrl()` or the `pageurl` attribute changed |
| `sendCommand` | `{ command: string, args: unknown[] }` (**Interim** payload) | **ow-tauri option** (`ads.experimentalElementApi`, OQ-32): `sendCommand()` |
| `eHashes` | `EmailHashes` (**Interim** payload) | **ow-tauri option** (`ads.exposeEmailHashesToGuest`, OQ-11) |

ow-tauri sends no other host messages (OQ-13).

### D.6 Consent in the guest and the consent page

Effective consent, in order: the consent saved by this app on this machine
(F.2 `cmp.unifiedConsentString`, else `cmp.cmpString`), else `""`.

The ad shim writes the consent to cookies on `.overwolf.com` before the page
loads its scripts: `euconsent-v2=<tcf>` and, for a unified string
`cmp=<tcf>&ac=<ac>` (URL-decoded), `acconsent=<ac>`; attributes
`SameSite=None; Secure; domain=.overwolf.com; path=/; max-age=33696000` (OQ-08).

The consent shim (`guest/cmp.js`) defines frozen globals for the consent page,
as the reference implementation does:

| Global | Behaviour |
|---|---|
| `window.cmp.saveConsent(value)` | `cmp_event saveConsent { consent }` (a TCData object is reduced to its `tcString`) |
| `window.cmp.saveUnifiedConsent(value)` | `cmp_event saveUnifiedConsent { consent }` |
| `window.privacy.enableAdOptimization(enabled)` | `cmp_event enableAdOptimization { enabled }`; returns a resolved promise |
| `window.privacy.getIsAdOptimizationEnabled()` | resolves the stored value (default `true`) |
| `window.close()` | `cmp_event close`; Rust closes the window |

The consent window URL is `consent.cmpUrl` or, by default,
`https://content.overwolf.com/monsdk/electron/latest/cmp/22.3.27/cmp.html`
(OQ-07), with the query `uid, appName, tabName, lang, firstRun, cmpRequired,
muid, muidv2, oweVersion, appVersion`; `oweVersion` is the `owVersion` value
of D.2 (`tauri-<version>`). `CMPWindowOptions.cmpURL` overrides the URL per
call. The typings put no restriction on it; ow-tauri accepts only
`https://content.overwolf.com/monsdk/electron/` URLs (`invalid-argument`
otherwise), because the consent window's one command is granted to exactly
that scope, and a page elsewhere could not save consent or close itself
(deliberate security choice, OQ-07).

If the consent page has not sent `cmp_event ready` within
`consent.readyTimeoutMs` (30 s), or its main-frame load fails, Rust closes the
window and the open call resolves; the user closing the window also resolves
it. `openCMPWindow` never stays pending.

On save, Rust validates the string, writes `cmp` in the state file (F.2),
delivers `{ type: 'consent' }` to every running guest, and does **not**
recreate guests. With `consent.gateAdsOnConsent`, the first `adview_mount` of
a session waits until `isCMPRequired` has resolved and, when it is `true` and
no consent is stored, until the consent window is closed or 30 s pass
(after which the mount fails with `not-ready`).

### D.7 Sizes, test mode, clicks and recovery

- **Sizes:** see B.3.2. The guest webview is created at the element's rect;
  `slotSize` tells the page what to request.
- **Test mode** (`--test-ad`, `OW_TAURI_TEST_AD=1`, `ads.testAd`, or
  `Builder::test_ad(true)`): `testAd: true` and the `unit` rewrite above.
  Otherwise the host runs live, as ow-electron does without `--test-ad`
  ([ADR 0005](adr/0005-ads-test-live-parity.md)). Live mode does not touch the
  guest's `localStorage`, so the documented `owAdTestAd` switch keeps working.
- **New windows:** always denied in the guest. The URL, if `http` or
  `https`, opens in the system browser and the guest receives `ad-clicked`;
  the element receives a host `ad-clicked` (B.3.5).
- **Navigation:** top-level navigation to a non-Overwolf host is cancelled. If
  a gesture was reported in the guest within `ads.gestureWindowMs`, the URL
  opens in the system browser (treated as a click); otherwise it is dropped and
  logged (OQ-17).
- **External-open limits.** Every open in the system browser from a guest
  (popup or navigation) needs a gesture: on Windows, WebView2's
  `NewWindowRequested.IsUserInitiated` for popups; elsewhere, and for
  navigations, a `__host:gesture` in the window above. One gesture allows one
  open. On top of that, a guest may open at most
  `ads.guestLimits.externalOpensPerMinute` URLs per minute (default 5); only
  `http` and `https` URLs without credentials are opened. Everything else is
  dropped and logged.
- **Mute:** guests start muted; `setAudioMuted` changes it.
- **Crash recovery:** a crashed guest is reloaded, at most `ads.maxRecoveries`
  times per element (counted per element across remounts in the same document);
  after that the guest is closed and the element receives no further events.
- **Load errors:** a failed main-frame load is retried after
  `ads.loadErrorRetryMs` (platforms that report load failures: Windows, Linux;
  macOS partial).

---

## E. Analytics

Anonymous app analytics are on by default in ow-electron (Overwolf
documentation). ow-tauri sends the same event names and request shapes as the
reference implementation, labelled as a Tauri host
([ADR 0006](adr/0006-analytics-labelling.md)). The event catalogue and wire
format must be confirmed by Overwolf (OQ-04); until then they are as below.

### E.1 Endpoints and wire format

**Counter**: `GET https://analyticsnew.overwolf.com/analytics/Counter?Name=<event>&MUID=<muid>&MUIDV2=<muidV2>&owver=<owver>&Extra=<json>`

- Query encoding as `URLSearchParams`. `MUIDV2` is omitted when empty. No body.
- `Extra` is a JSON object whose keys are, in order: `app_ver`, `app_id` (uid),
  `os` (Node platform: `win32`, `darwin`, `linux`), `os_ver`, `app_name`,
  `app_cuid`, then the event's own fields, then (only with
  `analytics.hostFields`) `host: "tauri"`, `hostVersion`, `platform`.
- `app_name` is the G.1 app name: `build.productName`, else `productName`,
  else `name`.
- `os_ver` is what Node's `os.release()` returns on that OS: on Windows
  `<major>.<minor>.<build>` (for example `10.0.26100`); on macOS the Darwin
  kernel release (for example `25.0.0`, from `uname -r`), not the marketing
  version; on Linux the kernel release (`uname -r`, for example
  `6.8.0-45-generic`).

**InsertStats**: `POST https://tracking.overwolf.com/tracking/InsertStats?Stats=true&owver=<owver with '.' replaced by '_'>`,
`Content-Type: application/json`, body `{"Kind": <number>, "Extra": "<v1>.<v2>..."}`.

- `Extra` is the values of the event fields, then `app_ver`, `app_id`, `os`,
  `app_name`, `app_cuid`, then the host fields when enabled; each value has
  `.` and `:` replaced by `_`; values are joined with `.`; `null` values are
  skipped.

`owver` is `tauri-<tauri crate version>`. One attempt per request, 30 s
timeout, no retry, no persistence; failures are logged only. User agent:
`<productName>/<version> tauri/<tauri version>`.

### E.2 Events

| Trigger | Counter `Name` (fields) | InsertStats `Kind` | Mandatory |
|---|---|---|---|
| first launch of this install (`firstLaunch` absent in the state file) | `electron_app_first_launch` | 400022 | Counter yes, Kind no |
| every launch, after `main_ready` | `electron_app_start` | none | no |
| launch, then an hourly check that sends when 12 h have passed since the last one in this session; forced when the first window is shown | `electron_app_heartbeat` (`hasVisibleWindow`) | 400023 | yes, yes |
| a window that was visible for at least 1 s becomes hidden or closes; every window class except `ow-main` and guests | `electron_window_closed` (`name`, `title`, `length` in seconds) | none | no |
| an ad guest is created | none | 400025 | no |
| an ad guest crashes (the first crash, then only if it lived more than 5 s; at most 10 per session) | `electron_owadview_crashed` (`sessionTS`, `reason`) | 400024 (`reason`) | no |

Window `name`: the `BrowserWindow` `name` option with whitespace and special
characters removed (as the ow-electron changelog documents for 34.4.1); else
the last path segment of the loaded URL without `.html`; else `index`. The
consent window is named `cmp`.

Event names keep the `electron_` prefix so Overwolf's pipeline needs no
change; the `owver` value identifies the host.

Not sent: events tied to the Windows ad-optimisation helper, which ow-tauri
does not ship (OQ-14), and any report for `setExternalPaymentUserId` (OQ-12).

### E.3 Opt-outs and switches

| Switch | Effect |
|---|---|
| `disableAnonymousAnalytics()` before `main_ready` | only mandatory requests for the session |
| `disableAnonymousAnalytics()` after `main_ready` | mandatory only from then on; a warning is logged |
| `analytics_set_user_enabled(false)` (with `analytics.userSwitch`) | nothing at all, persisted |
| test builds | `Builder::analytics_transport` replaces the HTTP client; the default transport refuses non-loopback hosts under `cfg(test)` |

### E.4 Identity options

| Option | Values | Default | Notes |
|---|---|---|---|
| `analytics.muidStrategy` | `per-install`: random UUID v4, upper-case, stored in `ow-tauri.json`; `machine-id`: derived from the OS machine identifier with the derivation Overwolf specifies | `per-install` | `machine-id` is unavailable (falls back to `per-install` with a warning) until Overwolf confirms the derivation (OQ-02) |
| `analytics.hostFields` | `true` / `false` | `false` | OQ-03 |
| `muidV2` | | equals `muid` | OQ-02 |

`phasePercent` = sum of the character codes of the lower-case hex MD5 of the
muid without `-`, modulo 100 (reference implementation; OQ-02 covers whether
Overwolf phases a Tauri host by the same value).

---

## F. Per-app state file

### F.1 Location

`<appData>/ow-electron/<uid>/`, where `<appData>` is the OS configuration
directory (`%APPDATA%` on Windows, `~/Library/Application Support` on macOS,
`$XDG_CONFIG_HOME` or `~/.config` on Linux) and `<uid>` is the app uid (G.2).
This is the directory ow-electron uses for the same uid, observed on a machine
running ow-electron apps ([ADR 0007](adr/0007-state-file-continuity.md)).

| File | Owner | Access |
|---|---|---|
| `ow-electron.json` | shared with ow-electron | read-write for the shared keys only; unknown keys preserved byte-for-byte in value |
| `ow-tauri.json` | ow-tauri | read-write |
| `logs/ow-tauri.log` | ow-tauri | append |

### F.2 `ow-electron.json` shared keys

```jsonc
{
  "firstLaunch": true,                    // set once the first-launch event was recorded
  "cmp": {
    "cmpString": "<TCF v2 string>",
    "unifiedConsentString": "cmp=<tcf>&ac=<ac>",
    "timeStamp": 1759750000000
  },
  "utmParams": { "utm_source": "..." }    // written by Overwolf's installer; read-only for ow-tauri
}
```

- ow-tauri reads `firstLaunch`, `cmp.*` and `utmParams`, and writes
  `firstLaunch` and `cmp.*`. It never writes `utmParams` and never removes keys.
- Writes are read-modify-write under an in-process lock, written to a temp file
  in the same directory and renamed over the original. If the existing file is
  not valid JSON it is left untouched, a warning is logged and ow-tauri keeps
  its values in `ow-tauri.json` for the session.

### F.3 `ow-tauri.json`

```jsonc
{
  "schema": 1,
  "muid": "8C7E...-...",                  // per-install strategy only
  "stagingId": "3f2a...",                 // updater staging bucket (I.2)
  "packageChannels": { "gep": "beta" },   // setChannel persistence
  "adOptimization": true,                 // consent page toggle
  "analyticsUserEnabled": true,           // only with analytics.userSwitch
  "createdBy": "ow-tauri 0.1.0"
}
```

Unknown `schema` values newer than the running version are read best-effort
and never downgraded on write.

### F.4 Logs

`logs/ow-tauri.log`, one line per entry:
`[YYYY-MM-DD HH:MM:SS.mmm] [<level>] <message>` in local time. Each session
starts with `ow-tauri <version> session start - app '<productName>' <version> - uid <uid> - pid <pid>`.
The file rolls at 5 MiB, keeping 3 files. `packages.logsFolderPath` is the
`logs` directory (a native runtime may write its own files there).

### F.5 Migration from ow-electron

| Data | Result for a user who ran the ow-electron build of the same app |
|---|---|
| uid | identical when `productName` and `author` are unchanged (G.2) |
| consent | kept: the same `cmp` block is read |
| first launch | not re-sent: `firstLaunch` is already set |
| UTM parameters | kept |
| package channels | not migrated (ow-electron keeps them in its own browser storage); users re-select |
| muid | differs under `per-install` (OQ-02) |
| app prefs in `userData` | kept: `app.getPath('userData')` is the same directory (B.2.1) |

---

## G. Manifest

### G.1 Fields

`package.json` stays the single manifest. The app's `build.rs` embeds it
(G.3); nothing reads it from disk at runtime.

| Field | ow-electron use | ow-tauri |
|---|---|---|
| `name` | app name fallback | `app.name` fallback; uid input |
| `productName`, `build.productName` | app name; uid input | same; `build.productName` wins when both exist (it is what the builder writes into the packaged app); checked against `tauri.conf.json` `productName` (mismatch = build warning) |
| `author` (string or `{ name }`) | uid input | same; a string `author` is used as-is (OQ-01) |
| `version` | app version | `app.getVersion()`, analytics `app_ver`, guest `version`; checked against `tauri.conf.json` `version` |
| `main` | Electron entry, hashed for signing | ignored; the main webview entry is `plugins.overwolf.main.url` |
| `overwolf.packages` | which packages load; the builder also adds `utility` whenever any package is listed | same: only listed names get `loading` / `ready` / `failed-to-initialize`, and channel calls validate against it. Accepted names: `gep`, `overlay`, `recorder`, `utility`, `crn`; other strings are passed to the runtime as-is. `utility` is **not** added implicitly (**Interim**, OQ-34): the list is used as written, and a build warning says so when packages are listed without `utility` |
| `overwolf.uid` | written by console signing | honoured as the uid when present (G.2) |
| `build.overwolf.disableAdOptimization` | builder: skip downloading the Windows optimisation helper | default of the runtime ad-optimisation switch (`settings.disableOptimization`, D.2); nothing is downloaded either way |
| `build.overwolf.enablePackageBundling` | builder: bundle `.owepk` packages | passed to a native runtime's build hook if one is registered; otherwise a build warning that it has no effect |
| `build.overwolf.overridePackagesUrl` | builder: package list URL | passed to a native runtime; otherwise a warning |
| `build.overwolf.requireSigning` | builder: Overwolf signing required (Windows) | validated; a release build with signing required emits a build warning that Overwolf signing for Tauri is not defined yet (OQ-09). Never faked |
| `build.overwolf.enableOWCertSigning` | builder: sign with Overwolf's certificate | same as above |
| other `build.*` (NSIS, files, asar, ...) | electron-builder | ignored; the Tauri bundler uses `tauri.conf.json` (PORT-MAP) |

### G.2 App uid

1. `plugins.overwolf.uid` (or `Builder` override) if set.
2. Else `overwolf.uid` from the manifest (console-signed builds).
3. Else computed: `sha1("{'author':'<author>','name':'<name>.electron'}")`
   where `<name>` is the app name from G.1 and `<author>` is `author.name` or
   the `author` string; each digest byte `b` becomes the two characters
   `'a' + (b & 15)` then `'a' + (b >> 4)`, giving 40 characters in `a`..`p`.

The `.electron` suffix is kept on purpose: the uid keys the developer console,
the ad configuration and the state directory, so a Tauri build of the same
app must keep it ([ADR 0007](adr/0007-state-file-continuity.md)). Overwolf's
CLI command `ow client calc-electron-uid` computes the uid of an Electron app
from the same inputs and is the recommended cross-check (OQ-01).
`cuid` is always the computed value (3), even when 1 or 2 apply.

### G.3 Build helper

```rust
// src-tauri/build.rs
fn main() {
    tauri_plugin_overwolf::build::embed_manifest("../package.json")
        .expect("package.json overwolf manifest");
    tauri_build::build();
}
```

`embed_manifest` parses and validates the fields above, emits
`cargo:rerun-if-changed`, prints `cargo:warning` lines for the conditions in
G.1, and writes `$OUT_DIR/ow-tauri-manifest.json`, which
`embedded_manifest!()` includes. Validation errors (missing `name` and
`productName`, `overwolf.packages` not an array of strings, non-boolean
`build.overwolf` flags) fail the build with the field path.

Build warnings, besides those in the table:

- The app name (G.1) contains `bot` in any case. Overwolf documents that app
  names containing "bot" are refused, because ad partners see the name.
- `overwolf.packages` lists packages but not `utility` (OQ-34).

In debug builds `embed_manifest` also embeds `dev-app-update.yml` when the
file exists next to `package.json`, for `forceDevUpdateConfig` (I.1).

`EmbeddedManifest` (JSON, also in `HostSnapshot.manifest`):

```ts
interface EmbeddedManifest {
  name: string; productName: string; version: string; author: string;
  overwolf: { packages: string[]; uid?: string };
  buildOverwolf: { disableAdOptimization: boolean; enablePackageBundling: boolean;
                   overridePackagesUrl?: string; requireSigning: boolean; enableOWCertSigning: boolean };
  raw: Record<string, unknown>;   // the whole package.json minus devDependencies and scripts
}
```

---

## H. Package runtime interface

The full guide for runtime authors will be `docs/PACKAGE-RUNTIME.md`; this
section is the binding contract ([ADR 0004](adr/0004-packages-backend-selection.md)).

### H.1 Backend selection and failure reasons

`packagesBackend` (A.1) resolves at setup:

| Value | Result |
|---|---|
| `native` | the registered native runtime; none registered: every listed package fails with `reason: 'no-native-runtime'` |
| `simulated` | the simulated backends (H.7) |
| `none` | every listed package fails with `reason: 'packages-disabled'` |
| `auto` (default) | native if registered; else simulated in debug builds; else every listed package fails with `reason: 'unsupported-host'` |

Failure emits, per listed package and in manifest order: `loading`, then
`failed-to-initialize(event, name, { reason, version })` where `version` is the
ow-tauri version. A runtime may report its own reasons; reasons are
lower-case kebab-case strings.

### H.2 Rust trait

```rust
/// A component that runs Overwolf packages for the host.
pub trait PackageRuntime: Send + Sync + 'static {
    /// Name and version reported in `PackagesSnapshot.runtime`.
    fn info(&self) -> RuntimeInfo;

    /// Called once at setup. The runtime keeps `host` to emit events, invoke
    /// callbacks and make host requests (windows, paths).
    fn initialize(&self, init: InitializeParams, host: HostHandle) -> BoxFuture<'_, Result<InitializeResult, RuntimeError>>;

    /// Load one listed package on its current channel. The result names the
    /// members the runtime provides and the initial sync state (H.4).
    fn load(&self, name: &str, channel: &str) -> BoxFuture<'_, Result<LoadedPackage, LoadFailure>>;

    /// Call a package member, e.g. ("gep", "setRequiredFeatures", [5426, null]).
    /// Arguments and the result use the remote-value encoding (H.2.1).
    fn call(&self, package: &str, method: &str, args: Vec<serde_json::Value>) -> BoxFuture<'_, Result<serde_json::Value, CallError>>;

    /// Call a method on a handle this runtime returned earlier (H.2.1).
    fn handle_call(&self, package: &str, handle: u64, method: &str, args: Vec<serde_json::Value>) -> BoxFuture<'_, Result<serde_json::Value, CallError>>;

    /// JS dropped these handles; the runtime may free them.
    fn handle_release(&self, package: &str, handles: &[u64]);

    /// An action on an actionable event (enable, inject, dismiss, prevent-default, abort).
    fn event_action(&self, event_id: u64, action: PackageEventAction, args: Vec<serde_json::Value>) -> Result<(), RuntimeError>;

    /// All JS listeners for the event returned; apply defaults for unanswered actions.
    fn event_settled(&self, event_id: u64) -> Result<(), RuntimeError>;

    /// Persist the channel and start its download. `ready` is invoked through
    /// `HostHandle::invoke_callback` once the download completes and a restart
    /// is required, never if the package is already on that version.
    fn set_channel(&self, name: &str, channel: Option<&str>, ready: Option<CallbackId>) -> BoxFuture<'_, Result<SetChannelResult, RuntimeError>>;
    fn available_channels(&self, names: &[String]) -> BoxFuture<'_, Result<BTreeMap<String, Vec<String>>, RuntimeError>>;
    fn relaunch(&self) -> BoxFuture<'_, Result<(), RuntimeError>>;
    fn shutdown(&self) -> BoxFuture<'_, ()>;
}

pub struct LoadedPackage {
    pub version: String,
    /// Member paths the runtime implements, e.g. "getFeatures", "hotkeys.register",
    /// "installHighElevationHelper". Optional upstream members not listed are
    /// absent in JS; required members not listed reject `unsupported`.
    pub members: Vec<String>,
    /// Initial values of the package's sync state (H.4).
    pub state: serde_json::Map<String, serde_json::Value>,
}
```

`HostHandle` lets the runtime emit `RuntimeEvent`s, call back into JS and use
host services:

```rust
pub enum RuntimeEvent {
    Manager(ManagerEvent),                       // loading, ready, failed-to-initialize, crashed, package-update-pending, updated
    Package { package: String, event: String, args: Vec<Value>, event_id: Option<u64>, actions: Vec<PackageEventAction> },
    State { package: String, path: String, value: Value },   // sync caches, H.4
    Log { level: LogLevel, message: String },
}

impl HostHandle {
    pub fn emit(&self, event: RuntimeEvent);
    pub fn invoke_callback(&self, id: CallbackId, args: Vec<Value>);   // -> package-callback host message
    pub fn release_callbacks(&self, ids: &[CallbackId]);              // -> package-callback-release
    // host services
    pub async fn create_window(&self, request: WindowCreateRequest) -> Result<WindowHandle, RuntimeError>;
    pub fn native_window_handle(&self, window_id: u32) -> Option<RawWindowHandle>;
    pub fn close_window(&self, window_id: u32);
    pub fn paths(&self) -> RuntimePaths;
}
```

`create_window` makes the same windows as the `BrowserWindow` facade and
announces them to `ow-main` with a `window` `created` message, so overlay
windows have a real `BrowserWindow` on the JS side. `native_window_handle`
returns the HWND on Windows and the `NSWindow*` on macOS, for capture or
injection.

#### H.2.1 Remote values

Package members take callbacks, take `BrowserWindow`s, and return objects with
methods. None of these are JSON, so arguments, results and event arguments use
this encoding between `ow-tauri/main` and the runtime (Rust passes it through
unchanged):

| Value | Encoded as | Notes |
|---|---|---|
| a function argument (callback) | `{ "$cb": <id> }` | `id` is unique per main-runtime session. The runtime invokes it with `package/callback { cbId, args }` (sidecar) or `HostHandle::invoke_callback`, which becomes a `package-callback` host message; JS calls the function with the decoded `args` and ignores its return value. The runtime releases it with `package/callbackRelease` once it will never call it again (after a one-shot callback ran; on `hotkeys.unregister`; when a later `setChannel` replaces a `ready`). JS keeps every unreleased callback alive |
| a `BrowserWindow` facade | `{ "$window": <id> }` | the Electron-style window id |
| a `WebContents` facade | `{ "$webContents": <id> }` | the owning window's id |
| an object with methods, returned or passed by the runtime | `{ "$handle": <id>, "kind": "<Kind>", "data": { ... } }` | JS materialises one facade per `(package, id)` and returns the same facade for the same id; methods call `package_handle_call` / `packages/handleCall`; JS sends `package_handle_release` when the facade can no longer be used (see kinds), and also from a `FinalizationRegistry` |
| a value-only object built in JS | `{ "$value": "<Kind>", "data": { ... } }` | materialised locally, no handle |
| an error (`data.error` of a failed call) | `{ "$kind": "RecorderError" \| "UtilityApiError" \| "Error", ... }` | rebuilt as in B.1.5 |

Plain JSON values that happen to have a key starting with `$` are wrapped as
`{ "$json": <value> }` by the side that sends them.

Kinds defined by contract version 1:

| Kind | Encoding | `data` | Methods and lifetime |
|---|---|---|---|
| `OverlayBrowserWindow` | handle | `{ windowId, id, name, scaleFactor, overlayOptions }` | `window` = `BrowserWindow.fromId(windowId)`; `overlayOptions` writes call `setOverlayOptions(partial)`; `startDragging()`; released when the window closes |
| `ActiveReplay` | handle | `{ timeout? }` | `stop(cb?)`, `stopAfter(ms, cb?)`; the `callback` property is the JS function passed to `captureReplay`; released after the replay ends |
| `CaptureSettingsBuilder` | value | `CaptureSettings` defaults | `add*Source` / `add*Capture` are synchronous and append `{ op, args }` to a local list (window arguments encoded as `$window`); `build()` returns `{ ...data, $builderOps: [...] }`, which the runtime applies when it receives the settings |

A runtime may define further kinds; JS materialises an unknown kind as a
frozen plain object of its `data` and logs a warning.

### H.3 JSON-RPC sidecar protocol

A native runtime can be a separate executable. ow-tauri starts it with
`--ow-tauri-runtime-protocol=1` and talks JSON-RPC 2.0 over its stdin and
stdout, framed like the Language Server Protocol:
`Content-Length: <bytes>\r\n\r\n<UTF-8 JSON>`. Stderr lines go to the
ow-tauri log at debug level. The trait methods map one-to-one:

| Direction | Method | Params | Result |
|---|---|---|---|
| host -> runtime | `initialize` | `InitializeParams` (below) | `{ runtime: { name, version }, packages: string[], protocolVersion: 1, pendingUpdates: PendingUpdatesResult }` |
| host -> runtime | `packages/load` | `{ name, channel }` | `{ version, members: string[], state: object }`, or error with `data.reason` |
| host -> runtime | `packages/call` | `{ package, method, args }` | any (H.2.1 encoding) |
| host -> runtime | `packages/handleCall` | `{ package, handle, method, args }` | any (H.2.1 encoding) |
| host -> runtime (notification) | `packages/handleRelease` | `{ package, handles }` | |
| host -> runtime | `packages/eventAction` | `{ eventId, action, args }` | `null` |
| host -> runtime | `packages/eventSettled` | `{ eventId }` | `null` |
| host -> runtime | `packages/setChannel` | `{ name, channel, ready?: { $cb } }` | `SetChannelResult` |
| host -> runtime | `packages/getAvailableChannels` | `{ names }` | `Record<string, string[]>` |
| host -> runtime | `packages/relaunch` | none | `null` |
| host -> runtime | `shutdown` (request), then `exit` (notification) | none | `null` |
| runtime -> host (notification) | `manager/event` | `{ type, name?, version?, details?, info?, canRecover?, eventId? }` | |
| runtime -> host (notification) | `package/event` | `{ package, event, args, eventId?, actions? }` | |
| runtime -> host (notification) | `package/callback` | `{ cbId, args }` | |
| runtime -> host (notification) | `package/callbackRelease` | `{ cbIds }` | |
| runtime -> host (notification) | `state/patch` | `{ package, path, value }` | |
| runtime -> host (notification) | `log` | `{ level, message }` | |
| runtime -> host (request) | `host/createWindow`, `host/closeWindow`, `host/nativeWindowHandle`, `host/paths` | as H.2 | |

```ts
interface InitializeParams {
  protocolVersion: 1;
  host: { name: 'ow-tauri'; version: string; tauriVersion: string; os: string; arch: string };
  app: { uid: string; name: string; version: string; packages: string[]; channels: Record<string, string>;
         packagesUrl?: string; enablePackageBundling: boolean };
  switches: { packageChannel?: string; forcePhasedPackage?: string | true; packagesUrl?: string };
  devMode: { email?: string; apiKey?: string; devKey?: string };   // from the environment, A.1
  identity: { muid: string; phasePercent: number };
  paths: { packagesDir: string; logsDir: string; cacheDir: string };
}
```

Error objects use JSON-RPC `code` -32000 to -32099 with `data: { reason }`;
`packages/call` and `packages/handleCall` errors carry the package's own error
in `data.error` with a `$kind` tag (H.2.1), which ow-tauri rebuilds in JS
(B.1.5). The sidecar is killed if it does not answer `shutdown` within 5 s. A
crash of the sidecar emits `crashed` `(event, canRecover: true)`; unless a
listener calls `preventDefault()`, ow-tauri restarts it (at most 3 times per
session) and reloads the listed packages. Every callback and handle of the
crashed process is released on the JS side.

### H.4 Package state caches

Synchronous package members are answered from per-package state that the
runtime publishes with `state/patch` (`HostSnapshot.packages.packageState.<package>.<path>`),
starting from `LoadedPackage.state`:

| Package | Path | Used by |
|---|---|---|
| `overlay` | `activeGameInfo` | `getActiveGameInfo()` |
| `overlay` | `hotkeys` | `hotkeys.all()` (the JS registry is authoritative for `update` / `unregister` return values) |
| `overlay` | `windows.<handle>` | `OverlayBrowserWindow` `name`, `scaleFactor`, `overlayOptions` |
| `overlay` | `version` | `version` |
| `recorder` | `version`, `ffmpegPath`, `ffprobePath`, `binFolderPath`, `options` | readonly properties and the `options` proxy |
| any | `version` | `packages[name].version` |

Manager state is kept by the host, not published by the runtime:
`packages.pendingUpdates` starts from the `initialize` result and, on every
`manager/event package-update-pending { info }`, becomes `{ hasPendingUpdate:
true, details: info }`; Rust applies that `state` patch before it dispatches
`package-update-pending` to JS. A relaunch starts a new process, so the value
resets to what the runtime reports at the next `initialize`.

### H.5 Actionable events and defaults

| Event | Actions | Default when settled without an action |
|---|---|---|
| packages `crashed` | `prevent-default` | relaunch the package |
| gep `game-detected` | `enable` | not enabled (no events for that game) |
| overlay `game-launched` (from detection or from `requestGameInjection`) | `inject(options?)`, `dismiss` | dismiss (OQ-16) |
| crn `before-notification` | `abort` | the notification shows |

Listeners may act asynchronously (ow-electron 39.x documents async
`game-detected` callbacks); ow-tauri sends `package_event_settled` after every
returned promise settles, or after 10 s.

### H.6 C ABI (sketch)

For runtimes shipped as a shared library (`OW_TAURI_PACKAGE_RUNTIME=<path to .dll/.dylib/.so>`).
The messages are exactly the JSON-RPC messages of H.3, without framing:

```c
#define OW_RUNTIME_ABI_VERSION 1

typedef struct OwHostCallbacks {
  void *host;
  /* runtime -> host: a complete JSON-RPC message (request, response or notification), UTF-8, NUL-terminated */
  void (*send)(void *host, const char *json);
} OwHostCallbacks;

typedef struct OwRuntimeV1 {
  uint32_t abi_version;                                  /* OW_RUNTIME_ABI_VERSION */
  void *ctx;
  int32_t (*start)(void *ctx, const OwHostCallbacks *cb);   /* 0 = ok */
  void (*receive)(void *ctx, const char *json);          /* host -> runtime message; must not block */
  void (*stop)(void *ctx);                               /* after this returns, cb is never used again */
} OwRuntimeV1;

/* The only exported symbol. Returns NULL if the runtime cannot run on this host. */
const OwRuntimeV1 *ow_runtime_v1_entry(void);
```

Strings passed in either direction are owned by the caller and valid only for
the duration of the call. `send` may be called from any thread.

### H.7 Simulated backends

Development only; never selected by `auto` in release builds. Every simulated
package reports `version: "0.0.0-simulated"` and logs one warning at load.

| Package | Data source | Notes |
|---|---|---|
| gep | Overwolf's public game-events status JSON (`https://game-events-status.overwolf.com/gamestatus_prod.json` and `<gameId>_prod.json`), cached in the app cache directory; offline fixtures under `crates/tauri-plugin-overwolf/fixtures/gep/` | feature keys with `category` become `new-info-update`, others `new-game-event`; `is_index` keys get the `_<index>` suffix. Fixtures use well-known public game ids already in the status data, for example 5426 (League of Legends); contributors do not add fixtures for other titles without review (CONTRIBUTING.md) |
| overlay | real windows + `tauri-plugin-global-shortcut` | B.1.4; `requestGameInjection` emits `game-launched` for a running simulated game and rejects otherwise |
| recorder | state machine | B.1.4; writes no media |
| utility | scenario data | B.1.4 |
| crn | scenario data | B.1.4 |

Scenarios (`SimScenario`) are JSON:
`{ "name": string, "steps": [{ "afterMs": number, "package": string, "event": string, "args": unknown[] }] }`.

---

## I. Updater

The update client reads the electron-updater **generic provider** feed that
Overwolf's console serves for ow-electron apps
(`https://electron-updates.overwolf.com/electron-updates/electron/<app id>`, per
Overwolf's release-management documentation) ([ADR 0008](adr/0008-updater-client.md)).

### I.1 Configuration

```ts
interface UpdaterConfig {
  provider: 'generic';                 // the only supported provider
  url: string;                         // https; http only for localhost in debug builds
  channel?: string;                    // default 'latest'
  allowDowngrade?: boolean;            // default false
  allowPrerelease?: boolean;           // default false
  autoDownload?: boolean;              // default true (electron-updater default)
  autoInstallOnAppQuit?: boolean;      // default true
  forceDevUpdateConfig?: boolean;      // use the embedded dev-app-update.yml (debug builds only)
  requestHeaders?: Record<string, string>;
}
```

- **`channel` side effect.** As in electron-updater, assigning `channel` also
  sets `allowDowngrade = true`. Code that wants downgrades off must set
  `allowDowngrade = false` *after* `channel`, as the sample does.
- **`forceDevUpdateConfig`.** A Tauri app has no "app root" on disk. In debug
  builds `embed_manifest` embeds `dev-app-update.yml` from the directory of
  `package.json` (G.3); with `forceDevUpdateConfig` the client reads its
  `provider` and `url` from that copy instead of `setFeedURL`. In release builds
  the property is ignored with a warning.
- **`logger`.** Any object with `info`, `warn`, `error` and optional `debug`
  methods (`console` works); the client calls them with one string argument
  per message, as electron-updater does. `null` silences it.

### I.2 Feed handling

1. `GET <url>/<file>` where `<file>` is `<channel>.yml` on Windows,
   `<channel>-mac.yml` on macOS and `<channel>-linux.yml` on Linux
   (`channel` defaults to `latest`), with a cache-busting query.
2. Parsed fields: `version` (semver), `files[]: { url, sha512, size }`,
   `path`, `sha512` (legacy top-level), `releaseDate`, `releaseName`,
   `releaseNotes`, `stagingPercentage`.
3. Update available when `version > current` (or `!=` with
   `allowDowngrade`); prereleases only with `allowPrerelease`.
4. `stagingPercentage`: available only if
   `stagingBucket < stagingPercentage`, where `stagingBucket` is derived from
   `ow-tauri.json` `stagingId` (0 to 99).
5. File choice per OS: Windows `.exe` (NSIS) else `.msi`; macOS `.zip`
   containing the `.app`; Linux `.AppImage`. Relative `url`s resolve against the
   feed URL.

### I.3 Download and verification

- HTTPS only (localhost excepted in debug builds); redirects allowed to HTTPS.
- The file is streamed to the app cache directory, then its size and base64
  SHA-512 are checked against the feed entry. A mismatch deletes the file and
  emits `error` with `backend`.
- **Publisher signature** ([ADR 0008](adr/0008-updater-client.md)), fail
  closed:
  - Windows: when the running executable carries a valid Authenticode
    signature, the installer must carry a valid signature whose subject is
    one of `updater.publisherNames` (default: the running executable's own
    signer subject). An unsigned running executable skips the check and logs
    a warning once per session.
  - macOS: the unpacked `.app` must pass code-signature validation and carry
    the running app's team id (I.4).
  - Detached signature: when `updater.pubkey` is set (a minisign public key,
    the format `tauri-plugin-updater` uses), the client downloads
    `<file url>.sig` and verifies it before anything else. This is required on
    Linux, where no OS signature exists, and optional elsewhere.
  - Any failure deletes the file and emits `error` with `backend`.
- Events (`updater` host messages, mirrored on `autoUpdater`): `checking-for-update`,
  `update-available(info)`, `update-not-available(info)`,
  `download-progress({ percent, bytesPerSecond, total, transferred })`,
  `update-downloaded(info)`, `error(error)`.

### I.4 Install per OS

| OS | Installer | Action |
|---|---|---|
| Windows | NSIS `.exe` | run `"<file>" /S /UPDATE` (`updater.installerArgs` overrides; without `isSilent` the `/S` is dropped), then exit |
| Windows | `.msi` | `msiexec /i "<file>" /quiet /norestart`, then exit |
| macOS | `.zip` with `.app` | unpack to a temp dir, verify the bundle's code signature matches the running app's team id, replace the running bundle atomically, relaunch |
| Linux | `.AppImage` | requires `updater.pubkey` (I.3; without it `quitAndInstall` fails with `unsupported` and the app should point the user to a manual update); replace `$APPIMAGE` atomically, relaunch |
| Linux | other | `error` with `unsupported` (manual update) |

`autoInstallOnAppQuit` installs a downloaded update on normal exit.
`isForceRunAfter` (and every macOS and Linux install) relaunches the app: on
Windows ow-tauri starts a detached copy of its own executable with
`--ow-tauri-relaunch-after=<installer pid>`, which waits for the installer to
exit with code 0, starts the installed app and exits without creating windows.

### I.5 `autoUpdater` in `ow-tauri/main`

A subset of electron-updater's `AppUpdater`: properties `autoDownload`,
`autoInstallOnAppQuit`, `allowDowngrade`, `allowPrerelease`, `channel`,
`forceDevUpdateConfig`, `logger`, `currentVersion`; methods `setFeedURL(options)`,
`checkForUpdates()`, `checkForUpdatesAndNotify()` (no OS notification is shown;
partial), `downloadUpdate()`, `quitAndInstall(isSilent?, isForceRunAfter?)`;
events as I.3. The returned `UpdateCheckResult` has `updateInfo`,
`versionInfo` and `isUpdateAvailable`; `cancellationToken` and
`downloadPromise` are not provided. `ow-tauri/main` exports the
`UpdateCheckResult`, `UpdateInfo`, `ProgressInfo` and `UpdaterConfig` types
(B.1). Whether Overwolf's console accepts Tauri installers and serves them in
this feed is OQ-18.
