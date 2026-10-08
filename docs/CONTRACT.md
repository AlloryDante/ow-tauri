# ow-tauri contract

This is the specification that the Rust plugin (`tauri-plugin-overwolf`) and the
npm package (`ow-tauri`) are both implemented against. When code and this
document disagree, the code is wrong until this document is changed in the
same pull request.

Contract version: **1** (ow-tauri 0.1.0). Reference versions: ow-electron
42.11.4, `@overwolf/ow-electron-packages-types` 1.1.12, Tauri 2.12.1. Parity
baseline: ow-electron 42.11.4 as observed with the parity harness
([PARITY.md](PARITY.md)).

**ow-tauri replicates ow-electron.** Everything Overwolf's services can see
(requests, headers, cookies, files on disk, the values the ad and consent pages
read) is copied from ow-electron. The one deliberate difference is the host
label ([section 0, "Host label"](#host-label)). Where a platform cannot
reproduce a behaviour, this document says so and names the fallback.

**Scope: the ads system first.** Ads, consent, analytics, identity, updates and
distribution are specified here in full. GEP, overlay, recorder, utility and
CRN are not implemented: `app.overwolf.packages` reports them exactly as
ow-electron reports packages that are not available on the host (section H).
The package runtime interface Overwolf could implement is kept as a deferred
design in [Appendix P](#appendix-p-deferred-design-package-runtime-interface)
([ADR 0004](adr/0004-packages-backend-selection.md)).

| Section | Covers |
|---|---|
| [0. Conventions](#0-conventions) | naming, casing, labels, sources, the host label, placeholders |
| [A. Rust plugin](#a-rust-plugin) | configuration, every command, every host message, errors, the Rust API, main-webview liveness and lifecycle |
| [B. JavaScript API](#b-javascript-api) | `ow-tauri/main`, `ow-tauri/electron`, `ow-tauri/renderer`, typings |
| [C. IPC routing protocol](#c-ipc-routing-protocol) | invoke, send, reply, ids, ordering, back-pressure, serialisation, errors |
| [D. Guest shim contract](#d-guest-shim-contract) | `window.__overwolf__` in the ad page, guest messages, the startup consent window, consent cookies, request shaping |
| [E. Analytics](#e-analytics) | user agent, request shapes, events and their order, opt-outs, machine id |
| [F. Per-app state file](#f-per-app-state-file) | paths, exact encoding of `ow-electron.json`, logs, migration |
| [G. Manifest](#g-manifest) | `package.json` fields, the uid rule with test vectors, the build helper, signing |
| [H. Packages](#h-packages) | what `app.overwolf.packages` does while no package runtime exists |
| [I. Updater and distribution](#i-updater-and-distribution) | Overwolf's update feed, verification, install per OS, installer parity |
| [Appendix P](#appendix-p-deferred-design-package-runtime-interface) | deferred design: Rust trait, JSON-RPC sidecar, C ABI, package objects |

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
  `ow-main`, `bw-<id>` (local UI windows), `bwr-<id>` (a remote page shown in
  window `bw-<id>`), `owad-<embedderLabel>-<n>` (ad guests), `ow-cmp-startup`
  (the hidden startup consent window, D.6.1), `ow-cmp-default` (the hidden
  default-consent window of the first settings-window call, D.6.4) and
  `ow-cmp` (the consent settings window, D.6.4). App code never sees labels; it sees Electron-style
  integer ids. Capabilities match webview labels only (ARCHITECTURE
  section 5.2).
- **Time** values are milliseconds (`u64` in Rust, `number` in JS) unless a
  field name says otherwise. Timestamps are Unix epoch milliseconds, except
  where ow-electron uses seconds (F.2 `cmp.timeStamp`).
- **Optional** fields may be absent; `null` is accepted wherever a field is
  optional.
- **Reference implementation.** Several modules (ads host, guest scripts,
  consent, analytics) are extracted from an earlier proof of concept that
  rendered Overwolf's test ads in Tauri child webviews and was verified
  headlessly. Where this document says "as the reference implementation does",
  that code and its tests are the source.

### Sources

Each behaviour copied from ow-electron names its source with a tag:

| Tag | Source |
|---|---|
| [DOC] | Overwolf's documentation (`https://dev.overwolf.com/ow-electron/`) and Overwolf's legal and privacy pages |
| [TYPES] | the published ow-electron 42.11.4 typings and `@overwolf/ow-electron-packages-types` 1.1.12 |
| [SAMPLE] | the official `ow-electron-packages-sample` |
| [BUILDER] | the published JavaScript of `@overwolf/app-builder-lib` 26.9.3 (including its NSIS templates) and of `@overwolf/ow-cli` 0.1.10 |
| [OBS] | matches ow-electron (observed): black-box observation of a running ow-electron 42.11.4 app with the parity harness, or of Overwolf's public update feed ([PARITY.md](PARITY.md)) |
| [POC] | the reference implementation |
| [DEC] | an ow-tauri decision where the sources above do not pin the behaviour down |
| [INF] | an inference that is not verified yet; a lab check confirms it before release ([PARITY.md, "Lab checks"](PARITY.md#lab-checks)) |

Markers used in the text:

- **Unknown (R2-n)** / **Unknown (R3-n)**: not settled yet; harness item
  `R2-n` (round 2, still running or needing another OS) or `R3-n` (round 3)
  ([PARITY.md](PARITY.md#harness-rounds)) will observe it. The text gives the
  interim behaviour.
- **Interim (OQ-nn)**: an interim behaviour tied to an open question in
  [OPEN-QUESTIONS.md](OPEN-QUESTIONS.md).
- **ow-tauri option**: a behaviour ow-electron does not have. Off by default,
  configurable, and documented as non-parity.

### Host label

The owner's rule for labelling is: wherever ow-electron names itself
("electron", its version), ow-tauri says "tauri" instead
([ADR 0006](adr/0006-analytics-labelling.md)). All such values derive from two
settings, so Overwolf can change them in one place:

- `analytics.hostLabel`, default `"tauri"` (written `<label>` below);
- `analytics.hostVersion`, default the Tauri crate version (`<tv>`, for
  example `2.12.1`).

`<owVersion>` is `<label>-<hostVersion>` (for example `tauri-2.12.1`), or
`<hostVersion>` alone when `<label>` is `electron`, so that
`hostLabel: "electron"`, `hostVersion: "42.11.4"` reproduces ow-electron's
values exactly [DEC].

| Where | ow-electron sends [OBS] | ow-tauri sends |
|---|---|---|
| Counter `Name` (E.2) | `electron_app_first_launch`, `electron_app_start`, `electron_app_heartbeat`, `electron_window_closed`, `electron_owadview_crashed`, `electron_sub_info` | `<label>_app_first_launch`, `<label>_app_start`, `<label>_app_heartbeat`, `<label>_window_closed`, `<label>_owadview_crashed`, `<label>_sub_info` |
| uninstall Counter from the installer (I.6) [BUILDER] | `ow_electron_app_uninstall` | `ow_<label>_app_uninstall` |
| Counter `owver` | `42.11.4` | `<owVersion>` (`tauri-2.12.1`) |
| InsertStats `owver` | `42_11_4` | `<owVersion>` with `.` replaced by `_` (`tauri-2_12_1`) |
| guest `__overwolf__.owVersion` (D.2) | `"42.11.4"` | `"<owVersion>"`, unless `ads.owVersionOverride` is set |
| consent page query `oweVersion` (D.6) | `42.11.4` | `<owVersion>`, unless `ads.owVersionOverride` is set |
| user agent token (E.1) | `Electron/42.11.4` | `<Label>/<hostVersion>` with the first letter of the label upper-cased (`Tauri/2.12.1`) |

Never substituted, because they are Overwolf's identifiers and changing them
breaks continuity or points at nothing:

- URLs: `https://www.overwolf.com/monsdk/electron/latest/adview.html`,
  `https://content.overwolf.com/monsdk/electron/latest/cmp/...`,
  `https://electron-updates.overwolf.com/electron-updates/electron/<id>`, and
  the signing endpoints `/sign/electron*` (G.4);
- the `.electron` suffix in the uid formula (G.2);
- the state directory and file `<appData>/ow-electron/<uid>/ow-electron.json`
  (F);
- the registry keys `HKCU\Software\OverwolfElectron`,
  `HKCU\Software\OverwolfPersist` and `Software\OverwolfElectron\<uid>` (E.4,
  I.6);
- cookie names, and the analytics that Overwolf's own pages send (the consent
  page's `electron_cmp_accept_full_launch`, the ad page's `owads_*` / `oam_*`).
  ow-tauri does not control them (E.5).

**Risk (OQ-03, OQ-04).** Console dashboards may filter on the `electron_`
names or on a numeric `owver`, and the ad and consent pages derive their own
`ClientVer` / `CurrentVersion` analytics values from
`owVersion` / `oweVersion`. The lab check in
[PARITY.md](PARITY.md#lab-checks) confirms that the ad page still fills and
reports with `tauri-<tv>`. If it does not, `owver` stays labelled and
`ads.owVersionOverride` gives the guest and the consent page a numeric value.

### Placeholders

| Placeholder | Meaning |
|---|---|
| `<uid>` | app uid (G.2) |
| `<cuid>` | computed uid (G.2 rule 3) |
| `<muid>`, `<muidV2>` | machine ids (E.4) |
| `<PN>` | app name: top-level `productName`, else `name` (G.1) |
| `<PNNS>` | `<PN>` with spaces removed (user agent token) |
| `<ver>` | `package.json` `version` |
| `<tv>` | Tauri crate version, for example `2.12.1` |
| `<label>`, `<owVersion>` | host label values (above) |
| `<UA>` | the composed user agent (E.1) |
| `<locale>` | the app locale in BCP 47 form, for example `en-US` |
| `<appData>` | the OS configuration directory (F.1) |

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
      "webview": {                    // browser arguments of the app environment (A.1.1)
        "disableGpu": false,          // --disable-gpu (Windows)
        "remoteDebuggingPort": null,  // --remote-debugging-port=<n> (Windows, debug builds)
        "additionalBrowserArgs": []   // appended verbatim (Windows)
      },
      "uid": null,                    // uid override; wins over overwolf.uid and the formula (G.2)
      "packagesBackend": "none",      // "none" | "native" (H; "native" is reserved for Appendix P)
      "ads": {
        "testAd": false,              // true = test inventory (same as --test-ad)
        "requestShaping": true,       // D.8; always on, the switch exists for debugging only
        "owVersionOverride": null,    // string given to the guest owVersion and consent oweVersion (section 0)
        "macPrivateHeaderApi": false, // ow-tauri option: macOS private header SPI prototype (D.8.3)
        "transparentGuests": true,    // ad guests transparent from creation (B.3.4); false = opaque guests
        "gestureWindowMs": 1500,      // user-gesture window for guest top-level navigation
        "maxRecoveries": null,        // null = no cap, as ow-electron (D.7); a number is an ow-tauri option
        "loadErrorRetryMs": 5000,     // reload interval after a failed main-frame load (D.7)
        "guestLimits": {              // per guest webview (D.4)
          "eventsPerSecond": 50, "eventBurst": 100,
          "bytesPerSecond": 262144,
          "externalOpensPerMinute": 20 // per guest; each open also needs its own gesture (D.7)
        }
      },
      "analytics": {
        "hostLabel": "tauri",         // section 0, "Host label"
        "hostVersion": null,          // null = the Tauri crate version
        "muidStrategy": "machine-id", // "machine-id" | "per-install" (non-parity option, E.4)
        "userSwitch": false           // ow-tauri option: expose analytics_set_user_enabled
      },
      "consent": {
        "cmpUrl": null,               // settings window page override; any https URL (D.6.4)
        "readyTimeoutMs": 30000,      // the hidden consent windows are closed after this if still open (D.6.1, D.6.4)
        "hostCookieFallback": "auto"  // "auto" | "never" (D.6.3)
      },
      "emailHashes": { "encoding": "hex" },   // "hex" (parity) | "base64" (ow-tauri option, OQ-10)
      "logging": { "enabled": false },        // F.4; ow-electron writes no log
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

The whole `plugins.overwolf` object is optional. Tauri hands a missing
object to the plugin as `null`, so the plugin's configuration type is
`Option<Config>` (A.5) and `null` means "all defaults".

Removed by the parity revision, and rejected with a build warning when
present: `analytics.hostFields` (ow-electron sends no host fields),
`ads.experimentalElementApi` (the element members are now always defined,
B.3.3, OQ-32), `ads.exposeEmailHashesToGuest` (OQ-11),
`ads.legacyHostMessages` (the observed messages are always sent, D.5),
`consent.gateAdsOnConsent` (replaced by the sequencing in D.6.5),
`consent.cmpRequired` (replaced by the observed source in D.6.2), and the
`packagesBackend` values `auto` and `simulated` (H).

Environment variables:

| Variable | Effect |
|---|---|
| `OW_TAURI_TEST_AD=1` | same as `ads.testAd: true` |
| `OW_TAURI_PACKAGES_BACKEND=<value>` | overrides `packagesBackend` |
| `OW_TAURI_REMOTE_DEBUGGING_PORT=<n>` | same as `webview.remoteDebuggingPort` (debug builds only) |
| `OW_CLI_EMAIL`, `OW_CLI_API_KEY`, `OW_DEV_KEY` | dev-mode credentials, read with Overwolf's documented precedence (`OW_CLI_EMAIL` with `OW_CLI_API_KEY`, else `OW_DEV_KEY`) in debug builds only and kept for a future package runtime (Appendix P); never logged and never sent anywhere by ow-tauri [DOC] |
| `OW_CLI_EMAIL`, `OW_CLI_API_KEY`, `OW_BUILD_KEY`, `OW_CLI_API_URL`, `OW_REQUIRE_SIGNING` | build time only: the signing step (G.4) [BUILDER] |
| `OW_TAURI_PACKAGE_RUNTIME=<path>` | reserved for Appendix P; ignored with a warning |
| `OW_TAURI_ALLOW_UNSIGNED=1` | build time only: a Windows release build that requires signing (G.4 e) but has no signed output builds anyway, with a warning. For local unsigned builds; never for a release that ships |
| `OW_TAURI_ALLOW_MISSING_JS=1` | build time only: the crate builds without the injected scripts (`js/bootstrap.js` and the guest scripts, ADR 0012) and embeds a placeholder that only reports their absence. For CI jobs that test the Rust side and never run the app. Without it a release build fails when a script is missing; a debug build embeds the placeholder and prints a build warning |
| `OW_TAURI_REQUIRE_JS=1` | build time only: a debug build also fails when an injected script is missing |

Command line switches (read from the process arguments at setup):

| Switch | Source of the name | Effect |
|---|---|---|
| `--test-ad` | ow-electron documentation | test ad inventory |
| `--owepm-package-channel=<pkg>:<channel>[,...]` | ow-electron documentation | accepted and ignored while no package runtime exists (H) |
| `--force-phased-package[=<pkg>,...]` | ow-electron documentation | accepted and ignored (H) |
| `--owepm-packages-url=<url>` | ow-electron documentation (QA) | accepted and ignored (H) |
| `--ow-tauri-packages-backend=<value>` | ow-tauri | overrides `packagesBackend` |

`app.commandLine.hasSwitch()` / `getSwitchValue()` in `ow-tauri/electron` see
the same argument list.

#### A.1.1 Webview environments

WebView2 requires every webview that shares a user-data directory to use the
same browser arguments, and passing custom arguments replaces wry's default
`--disable-features=...` value. ow-electron gives its ad guests web security
off and insecure content allowed (D.8.1) while app windows keep the defaults.
ow-tauri therefore uses **two** webview environments:

| Environment | Webviews | Data store | Windows browser arguments |
|---|---|---|---|
| app | `ow-main`, `bw-*`, `bwr-*` | Windows: user data folder `<appData>/<PN>/EBWebView` (the app's userData directory, F.1); macOS: the default `WKWebsiteDataStore`; Linux: the default WebKitGTK context | the app set below |
| ads | `owad-*`, `ow-cmp-startup`, `ow-cmp-default`, `ow-cmp` | the **ads data store** (D.8.1): Windows: user data folder `<appData>/<PN>/EBWebView-ow`; macOS: the default `WKWebsiteDataStore` (shared with the app environment, as ow-electron's guests share the default session [OBS]); Linux: the default WebKitGTK context | the app set plus `--disable-web-security --allow-running-insecure-content` |

The app set is computed once at setup, from `webview.*`, the environment and
the command line:

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
`ow-tauri.json` (`pendingBrowserArgs`), applied on the next launch to both
environments, and logs a warning (partial, B.2.1). Only `--disable-gpu` and
`--remote-debugging-port=<n>` (n > 0; used in debug builds only) are kept;
other switches are dropped, and duplicates are removed. Each session replaces
the stored set with the set it recorded: `main_ready` carries the set recorded
so far (`pendingBrowserArgs`, A.2.1; absent means none), and every change
sends the session's whole set again with `app_record_browser_args`. An app that stops calling them gets the defaults
back on the following launch. macOS and Linux have no browser arguments; the
set is stored all the same and ignored there with a warning.

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
| `ipc_subscribe` | `{ onMessage: Channel<HostMessage[]>; userAgent?: string }` | `{ epoch: string }` | none | Registers the calling document's host-message channel (A.3, C.1). Called once per document load, before any other command except `bootstrap`. The first call from `ow-main` carries the platform webview's default `navigator.userAgent`, from which Rust composes `<UA>` (E.1). Returns a fresh random `epoch` that tags this document's IPC sequence numbers (C.3). A second call from the same webview replaces the first channel and resets that webview's IPC state. |
| `bootstrap` | none | `HostSnapshot` | none | Returns the current snapshot, the same shape as the injected `window.__OW_TAURI_BOOTSTRAP__`. Used to resynchronise the cache after a state sequence gap (B.1.6). |
| `main_ready` | `{ pendingBrowserArgs?: string[] }` | `void` | none | The app's top-level module code has run and `app.whenReady()` is about to resolve. The main runtime sends it after `DOMContentLoaded` plus one macrotask, by which time the app's scripts, modules included, have run their synchronous top-level code. Seals pre-ready switches, replaces the stored `pendingBrowserArgs` with the filtered argument (A.1.1; absent means none) and starts the launch analytics sequence (E.2). Idempotent: only the first call has an effect. If it never arrives, analytics start after 10 s with a warning. |
| `app_record_browser_args` | `{ args: string[] }` | `void` | none | Replaces the stored `pendingBrowserArgs` with the filtered `args` (A.1.1). The main runtime sends the session's whole recorded set each time `app.commandLine.appendSwitch()`, `removeSwitch()` or `app.disableHardwareAcceleration()` changes it, before or after `main_ready`; the last call (or `main_ready`, whichever is later) wins. Takes effect from the next launch. |
| `ipc_main_ready` | none | `void` | none | `ipcMain` is installed; flushes the startup queue (section C.4). |
| `app_quit_reply` | `{ requestId: number; prevent: boolean }` | `void` | `not-found` (unknown or expired request) | Answer to a `lifecycle` `before-quit` message (A.6). |
| `app_relaunch` | `{ args?: string[], execPath?: null }` | `void` | `unsupported` (non-null `execPath`) | Schedules a relaunch on the next `app_exit` / `app_quit`, like Electron `app.relaunch()`. `args` replaces the original arguments; absent means the original arguments without the executable. The new process is started at `RunEvent::Exit` (A.6). |
| `app_quit` | none | `void` | none | Graceful exit: the A.6 quit sequence (`before-quit`, window `close` events, `will-quit`), analytics drain, then exit. |
| `app_exit` | `{ code?: number }` | `void` | none | Immediate exit with `code` (default 0) after an analytics drain of at most 1.5 s. |
| `app_focus` | `{ steal?: boolean }` | `void` | none | Focuses the most recent visible UI window. `steal` is accepted and ignored (partial, B.2.1). |
| `log` | `{ level: 'debug'\|'info'\|'warn'\|'error', message: string }` | `void` | none | Appends to the ow-tauri log file when `logging.enabled`; dropped otherwise (F.4). |
| `ipc_reply` | `{ id: number; ok: boolean; value?: OtjValue; error?: IpcErrorWire; seq: number }` | `void` | none (unknown ids are ignored) | Answers an `invoke` request (C.2). `seq` is the main runtime's per-target counter shared with `ipc_emit`, so a reply is delivered after every message the handler sent to the same window before returning (C.5). An absent `value` decodes to `undefined`. A reply to a request that already timed out still uses up its `seq`. A reply that cannot be ordered (its `seq` was already used, or is too far ahead of the target's order) settles the invoke at once with `ipc-overloaded`, so it never hangs. Rust does not check the reply's size; the main runtime does (C.5). |
| `ipc_emit` | `{ target: number; channel: string; args: OtjValue[]; seq: number }` | `void` | `ipc-serialization`, `ipc-overloaded` | `webContents.send` / `event.reply` to window id `target` (C.5). A rejected call still uses up its `seq`. |
| `ipc_emit_skip` | `{ target: number; seq: number }` | `void` | none | The main runtime reports that the `ipc_emit` or `ipc_reply` it numbered `seq` for window `target` never reached the plugin (C.5), so the target's outbound order does not wait for it. Numbers already used or skipped are ignored. |

`HostSnapshot`:

```ts
interface HostSnapshot {
  seq: number;                              // last applied state sequence number (C.6)
  versions: { owTauri: string; tauri: string; app: string; webview: string; os: string };
  manifest: EmbeddedManifest;               // section G
  identity: { uid: string; cuid: string; muid: string; muidV2: string; phasePercent: number };
  utmParams?: unknown;                      // ow-electron.json utmParams; absent (undefined in JS) when there are none (F.2)
  switches: { argv: string[]; testAd: boolean };
  paths: Record<ElectronPathName | 'appPath', string>;  // see B.2 app.getPath; appPath backs getAppPath()
  isPackaged: boolean;
  locale: string;
  displays: ElectronDisplay[];              // see B.2 screen
  primaryDisplayId: number;
  packages: PackagesSnapshot;               // A.2.4 (H)
  flags: { anonymousAnalyticsDisabled: boolean; adsOptimizationDisabled: boolean; adsFpdDisabled: boolean };
  platform: string;                         // Node-style: 'win32' | 'darwin' | 'linux'
  arch: string;                             // Node-style: 'x64' | 'arm64' | ...
  cursor?: { x: number; y: number };        // cursor in DIP when the snapshot was taken
  ipcLimits?: { maxMessageBytes: number };  // ipc.maxMessageBytes (A.1), for the runtime's own size checks
}
```

`paths.appPath` is the directory that holds the app's resources (Tauri's
resource directory, or the executable's directory when Tauri cannot name
one); the `fs_*` scope serves the embedded `package.json` under it (A.2.3).
The JS side treats `cursor` and `ipcLimits` as optional: without `cursor` the
first `screen.getCursorScreenPoint()` returns the cached `(0, 0)` and starts a
refresh; without `ipcLimits` the runtime checks against the default 8 MiB.

UI windows get a smaller object as `window.__OW_TAURI_BOOTSTRAP__`, the
subset the `process` shim needs: `{ versions, switches, platform, arch }`.

#### A.2.2 Main webview: Overwolf API (`overwolf:main`)

| Command | Arguments | Returns | Errors | Mirrors |
|---|---|---|---|---|
| `disable_anonymous_analytics` | none | `void` | none | `app.overwolf.disableAnonymousAnalytics()`. Before `main_ready`: the session sends only the mandatory set (E.3). After: applies to later events and logs a warning. Per session; not persisted. Guest `settings.anonymous` stays `false` [OBS]. |
| `disable_ads_optimization` | none | `void` | none | `disableAdsOptimization()`. Sends nothing to guests and leaves running guests' `settings` unchanged [OBS]; sets `__settings__.adsOptimization` to `{ anonymous: true, disable: true }` (B.1.1) [OBS]. Guests mounted afterwards get `settings.disableOptimization: true` [INF; **Unknown (R3-7)**]. Per session. |
| `disable_ads_fpd` | none | `void` | none | `disableAdsFPD()`. Sends nothing to guests; sets `__settings__.adsOptimization` as above [OBS]. Clears stored hashes; later hashes are not sent [DEC; **Unknown (R3-3)**]. Per session. |
| `is_cmp_required` | none | `boolean` | none (never fails) | `isCMPRequired()`, D.6.2: awaits this launch's `cmp-eu-only` request and the startup consent page's load; no timeout [OBS]; always `true` for every response ow-electron was given [OBS]. |
| `open_cmp_window` | `{ options?: CmpWindowOptions }` | `void` | `invalid-argument` (`cmpURL` is not an `https:` URL), `io` | `openCMPWindow(options)`, deprecated upstream; identical to `open_ad_privacy_settings_window` [DOC] [OBS]. |
| `open_ad_privacy_settings_window` | `{ options?: CmpWindowOptions }` | `void` | `invalid-argument`, `io` | `openAdPrivacySettingsWindow(options)`: the consent settings window (D.6.4). Resolves once the window has been created, not when it closes [OBS]. If one is open it is focused and the call resolves at once [OBS]. The first call of a launch also opens the default-consent window (D.6.4) [OBS]. |
| `set_user_email_hashes` | `{ hashes?: EmailHashes \| null }` | `void` | none | `setUserEmailHashes()`. Sends an `eHashes` host message with the hashes to every existing ad guest (D.5) [OBS]; no request, no file [OBS]; not replayed to guests that load later [OBS]. `null`, absent, or all fields empty sends nothing [DEC]. Ignored (with a warning) after `disable_ads_fpd` [DEC]. `ow-tauri/main` also calls it, fire-and-forget, with the result of every `generateUserEmailHashes()` (B.1.1) [OBS]. |
| `set_external_payment_user_id` | `{ options: ExternalPaymentUserIdOptions }` | `void` | `invalid-argument` (no `userId`; `data.message` exactly `providerName and userId are mandatory`), `not-ready` (before `main_ready`, message exactly `ow-electron is not ready yet!`) | `setExternalPaymentUserId()`: sends the Counter `<label>_sub_info` (E.2 #10) and resolves after the HTTP response, or after it fails: reporting never rejects [OBS] [TYPES]. |
| `analytics_set_user_enabled` | `{ enabled: boolean }` | `void` | `unsupported` unless `analytics.userSwitch` | **ow-tauri option**, off by default and stricter than ow-electron: off sends nothing at all, including the mandatory set. Persisted in `ow-tauri.json`. |

`CmpWindowOptions` is the ow-electron type: `{ tab?: 'purposes'|'features'|'vendors'; modal?: boolean; parentId?: number; center?: boolean; backgroundColor?: string; preLoaderSpinnerColor?: string; width?: number; height?: number; x?: number; y?: number; cmpURL?: string; language?: string }`. On the wire `parent` (a `BrowserWindow`) is replaced by its integer `parentId`. Defaults [OBS]: 800 x 800, centred, background `#0D0D0D`, `tab` `purposes`, `language` `en`; ow-electron honoured `x`, `y`, `width`, `height`, `backgroundColor`, `tab` and `language`, and opened the window without a parent and not modal by default (the typings say `modal` defaults to `false`; Overwolf's consent page documentation says `true`). ow-tauri also applies `preLoaderSpinnerColor` to the preloader, and `modal: true` with `parentId` makes the window owned by the parent and keeps it on top of it (true input modality is Windows-only) [DEC].

`EmailHashes` is `{ sha1?: string; sha256?: string; md5?: string }` [TYPES].
ow-electron returns lower-case keys in the order `sha1`, `md5`, `sha256`
[OBS] (Overwolf's user-identity page writes them in upper case).
`ExternalPaymentUserIdOptions` is `{ providerName: string; userId: string; paymentId?: string }`. When `providerName` is absent it defaults to `'tebex'`, appended after the other options [OBS]; an empty string is treated as absent [INF].

Email hash generation is a pure function implemented identically in Rust
(`identity::email_hashes`) and in `ow-tauri/main` (synchronous, see B.1.1),
checked against the shared test vectors in
`crates/tauri-plugin-overwolf/tests/fixtures/email-hashes.json`. Normalisation
trims and lower-cases the input [OBS] (`'  Test.Email@Overwolf.COM  '` hashes
like `test.email@overwolf.com`); an input that is not an email address is
hashed all the same [OBS]. The UID2 rule the typings link to also removes `.`
and any `+suffix` from `gmail.com` local parts [DOC]; ow-tauri applies it
[DEC; **Unknown (R3-6)** whether ow-electron does]. The output is lower-case
hex [OBS] [DOC]: for `test.email@overwolf.com` ow-electron and Overwolf's
user-identity page give:

| Hash | Value |
|---|---|
| md5 | `170d78feecf2b8e7b804ba6b45af7ac2` |
| sha1 | `2c44f8a418bbfa88e80e3ce17d56cb30944f7675` |
| sha256 | `ac43b559f15c2eb262ea8d5d4921f639aaf1cde84bc280bad2e1879d0ded68c2` |

`emailHashes.encoding: "base64"` is an ow-tauri option kept for apps that
relied on it; it is not parity.

#### A.2.3 Main webview: windows, screen, shell, dialogs, files (`overwolf:main`)

These back the `ow-tauri/electron` facade. Operations that `@tauri-apps/api`
already exposes with equivalent semantics (show, hide, focus, size, position,
minimize, maximize, title, always-on-top, ignore cursor events, ...) are called
directly by the facade through `core:window:*` / `core:webview:*` permissions
and are not repeated here.

The opener, dialog and global-shortcut plugins are dependencies of
`tauri-plugin-overwolf`; the plugin registers them itself unless the app has
already registered them, and calls them from Rust. No webview, including
`ow-main`, is granted their permissions. The registration
(`AppHandle::plugin`) runs in a task the setup hook posts to the event loop,
not in the setup hook itself: Tauri holds its plugin-store lock during plugin
setup and during `on_event`, so registering from there deadlocks.

| Command | Arguments | Returns | Errors | Behaviour |
|---|---|---|---|---|
| `window_create` | `WindowCreateRequest` | `{ id: number; label: string }` | `invalid-argument`, `io`, `not-found` (unknown `parentId`), `unsupported` (`windowClass: 'overlay'`, which needs a package runtime) | Creates the window behind `new BrowserWindow(options)` (B.2): native window `bw-<id>` with one webview `bw-<id>`. Injects the renderer bootstrap and the preload script as initialization scripts, both wrapped in an app-origin guard (A.2.3.1). Installs the navigation policy and the `window.open` handler (A.2.3.1). Registers the window class. `id` is the plugin's window id; the main runtime maps it to the app-visible `BrowserWindow.id` (B.2.2). |
| `window_load` | `{ id: number; target: LoadTarget }` | `void` | `not-found`, `io` | `loadURL` / `loadFile`. A target that is an app asset loads in the existing webview. An `http(s)` URL that is not an app asset switches the window to class `remote` for good: the `bw-<id>` webview is closed and a fresh child webview `bwr-<id>` filling the window is created for the URL, with no initialization scripts and no capability (A.2.3.1). The window keeps its id, bounds and native handle. |
| `window_close_reply` | `{ id: number; requestId: number; prevent: boolean }` | `void` | `not-found` | Answer to a `close` window event (A.3); `prevent: false` lets the close proceed. |
| `window_destroy` | `{ id: number }` | `void` | `not-found` | `destroy()`: closes without a `close` event. |
| `window_eval` | `{ id: number; code: string; wantResult: boolean }` | `unknown` | `not-found`, `ipc-remote-error`, `ipc-timeout` | `webContents.executeJavaScript(code)`. The code always runs through the platform's native script evaluation (`Webview::eval`), never through page-level `eval`, so the page's CSP needs no `unsafe-eval`. For `ui` / `overlay` windows with `wantResult`, Rust evaluates `__OW_TAURI_RUNTIME__.evalBegin(<n>, () => (<code>\n))` and then `__OW_TAURI_RUNTIME__.evalFallback(<n>, () => { <code>\n})`; the second runs only when the first did not parse (statement code), and resolves `undefined`. The runtime reports through `eval_result` (A.2.5), 30 s timeout. For `remote` windows the code runs and the call resolves `undefined` (partial, B.2). |
| `window_devtools` | `{ id: number; open: boolean }` | `void` | `unsupported` (release build without the `devtools` feature) | `webContents.openDevTools()` / `closeDevTools()`. |
| `window_set_name` | `{ id: number; name: string }` | `void` | `not-found` | ow-electron `BrowserWindow` `name` option; normalised as E.2 describes and used for analytics (`<label>_window_closed`, guest `windowName`). |
| `window_show_inactive` | `{ id: number }` | `void` | `not-found` | `BrowserWindow.showInactive()`: macOS `-[NSWindow orderFrontRegardless]` (on screen, not key, the app is not activated), as ow-electron shows an inactive window; Windows `ShowWindow(SW_SHOWNOACTIVATE)` (shown, not activated, as Electron), after which the window counts as shown for a later `hide()`; Linux a plain show. `plugin:window|show` would activate the app. |
| `screen_snapshot` | none | `{ displays: ElectronDisplay[]; primaryDisplayId: number; cursor: { x: number; y: number } }` | none | Fresh screen state; the cache is also pushed (C.6). |
| `shell_open_external` | `{ url: string }` | `void` | `invalid-argument` (not an absolute URL by the WHATWG parser, scheme not `http`, `https` or `mailto`, or credentials in the URL), `io` | via `tauri-plugin-opener`. |
| `shell_open_path` | `{ path: string }` | `string` (empty on success, Electron semantics) | none | Checked by the plugin before opener runs (A.2.3.2); a refused or failed open returns the error string, never throws. |
| `shell_show_item_in_folder` | `{ path: string }` | `void` | `io` | via opener `reveal_item_in_dir`; the path is canonicalised first. Revealing never executes anything, so no scope applies. |
| `dialog_open` | `OpenDialogOptions` (Electron shape, `windowId` instead of `BrowserWindow`) | `{ canceled: boolean; filePaths: string[] }` | `io` | via `tauri-plugin-dialog`. |
| `dialog_save` | `SaveDialogOptions` | `{ canceled: boolean; filePath: string }` | `io` | via `tauri-plugin-dialog`. |
| `dialog_message` | `MessageBoxOptions` | `{ response: number; checkboxChecked: boolean }` | `io` | Up to 3 buttons (partial, B.2). |
| `global_shortcut_register` | `{ accelerator: string; id: number }` | `boolean` | none | Electron accelerator syntax; presses arrive as `global-shortcut` host messages (A.3). |
| `global_shortcut_unregister` | `{ accelerator?: string }` | `void` | none | Absent `accelerator` unregisters all. An accelerator matches a registration that names the same keys, whatever the spelling (`Ctrl+K` unregisters a `CommandOrControl+K` registration on Windows and Linux). |
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
  windowClass: 'ui' | 'overlay';      // 'overlay' is reserved for a package runtime (Appendix P) and is `unsupported` today
  overlayOptions?: OverlayOptions;    // only with windowClass 'overlay'
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

- **Origin guard.** The renderer bootstrap, every preload and the `ow-main`
  bootstrap are wrapped in `if (location.origin === <app origin>) { ... }`,
  where the app origin is the platform's asset origin (`tauri://localhost`,
  `http://tauri.localhost`, or the dev server origin in debug builds). They do
  nothing in any other document. The UI capability is also `local: true`, so
  a remote document in a `bw-*` webview cannot call a command.
- **Navigation policy.** Every `bw-*` webview gets an `on_navigation`
  handler. Navigations to the app origin and to in-page documents
  (`about:`, `blob:`, `data:`) are allowed everywhere; any scheme other than
  those and `http(s)` is cancelled and logged. For `http(s)` targets outside
  the app origin the platforms differ, because only WebView2 reports
  top-level navigations alone to the hook; WKWebView and WebKitGTK also
  report every frame's navigations without saying which frame navigates:

  | Platform | `http(s)` navigation outside the app origin |
  |---|---|
  | Windows | cancelled; the URL is opened in the system browser (validated as `shell_open_external` does) and `ow-main` gets a `will-navigate { url }` window message |
  | macOS, Linux | allowed by the hook, so embedded frames (video or stream widgets) work. To keep top-level behaviour the same as on Windows, the renderer bootstrap intercepts primary clicks on links (`a[href]` without a `target` other than `_self` / `_top`) and form submissions in the top frame whose target is an `http(s)` URL outside the app origin: it cancels them and calls `navigation_external { url }` (A.2.5), which opens the URL in the system browser and sends the same `will-navigate`. A page script that assigns `location` directly is not intercepted: the remote page then loads inside the `bw-*` webview, without IPC (origin guard, local-only capability), until the app reloads the window |

  `window_load` with a remote URL does not navigate; it uses the recreate
  path above.
- **New windows.** `window.open()` and `target="_blank"` requests from any
  webview of a `bw-<id>` window are always denied natively and reported to
  `ow-main` as a `new-window { url }` window message, where the main runtime
  runs the app's `setWindowOpenHandler` (B.2.2).
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
   executable or a launcher (extensions compared case-insensitively):
   - Windows: any extension in `PATHEXT` (default
     `.COM;.EXE;.BAT;.CMD;.VBS;.VBE;.JS;.JSE;.WSF;.WSH;.MSC` when unset), plus
     shortcuts, installers, scripts and file types whose shell handler runs or
     installs code: `.lnk`, `.url`, `.website`, `.scf`, `.ps1`, `.exe`,
     `.scr`, `.pif`, `.msi`, `.msp`, `.msix`, `.msixbundle`, `.appx`,
     `.appxbundle`, `.appinstaller`, `.appref-ms`, `.application`, `.reg`,
     `.hta`, `.cpl`, `.inf`, `.jar`, `.chm`, `.diagcab`,
     `.settingcontent-ms`, `.library-ms`, `.search-ms`, `.xll`, and the
     auto-mounting disk images `.iso`, `.img`, `.vhd`, `.vhdx`;
   - macOS: `.app`, `.command`, `.tool`, `.terminal`, `.workflow`, `.pkg`,
     `.mpkg`, `.prefpane`, `.saver`, `.scpt`, `.scptd`, `.applescript`,
     `.jar`, `.dmg`, the Finder location files `.fileloc`, `.inetloc`,
     `.webloc`, and any file with an execute bit;
   - Linux: `.desktop`, `.AppImage`, `.jar`, and any file with an execute
     bit.

   Directories are allowed, except macOS bundles that launch something
   (`.app`, `.prefpane`, `.saver`, `.workflow`, `.pkg`, `.mpkg`, `.scptd`).
   A refused path returns `"opening executables is disabled"`.

#### A.2.4 Main webview: packages (`overwolf:main`)

While no package runtime exists (section H), these commands return exactly
what ow-electron returns for packages it cannot load [OBS]. The commands of
the deferred design are listed in Appendix P.7.

| Command | Arguments | Returns | Errors | Mirrors |
|---|---|---|---|---|
| `packages_snapshot` | none | `PackagesSnapshot` | none | refresh of the cached state |
| `packages_relaunch` | none | `void` | none | `packages.relaunch()`: no effect while no package is loaded [DEC] |
| `packages_set_channel` | `{ name: string; channel?: string \| null }` | never resolves successfully | `not-found` with `data.message` `setChannel - package '<name>' is not registered in this app` [OBS] | `packages.setChannel(name, channel, ready?)` |
| `packages_get_available_channels` | `{ names: string[] }` | never resolves successfully | `not-found` with `data.message` `getAvailableChannels - package '<first name>' is not registered in this app` [OBS], also for a listed name | `getAvailableChannels(...names)`. With no names: **Unknown (R3-9)**; interim resolves `{}` [DEC] |
| `packages_get_channel` | `{ names: string[] }` | `{}` | none | `getChannel(...names)` returns `{}` for any arguments [OBS] |

```ts
interface PackagesSnapshot {
  backend: 'none' | 'native';                        // 'native' is reserved (Appendix P)
  logsFolderPath: string;                            // literal ow-electron string, F.4
  phasePercent: number;                              // E.4
  listed: string[];                                  // manifest overwolf.packages
  pendingUpdates: { hasPendingUpdate: false; details: [] };
}
```

`ow-tauri/main` turns the two `not-found` errors into a rejected promise with
a plain `Error` whose `message` is `data.message` (B.1.3), so app code sees
exactly ow-electron's error text.

#### A.2.5 UI windows (`overwolf:renderer`)

The caller must be the app webview of a window that `window_create` made
(class `ui`, or `overlay` once a package runtime exists); any other webview
gets `forbidden`, whatever its capability says.

| Command | Arguments | Returns | Errors | Behaviour |
|---|---|---|---|---|
| `ipc_subscribe` | `{ onMessage: Channel<HostMessage[]> }` | `{ epoch: string }` | none | As in A.2.1; also flushes messages buffered for this window (C.5). Once per document load. |
| `ipc_invoke` | `{ channel: string; args: OtjValue[]; epoch: string; seq: number }` | `{ id: number }` | `ipc-serialization`, `ipc-overloaded`, `not-ready` (stale `epoch`) | section C.2. Resolves as soon as the request is accepted; the result arrives later as an `ipc-result` host message. |
| `ipc_send` | `{ channel: string; args: OtjValue[]; epoch: string; seq: number }` | `void` | `ipc-serialization`, `ipc-overloaded`, `not-ready` (stale `epoch`) | section C.3 |
| `ipc_skip` | `{ epoch: string; seq: number }` | `void` | none | The runtime reports that the call carrying `seq` was rejected, by Tauri before the command ran or by the plugin (C.3), so the reorder buffer does not wait for it. Numbers already used or skipped, and stale epochs, are ignored. |
| `eval_result` | `{ id: number; ok: boolean; value?: OtjValue; error?: IpcErrorWire }` | `void` | `not-found` | Result of a `window_eval` with `wantResult` targeted at the calling window. |
| `navigation_external` | `{ url: string }` | `void` | `invalid-argument` (the `shell_open_external` checks, or a URL on the app origin) | macOS and Linux: a top-level navigation the renderer bootstrap cancelled (A.2.3.1). Rust opens the URL in the system browser and sends `will-navigate { url }` for the calling window to `ow-main`, as the Windows navigation hook does. |
| `navigation_in_page` | `{ url: string }` | `void` | `invalid-argument` (not a valid URL, no loaded document in the window, or a different origin) | The renderer bootstrap reports a top-document URL change without a new load: `hashchange`, `popstate`, and the patched `history.pushState` / `replaceState`. The URL becomes the window's URL (`webContents.getURL()`, and the next `did-finish-load` while the document is still loading) and `ow-main` gets the window event `did-navigate-in-page` with `{ url, isMainFrame: true }`; the facade emits `did-navigate-in-page(event, url, isMainFrame)` on `webContents` (B.2.2), as Electron does. Reporting the current URL again does nothing. |
| `adview_mount` | `AdviewMount` | `{ guestLabel: string }` | `invalid-argument`, `io` | Creates the guest for one `<owadview>` (B.3, D). Never waits for consent; the guest's first navigation is sequenced by D.6.5. Counts InsertStats Kind 400025 (E.2). |
| `adview_update` | `{ elementId: string; rect?: AdviewRect; visible?: boolean; attributes?: Partial<AdviewAttributes> }` | `void` | `not-found` | Moves, resizes, shows or hides the guest; attribute changes per B.3.4. |
| `adview_unmount` | `{ elementId: string }` | `void` | none (idempotent) | Closes the guest. |
| `adview_command` | `{ elementId: string; command: 'setAudioMuted' \| 'reload' \| 'setPageUrl' \| 'sendCommand'; args: unknown[] }` | `void` | `not-found` | Element methods (B.3.3). |

```ts
interface AdviewMount {
  elementId: string;            // runtime-assigned, unique per embedder webview ("e1", "e2", ...)
  attributes: AdviewAttributes;
  rect: AdviewRect;
  visible: boolean;
  documentTitle?: string;       // the embedder's document.title at mount (D.2 windowTitle), cut to 1024
}
interface AdviewAttributes {
  cid: string;                  // trimmed, at most 20 characters
  slotsize: string;             // "WxH"
  adstyle: string;              // "" or e.g. "high-impact-ad;"
  customTracking: unknown;      // parsed JSON object, or null
  performance: boolean;
  unit: string | null;
  pageurl: string;              // "" when absent (D.2 pageUrl)
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

#### A.2.7 Consent windows (`overwolf:cmp-window`)

| Command | Arguments | Returns | Behaviour |
|---|---|---|---|
| `cmp_event` | `{ name: 'ready' \| 'saveConsent' \| 'saveUnifiedConsent' \| 'enableAdOptimization' \| 'close'; data?: { consent?: string; enabled?: boolean } }` | `void` | Consent page to host, from the startup consent window (`ow-cmp-startup`), the default-consent window (`ow-cmp-default`) or the settings window (`ow-cmp`), D.6. Consent strings must be empty or printable ASCII (0x21 to 0x7E) and at most 16 KiB; an empty string clears the stored value (`saveConsent("")` stores `timeStamp: 0`), as the clearing startup page does (D.6.2) [OBS]. Calls from a page outside `https://content.overwolf.com/monsdk/electron/` are refused (D.6.4). |

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
| `window` | `ow-main` | `{ id: number, event: WindowEventName, requestId?: number, data?: unknown }` | window lifecycle (B.2); `close` carries `requestId` and waits for `window_close_reply` (5 s, then closes). `id` is the plugin's window id. `data` per event below |
| `lifecycle` | `ow-main` | `{ event: 'before-quit' \| 'will-quit', requestId: number }` or `{ event: 'quit', exitCode: number }` | the quit sequence (A.6) |
| `lifecycle` | `ow-main` | `{ event: 'second-instance', argv: string[], cwd: string, additionalData?: unknown }` | `Overwolf::emit_second_instance` (A.5); `additionalData` is absent today (the single-instance plugin carries none) |
| `lifecycle` | `ow-main` | `{ event: 'activate', hasVisibleWindows: boolean }` | macOS: the dock icon was clicked (`RunEvent::Reopen`) |
| `packages` | `ow-main` | `{ event: 'loading' \| 'ready' \| 'failed-to-initialize' \| 'crashed' \| 'package-update-pending' \| 'updated', ... }` | reserved: never sent while no package runtime exists (H). The package messages of the deferred design are in Appendix P.7 |
| `adview-event` | the embedder webview | `{ elementId: string, name: string, data?: unknown, source: 'guest' \| 'host' }` | B.3.5 |
| `updater` | `ow-main` | `{ event: 'checking-for-update' \| 'update-available' \| 'update-not-available' \| 'download-progress' \| 'update-downloaded' \| 'error', info?, progress?, error? }` | I.3 |
| `global-shortcut` | `ow-main` | `{ id: number, accelerator: string, state: 'pressed' \| 'released' }` | registered accelerator |

`type` is the message tag, so the `packages` and `updater` messages carry
their own kind in `event`.

Messages for a webview that has not subscribed yet are buffered (C.4, C.5).
When a webview is gone, Rust drops its channel, its buffers, its reorder
state, its pending invokes (rejected with `not-ready`) and every ad guest it
embeds. Tauri has no event for a webview destroyed on its own, so this
cleanup runs from the plugin's own close paths (a window switching to a
remote page, a soft restart, a guest unmount) and from the window's
`WindowEvent::Destroyed`.

`IpcSender` is `{ windowId: number, label: string, url: string, frameId: 0 }`.
`WindowEventName` is one of `created`, `close`, `closed`, `focus`, `blur`,
`show`, `hide`, `minimize`, `maximize`, `unmaximize`, `restore`, `resize`,
`move`, `enter-full-screen`, `leave-full-screen`, `ready-to-show`,
`did-finish-load`, `dom-ready`, `did-fail-load`, `render-process-gone`,
`will-navigate`, `did-navigate-in-page`, `new-window`.

| Event | `data` |
|---|---|
| `resize`, `move` | `{ bounds: { x, y, width, height } }`, outer bounds in logical pixels |
| `created` | `{ options }` (the window's constructor options). Reserved for windows the JS side did not create itself (overlay windows made by a package runtime, Appendix P.1); not sent today |
| `did-finish-load` | `{ url }` |
| `did-fail-load` | `{ errorCode, errorDescription, validatedURL }` |
| `render-process-gone` | `{ exitCode }` |
| `will-navigate` | `{ url }`: a top-level navigation the A.2.3.1 policy cancelled |
| `did-navigate-in-page` | `{ url, isMainFrame: true }`: a same-document URL change of the top document (`navigation_in_page`, A.2.5) |
| `new-window` | `{ url, frameName?, features?, disposition? }`: a `window.open` / `target="_blank"` request, always denied natively (A.2.3.1). Rust sends `url` only today; the main runtime uses `''`, `''` and `'new-window'` for the others |
| others | none |

Tauri has no minimize event: Rust derives `minimize` and `restore` from
`WindowEvent::Resized` plus `is_minimized()`, with a 2 s poll of
`is_minimized()` as a fallback for platforms that do not report a resize on
minimize. Tauri has no visibility event either, so Rust sends no `show` or
`hide`; the `BrowserWindow` facade emits them itself (B.2.2). `ready-to-show`
follows the first `did-finish-load` of a window.

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
use tauri_plugin_overwolf::{Builder, OverwolfExt};

fn main() {
    tauri::Builder::default()
        .plugin(
            Builder::new()
                .manifest_json(tauri_plugin_overwolf::embedded_manifest!())
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
| `Builder::packages_backend` | `(self, PackagesBackend) -> Self` | `None` (default) or `Native` (H.2); runtime registration is part of Appendix P.7 |
| `Builder::uid` | `(self, impl Into<String>) -> Self` | uid override (G.2 rule 1) |
| `Builder::host_label` | `(self, impl Into<String>, Option<String>) -> Self` | `analytics.hostLabel` and `analytics.hostVersion` (section 0) |
| `Builder::test_ad` | `(self, bool) -> Self` | |
| `Builder::analytics_transport` | `(self, Arc<dyn Transport>) -> Self` | tests: capture requests instead of sending |
| `Builder::build` | `<R: Runtime>(self) -> TauriPlugin<R, Option<Config>>` | `Option`, because Tauri passes a missing `plugins.overwolf` as `null` (A.1) |
| `OverwolfExt::overwolf` | `(&self) -> &Overwolf<R>` | on `App`, `AppHandle`, `Window`, `Webview`, `WebviewWindow` |
| `Overwolf::uid`, `cuid`, `muid`, `muid_v2`, `phase_percent`, `utm_params` | getters | |
| `Overwolf::disable_anonymous_analytics`, `disable_ads_optimization`, `disable_ads_fpd` | `(&self)` | same semantics as A.2.2 |
| `Overwolf::is_cmp_required` | `async (&self) -> bool` | |
| `Overwolf::open_cmp_window`, `open_ad_privacy_settings_window` | `async (&self, CmpWindowOptions) -> Result<()>` | |
| `Overwolf::generate_user_email_hashes` | `(&self, &str) -> EmailHashes` | |
| `Overwolf::set_user_email_hashes` | `(&self, Option<EmailHashes>)` | |
| `Overwolf::packages` | `(&self) -> &Packages<R>` | `snapshot`, `set_channel`, `get_available_channels`, `get_channel`, `relaunch`, with the results of H.1 |
| `Overwolf::updater` | `(&self) -> &Updater<R>` | `configure`, `check`, `download`, `quit_and_install` |
| `Overwolf::emit_second_instance` | `(&self, argv: Vec<String>, cwd: String)` | call from the app's `tauri-plugin-single-instance` callback; fires `app.on('second-instance')` in `ow-main` (B.2.1) |
| `Overwolf::report_web_content_terminated` | `(&self, label: &str)` | macOS: call from `tauri::Builder::on_web_content_process_terminate` for every webview; Tauri offers that hook only on the app's builder. `ow-main` restarts (A.6), an ad guest recovers (D.7), a consent window takes its failure path (D.6.1), a `bw-*` or `bwr-*` webview emits `render-process-gone` (A.3); other labels are ignored |
| `Overwolf::report_main_webview_crash` | `(&self)` | the `ow-main` case of `report_web_content_terminated` alone (A.6) |
| `Flags`, `LogLevel` | types at the crate root | the session switches (`HostSnapshot.flags`) and the `log` levels |
| `build::embed_manifest` | `(path: impl AsRef<Path>) -> Result<(), BuildError>` | in the app's `build.rs` (G.3) |

Internal modules are hidden from the rustdoc output and are not part of the
stable API. The `test-util` feature adds hooks for tests on Tauri's mock
runtime: `Builder::skip_os_queries` and hidden `Overwolf::test_*` methods
that drive the window, navigation, page-load and exit-request handlers the
mock runtime never fires. They are not a stable API either.

`tauri-plugin-single-instance` must be the first plugin an app registers, so
the app registers it, not ow-tauri. The order also matters for
`app.relaunch()`: Tauri delivers `RunEvent::Exit` to plugins in registration
order, so the single-instance lock is released before ow-tauri starts the new
process (A.6). On macOS the app also forwards web-content process
terminations, which Tauri reports only on the app's builder:

```rust
let builder = tauri::Builder::default()
    .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
        app.overwolf().emit_second_instance(argv, cwd);
    }))
    .plugin(tauri_plugin_overwolf::Builder::new() /* ... */ .build());
#[cfg(target_os = "macos")]
let builder = builder.on_web_content_process_terminate(|webview| {
    webview
        .overwolf()
        .report_web_content_terminated(webview.label());
});
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
| macOS 12 and 13, Linux | `ow-main` is a *technically visible* window: 1 x 1 logical pixel, fully transparent, ignores the cursor, skips the taskbar and window switcher, never focused, placed at the origin of the primary display (Wayland ignores positions; the window is still 1 x 1 and transparent). On macOS a transparent window needs Tauri's private-API switch (`app.macOSPrivateApi: true` and the `macos-private-api` feature); without it the 1 x 1 window is opaque (a known gap; timers still run) |

The requirement is measurable: over a 10-minute run with every app window
hidden or minimized, a 1 s `setInterval` in `ow-main` fires with a median
period under 1.5 s. The scheduled CI soak job checks it on all three
platforms (`tests/liveness_soak.rs`, test ads only).

**Navigation and reloads.** After its first load, `ow-main` may navigate only
in debug builds, and only a navigation to its current document URL counts as
a reload. A reload in a debug build (manual or a dev-server hot reload) is a
*soft restart*: Rust closes every `bw-*`, `bwr-*`, guest and consent window,
rejects pending invokes with `not-ready`, clears the IPC, package-callback
and handle registries, and lets the reloaded page run the app's main code
again from a fresh snapshot. Package runtimes keep running; their state is
replayed through the snapshot. In release builds every top-level
navigation of `ow-main` is cancelled. Where the engine reports frame navigations to the
same hook (macOS, Linux; A.2.3.1), same-origin and in-page (`about:`,
`blob:`, `data:`) frames inside `ow-main` are allowed; every other target is
cancelled and logged.

**Crashes.** When the `ow-main` render process dies, Rust logs it, drains
analytics for at most 1.5 s and relaunches the app with its original
arguments. If `ow-main` crashes `main.crashRestartLimit` times within 60 s,
the app exits with code 1 instead and the log names the cause. The crash
times are kept in `ow-tauri.json` (`mainCrashes`, F.3), so the limit holds
across the relaunches. A second report of the same crash is ignored. There
is no in-place rehydration: the JS-side registries (windows, `ipcMain`
handlers, hotkeys, package listeners) cannot be rebuilt without the app's
own code.

| Platform | Crash signal |
|---|---|
| Windows | WebView2 `ProcessFailed` on the `ow-main` webview |
| macOS | WKWebView web-content termination; Tauri exposes it only on the app's builder, so the app forwards it to `Overwolf::report_web_content_terminated` (A.5) |
| Linux | WebKitGTK `web-process-terminated` |
| all | the `ow-main` window's `WindowEvent::Destroyed` while the app is not exiting |

**Relaunch.** A relaunch, from `app.relaunch()` or a crash, starts the new
process at `RunEvent::Exit`, after the plugins registered before ow-tauri
have handled that event. `tauri-plugin-single-instance` is registered first
(A.5), so its lock is already released and the new process is not turned
away as a second instance.

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

Two cases differ:

- With no main webview (it is disabled or gone), an exit request skips
  steps 1 to 4 and goes straight to step 5 with code 0: nobody could answer
  `before-quit`.
- During a soft restart (debug builds), the request is held and the quit
  sequence runs once the new `ow-main` document has loaded.

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
`globalThis.__OW_TAURI_RUNTIME__` before any page script runs:

```ts
interface RuntimeGlobal {              // frozen; the global is non-writable, non-configurable
  readonly version: string;            // package version of the injected runtime
  readonly contract: number;           // this document's contract version
  readonly api: number;                // facade API version (FacadeKernel, below)
  readonly context: 'main' | 'ui' | 'none';
  evalBegin(id: number, fn: () => unknown): void;     // window_eval, expression form (A.2.3)
  evalFallback(id: number, fn: () => unknown): void;  // window_eval, statement form
}
```

The npm entry points that an app bundles (`ow-tauri/main`, `/electron`,
`/renderer`, `/testing`) are thin facades. On first use they attach to the
runtime's kernel through a documented interface, `FacadeKernel`
(`packages/ow-tauri/src/bootstrap/facade-kernel.ts`), and are typed against
that interface only. The bootstrap ships with the crate and the facades with
npm, so the interface is versioned separately from the contract: adding an
optional member keeps `api`; removing or changing a member, or its
behaviour, increments it. A facade attaches only when **both** `contract`
and `api` equal its own; otherwise every member throws
`OwTauriError('not-ready')` with "ow-tauri runtime <version> (contract <c>,
api <a>) does not match package <version> (contract <c>, api <a>)". When no
runtime is installed at all (a page outside Tauri, a unit test, a dev page
opened in a browser), the facade installs one itself.

So there is exactly one `ipcRenderer`, one IPC sequence counter, one listener
registry and one `<owadview>` observer per webview, however many bundles
import the package. Errors are branded with
`Symbol.for('ow-tauri.error.brands')`, so `instanceof OwTauriError` holds
across copies.

**Context check.** Each entry point decides where it runs with an injectable
host detector (`__OW_TAURI_RUNTIME__.context`: `'main' | 'ui' | 'none'`),
derived from the webview label: `ow-main` is `main`, `bw-*` is `ui`,
anything else (or no Tauri) is `none`.
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
| `whenHostReady()` | `() => Promise<void>` | resolves after `ipc_main_ready` and `main_ready` were sent and settled; it also resolves when an acknowledgement fails, and the failure is logged, so app start-up never hangs on it |
| `RecorderError` | class | runtime class for `instanceof RecorderError` checks (B.1.5); never raised while no package runtime exists |
| `UpdateCheckResult`, `UpdateInfo`, `ProgressInfo`, `UpdaterConfig` | types | for `autoUpdater` code (I.5) |

#### B.1.1 `app.overwolf` (OverwolfApi)

Every member of the ow-electron 42.11.4 typings, with the same signature.

| Member | Signature | Status | Implementation |
|---|---|---|---|
| `disableAnonymousAnalytics` | `(): void` | supported | fire-and-forget `disable_anonymous_analytics`; recorded synchronously in the cache so a later `main_ready` carries it |
| `disableAdsOptimization` | `(): void` | supported | `disable_ads_optimization` |
| `disableAdsFPD` | `(): void` | supported | `disable_ads_fpd` |
| `isCMPRequired` | `(): Promise<boolean>` | supported | `is_cmp_required`; never rejects; resolves `true` on any failure |
| `openCMPWindow` | `(options?: CMPWindowOptions): Promise<void>` | supported | `open_cmp_window` (deprecated upstream) |
| `openAdPrivacySettingsWindow` | `(options?: CMPWindowOptions): Promise<void>` | supported | `open_ad_privacy_settings_window` |
| `packages` | `overwolf.packages.OverwolfPackageManager` | supported | B.1.3 |
| `generateUserEmailHashes` | `(email: string): EmailHashes` (**sync**) | supported | computed in JS (pure TypeScript md5, sha1, sha256), keys in the order `sha1`, `md5`, `sha256` [OBS]; then a fire-and-forget `set_user_email_hashes` with the result, because ow-electron sends guests an `eHashes` message after this call too [OBS]. Empty or whitespace email returns `{}` and sends nothing [DEC] |
| `setUserEmailHashes` | `(emailHashes?: EmailHashes): void` | supported | `set_user_email_hashes` |
| `setExternalPaymentUserId` | `(options: ExternalPaymentUserIdOptions): Promise<void>` | supported | `set_external_payment_user_id`; a missing `userId` gives a rejected promise (never a synchronous throw) with `Error('providerName and userId are mandatory')` [OBS] |
| `phasePercent` | `readonly number` | supported | cache `identity.phasePercent` (E.4) |
| `utmParams` | `readonly any` | supported | cache `utmParams`; `undefined` (not `null`) when `ow-electron.json` has none [OBS] (F.2) |
| `muid` | `readonly string` | supported | cache `identity.muidV2`, else `identity.muid` (E.4): ow-electron's getter answers the `MUIDV2` id [OBS: Windows lab]; equal on macOS |
| `uid` | `readonly string` | supported | cache `identity.uid` (G.2) |
| `__settings__` | `readonly object` (not in the typings) | supported | a deep-frozen constant, copied from ow-electron [OBS]: `src` = `https://www.overwolf.com/monsdk/electron/latest/adview.html`, `forceSandboxMode` = `false`, `adsSetting` = `{ gvlUrlV1: 'https://content.overwolf.com/cmp', gvlUrl: 'https://content.overwolf.com/cmp/v3', cmpFeatureUrl: 'https://features.overwolf.com/experiments/cmp-eu-only', cmpUrl: 'https://content.overwolf.com/monsdk/electron/latest/cmp/22.3.27/ow-cmp-v2.html', cmpSettingUrl: 'https://content.overwolf.com/monsdk/electron/latest/cmp/22.3.27/cmp.html' }`, `logger` = `{ enabled: false }`, `adsOptimization` = `{ anonymous: true }`, which becomes `{ anonymous: true, disable: true }` after `disableAdsOptimization()` or `disableAdsFPD()` [OBS]. Frozen except for that one change |

`process.env.OVERWOLF_APP_UID` equals the uid when the main module loads in
ow-electron [OBS]. ow-tauri sets it on the `process.env` shim before any app
script runs (B.2.5), and the Rust process sets the real environment variable
at setup.

#### B.1.2 Event emitters and the synthetic `Event`

`overwolf.packages` is a Node-style event emitter: `on`, `once`, `off`,
`addListener`, `prependListener`, `prependOnceListener`, `removeListener`,
`removeAllListeners`, `emit`, `listeners`, `rawListeners`, `listenerCount`,
`eventNames`, `setMaxListeners`, `getMaxListeners`, with Node's ordering and
`'error'` semantics for app-initiated `emit` (an `'error'` emit with no
listener throws; `errorMonitor` listeners run first). Listeners run
synchronously in registration order when the host message arrives.

Where ow-electron passes an Electron `Event` as the first listener argument
(package manager events), ow-tauri passes:

```ts
interface SyntheticEvent {
  preventDefault(): void;
  readonly defaultPrevented: boolean;
}
```

While no package runtime exists, the manager emits no events at all (H). The
emitter still accepts listeners, so code written for ow-electron (the sample
subscribes at startup) runs unchanged. Package objects and their emitters are
part of the deferred design (Appendix P.6).

#### B.1.3 `overwolf.packages` (OverwolfPackageManager)

Behaviour while no package runtime exists, on every OS. It is what
ow-electron 42.11.4 does on a host where packages are not available (macOS,
`overwolf.packages: ["gep", "overlay"]`) [OBS]:

| Member | Signature | Behaviour |
|---|---|---|
| `on('loading' \| 'ready' \| 'failed-to-initialize' \| 'crashed' \| 'package-update-pending' \| 'updated')` | upstream signatures; `failed-to-initialize` is `(event, packageName)` [TYPES] | accepted; **never emitted** [OBS] |
| `relaunch` | `(): void` | no effect; returns `undefined` [OBS] |
| `hasPendingUpdates` | `(): PendingUpdatesResult` (**sync**) | `{ hasPendingUpdate: false, details: [] }` [OBS] |
| `setChannel` | `(name, channel?, ready?): Promise<SetChannelResult>` | rejects asynchronously with `Error("setChannel - package '<name>' is not registered in this app")` [OBS] |
| `getAvailableChannels` | `(...names): Promise<AvailableChannelsResult>` | rejects asynchronously (a rejected promise, never a synchronous throw) with `Error("getAvailableChannels - package '<name>' is not registered in this app")` and the first name, also for a listed name [OBS]. No names: resolves `{}` [DEC; **Unknown (R3-9)**] |
| `getChannel` | `(...names): Promise<CurrentChannelsResult>` | resolves `{}` [OBS] |
| `logsFolderPath` | `readonly string` | the literal ow-electron string (F.4) [OBS] |
| `phasePercent` | `readonly number` | E.4 |
| `gep`, `overlay`, `recorder`, `utility`, `crn` | package objects | `undefined` [OBS] |

The rejections are plain `Error`s with exactly these messages, not
`OwTauriError`s, so code that matches ow-electron's text keeps working.

#### B.1.4 Package objects

Not defined while no package runtime exists (B.1.3). Their member-by-member
mapping onto a future runtime is part of the deferred design
(Appendix P.6).

#### B.1.5 `RecorderError`

`ow-tauri/main` exports a `RecorderError` class (`name` `'RecorderError'`,
`message`, `code`, `codeStr`, optional `internalError`), so app code that
imports it for `instanceof` checks compiles and runs. Nothing raises it while
no package runtime exists; Appendix P.6 describes how a runtime's errors are
rebuilt.

#### B.1.6 Synchronous members and the state cache

Electron and ow-electron expose synchronous members that need host state
(`uid`, `hasPendingUpdates()`, `screen.getAllDisplays()`, `app.getPath()`, ...). Webviews cannot block on IPC, so:

1. At window creation Rust injects `window.__OW_TAURI_BOOTSTRAP__ = <HostSnapshot>`
   into `ow-main` as an initialization script that runs before any page script
   (and again after a soft restart, A.6).
2. `ow-tauri/main` builds its cache from the snapshot at import time. Every
   synchronous member reads the cache.
3. Rust pushes changes as `state` host messages `{ seq, patches }`. `path`
   is a dot path into `HostSnapshot` (for example `displays` or
   `flags.adsOptimizationDisabled`). Patches with
   `seq <= cache.seq` are ignored; a gap (`seq > cache.seq + 1`) triggers a
   `bootstrap` call that replaces the cache.
4. A patch is applied before the event that caused it is dispatched to app
   listeners, so a listener always sees the state that caused its event.
5. Writes that are synchronous upstream update the JS cache immediately and
   are sent to Rust in call order; Rust's acknowledgement does not change the
   return value. This covers the `BrowserWindow` setters `show`, `showInactive`, `hide`, `minimize`,
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
| `on('activate' \| 'browser-window-created' \| 'browser-window-focus' \| 'browser-window-blur')` | S | `activate` on macOS dock click, with `(event, hasVisibleWindows)` |
| `on('second-instance')`, `requestSingleInstanceLock()`, `hasSingleInstanceLock()`, `releaseSingleInstanceLock()` | P | lock always granted in JS; real single-instance behaviour comes from `tauri-plugin-single-instance`, registered by the app, whose callback calls `emit_second_instance` (A.5). Listeners get `(event, argv, workingDirectory, additionalData)`; `additionalData` is `undefined`, and the argument given to `requestSingleInstanceLock` is ignored |
| `quit()`, `exit(code?)`, `relaunch(options?)` | S | `relaunch({ execPath })` is U |
| `focus(options?)` | P | `{ steal }` is ignored (A.2.1 `app_focus`) |
| `getAppPath()` | S | virtual app root; `getAppPath() + '/package.json'` is readable through `files` |
| `getPath(name)` | S | `appData`, `userData`, `sessionData`, `temp`, `home`, `desktop`, `documents`, `downloads`, `music`, `pictures`, `videos`, `logs`, `exe`, `crashDumps`; `userData` is `<appData>/<PN>` like Electron and ow-electron (F.1), so migrated prefs files are found; `module` and `recent` are U |
| `setPath(name, path)` | P | affects only ow-tauri lookups (`userData`, `logs`) |
| `getName()`, `name`, `getVersion()`, `isPackaged`, `getLocale()`, `getSystemLocale()` | S | from the manifest and the OS |
| `setName(name)` | P | changes `app.name` for the session only; never changes the uid |
| `commandLine.hasSwitch()`, `getSwitchValue()` | S | the process arguments plus the switches appended in this session, read as Chromium does: reading stops at a bare `--`, the last occurrence wins, and only `--name=value` carries a value (`--name value` gives `''`) |
| `commandLine.appendSwitch()`, `appendArgument()`, `removeSwitch()` | P | recorded, so `hasSwitch()` sees them; `--disable-gpu` and `--remote-debugging-port` take effect from the next launch (A.1.1), because `ow-main` already exists when app code runs; other switches are ignored with a warning. The recorded set reaches Rust with `main_ready` and, on every change, with `app_record_browser_args` (A.2.1). For the current launch use `plugins.overwolf.webview` |
| `disableHardwareAcceleration()` | P | records `--disable-gpu` as above. Windows: applies from the next launch (A.1.1); set `webview.disableGpu` for the first launch; other platforms no-op with a warning |
| `setAppUserModelId(id)` | P | no-op; Tauri sets the AUMID from the bundle identifier |
| `getGPUInfo`, `getAppMetrics`, `setLoginItemSettings`, `getLoginItemSettings`, `dock`, `setBadgeCount`, `setJumpList`, `setUserTasks`, `showAboutPanel`, `setAsDefaultProtocolClient`, `importCertificate`, `moveToApplicationsFolder` | U | |

#### B.2.2 `BrowserWindow` (main)

Constructor options:

| Option | Status | Notes |
|---|---|---|
| `width`, `height`, `x`, `y`, `center`, `minWidth`, `minHeight`, `maxWidth`, `maxHeight` | S | logical pixels |
| `useContentSize` | P | ignored: sizes are always the window's inner (content) size. ow-electron sizes a framed window's outer frame by default and fits it to the work area, so the same `width` / `height` gives an ow-tauri window a larger content area (1200 x 800 gives an outer 1216 x 839 on Windows) and a window larger than the screen is clamped differently [OBS: Windows lab]. Not changed: it needs a product decision |
| `show`, `title` (default `<PN>`; also the `title` field of `<label>_window_closed`, E.2), `resizable`, `movable`, `minimizable`, `maximizable`, `closable`, `focusable`, `alwaysOnTop`, `fullscreen`, `skipTaskbar`, `transparent`, `backgroundColor`, `parent` | S | |
| `frame` | P | `false` = no decorations; on macOS mapped to an overlay title bar with a hidden title so native dragging works (ARCHITECTURE section 6) |
| `fullscreenable` | P | `false` is honoured by the facade (ignores `setFullScreen(true)`); no native flag |
| `modal` | P | owned by `parent` and kept above it; input modality Windows-only |
| `name` | S | ow-electron option; normalised (E.2); analytics window name and guest `windowName` |
| `icon` | P | app asset path only |
| `webPreferences.preload` | S | app-asset path of a bundled preload script; injected as an initialization script |
| `webPreferences.devTools` | S | |
| `webPreferences.contextIsolation` | P | preload and page share one JavaScript world in Tauri; `contextBridge.exposeInMainWorld` defines frozen globals, so code written for isolation works; `false` changes nothing |
| `webPreferences.nodeIntegration` | P | ignored with a warning; renderer `require('electron')` works only through the bundler alias, other Node modules are unavailable |
| `webPreferences.sandbox`, `webSecurity`, `partition`, `session`, `offscreen`, `webviewTag`, `zoomFactor`, `backgroundThrottling` | P | ignored with a warning, except `zoomFactor` (applied) |
| `titleBarStyle`, `trafficLightPosition`, `vibrancy`, `visualEffectState`, `roundedCorners`, `thickFrame`, `type`, `tabbingIdentifier`, `kiosk`, `simpleFullscreen` | U | |

Static members: `getAllWindows()`, `getFocusedWindow()`, `fromId(id)`,
`fromWebContents(wc)` are S (from the JS registry).

**Window ids.** Electron's `BrowserWindow.id` exists as soon as the
constructor returns, but `window_create` is asynchronous. The main runtime
therefore allocates the app-visible id itself, at construction, and maps it
to the plugin's id once `window_create` returns. Every id that crosses the
wire (`ipc_emit` targets, `IpcSender.windowId`, `window` messages,
`parentId`) is translated, so app code only ever sees app-visible ids;
`window` messages for a plugin id are held while a create is pending.
Calls made before the native window exists are queued and run in order.
If `window_create` fails, the error is logged, the window leaves the
registry, its pending loads reject and `whenCreated()` rejects; no `closed`, `window-all-closed` or
quit follows (Electron would have thrown from the constructor).

Instance members:

| Member | Status | Notes |
|---|---|---|
| `id`, `webContents`, `isDestroyed()` | S | |
| `whenCreated()` | ow-tauri addition | `Promise<void>` that resolves once the native window exists and rejects when creation failed |
| `loadURL(url)`, `loadFile(path, { query, hash })` | S | returns a promise that resolves on `did-finish-load`; a remote URL switches the window to class `remote`: its content becomes a fresh `bwr-<id>` webview with no IPC, no preload and no init scripts (A.2.3.1). The facade calls a URL remote when it is `http(s)` and its origin differs from the main webview's `location.origin` (the app origin), the same test Rust applies |
| `show()`, `hide()`, `close()`, `destroy()`, `focus()`, `isVisible()`, `isFocused()` | S | Tauri reports no visibility change, so the facade emits `show` / `hide` itself when a call changes the cached visibility, after the native call (A.3) |
| `blur()` | P | Tauri has no command for it; only the cached state changes (`isFocused()` returns `false`) |
| `showInactive()` | P | `window_show_inactive`: on macOS the window is not made key and the app is not activated (as ow-electron); on Windows it is shown without being activated (as Electron); Linux uses a plain show, which may activate it |
| `minimize()`, `maximize()`, `unmaximize()`, `restore()`, `isMinimized()`, `isMaximized()`, `setFullScreen()`, `isFullScreen()` | S | state reads use the cache, refreshed on every window event |
| `setBounds()`, `getBounds()`, `getContentBounds()`, `setSize()`, `getSize()`, `setPosition()`, `getPosition()`, `setMinimumSize()`, `setMaximumSize()`, `center()` | S | getters are synchronous from the cache |
| `setMovable()` | P | Tauri has no command for it; the flag is cached (`isMovable()`) and applied only through the constructor's `movable` option |
| `setResizable()`, `setAlwaysOnTop()`, `setSkipTaskbar()`, `setFocusable()`, `setIgnoreMouseEvents(ignore, { forward })`, `setTitle()`, `getTitle()`, `setBackgroundColor()`, `setProgressBar()`, `flashFrame()`, `setVisibleOnAllWorkspaces()`, `setContentProtection()` | S | `forward` is ignored |
| `moveTop()` | P | brings the window to the front by toggling always-on-top |
| `setMenu()`, `removeMenu()`, `setMenuBarVisibility()`, `setAutoHideMenuBar()` | P | no-op (Tauri windows have no menu unless the app adds one in Rust) |
| `setOpacity()`, `setVibrancy()`, `setShape()`, `capturePage()`, `setThumbarButtons()`, `setOverlayIcon()`, `previewFile()`, `setBrowserView()`, `addBrowserView()`, `setTouchBar()` | U | |
| events `close` (preventable), `closed`, `focus`, `blur`, `show`, `hide`, `ready-to-show`, `minimize`, `maximize`, `unmaximize`, `restore`, `resize`, `move`, `enter-full-screen`, `leave-full-screen` | S | from `window` host messages; `show` and `hide` from the facade's own calls (above) |

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
| `on('did-navigate-in-page')` | S | `(event, url, isMainFrame)` for a fragment change, `history.pushState` / `replaceState`, or back and forward between such entries in the top document; `getURL()` follows it (A.2.5 `navigation_in_page`). Only the top document is reported, so `isMainFrame` is always `true` |
| `setWindowOpenHandler(handler)` | P | the handler is called with `{ url, frameName, features, disposition }` from the `new-window` message (A.3); `{ action: 'allow' }` is treated as deny + open in the system browser (`http`, `https` only) |
| `on('will-navigate')` | P | emitted for top-level navigations the A.2.3.1 policy cancels (Windows: the navigation hook; macOS and Linux: link clicks and form submissions the bootstrap intercepts); the navigation has already been cancelled and the URL opened in the system browser, so `preventDefault()` has no further effect |
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
| `screen.getCursorScreenPoint()` | P | cached, refreshed at most every 100 ms; the first call returns the snapshot's `cursor` (`(0, 0)` when absent) and starts a refresh |
| `screen.on('display-added' \| 'display-removed' \| 'display-metrics-changed')` | P | detected by a 2 s poll |
| `screen.dipToScreenPoint()`, `screenToDipPoint()`, `dipToScreenRect()`, `screenToDipRect()` | P | computed from the cached display list |
| `shell.openExternal(url)` | S | `http`, `https`, `mailto` only |
| `shell.openPath(path)`, `shell.showItemInFolder(path)` | S | opener plugin, no shell |
| `shell.trashItem`, `beep`, `writeShortcutLink`, `readShortcutLink` | U | |
| `dialog.showOpenDialog`, `showSaveDialog`, `showMessageBox`, `showErrorBox` | S / S / P / S | `showMessageBox`: up to three buttons |
| `dialog.showOpenDialogSync`, `showSaveDialogSync`, `showMessageBoxSync`, `showCertificateTrustDialog` | U | |
| `globalShortcut.register(accelerator, cb)` | P | returns `true` synchronously; a later registration failure is logged and `isRegistered` turns false |
| `globalShortcut.unregister`, `unregisterAll`, `isRegistered`, `registerAll` | S | an accelerator matches by the keys it names, not by spelling (A.2.3 `global_shortcut_unregister`) |
| `crashReporter.start(options)` | P | no-op with a warning; use a Rust crash handler (see PORT-MAP) |
| `crashReporter.*` (other members) | U | |
| `nativeTheme.shouldUseDarkColors`, `on('updated')` | P | follows the webview's `prefers-color-scheme` media query, which the webview takes from the OS or window theme; `updated` fires when it changes. Setting `themeSource` changes only what the object reports, not the windows |
| `Menu`, `MenuItem`, `Tray`, `Notification`, `session`, `protocol`, `net`, `netLog`, `powerMonitor`, `powerSaveBlocker`, `autoUpdater` (Electron's), `clipboard`, `nativeImage`, `systemPreferences`, `desktopCapturer`, `webFrame`, `webFrameMain`, `utilityProcess`, `MessageChannelMain`, `BrowserView`, `WebContentsView`, `BaseWindow`, `TouchBar`, `inAppPurchase`, `pushNotifications`, `safeStorage`, `contentTracing` | U | module objects exist so imports compile; every member throws |
| `process.platform`, `process.arch`, `process.argv`, `process.env`, `process.versions` | P | the bootstrap installs a `globalThis.process` shim in `ow-main` and every `bw-*` webview before any app script runs (unless the document already has a `process`, from a bundler polyfill), so code that uses the global `process` without importing it works (the sample's `index.ts` and preload); `ow-tauri/electron` also exports it. `platform` and `arch` Node-style (`win32`, `darwin`, `linux`; `x64`, `arm64`), `argv` the process arguments, `versions` has `owTauri`, `tauri`, `chrome` (WebView2 only) and no `electron`. The shim object is frozen, but `env` is an ordinary object that libraries may assign to; it starts with only `OVERWOLF_APP_UID`, a read-only value set before any app script runs, as ow-electron sets it before the main module loads (B.1.1). Electron's `process.type` is `'browser'` in `ow-main` and `'renderer'` in UI windows, and `process.nextTick(cb, ...args)` runs `cb` as a microtask; `process.env.NODE_ENV` is a build-time constant the bundler defines (webpack 5 does it from `mode`; other bundlers: define it explicitly, see MIGRATION.md) |

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
   `:where(owadview) { display: inline-flex; width: 100%; height: 100%; }
   :where(owadview[performance]) { display: block; width: 0; height: 0; }
   :where(owadview[performance]) > :where(div) { display: flex; }`
   (ow-electron's element computes `inline-flex`; a performance element is
   a 0 x 0 `block` whose overlay `div` is a flex box [OBS]). `:where()` has
   zero specificity, so any app rule wins. Without it an
   unknown element is `display: inline` with no content and a 0 x 0 box, and
   the sample's ads (an unstyled `owadview` appended to a sized
   `div.ad-container`) would never mount.
2. **Upgrade at creation.** It wraps `Document.prototype.createElement` and
   `createElementNS` so that an element created with the local name
   `owadview` (any case) is upgraded before it is returned. Upgrading tracks
   the element and assigns it an `elementId`; its properties and methods are
   defined later, at attach (B.3.3), because ow-electron's element is a plain
   `HTMLElement` until then [OBS].
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
only: `cid`, `slotsize`, `adstyle`, `customtracking`, `performance`, `unit`
and `pageurl`.

| Attribute (as written / as stored) | Meaning | Change after mount |
|---|---|---|
| `cid` | container id reported with the ad; trimmed to 20 characters [DOC] | remount |
| `slotsize` | requested inventory `"WxH"`; Overwolf documents `400x300`, `400x600`, `300x250`, `160x600`, `728x90`, `970x90`, `400x60` with fallbacks [DOC], and all seven load and fill in ow-electron [OBS]; the host enforces no size and passes other values through | remount |
| `adstyle` | `"high-impact-ad;"`, `"rewarded-ad;"` and other style tokens; passed through unchanged. The ad page matches the tokens itself (a substring match: `"rewarded-ads;"` also gives the rewarded flow) [OBS]; the host interprets none of them | remount |
| `customTracking` / `customtracking` (also the `customTracking` property) | JSON string; invalid JSON clears it silently; an update replaces it; the last value survives guest reloads and crash recovery [DOC] | delivered live (`{ type: 'customTracking' }`, D.5) and again after every later guest reload [OBS]; no remount. The guest's `__overwolf__.customTracking` keeps the attach-time value [OBS] |
| `performance` (boolean) | performance ad (B.3.4); one per window: a second `performance` element that would attach while one is mounted is removed from the document in the same task, with no guest, no event and only a debug log, as ow-electron removes it [OBS] | remount |
| `unit` | ad unit override (the sample shows it commented out on its performance ad) | remount |
| `pageurl` | the guest's `__overwolf__.pageUrl` (D.2), read at mount [OBS] | applies at the next guest load; no message to the running page (`setPageUrl()` sends one, B.3.3) [OBS] |
| `id` | ordinary DOM id; not used by the runtime | none |

Any other attribute is ignored.

#### B.3.3 Properties and methods

| Member | Behaviour |
|---|---|
| `customTracking: string` | getter returns the `customtracking` attribute; setter sets it (same as `setAttribute`) [DOC] |
| `setAudioMuted(muted: boolean): void` | `adview_command setAudioMuted`; guests start muted [DOC] |
| `reload(): void` | reloads the guest |
| `pageUrl: string` | getter returns the `pageurl` attribute (`""` when absent); setter sets it [OBS: an own property after attach] |
| `setPageUrl(url: string): void` | sets the `pageurl` attribute (applied at the next guest load) and, on an attached element, sends `adview_command setPageUrl [url]`; Rust forwards it to the running ad page as the private message `{ type: 'setPageUrl', data: [url] }` (D.5) [OBS]. On an element that is not attached, or is dead (B.3.4), only the attribute is set. No visible effect was observed in ow-electron (OQ-32) |
| `sendCommand(...args): void` | `adview_command sendCommand` with the arguments copied as JSON (JSON values unchanged, `undefined` and functions inside arrays become `null`, a `bigint` its decimal string; arguments that cannot be encoded give `[]`); Rust forwards them to the running ad page as the private message `{ type: 'sendCommand', data: [...args] }` (D.5) [OBS]. Before attach the call is ignored with a debug log. The ad page decides what, if anything, it does with it; no visible effect was observed in ow-electron (OQ-32) |

After attach, ow-electron upgrades the element: its prototype becomes an
`OwAdViewElement` class carrying Electron's `<webview>` methods
(`getWebContentsId`, `getURL`, `executeJavaScript`, `send`, ...) plus
`setPageUrl` and `sendCommand`, and the instance gets own properties (`src`,
`cid`, `slotsize`, `pageUrl`, `performance`, `unit`, `adstyle`,
`customTracking`, `contentWindow`, ...) [OBS]. Before attach it is a plain
`HTMLElement` [OBS]. ow-tauri does the same at attach: the attribute-backed
properties `cid`, `slotsize`, `pageUrl`, `performance`, `unit`, `adstyle`
and `customTracking` become own accessors of the instance, in ow-electron's
order, and the methods `setPageUrl`, `sendCommand`, `setAudioMuted` and
`reload` live on a prototype inserted between the element and
`HTMLUnknownElement.prototype` (one per base prototype), so they are not own
properties, as on ow-electron's element [OBS]. A value an app assigned to one
of those properties on the plain element is moved into its attribute first.
Electron's generic `<webview>` methods are
not provided: Overwolf does not document them for `<owadview>`, and several
would give app code control over remote ad content [DEC].

#### B.3.4 Lifecycle and geometry

| Trigger | Action |
|---|---|
| element connected, and box non-empty or `performance` set | `adview_mount` |
| `ResizeObserver` change, `scroll` / `resize` on window or any scrollable ancestor (coalesced per animation frame) | `adview_update { rect }` |
| visibility changes | `adview_update { visible }` |
| attribute in the "remount" column changes | `adview_unmount` + `adview_mount` |
| element disconnected (including via an ancestor) | `adview_unmount` |
| document `pagehide` / unload | Rust closes every guest owned by the webview |

**Removal and moves.** An element removed or moved (removed and inserted
again in one task, also through an ancestor) after attach is dead: its guest
closes and it never attaches again. Once `adview_unmount` returned, it gets a
plain `destroyed` event only if it is in the document again by then (a move);
an element removed for good hears nothing. ow-electron sends `destroyed` after
the guest closed and only an attached element receives it [OBS lab
standard-remove, perf-remove].

Attribute changes do not reopen a dead element, and inserting it again only
logs a debug line; the app creates a new element instead. A live element
whose "remount" attribute changes (B.3.2) is closed and mounted again under
a new `elementId`.

**Performance element DOM.** At attach a `performance` element gets no
shadow root, the inline style `pointer-events: none;`, and one light-DOM
child `div` whose inline style is, character for character,
`position: fixed; top: 0px; left: 0px; width: 100vw; height: 100vh; background: transparent; z-index: 999999;`
as ow-electron writes it [OBS]. The element turns `pointer-events: auto`
just before its first `performance_ad_loaded` is dispatched (the `div`
inherits it; its style does not change); it is still `none` after
`display_ad_loaded` in ow-electron [OBS]. At the same event Rust stops
passing the guest's input through (below).

**Shadow root.** At attach the runtime tries to attach an open shadow root to
any other element, holding a `<style>` and a transparent `about:blank`
`<iframe>` (`pointer-events: none`, 100 % of the box), because ow-electron's
element has an open shadow root with exactly those two children after attach
[OBS]. Outside Electron the engines refuse `attachShadow()` on `owadview`
(not a valid custom element name), so in practice the element has no shadow
root: the runtime logs one debug line and does not try again in that
document. Nothing depends on the anchor; the ad itself is the native guest
webview.

**Visibility.** In ow-electron a guest whose embedder window was never shown
loads, logs `<owadview> is not visible. waiting...` and never fills; a shown
window fills, even at opacity 0 [OBS]. Overwolf documents that containers
should stay visible, that `display: none` pauses ads, and that ads should not
move or be made transparent [DOC]. ow-tauri reports the guest visible only
when all hold: the embedder window is shown and not minimized;
`IntersectionObserver` ratio at least 0.5 (half of the box inside the
viewport, on either axis; the measured ow-electron boundary, below);
`el.checkVisibility({ opacityProperty: true, visibilityProperty: true })`
(polled every 500 ms, because ancestor style changes do not fire observers);
`document.visibilityState === 'visible'`. A hidden guest is hidden, not
destroyed. For a `performance` element the box conditions do not apply. The
guest's own `document.visibilityState` stayed `visible` in a never-shown
embedder [OBS], so the page also judges visibility by other means; ow-tauri
reports a never-shown embedder as hidden [DEC], which fills no ad either way.
`hasFocus()` is `false` while the embedder is unfocused [OBS].

ow-electron signals the guest `hidden` when the element is `display: none`,
when it is scrolled out of the viewport, and when the embedder window is
hidden (plus a `window-hidden` message, D.5); a resize signals nothing, and
the window's position on the screen plays no part (an off-screen window
still fills test ads) [OBS]. A minimize signals `hidden` plus the
`window-minimized` and `window-hidden` messages (D.5) [OBS]; on Windows the
guest turns `hidden` first and then gets only `window-minimized`; a running
performance ad then stops, with `performance_ad_dismiss` before its `shutdown`
in some ow-electron runs and without it in others [OBS: Windows lab].
There a minimized window has an empty client area, so the guests stop
rendering; ow-tauri hides each guest webview natively on a Windows minimize
and shows it again on restore unless the app hid the element meanwhile
[DEC]. About 2 s after `hidden` the ad page stops and
calls `__overwolf__.reload()`; the host reloads the guest 3 to 5 s after
`hidden`, and the reloaded page waits until it is `visible` again [OBS].
ow-tauri passes its visibility result to the guest the same way (D.5) and
lets the page drive the reload. The page asks 2.6 to 4.8 s after `hidden` in
ow-electron, whose hidden document's timers are throttled, and a slot hidden
for 2 s and shown again keeps its ad [OBS]. Chromium runs a hidden page's
timers only at aligned wake-ups, once per second ("Timer throttling in Chrome
88", developer.chrome.com) [DOC]; WebKit runs them on time. The shim aligns
the guest main frame's timeouts of 1 s or more to whole-second wake-ups while
it reads `hidden`; shorter timeouts and intervals run on time [DEC]: aligning
every timer of the main frame alone (its cross-origin ad frames keep running
on time) left the ad silent after a 1-frame and a 2 s hide [OBS: lab,
reward-optin, two runs]. Chromium's one-per-minute intensive throttling after
5 minutes hidden is not emulated. ow-tauri also holds a reload asked for
while hidden until 2.5 s after `hidden`, or until the guest is visible again
if that comes first [DEC]. A page that asked has given up its ad and plays
nothing more until it reloads [OBS: lab, 2 s hide], so the request is never
dropped. With the alignment and the `visibilitychange` events of D.5 a 2 s
hide keeps the ad, and a minimized window's performance ad shuts down with
the same messages as in ow-electron, where `performance_ad_dismiss` before
the `shutdown` comes in some runs and not in others, on both hosts [OBS: lab,
reward-optin and perf-minimize]. ow-electron signals a 300x250 guest
`visible` from exactly half of it inside the viewport and `hidden` again at
49 %, scrolled off the top or the left edge alike, with no hysteresis and
within a few milliseconds of the move; a slot 25 % in view never fills, 50 %,
75 % and fully in view all fill [OBS: lab, harness `inview-probe` and
`inview-fine`, TEST mode]. ow-tauri's 0.5 ratio gives the same boundary on
both axes [OBS: same scenarios on ow-tauri]. The guests of
a minimized embedder stay hidden until the restore (R3-1, OQ-27).

Native child webviews always paint above the page. Any HTML that must cover an
ad (menus, modals) needs the app to hide the element; the runtime does that
automatically when an ancestor is hidden. CSS transforms and `clip-path` on
ancestors are not reflected in the guest's geometry.

**Z-order.** ow-electron's performance overlay (`z-index: 999999`) covers
every other ad of the page [OBS]. Native guests stack in creation order, so
after every guest mount in a window Rust raises that window's newest
performance guest to the top: Windows `SetWindowPos(HWND_TOP)` on the
WebView2 controller's parent window (no move, no size, no activation); macOS
re-adds the guest view above its siblings
(`addSubview:positioned:NSWindowAbove relativeTo:nil`); Linux
`gdk_window_raise`, best effort. Standard guests keep creation order among
themselves [OBS lab L2, L2-W].

**Transparency.** ow-electron's guest is part of the page, so a slot with no
ad shows the app's own container background and an interstitial's dim shows
the app behind it [OBS]. With `ads.transparentGuests` (default `true`, A.1)
every guest webview is created transparent: Windows and Linux use the
webview's transparent background; macOS also clears the `WKWebView`
background (the `drawsBackground` key-value key, used only when the view
responds to it, and the public `underPageBackgroundColor`) without Tauri's
`macos-private-api` feature [DEC]. `false` keeps opaque guests [OBS lab L1,
L1-W].

**Input pass-through.** ow-electron's performance element takes no input
until its ad has loaded (`pointer-events: none`, above) [OBS], so the page
under an empty interstitial stays usable. A native guest would swallow every
click over the whole window, so Rust mounts a performance guest
pass-through until its first `performance_ad_loaded` (`MODAL_EVENT`):
Windows an empty window region (`SetWindowRgn`), cleared at the event;
macOS a `hitTest:` that returns `nil` for that view, installed once on the
web view's own class and switched per view; Linux an empty input shape
[DEC]. From the event on the guest takes input like any other [OBS lab L3,
L3-W].

**Linux.** tauri-runtime-wry packs a window's child webviews into a
`GtkBox` (`pack_start`), so guests never overlap the page or each other: on
Linux the ad rectangles, the z-order and the pass-through above do not
apply. This is a known gap (PARITY); the raise and the input shape are
applied best effort [DEC].

High-impact ads: the app grows the container (the sample sets it to the zone's
size after `high-impact-ad-loaded`); `ResizeObserver` reports the new rect.
Performance ads [DOC] (OQ-29): the element's own box is ignored; the guest
covers the embedder window's content area (the sample appends a bare
`<owadview performance>` to `document.body`) and follows its size; `adstyle`
and `unit` are passed through; one per window (B.3.2). On `shutdown` the
runtime dispatches the event (and nothing after it), closes the guest, and
removes the element from the document in the next task (`setTimeout(0)`),
after the `shutdown` listeners ran; no `destroyed` follows. ow-electron
removes it about 185 ms after `shutdown`, ow-tauri after about 1 ms [OBS lab
perf-remove]. The host enforces no minimum window size: the ad page itself
answers an embedder smaller than 500 x 500 with `performance_ad_error`
(a string) and `shutdown` [OBS]; the documented 1000 x 600 minimum is
Overwolf's guidance.

Conformance tests use the sample's exact DOM: an unstyled `owadview` in a
400 x 600 `div`, and a bare `performance` element appended to `body`; both
must mount.

#### B.3.5 DOM events

Each guest message `adview_event { name, data }` that is not internal (D.4) is
dispatched on the element as a plain event, exactly as ow-electron does [OBS]:

```js
const event = new Event(name, { bubbles: false, cancelable: false });
Object.assign(event, data);   // own properties; detail stays null
element.dispatchEvent(event);
```

`data` is copied as `Object.assign` copies it [OBS]: the own enumerable keys
of `Object(data)` become own properties of the event, so an object gives its
fields, an array its indexes, and a string one property per character (seen
with `performance_ad_error`, whose data is a string: `event[0]` is its first
character); `null`, `undefined`, numbers and booleans add nothing. Keys the
event already has (`type`, `target`, `isTrusted`, ...) are skipped, where
`Object.assign` would throw on the read-only ones. Every name that passes the A.2.6 checks is forwarded unchanged and in
order; `display_ad_loaded` arrives twice per fill in ow-electron and is
passed through twice [OBS]. The runtime keeps no list of names. Spelling
variants that Overwolf documents in both forms are also dispatched in the
other spelling, the received spelling first:

| Received | Also dispatched |
|---|---|
| `ad_clicked` | `ad-clicked` |
| `ad-clicked` | `ad_clicked` |
| `house_ad_action` | `house-ad-action` |
| `house-ad-action` | `house_ad_action` |

Names seen from the ad page [OBS] or documented [DOC]: `display_ad_loaded`,
`impression`, `play`, `pause`, `ended`, `player_loaded`, `video_ad_ready`,
`complete`, `house-ad-action` / `house_ad_action` (`{ action }`),
`high-impact-ad-loaded`, `high-impact-ad-removed`, `shutdown`,
`performance_ad_loaded`, `performance_ad_error` (a string), `performance_ad_dismiss`,
`performance_ad_clicked`, `performance_ad_video_complete`,
`performance_ad_video_skipped`, and the documented `performance_ad_no_fill`,
which no lab run has seen: a performance ad without fill sent only
`shutdown` [OBS]. The orders observed per ad format are in
[AD-FORMATS.md](AD-FORMATS.md).

Host lifecycle events (`source: 'host'`) dispatched the same way [OBS]:

| Event | When | Own properties |
|---|---|---|
| `did-attach` | the guest webview exists | none |
| `dom-ready` | the guest document's `DOMContentLoaded` | none |
| `did-finish-load` | the guest main frame finished loading | none |
| `did-fail-load` | a load failed: main frame on every platform; sub-frames where the platform reports them (WebView2 `FrameNavigationCompleted`). ow-electron fires it for sub-frames too (`isMainFrame: false`, `errorCode: -3`) [OBS] | `errorCode`, `errorDescription`, `validatedURL`, `isMainFrame`, `frameProcessId`, `frameRoutingId`; the two frame ids are `0` where the platform has none [DEC] |
| `render-process-gone` | the guest crashed (D.7); ow-electron sends no `crashed` event [OBS] | `details` (`{ reason, exitCode }`) |
| `did-start-navigation`, `load-commit` | the guest started or committed a navigation, where the platform reports it | Electron's documented `<webview>` properties where available [DEC] |
| `console-message` | a console call in the guest's main frame, reported by the shim | Electron's documented `<webview>` properties [DEC] |
| `ad-clicked` | a popup or gesture navigation opened the system browser (D.7) | `url` |
| `destroyed` | dispatched by the runtime itself once `adview_unmount` returned: the guest of an element removed or moved after attach has closed, and the element is in the document again (a move, B.3.4); an element removed for good gets nothing [OBS lab standard-remove, perf-remove] | none |

ow-electron forwards all of Electron's standard `<webview>` events to the
element, including `did-frame-*` and `media-*` events and focus changes
[OBS]. ow-tauri emits the rows above; the others have no platform
equivalent and are not emitted (a known gap) [DEC].

The host `ad-clicked` is dispatched only if no guest-originated click
spelling was dispatched for that element in the previous 1000 ms, so one
click yields one pair of events.

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

A port stops building on `@overwolf/ow-electron`, which is what supplied Electron's
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

The `@overwolf/ow-electron` path is required while `@overwolf/ow-electron`
is still installed (a project that builds both hosts during a migration):
the packages-types `import '@overwolf/ow-electron'` would otherwise resolve
to the installed package and add its `declare module 'electron'` and global
`Electron` namespace to the program, next to these. With the path, the
installed package stays out of the ow-tauri program. The runtime entries
(`ow-tauri/main`, `ow-tauri/renderer`, `ow-tauri/electron`) declare no
globals and no ambient modules, so the ow-electron build of the same project
can import them and still compile against ow-electron's types.
`src/typings-coexist.test.ts` checks both programs with ow-electron's
typings installed.

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
`ipc_reply`, `ipc_emit`, `ipc_emit_skip` (`overwolf:main`, A.2.1). `IpcErrorWire` is the
`OverwolfErrorWire` shape of A.4.

### C.2 `invoke`

1. The renderer runtime encodes `args` with the OTJ codec (C.7), assigns the
   next sequence number `seq` of its current `epoch` (C.3), records a pending
   entry, and calls `plugin:overwolf|ipc_invoke { channel, args, epoch, seq }`.
2. Rust checks the caller class (`ui` or `overlay`), the epoch, the encoded
   size (`ipc.maxMessageBytes`), the sender's in-flight count
   (`ipc.maxInFlightInvokes`, else `ipc-overloaded`) and that `channel` is a
   non-empty string of at most 256 UTF-16 code units, and that `seq` is
   neither used already (`invalid-argument`) nor too far ahead of the
   sender's order (`ipc-overloaded`). It allocates a request
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
   name, message, text } }`, where `text` is `String(thrown)`: the text
   Electron puts after its prefix (`"<name>: <message>"` for an `Error`, just
   the name when the message is empty, the value's own string for a
   non-`Error` throw). A return value that cannot be encoded, or whose
   encoding exceeds `ipc.maxMessageBytes`, is replaced by an
   `ipc-serialization` error reply (C.5).
6. Rust queues `ipc-result { id, ok, value | error }` on the sender's channel
   in the main runtime's per-target `seq` order (C.5), so every message the
   handler sent to that window before returning arrives first, as Electron's
   single pipe guarantees. The renderer runtime resolves or rejects the
   pending entry. A remote error rejects with `OwTauriError('ipc-remote-error',
   "Error invoking remote method '<channel>': <text>")`, the same text
   Electron produces, with `data.name`, `data.message` and `data.text`
   attached (`<name>: <message>` when `text` is missing). Every other error
   in an `ipc-result` keeps its `code` and gets the same prefix in front of
   its message. A result that arrives before the `ipc_invoke` acknowledgement
   (possible, because the two travel on different paths) is held until the
   acknowledgement names its `id`.

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
- **Gaps.** A rejected call never enters the reorder buffer, whether Tauri
  rejected it before the command ran (A.4) or the plugin refused it with an
  error. The rule is that **every rejected call reports its number**: the
  renderer runtime calls `ipc_skip { epoch, seq }` for each rejection except
  a stale epoch (the router ignores skips for old epochs, and for numbers it
  has already used or skipped). As a fallback, an out-of-order message waits
  for a missing number for at most 1 s, after which the gap is skipped and a
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
- applies the target's `seq` order to `ipc_emit` and `ipc_reply` together.
  Every outbound number is accounted for: an `ipc_emit` the plugin rejects
  uses up its `seq` on the Rust side, and the main runtime reports every
  `ipc_emit` or `ipc_reply` that never reached the plugin (rejected by Tauri,
  or failed in the runtime) with `ipc_emit_skip { target, seq }`. Skipping a
  number that is already used is ignored, so both may happen for one call.
  The 1 s gap timeout of C.3 also applies to the outbound order. A reply that
  cannot be ordered settles its invoke with `ipc-overloaded` (A.2.1);
- buffers messages until that webview has called `ipc_subscribe` (bounded by
  `ipc.maxQueuedMessages`); a reload empties the buffer, as Electron's
  renderer reload does;
- queues `ipc { kind: 'message', channel, args }` on the target's channel.

The renderer runtime calls each `ipcRenderer` listener for `channel` with
`(event, ...decodedArgs)`, where `event = { sender: ipcRenderer, senderId: 0, ports: [] }`.

**Size.** Rust checks the size of `ipc_emit` but not of `ipc_reply`, so the
main runtime checks both against `ipc.maxMessageBytes` (from
`HostSnapshot.ipcLimits`, else the default 8 MiB) before it sends them. An
oversized `webContents.send` throws `OwTauriError('ipc-serialization')`
synchronously, as an unencodable argument does. An oversized handler result
becomes the reply `{ ok: false, error: { code: 'ipc-serialization', message,
data: { bytes, limit } } }`, so the renderer's `invoke` rejects with
`ipc-serialization` instead of hanging.

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
| `Error` and subclasses | `{ "$otj": "error", "name", "message", "stack"? }` | the standard constructor of that name (`EvalError`, `RangeError`, `ReferenceError`, `SyntaxError`, `TypeError`, `URIError`), else `Error` with `name` set |
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
| handler threw | `OwTauriError('ipc-remote-error', "Error invoking remote method '<ch>': <text>")`, where `<text>` is `String(thrown)` (C.2 step 5): `<name>: <message>` for an `Error` |
| timeout | `OwTauriError('ipc-timeout', ...)` |
| unencodable argument | `OwTauriError('ipc-serialization', ...)`, thrown synchronously by `invoke` and `send` |
| handler result unencodable or larger than `ipc.maxMessageBytes` | `OwTauriError('ipc-serialization', "Error invoking remote method '<ch>': ...")` (C.5) |
| main webview not ready or restarted, or a stale epoch | `OwTauriError('not-ready', ...)` |
| too many invokes in flight or messages queued | `OwTauriError('ipc-overloaded', ...)` |
| caller is not a `ui` / `overlay` window | `OwTauriError('forbidden', ...)` |

All are `instanceof Error`, so existing `try/catch` and `.catch()` code keeps
working; only code that parses Electron's message text sees the same prefix.

---

## D. Guest shim contract

The ad page is `https://www.overwolf.com/monsdk/electron/latest/adview.html`;
the consent pages are under
`https://content.overwolf.com/monsdk/electron/latest/cmp/22.3.27/`
(`ow-cmp-v2.html` for the startup consent window, `cmp.html` for the settings
window) [OBS]. ow-tauri never modifies those pages. It loads them with the
same inputs ow-electron gives them (URL, query, headers, cookies, globals),
so the pages behave, and report, as they do under ow-electron.

ow-tauri injects one script into each guest webview as an initialization
script: `adview-host.js` for ads, `cmp.js` for consent. Both are written in
TypeScript in `packages/ow-tauri/src/guest/`, linted and unit-tested with the
rest of the package, built into `crates/tauri-plugin-overwolf/js/`
(committed, with a CI drift check) and embedded with `include_str!`
([ADR 0012](adr/0012-js-runtime-singleton.md)).

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

**Ad guest configuration.** Rust builds one object per guest (`guest_config`
in `ads/mod.rs`, completed in `host/ads.rs`). The shim reads it once at
document start; the page never sees the object itself.

| Key | Type | Use |
|---|---|---|
| `muid` ... `customTracking` | as D.2 | the D.2 data keys, in D.2 order (`customTracking` only when the attribute holds a JSON object) |
| `slotId` | string | the guest label `owad-<embedderLabel>-<n>`, sent back as `slotId` in every `adview_event` (D.4) |
| `visibilityState` | `'visible' \| 'hidden'` | the guest's visibility at document start (B.3.4); later changes arrive through `setVisibility` (D.5) |
| `hostKey` | string | the name of the window property that holds the shim's host API (D.5): `_` followed by 32 random hex digits (`uuid` v4, simple form), new for every guest (`host_key` in `host/ads.rs`). The property is non-enumerable, non-writable and non-configurable, so the page cannot find it by a fixed name. The shim also keys the next load's `pageurl` in `sessionStorage` with it (B.3.3). Without a valid key (only in unit tests) the shim falls back to `__owTauriHost` |
| `documentReferrer` | string | Windows only: the referrer the shim answers for `document.referrer` (D.8.2); absent elsewhere |
- The guest's own transport is `window.__TAURI_INTERNALS__.invoke`, kept in a
  closure at startup so page scripts cannot redirect it later. Messages are
  queued (at most 200, retried every 250 ms) until it is available.
- Functions the shim defines for the page have `length` 0 (rest parameters)
  and are frozen, as ow-electron's native functions look to the page [OBS].

### D.2 `window.__overwolf__` data

Defined with `Object.defineProperty(window, '__overwolf__', { writable: false, configurable: false, enumerable: true })`
and deep-frozen [OBS]. The keys below are defined **in this order**, which is
the enumeration order the page sees in ow-electron, and none is ever omitted;
an empty value is `""` [OBS].

| # | Key | Type | Value |
|---|---|---|---|
| 1 | `muid` | string | `<muid>` (E.4) |
| 2 to 10 | `setMute`, `triggerEvent`, `applySetting`, `crash`, `reload`, `getSystemInformation`, `getCustomTracking`, `hasWindowFocus`, `onmessage` | function | D.3 |
| 11 | `uid` | string | `<uid>` (G.2) |
| 12 | `name` | string | `<PN>` (G.1) |
| 13 | `owVersion` | string | `<owVersion>` (section 0; `"42.11.4"` in ow-electron), or `ads.owVersionOverride` when set |
| 14 | `version` | string | `<ver>` |
| 15 | `windowName` | string | analytics name of the embedder window (E.2; `"index"` for `index.html`) |
| 16 | `windowTitle` | string | the embedder window's current document title |
| 17 | `windowFocused` | boolean | embedder focus when the guest document started (`false` observed) |
| 18 | `testAd` | boolean | `true` in test mode, `false` live (D.7) |
| 19 | `consent` | string | `cmp.unifiedConsentString` of `ow-electron.json` as it was at launch (URL-encoded, `cmp%3D...`), or `""` before the first consent; a consent saved later reaches running guests as a message (D.5) and new guests through the cookies (D.6.3), not through this key [OBS] |
| 20 | `consentFull` | string | the same value as `consent` [OBS] |
| 21 | `slotSize` | string | element `slotsize` (`"400x600"`) |
| 22 | `containerId` | string | element `cid` (at most 20 characters) |
| 23 | `systemInfo` | object | below |
| 24 | `settings` | object | `{ disableOptimization: boolean, anonymous: false }`: `disableOptimization` is `true` when `disableAdsOptimization()` was called or the manifest's `build.overwolf.disableAdOptimization` is `true`; `anonymous` stays `false` even after `disableAnonymousAnalytics()` [OBS] |
| 25 | `muidV2` | string | `<muidV2>` (E.4) |
| 26 | `phasePercent` | number | E.4 |
| 27 | `pageUrl` | string | element `pageurl` at mount, or `""` [OBS] |
| 28 | `performanceAd` | boolean | element has `performance` |
| 29 | `adStyle` | string | element `adstyle`, or `""` |
| 30 | `unit` | string | element `unit`, or `""`; passed through unchanged in test mode too, as ow-electron does [OBS] |
| 31 | `customTracking` | object or null | parsed element `customTracking` at mount, kept across reloads (the live value arrives as a message, D.5) [OBS]; `null` when unset [INF] |

Keys that earlier drafts had and ow-electron does not expose: `runTimeInfo`
and `emailHashes`. Email hashes reach the page only as `eHashes` messages
(D.5) [OBS].

**`unit` in test mode.** ow-electron passes `unit` through unchanged in test
mode [OBS], and so does ow-tauri. An earlier ow-tauri rewrote a non-empty
`unit` to `"testAd"` in test mode; that guard is removed (wave 3e,
[ADR 0005](adr/0005-ads-test-live-parity.md) amendment): the ad page already
serves test inventory whenever `testAd` is `true`, and the rewrite changed
what the page saw.

**`systemInfo`** (`getSystemInformation()` returns a copy of the same object).
Shape observed on macOS [OBS]:

```json
{"gpus":[{"name":"","model":"","driverVersion":"","vendor":""}],
 "cpu":"Apple M4",
 "displays":[{"name":"Built-in Retina Display","isMain":true,"position":[0,0],"resolution":[1470,956],"dpi":192}]}
```

| Field | Value |
|---|---|
| `cpu` | the CPU brand string. macOS `sysctl machdep.cpu.brand_string`; Windows `HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0\ProcessorNameString` [INF]; Linux `/proc/cpuinfo` `model name` [INF] |
| `gpus` | macOS: exactly one entry with four empty strings, as observed on Apple Silicon [OBS]. Windows: one entry per DXGI adapter (`EnumAdapters`, software adapters included) with `name`, `model` and `vendor` empty and `driverVersion` = the user-mode driver version from `CheckInterfaceSupport(IDXGIDevice)` as `a.b.c.d` (`10.0.26100.33438` on a Windows Server 2025 runner with two adapters) [OBS: Windows lab]. Linux: **Unknown (R2-11)**; one blank entry, as on macOS [DEC] |
| `displays` | one entry per display: `name` (macOS `NSScreen.localizedName`; Windows the monitor friendly name from `DisplayConfigGetDeviceInfo`, e.g. `HyperVMonitor`, not the GDI name `\\.\DISPLAY1` [OBS: Windows lab]), `isMain`, `position` and `resolution` in logical (DIP) pixels, `dpi` = `round(96 x scaleFactor)` |

There are no `os`, `arch` or `scaleFactor` keys [OBS]. Overwolf's privacy
policy covers this data ("device type, operating system, graphics card")
[DOC] (OQ-19).

### D.3 `window.__overwolf__` functions and `window.gc`

| Function | Behaviour |
|---|---|
| `setMute(muted)` | `adview_event '__host:setMute' { muted }`; Rust mutes or unmutes the guest |
| `triggerEvent(name, ...args)` | `adview_event name` with `data` = the single argument, or the argument array when there are several; `message` and `messageerror` are dropped |
| `applySetting(setting)` | `adview_event '__host:applySetting'`; recorded only (ow-tauri implements no setting-driven behaviour, and never scans user data). The ad page calls it with `{ enableHashes: true }` on every load [OBS] |
| `crash()` | `adview_event '__host:crash'`; Rust treats it as a crash for recovery purposes |
| `reload()` | `adview_event '__host:reload'`; Rust reloads the guest, as ow-electron's host does about 70 ms after the request [OBS]. The ad page calls it itself after a `hidden` signal (B.3.4) |
| `getSystemInformation()` | a copy of `systemInfo` [OBS] |
| `getCustomTracking()` | a copy of the current `customTracking` [OBS] |
| `hasWindowFocus()` | the embedder window's current focus, as a boolean [OBS]; Rust keeps the shim's copy current with the host API's `setEmbedderFocus(<bool>)` (D.5), which never reaches `onmessage` handlers |
| `onmessage(handler)` | registers `handler` (at most 16); host messages (D.5) are passed to every handler as a fresh copy; handler exceptions are swallowed |

`window.gc` is defined as a no-op function when the page has none; it is a
function in ow-electron's guest [OBS], and the ad page forwards its events
only when it exists [POC].

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
| `__host:ready` | `{ href, testAd, visibilityState, pageUrl }` | guest initialised; `pageUrl` is the `pageUrl` of this load (D.2), so Rust can tell whether a `setPageUrl()` made before a reload still has to be stored for the next one (B.3.3) |
| `__host:domReady` | none | the guest document's `DOMContentLoaded` (sent at once when the shim runs after it). Rust dispatches the element's `dom-ready`, then a `did-finish-load` the platform reported earlier and held for it, so the two keep ow-electron's order (B.3.5) |
| `__host:gesture` | `{ kind: 'pointerdown' \| 'keydown' \| 'iframe-focus' }` | user gesture reported by the shim (debounced 100 ms); opens the navigation window (D.7). Page scripts can forge it, so it grants at most one external open (D.7) |
| `__host:focus` | `{ focused }` | guest focus changes |
| `__host:setMute`, `__host:applySetting`, `__host:crash`, `__host:reload` | see D.3 | |

Every other name is forwarded to the embedder as an `adview-event` host message
(B.3.5).

### D.5 Host to guest

The shim's host API sits on the guest's random window property `hostKey`
(D.1). Rust calls it with `webview.eval` and a script of the form
`(function(h){h&&h.deliver(<json>)})(window["<hostKey>"])`
(`host_call_script` / `deliver_script` in `ads/mod.rs`); a page that has
not run the shim yet, or a document of another origin, ignores it.
`deliver` validates `{ type: string, data? }` and passes it to the
`onmessage` handlers. Shim-internal state (the embedder focus behind
`hasWindowFocus()`, the guest's visibility, the next load's `pageUrl`) is
updated with the host API's `setEmbedderFocus(<bool>)`,
`setVisibility(<state>)` and `setNextPageUrl(<url>)` instead, so the page's
handlers never see a message ow-electron does not send.

ow-electron passes the page exactly these messages [OBS] (OQ-13), and
ow-tauri sends the same:

| `type` | `data` | Sent when |
|---|---|---|
| `consent` | string | a consent page saves (D.6.6): **twice**, first the TCF string (`saveConsent`), then the stored, URL-encoded unified string `cmp%3D...` (`saveUnifiedConsent`). Sent to every existing guest, including one that has not finished loading; **not** resent after a guest reloads [OBS] |
| `customTracking` | object or `null` | the element's `customTracking` changed, and again after every later reload of that guest [OBS]; Overwolf documents that updates reach the running ad page [DOC] |
| `eHashes` | `{ sha1, md5, sha256 }` | `setUserEmailHashes()` or `generateUserEmailHashes()` was called (A.2.2); sent to every existing guest; not resent after a reload [OBS] |
| `window-minimized` | none | the embedder window was minimized, when the minimize ends, before `window-hidden`; the guest document turns `hidden` after both [OBS]. A running performance ad then shuts down about 1 s later, in some runs after dismissing itself (`performance_ad_dismiss`) and in others without it, on both hosts [OBS] (ow-tauri: with the hidden-page timer alignment of B.3.4) |
| `window-hidden` | none | the embedder window was hidden or minimized (not again when a hidden window is minimized; on Windows a minimize sends `window-minimized` only, B.3.4); nothing is sent when it is shown or restored again [OBS] |
| `sendCommand` | array: the arguments of `element.sendCommand(...args)`, as JSON (B.3.3) | the app called `sendCommand()` on the attached element [OBS] |
| `setPageUrl` | array: `[url]` | the app called `setPageUrl(url)` on the attached element; the URL is also the `pageUrl` of the guest's next load (D.2) [OBS] |
| `ad-clicked` | URL string | a popup or gesture navigation was opened in the system browser (D.7); ow-tauri only [DEC, Low; OQ-17] |

`disableAdsFPD()`, `disableAdsOptimization()` and resizes send nothing
[OBS]. A restore with a live guest was not observed (the performance ad of
R3-1 had dismissed itself); ow-tauri sends nothing for it beyond the guest's
visibility [DEC]. ow-tauri sends no other host messages. The
page's own `postMessage` traffic (`owCustomTracking`, `owPageUrl`, `oam-*`,
`display-ad-*`) is not host traffic.

**Guest state signals** [OBS]. On every guest load ow-electron also mutes the
guest, signals its visibility (`visible` or `hidden`, B.3.4) and its focus
(`false`), and later signals focus `true` / `false` when the embedder window
gains or loses focus. ow-tauri reproduces them inside the shim, never through
`onmessage`: visibility through the host API's `setVisibility(<'visible'|'hidden'>)`,
which overrides `document.visibilityState` and `document.hidden` and fires
`visibilitychange` (in ow-electron the guest's `document.visibilityState`
reads `hidden` while its window is hidden [OBS]). An ow-electron guest
document sees more than one `visibilitychange` per change: on a hide one
event that still reads the old state, then the change; on a show three that
still read `hidden`, then the change [OBS: lab, every hide and show of the
reward and minimize captures]. The shim fires the same; a repeated state
fires nothing; focus through
`setEmbedderFocus` (D.3), which also drives `document.hasFocus()` (it reads
`true` while the embedder window has focus [OBS]).

### D.6 Consent

ow-electron's consent flow has a hidden startup consent window on every
launch, and the settings window that `openAdPrivacySettingsWindow()` opens,
whose first call of a launch also opens a hidden default-consent window
[OBS] [DOC]. Consent reaches ads through cookies the consent page writes in
the ads data store, and through `consent` messages to running guests (D.5)
[OBS].

#### D.6.1 Startup consent window

On **every launch** the host creates the webview window `ow-cmp-startup`
as soon as the `cmp-eu-only` request (D.6.2), started at `main_ready`,
has completed, whatever its outcome (a response of any status, an invalid
body, a dropped connection) [OBS]:

- 1 x 32 logical pixels, centred, title `<PN>`, never shown, not focusable,
  no decorations, skipped in the taskbar; ads data store and `<UA>` (D.8.1).
- URL: `https://content.overwolf.com/monsdk/electron/latest/cmp/22.3.27/ow-cmp-v2.html?unifiedcmp=<X>&muid=<muid>&uid=<uid>&muidv2=<muidV2>&oweVersion=<owVersion>&appVersion=<ver>`,
  query keys in that order. `<X>` is `""` when nothing is stored; otherwise
  it is the stored `cmp.unifiedConsentString` (which is already URL-encoded,
  F.2) URL-encoded **once more**, so `cmp%3D...` becomes `cmp%253D...`.
- The document request is a plain navigation: the host adds no headers, and
  no Origin, Referer or cookie is sent on a first launch [OBS].
- The page closes itself with `window.close()` (routed to `cmp_event close`)
  about 0.5 to 1.5 s after it loads; ow-electron closed it 1.5 to 5 s after
  creation [OBS]. If it is still open after `consent.readyTimeoutMs`, or its
  main-frame load fails, the host closes it [DEC].
- `isCMPRequired()` resolves when this page reaches `did-finish-load`, or
  right after its load fails [OBS]. When consent is not required (a
  `no-cmp` answer, D.6.2) the window still opens, on
  `ow-cmp-v2.html?clear=true` [OBS: Windows lab]; ow-tauri does the same.
- The window sends no analytics of its own (it is never shown, E.2).

What the page does [OBS]:

| | First launch (no stored consent) | Later launches |
|---|---|---|
| consent | generates a default **Full** consent | reuses the stored consent |
| fetches | `https://content.overwolf.com/cmp/v3/vendor-list.json`, `.../cmp/v3/gac/gac.json` | `gac.json` only |
| page analytics | Counter `electron_cmp_accept_full_launch` (`consent_type: "Full"`, `legitimate_interest_type: "Full"`, `vendor_list_version`, `cmp_version`, `appId`) | none |
| state file (via D.6.6) | `cmp` written | `cmp` rewritten; only `timeStamp` changes |
| cookies | `euconsent-v2`, `acconsent` inserted | both rewritten with a fresh expiry |

Overwolf documents that an app installed by its own installer (a Tauri
installer is one) shows the first consent layer itself, at the earliest on
first launch [DOC]; the startup window above is what ow-electron runs in
every case, so ow-tauri runs it too.

#### D.6.2 `isCMPRequired()`

- At `main_ready` (or its 10 s fallback, E.2), together with the startup
  analytics, the host sends one
  `GET https://features.overwolf.com/experiments/cmp-eu-only` with the host
  request headers of E.1 plus `cache-control: no-cache` [OBS]. The observed
  response is `{"params":[]}`.
- One request per launch [OBS]. Every `isCMPRequired()` call awaits it and
  the startup consent page's load (D.6.1); calls after that resolve at once.
  The result is not persisted: the next launch requests again [OBS].
  ow-electron resolved `true` 0.85 to 3.3 s after launch [OBS].
- **No client timeout** [OBS]: when the server hung for 45 s, the call
  resolved after 45.8 s. This request is exempt from the 30 s timeout of E.1.
  The typings say it never throws and defaults to `true` [TYPES].
- The result was `true` for every response ow-electron was given [OBS]:
  `{"params":[]}`, `params` of `[false]`, `[true]`, `["false"]`, name/value
  and key/value objects, `enabled: false`, HTTP 500 and 404, invalid JSON and
  a dropped connection. ow-tauri logs a non-empty `params` body once at
  debug level.
- **`{"params":["no-cmp"]}`** (the answer outside the consent region; Windows
  lab, a US runner) [OBS]: `isCMPRequired()` resolves `false`. The startup
  consent window loads `ow-cmp-v2.html?clear=true` (no other query), whose
  page calls `saveConsent("")` and `saveUnifiedConsent("")`, so `cmp` is
  stored as `{"cmpString":"","timeStamp":0,"unifiedConsentString":""}` and no
  consent cookies exist; the settings window URL carries `cmpRequired=false`;
  no default-consent window opens (D.6.4). ow-tauri does the same: `false`
  only when `params` is an array holding the string `"no-cmp"` (OQ-06,
  answered for this value).
- **`{}` body** (no `params` key) [OBS]: the result is not cached, so every
  `isCMPRequired()` call sends a new request **and** opens a new startup
  consent window (five calls gave five requests and five windows). ow-tauri
  does the same.

#### D.6.3 Consent cookies

- `euconsent-v2=<cmpString>` and `acconsent=<AC>`; domain `.overwolf.com`,
  path `/`, `Secure`, `SameSite=None`, not `HttpOnly`, expiring 365 days
  after they are written [OBS].
- **The consent page writes them itself**, on every launch, while the startup
  consent window is open, before the first ad document request (in every
  observed launch the cookies changed 0.35 to 0.45 s before the first
  `adview.html` request) [OBS].
- The host does **not** write consent cookies, from Rust or from the ad shim.
  It loads the page in the ads data store, which the guests share (D.8.1).
- **Fallback** (`consent.hostCookieFallback`, default `auto`): if both cookies
  are missing from the ads data store after the startup consent window has
  closed (for example because a WebKit tracking-prevention policy blocked the
  page's write), Rust writes them with exactly the attributes above from the
  stored `cmp` values. `never` disables the fallback [DEC]. The lab check in
  [PARITY.md](PARITY.md#lab-checks) records whether the fallback was needed on
  each platform.

#### D.6.4 Settings window (`openAdPrivacySettingsWindow`, `openCMPWindow`)

`openAdPrivacySettingsWindow()` and `openCMPWindow()` behave identically
[OBS].

- **Window** [OBS]: label `ow-cmp`, title `"CMP"`, 800 x 800 and centred
  unless the options say otherwise (A.2.2), not resizable, not maximizable,
  minimizable, no parent and not modal by default, background `#0D0D0D` (or
  `backgroundColor`). Ads data store, `<UA>`. ow-electron shows it itself
  170 to 500 ms after creating it; ow-tauri shows it once created.
- **Content** [OBS]: first a preloader (a spinner and a close button), then
  the consent page filling the window. ow-tauri shows an equivalent local
  preloader (spinner in `preLoaderSpinnerColor`) until the page has loaded
  [DEC].
- **URL**: `consent.cmpUrl`, else `CmpWindowOptions.cmpURL`, else
  `https://content.overwolf.com/monsdk/electron/latest/cmp/22.3.27/cmp.html`
  (`__settings__.adsSetting.cmpSettingUrl`) [OBS], with the query
  `uid=<uid>&appName=<PN>&tabName=<tab>&lang=<language>&firstRun=<b>&cmpRequired=<b>&muid=<muid>&muidv2=<muidV2>&oweVersion=<owVersion>&appVersion=<ver>`
  in that order [OBS]. `tabName` is `options.tab` (default `purposes`),
  `lang` is `options.language` (default `en`). `appName` is inserted without
  URL encoding, so a space reaches the wire as `%20` [OBS]. `firstRun` and
  `cmpRequired` were `true` in every observed run (fresh profiles, consent
  always required); **Unknown (R3-5)** on a later launch; interim `firstRun`
  is `true` on the first launch of the app and `false` after,
  `cmpRequired` is the `isCMPRequired()` result [INF].
- **Promise** [OBS]: resolves once the window has been created (54 to 308 ms
  after the call), not when it closes (OQ-26). A second call while the window
  is open focuses it and resolves at once; no second window is created.
- **Close** [OBS]: closing writes nothing and sends no host analytics (no
  `window_closed`, E.2). The page fetches
  `https://features.overwolf.com/get-supported-consent` itself.
- **Default-consent window** [OBS]: the first call of a launch also opens a
  hidden 1 x 32 window, `ow-cmp-default`, on
  `https://content.overwolf.com/monsdk/electron/latest/cmp/22.3.27/ow-cmp-v2.html?unifiedcmp=&firstRun=true`
  (always an empty `unifiedcmp`). That page generates a **new default consent
  string** and saves it, overwriting `cmp` in `ow-electron.json` and both
  consent cookies, and sending `consent` messages to running guests (D.5).
  ow-tauri copies this [DEC: replicate]; Overwolf is asked whether it is
  intended (OQ-38). The window follows the startup window's rules (D.6.1:
  never shown, closes itself, `consent.readyTimeoutMs`).
- `cmpURL` / `consent.cmpUrl` accept any `https:` URL, as the typings allow
  [TYPES]. The consent globals (D.6.6) and the `cmp_event` command are granted
  only to pages under `https://content.overwolf.com/monsdk/electron/`
  ([ADR 0011](adr/0011-remote-guest-ipc.md)); a page elsewhere opens, cannot
  save consent, and a warning is logged [DEC].
- Navigation is limited to Overwolf hosts.

#### D.6.5 Ads and consent sequencing

ow-electron attaches guests immediately; in every observed launch the ad
document requests went out after the consent cookies existed [OBS]. ow-tauri
makes that ordering deterministic [DEC]:

1. Each guest's first navigation waits until the startup consent window has
   closed, or until 3 s have passed since the guest was mounted, whichever
   comes first. (The window itself waits for the `cmp-eu-only` response,
   D.6.1, so the 3 s bound is measured from the mount.)
2. If the startup consent window fails to load, guests navigate at once.

`adview_mount` itself never waits and never fails because of consent.

#### D.6.6 Consent page globals and storage

`cmp.js` defines, in both consent windows and only under the scope above,
frozen functions with `length` 0; the page gets no `window.overwolf` [OBS]:

| Global | Behaviour |
|---|---|
| `window.cmp.saveConsent(value)` | `cmp_event saveConsent { consent }` (a TCData object is reduced to its `tcString`; `""` is sent and clears the value, D.6.2; any other value without a consent string is dropped); Rust stores `cmp.cmpString` and `cmp.timeStamp` = now in seconds (F.2) |
| `window.cmp.saveUnifiedConsent(value)` | `cmp_event saveUnifiedConsent { consent }`; Rust stores `cmp.unifiedConsentString`, URL-encoded (F.2) |
| `window.privacy.enableAdOptimization(enabled)` | `cmp_event enableAdOptimization { enabled }`; stored as `adOptimization` in `ow-tauri.json`; returns a resolved promise |
| `window.privacy.getIsAdOptimizationEnabled()` | resolves the stored value; before one is stored, `true` on Windows and `false` elsewhere (ow-electron answered `true` on Windows [OBS: Windows lab] and `false` on macOS [OBS]; the same value as `app.overwolf.enableAdsOptimization`, which ow-electron has at module load, before any request); whether `enableAdOptimization(true)` changes ow-electron's answer: **Unknown (R3-8)** |
| `window.close()` | `cmp_event close`; Rust closes the window |

The argument shapes of `saveConsent` and `saveUnifiedConsent` come from the
reference implementation [POC]; the Tauri lab logs the first real call and
this table is corrected if they differ. Rust validates each string, writes
the state file atomically, sends the matching `consent` message to every
existing guest (D.5) [OBS], and does not recreate guests.

### D.7 Sizes, test mode, clicks and recovery

- **Sizes:** see B.3.2. The guest webview is created at the element's rect;
  `slotSize` tells the page what to request.
- **Test mode** (`--test-ad`, `OW_TAURI_TEST_AD=1`, `ads.testAd`, or
  `Builder::test_ad(true)`): `testAd: true`; `unit` and every other
  attribute pass through unchanged (D.2). Otherwise the host runs live, as ow-electron does without `--test-ad`
  ([ADR 0005](adr/0005-ads-test-live-parity.md)). The wire shaping is the same
  in both modes; only `testAd` and the demand the ad page picks differ [OBS].
  Live mode does not touch the guest's `localStorage`, so the documented
  `owAdTestAd` switch keeps working.
- **New windows** [DEC, Low]: always denied in the guest. The URL, if `http`
  or `https`, opens in the system browser and the guest receives
  `ad-clicked`; the element receives a host `ad-clicked` (B.3.5).
- **Navigation** [DEC, Low; OQ-17]: top-level navigation to a non-Overwolf
  host is cancelled. If a gesture was reported in the guest within
  `ads.gestureWindowMs`, the URL opens in the system browser (treated as a
  click); otherwise it is dropped and logged.
- **External-open limits.** Every open in the system browser from a guest
  (popup or navigation) needs a gesture: on Windows, WebView2's
  `NewWindowRequested.IsUserInitiated` for popups; elsewhere, and for
  navigations, a `__host:gesture` in the window above. One gesture allows one
  open. On top of that, a guest may open at most
  `ads.guestLimits.externalOpensPerMinute` URLs per minute (default 20, so
  Overwolf's ad QA step of clicking one ad five times in a row passes with
  room to spare; the gesture rule is the real safeguard); only
  `http` and `https` URLs without credentials are opened. Everything else is
  dropped and logged.
- **Mute:** guests start muted [DOC]; `setAudioMuted` changes it.
- **Crash recovery** [OBS] (OQ-28): a crashed guest is reloaded at once, in
  the same guest, with no cap (six crashes in a row all recovered). The
  element receives `render-process-gone` (B.3.5); the crash is reported
  (E.2 #8). `ads.maxRecoveries` (default `null`, no cap) is an ow-tauri
  option: with a number, the guest is closed after that many recoveries per
  element. Guest crashes never reach the app's own crash handling [DOC].
- **Load errors** [OBS]: a failed **main-frame** load is reloaded every
  `ads.loadErrorRetryMs` (5000 ms), with no cap, no backoff and no analytics;
  a failed sub-frame load (`-3`, aborted) reloads nothing. Platforms that
  report main-frame load failures: Windows, Linux; macOS partial.

### D.8 Request shaping and guest configuration

#### D.8.1 Guest webview configuration

| Setting | ow-electron [OBS] | ow-tauri |
|---|---|---|
| data store | the default session, shared with app windows (host requests use none of its cookies, E.1) | the **ads data store**, shared by `ow-cmp-startup`, `ow-cmp` and every ad guest; per platform in A.1.1 |
| web security | disabled (the guest logs Electron's "Disabled webSecurity" warning) | Windows: `--disable-web-security` in the ads environment (A.1.1); Linux: `WebKitSettings` `enable-web-security` off (WebKitGTK 2.40 or newer [INF]); macOS: no public API, so it stays on (a known gap, D.8.3) |
| insecure content | allowed (`allowRunningInsecureContent`) | Windows: `--allow-running-insecure-content`; macOS and Linux: platform default |
| user agent | the app UA | `<UA>` (E.1) |
| `window.gc` | a function | D.3 |
| IPC to the app | none | none: exactly one command, `adview_event` |
| audio | muted at start | muted at start |

Web security is off only in the ads environment, whose webviews run no app
code and hold no capability beyond their one command
([ARCHITECTURE section 5.3](ARCHITECTURE.md#53-remote-content-rules)).

#### D.8.2 Wire shape (Q05: replicate exactly)

**Document request** [OBS]: `GET https://www.overwolf.com/monsdk/electron/latest/adview.html`,
with no query string, headers in this order:

```
upgrade-insecure-requests: 1
user-agent: <UA>
accept: text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7
sec-fetch-site: none
sec-fetch-mode: navigate
sec-fetch-user: ?1
sec-fetch-dest: document
referer: https://www.overwolf.com/<uid>
accept-encoding: gzip, deflate, br, zstd
accept-language: <locale>
cookie: <ads data store cookies, including euconsent-v2 and acconsent>
origin: https://www.overwolf.com
priority: u=0, i
```

The guest's `document.referrer` is `https://www.overwolf.com/<uid>` [OBS].
Header order inside HTTP/2 and HTTP/3 frames is best effort.

**Subresources** from the guest, in any frame and from any initiator
(including third-party frames), in `cors`, `no-cors` and `navigate` mode: the
header `Origin: https://www.overwolf.com` is set, replacing any existing
value [OBS]. Exceptions, which carry **no** `Origin` [OBS]:

- the ad library
  `https://content.overwolf.com/libs/ads/latest/owads.min.js?uid=<uid>&phase=<phasePercent>&window=<windowName>`,
  which instead gets `x-ow-uid: <uid>`, `x-ow-phase: <phasePercent>` and
  `x-ow-window: <windowName>`, appended last; its Referer is the normal
  `https://www.overwolf.com/`;
- keepalive beacons sent after unload (no special handling: the platform does
  not let a host change them).

In ow-electron the app's own request hooks cannot remove this shaping [OBS];
in ow-tauri apps get no hook into guest requests at all. `ads.requestShaping`
is on by default and exists only to switch shaping off while debugging.

#### D.8.3 Per platform

| Platform | Document | Subresource `Origin` | `x-ow-*` on `owads.min.js` |
|---|---|---|---|
| Windows (WebView2) | `WebResourceRequested` handler (filter `*`, all resource contexts) on the guest sets `Referer` and `Origin` on the main-frame document request; alternatively the first navigation uses `NavigateWithWebResourceRequest` with those headers | same handler sets `Origin` on every other request | same handler appends the three headers |
| macOS (WKWebView) | the first navigation is `WKWebView.load(URLRequest)` with `Referer` and `Origin`; cookies come from the data store | **gap**: no public API | **gap**: no public API; uid, phase and window still reach the server in the query string |
| Linux (WebKitGTK) | `webkit_web_view_load_request` with a `WebKitURIRequest` carrying `Referer` and `Origin` | **gap until built**: needs a web-process extension (`WebKitWebPage::send-request`), deferred | same: deferred |

Notes:

- **Windows:** WebView2 sends the changed `Referer` and `Origin` of the
  document, the `Origin` of subresources and the ad library's `x-ow-*` headers
  on the wire, with the same values as ow-electron [OBS: Windows lab, each
  guest request recorded as sent through the DevTools protocol]. **Gap:**
  WebView2 lets the host change a request once, not each redirect hop; after
  a cross-origin redirect Chromium sends `Origin: null` on the following hops
  (cookie-sync pixels, about 1 to 2 % of a guest's requests), where
  ow-electron sets `https://www.overwolf.com` again [OBS: Windows lab]. WebView2 takes `document.referrer`
  from the navigation's initiator, not from the `Referer` header, so a host
  navigation leaves it empty even with the header set [OBS: Windows lab].
  The plugin passes the referrer to the guest shim (configuration key
  `documentReferrer`, Windows only; not part of `__overwolf__`), which
  answers `document.referrer` with it while the platform's own value is
  empty. With web security off (D.8.1) the
  forced `Origin` does not break CORS, as in ow-electron.
- **macOS gap.** The subresource `Origin` and the `x-ow-*` headers cannot be
  set with public WebKit API, and web security cannot be turned off. The gap
  is reported to Overwolf (OQ-05). The reference implementation loaded the ad
  page and filled test ads without them [POC]; the macOS lab check compares
  fill (test and at most 10 live loads) against ow-electron's live baseline
  ([PARITY.md](PARITY.md#lab-checks)). A lab check also confirms that WebKit
  keeps the custom `Origin` on the document request [INF]. ow-tauri uses no
  private API by default; a lane may prototype WebKit's private
  per-navigation header fields behind `ads.macPrivateHeaderApi` (off) and
  report the result.
- **Linux:** ads are supported on Linux as Overwolf documents (OQ-30);
  subresource shaping waits for the web-process extension.

The consent windows get no shaping: ow-electron does not modify their
requests [OBS].

---

## E. Analytics

Anonymous app analytics are on by default in ow-electron and reduced to a
mandatory minimum by `disableAnonymousAnalytics()` [DOC]. ow-tauri sends the
same requests, in the same order, with the same fields, as ow-electron
42.11.4 [OBS]; the host label replaces "electron" (section 0,
[ADR 0006](adr/0006-analytics-labelling.md)). Every host request is made from
Rust (`reqwest`) [DEC].

### E.1 User agent and request shape

**User agent** (`<UA>`) [DEC from OBS]. ow-electron uses one UA for host
requests, ad guests and consent windows:
`Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) <PNNS>/<ver> Chrome/148.0.7778.280 Electron/42.11.4 Safari/537.36`
[OBS]. ow-tauri composes `<UA>` from the platform webview's default UA, which
the main webview's bootstrap reports once at its first `ipc_subscribe`,
before any app code runs:

- If it contains a ` Chrome/<x>` token (WebView2): insert `<PNNS>/<ver> `
  immediately before `Chrome/`, and ` <Label>/<hostVersion>` (section 0,
  `Tauri/2.12.1`) immediately after the `Chrome/<x>` token. This is where
  Electron places its tokens.
- Otherwise (WKWebView, WebKitGTK): append ` <PNNS>/<ver> <Label>/<hostVersion>`.
  The WKWebView default carries no browser product tokens, and ad stacks
  rate such a UA as an unknown browser. When the default has an
  `AppleWebKit/<w>` token but no `Safari/` token and the installed Safari's
  version is known, Safari's own tokens are added around the labels the way
  Electron keeps Chromium's:
  ` <PNNS>/<ver> Version/<safari major.minor> <Label>/<hostVersion> Safari/<w>`
  [DEC].
- The engine part is never faked: no `Chrome/` token is added to a WebKit UA.
  Ad-quality scripts check that the UA matches the engine, and Overwolf's ad
  policy forbids invalid traffic [DOC].
- The same `<UA>` is used for every host request, every ad guest and both
  consent windows. App windows keep the platform default.

**Host request headers** [OBS], in this order (HTTP/2 pseudo-headers come
first and are set by the HTTP stack):

```
[content-length: N]               (POST only)
[content-type: application/json]  (POST only)
[cache-control: no-cache]         (the cmp-eu-only request only, D.6.2)
sec-fetch-site: none
sec-fetch-mode: no-cors
sec-fetch-dest: empty
user-agent: <UA>
accept-encoding: gzip, deflate, br, zstd
accept-language: <locale>
priority: u=4, i
```

- **No cookies.** ow-electron's host requests send no cookie and store none:
  its net log shows every cookie of the session excluded by the request's
  credentials mode, and no `Set-Cookie` stored [OBS]. ow-tauri's client has
  no cookie jar, sends no `cookie` header and ignores `Set-Cookie`.
- **No `accept`.** No `accept` header is sent [OBS]; the client adds none.
- `accept-language` is the app locale in Chromium's format (`en-US`) [OBS],
  from `sys-locale` converted to BCP 47 [DEC].
- No `Origin`, no `Referer` [OBS].
- `accept-encoding` is honoured: the client decodes gzip, deflate, brotli and
  zstd.
- HTTP/2 negotiated by ALPN, as observed; header order inside HTTP/2 frames
  is best effort.
- An idle connection is kept until the server ends it: ow-electron sends the
  close counter on its existing session after 90 s or more of silence [OBS],
  so the client sets no pool idle timeout [DEC].
- One attempt, 30 s timeout (none for `cmp-eu-only`, D.6.2), no retry, no
  queue, no persistence; failures are logged at debug level [POC, OBS: no
  retries seen].

**Counter** [OBS]:
`GET https://analyticsnew.overwolf.com/analytics/Counter?Name=<event>&MUID=<muid>&MUIDV2=<muidV2>&owver=<owVersion>&Extra=<json>`

- Query keys exactly in the order `Name, MUID, MUIDV2, owver, Extra`, encoded
  like `URLSearchParams` (space becomes `+`, JSON punctuation is
  percent-encoded). No body. The response is `200 {}`.
- `Extra` is a compact JSON object with keys in this order: `app_ver`
  (`<ver>`), `app_id` (`<uid>`), `os` (`darwin`, `win32` or `linux`), `os_ver`
  (what Node's `os.release()` returns: the Darwin kernel release such as
  `25.5.0` on macOS, `10.0.<build>` on Windows, `uname -r` on Linux),
  `app_name` (`<PN>`, spaces kept), `app_cuid` (`<cuid>`), then the event
  fields in the order E.2 lists them.
- There are no host fields.

**InsertStats** [OBS]:
`POST https://tracking.overwolf.com/tracking/InsertStats?Stats=true&owver=<owVersion with "." replaced by "_">`,
`content-type: application/json`.

- Body, compact JSON in this key order:
  `{"Kind":<n>,"Extra":"<app_ver>.<uid>.<os>.<PN>.<cuid>"}`.
- In each of the five values, `.` and `:` are replaced by `_`; spaces are
  kept. Example: `"1_0_0.<uid>.darwin.Parity Harness.<uid>"`.
- No event fields are added for Kinds 400022, 400023 and 400025; the
  heartbeat's `hasVisibleWindow` is not included [OBS].
- Kind 400024 (guest crash) puts the `reason` first:
  `"Extra":"<reason>.<app_ver>.<uid>.<os>.<PN>.<cuid>"`, for example
  `"killed.1_0_0.<uid>.darwin.<PN>.<uid>"` [OBS].

### E.2 Events and order

| # | Trigger | Counter `Name` (event fields, in order) | InsertStats Kind | With `disableAnonymousAnalytics()` | Source |
|---|---|---|---|---|---|
| 1 | launch, `firstLaunch` absent from `ow-electron.json`; then `firstLaunch: true` is written | `<label>_app_first_launch` | 400022 | Counter **kept**, Kind dropped | [OBS] |
| 2 | launch, in parallel with #1 | `GET https://features.overwolf.com/experiments/cmp-eu-only` (D.6.2); the startup consent window opens when it completes (D.6.1) | none | kept | [OBS] |
| 3 | every launch | `<label>_app_start` | none | dropped | [OBS] |
| 4 | every launch | `<label>_app_heartbeat` (`hasVisibleWindow`: `false`) | 400023 | kept (both) | [OBS] |
| 5 | the first app window becomes visible | `<label>_app_heartbeat` (`hasVisibleWindow`: `true`) | 400023 | kept (both) | [OBS] |
| 6 | each ad guest attaches (one per `<owadview>`, test and live) | none | 400025 | dropped | [OBS] |
| 7 | a visible period of a window ends: `hide()`, close, or quit while it is visible | `<label>_window_closed` (`name`, `title`, `length`) | none | dropped | [OBS] |
| 8 | an ad guest crashes, unless it crashed shortly after its previous recovery (below) | `<label>_owadview_crashed` (`sessionTS`, `reason`) | 400024 | dropped [INF] | [OBS] |
| 9 | periodic heartbeat: an hourly check that sends when 12 h have passed since the last heartbeat of the session | as #4, `hasVisibleWindow` = current | 400023 | kept | [OBS] (R2-9, below); the hourly check is [DEC] |
| 10 | `setExternalPaymentUserId(options)` | `<label>_sub_info` (below) | none | kept [DEC]; **Unknown (R3-3)** | [OBS] |

**Order and timing.** In ow-electron #1, #2, #3, the #4 Counter, 400022 and
400023 go out in that order within about 100 ms of Electron's `ready`; #5
follows when the first window is shown, 1 to 3 s later [OBS]. In ow-tauri,
#2 and the analytics sequence (#1, #3, #4, 400022, 400023) start together at
`main_ready` (A.2.1), which is the point where the app's top-level code has
run, as Electron's `ready` is for an ow-electron app; the startup consent
window follows #2's response (D.6.1). Starting #2 earlier, at
`RunEvent::Ready`, put it 90 to 280 ms ahead of the rest of the burst on
Windows, where the main page loads after the webview exists [OBS: Windows
lab]. This keeps
`disableAnonymousAnalytics()` called at module load effective, as in
ow-electron. If `main_ready` never arrives, the sequence starts after 10 s
with a warning, and #2 with it. #6 leaves when the guest's webview exists:
ow-electron creates the guests mounted together within about 100 ms, while
on Windows WebView2 creates them on the main thread one after another
(80 to 150 ms each), so their 400025 reports spread over that time [OBS:
Windows lab].

**Periodic heartbeat (#9)** [OBS: R2-9, a 13-hour session of ow-electron
42.11.4 whose window was never shown]. After the launch burst (#1 to #4,
400022, 400023) and the startup consent page's own request at 1.4 s, the
session sent nothing for 12 hours. At 43 200.2 s it sent one
`<label>_app_heartbeat` Counter (`hasVisibleWindow`: `false`) and 54 ms
later one 400023; nothing followed in the remaining hour, and quitting a
session whose window was never shown sends no `<label>_window_closed`
(#7 needs a visible period). ow-electron's 12-hour Counter went out as a
conditional request (`if-none-match` / `if-modified-since`, answered 304)
because Chromium's HTTP cache held the URL; ow-tauri has no HTTP cache for
host requests and sends the plain request, which reaches the same server
(PARITY, optimised). ow-tauri sends #9 from an hourly check once 12 hours
have passed since the session's last heartbeat, which matches this run. Not
settled by it: whether ow-electron uses a 12-hour timer or an hourly check
with a 12-hour threshold (the two differ by at most an hour), and whether
the first-show heartbeat (#5) restarts the 12 hours (no window was shown).

**Second launch:** #1 and 400022 are not sent; everything else is unchanged
[OBS].

**#7 fields** [OBS]:

- `name`: the window's analytics name, fixed when the window is first
  shown and never changed by later navigations. The `BrowserWindow` `name`
  option is **ignored** [OBS]. The name is the last segment of the path of the
  URL loaded at that moment, decoded, without query or fragment; a trailing
  `.html` or `.htm` (any case) is removed and other extensions are kept
  (`page.php`, `a.b`); whitespace and characters such as `<` and `>` are
  removed; non-ASCII letters, `-`, `_` and `.` are kept; there is no
  truncation (40 characters were kept) [OBS]. An empty path gives the host
  name (`https://example.com/` reports `example.com`), `about:blank` reports
  `blank`, `data:text/html,<title>d</title>` reports `title`, and `index.html`
  reports `index` [OBS]. (Overwolf's window-names page describes a 20-character
  limit [DOC]; ow-electron 42.11.4 does not apply it.)
- `title`: the `title` **constructor option**, else `<PN>`; an explicit empty
  title stays `""` [OBS]. Not the current page title.
- `length`: whole seconds the window was visible, rounded down (1.5 s gives
  `1`; 90 and 360 observed) [OBS].
- Trigger [OBS]: each visible period ends with one event. `hide()` sends it;
  a later `close()` does not send it again; showing the window again and
  closing it sends another with the new length; quitting while the window is
  visible sends it. A minimize ends the visible period like `hide()`, and the
  restore starts none: quitting after it sends nothing [OBS]. ow-tauri starts
  a new period at the next `show()` after a `hide()` [DEC]. A visible period shorter than 1 s sends nothing.
- Windows that were never shown send nothing (including `ow-main`, the
  startup consent window and the default-consent window). Ad guests are not
  windows. The consent settings window (`ow-cmp`) sends nothing either,
  although it is shown [OBS].
- #5 is sent once per run, for the first window shown [OBS].

**#8 fields** [OBS]: `sessionTS` is the whole seconds since the guest's last
load or recovery; `reason` is the platform's termination reason (`"killed"`
observed; ow-tauri maps WebView2 `ProcessFailedKind` / WebKit termination to
Electron's `render-process-gone` reasons [DEC]). Crashes 20 to 30 s apart
were each reported; crashes 2 s after the previous recovery were recovered
but **not reported** [OBS]. The threshold lies between 3 and 20 s:
**Unknown (R3-4)**; interim: no report when `sessionTS < 10` [DEC].

**#10 fields** [OBS]: the Counter only, no InsertStats. `Extra` holds the
usual six fields (E.1), then the options object's own keys in the order the
app passed them, then `providerName: "tebex"` when the options had no
`providerName`. Examples: `{..., providerName, userId}` for
`{ providerName: 'tebex', userId }`; `{..., userId, paymentId, providerName: "tebex"}`
for `{ userId, paymentId }`. The call resolves after the response (250 to
374 ms observed).

Not sent: events tied to the Windows ad-optimisation helper, which ow-tauri
does not ship (OQ-14). Email hash calls send no request [OBS]. No other host
requests exist.

### E.3 Opt-outs and switches

| Switch | Effect |
|---|---|
| `disableAnonymousAnalytics()` before `main_ready` | only the mandatory set for the session: #1 Counter, #2, #4 and #5 (Counter and 400023), #9 [OBS], and #10 [DEC; **Unknown (R3-3)**]. Dropped: #3, 400022, #6, #7, #8 |
| `disableAnonymousAnalytics()` after `main_ready` | the mandatory set from then on; a warning is logged |
| `analytics_set_user_enabled(false)` (**ow-tauri option**, `analytics.userSwitch`) | nothing at all, persisted; stricter than ow-electron |
| test builds | `Builder::analytics_transport` replaces the HTTP client; the default transport refuses non-loopback hosts under `cfg(test)` |

The ad and consent pages keep sending their own analytics in every case
(E.5) [OBS].

### E.4 Machine id (`muid`, `muidV2`, `phasePercent`)

`analytics.muidStrategy`: `machine-id` (default, parity) or `per-install`
(**ow-tauri option**, non-parity: a random upper-case UUID v4 stored as `muid`
in `ow-tauri.json`, with `muidV2 = muid`). The machine-id derivation per OS
([ADR 0014](adr/0014-machine-id-parity.md)):

**macOS** [OBS]:

```
id     = IOPlatformUUID of IOPlatformExpertDevice, read through IOKit (not by running ioreg)
h      = lowercase hex of sha256(lowercase(id))
muid   = h[0..8] + "-" + h[8..12] + "-" + h[12..16] + "-" + h[16..20] + "-" + h[20..32]
muidV2 = muid
```

The result is lower-case, with no UUID version or variant bits forced.

**Windows** [BUILDER] for the registry, [INF] for the derivation:

1. Read `HKCU\Software\OverwolfElectron` value `MUID` and
   `HKCU\Software\OverwolfPersist` value `MUIDV2`. Overwolf's uninstaller
   reads both to tag its uninstall event [BUILDER]. When present, use them as
   they are, so ow-tauri shares the ids of any ow-electron app on the machine.
2. Otherwise derive a missing `muid` with the macOS formula from
   `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid`, create a missing
   `muidV2` as a lower-case random UUID v4 (as ow-electron does, item 3),
   and write both registry values, so the installer's uninstall event works
   for Tauri installs (I.6).
3. **Observed (Windows lab, R2-10):** `MUIDV2` is a separate per-install
   id, a lower-case random UUID v4 that ow-electron creates on a machine
   without one; its analytics carry `MUID` (the machine id) and `MUIDV2`,
   the ad guests and consent pages get `muid` = `MUID`, and
   `app.overwolf.muid` answers `MUIDV2`. **Unknown:** whether `MachineGuid`
   is ow-electron's `MUID` source.

**Linux** [INF]: the macOS formula over `/etc/machine-id`, else
`/var/lib/dbus/machine-id`, trimmed; `muidV2 = muid`. **Unknown (R2-10).**

**`phasePercent`** [OBS]: the sum of the character codes of the lower-case
hex MD5 of `muid` with the `-` characters removed, modulo 100.

Test vectors (stand-in platform UUIDs, observed against ow-electron) [OBS]:

| `IOPlatformUUID` | `muid` (= `muidV2`) | `phasePercent` |
|---|---|---|
| `2D59BF70-9641-826A-F003-C362834EC045` | `5bd79133-f3bf-be27-e448-a4581ab5f3cd` | 80 |
| `DA3889E5-CB8A-8A15-CD1B-DCE6B5A71203` | `601860a3-90c7-b77b-a42e-636035921a81` | 51 |
| `D668AFF2-C8FD-39A3-6B92-D57DED8E5461` | `5d841b98-54cb-5f57-73bc-297706f34221` | 62 |
| `58468E7A-3E77-8816-5D62-7371174B102C` | `cbec68c3-9e97-b465-f479-f8493f973f32` | 24 |

`muid` is a string in `app.overwolf`, in the guest's `__overwolf__`, and in
the consent page's `localStorage` (`muid`, `muidv2`) [OBS]. Overwolf keys
unique users, installs and staged package rollouts on the machine id [DOC].

### E.5 Analytics that Overwolf's pages send

The consent page sends `electron_cmp_accept_full_launch` (D.6.1) and the ad
page sends `owads_*`, `oam_*` and Kind 400051, with `ClientVer` /
`CurrentVersion` derived from `oweVersion` / `owVersion` [OBS]. ow-tauri
gets these by loading the same pages with the same inputs and never renames
or filters them.

---

## F. Per-app state file

### F.1 Location

`<appData>` is the OS configuration directory: `%APPDATA%` on Windows,
`~/Library/Application Support` on macOS, `$XDG_CONFIG_HOME` or `~/.config`
on Linux ([ADR 0007](adr/0007-state-file-continuity.md)).

| Path | Owner | Content |
|---|---|---|
| `<appData>/ow-electron/<uid>/ow-electron.json` | shared with ow-electron | F.2; the only file ow-electron writes in that directory [OBS] |
| `<appData>/ow-electron/<uid>/ow-tauri.json` | ow-tauri | F.3 |
| `<appData>/ow-electron/<uid>/logs/ow-tauri.log` | ow-tauri, only with `logging.enabled` | F.4 |
| `<appData>/<PN>/` | the app's userData | `app.getPath('userData')` (B.2.1). ow-electron keeps its Chromium profile there [OBS]; ow-tauri uses it as the app environment's webview data directory (A.1.1). Cookies and web storage cannot migrate between engines |

### F.2 `ow-electron.json`

Exact shape and encoding, as ow-electron writes it [OBS]:

```json
{"firstLaunch":true,"cmp":{"cmpString":"<TCF v2 string>","timeStamp":1791302123,"unifiedConsentString":"cmp%3D<tcf>%26ac%3D<ac>"}}
```

| Key | Meaning and encoding |
|---|---|
| `firstLaunch` | `true` means "the first launch was already reported" (E.2 #1). Written on the first launch, never reset |
| `cmp.cmpString` | the TCF v2 string from `saveConsent` (D.6.6) |
| `cmp.timeStamp` | **Unix seconds**, refreshed on every launch by the startup consent flow (D.6.1) |
| `cmp.unifiedConsentString` | stored **URL-encoded**: `cmp%3D<tcf>%26ac%3D<ac>` |
| `utmParams` | written by Overwolf's installer; absent for apps installed any other way, and `app.overwolf.utmParams` is then `undefined`, not `null` [OBS] [TYPES] |
| `eHashes` | `{ sha1, md5, sha256 }`: the last email hashes the app set (`setUserEmailHashes()` or `generateUserEmailHashes()`, A.2.2), written after `cmp` and replaced by every later call; absent until the first call [OBS]. Not written after `disableAdsFPD()` |

Rules:

- Compact JSON, keys in the order above. ow-tauri reads `firstLaunch`, `cmp.*`
  and `utmParams`, and writes `firstLaunch`, `cmp.*` and `eHashes`. It never writes
  `utmParams`, never removes keys, and preserves unknown keys and their
  values.
- Writes are read-modify-write under an in-process lock, written to a temp file
  in the same directory and renamed over the original. If the existing file is
  not valid JSON it is left untouched, a warning is logged and ow-tauri keeps
  its values in memory for the session.

### F.3 `ow-tauri.json`

```jsonc
{
  "schema": 1,
  "stagingId": "3f2a...",                 // updater staging bucket (I.2)
  "adOptimization": true,                 // consent page toggle (D.6.6)
  "pendingBrowserArgs": [],               // A.1.1
  "analyticsUserEnabled": true,           // only with analytics.userSwitch (ow-tauri option)
  "muid": "8C7E...-...",                  // only with analytics.muidStrategy "per-install" (ow-tauri option)
  "mainCrashes": [1791302123456],         // ow-main crash times, Unix ms, within the last 60 s (A.6)
  "createdBy": "ow-tauri 0.1.0"
}
```

The file is parsed field by field: a known field with an unexpected type is
kept as it is (and written back unchanged) instead of failing the whole file,
and unknown keys are preserved. A file that is not valid JSON is renamed to
`ow-tauri.json.corrupt-<Unix ms>` and the state starts from defaults, with a
warning. Unknown `schema` values newer than the running version are read
best-effort and never downgraded on write. Package channel choices are not stored while
no package runtime exists (H).

### F.4 Logs

ow-electron writes no log: `__settings__.logger.enabled` is `false` and no
`logs` directory is created [OBS]. ow-tauri logging is **off by default**
[DEC]. With `logging.enabled: true`, ow-tauri appends to
`<appData>/ow-electron/<uid>/logs/ow-tauri.log`, one line per entry:
`[YYYY-MM-DD HH:MM:SS.mmm] [<level>] <message>` in local time. Each session
starts with `ow-tauri <version> session start - app '<PN>' <ver> - uid <uid> - pid <pid>`.
The file rolls at 5 MiB, keeping 3 files. The `log` command (A.2.1) is
dropped while logging is off.

`packages.logsFolderPath` is the literal string ow-electron reports,
`<userData>` + `/..\ow-electron/` + `<uid>` + `/logs`, with that backslash
on every OS [OBS]. It is copied as is (a Low item, copied from ow-electron)
[DEC]; nothing is created at that path.

### F.5 Migration from ow-electron

| Data | Result for a user who ran the ow-electron build of the same app |
|---|---|
| uid | identical when `productName` (else `name`) and `author` are unchanged (G.2) |
| muid | identical: same machine-id derivation (E.4); on Windows the registry values are shared |
| consent | kept: the same `cmp` block is read, and the startup consent window reuses it (D.6.1). The `.overwolf.com` cookies live in the browser profile and are rewritten by the consent page on the first ow-tauri launch |
| first launch | not re-sent: `firstLaunch` is already set |
| UTM parameters | kept |
| app prefs in `userData` | kept: `app.getPath('userData')` is the same directory (B.2.1) |
| package channels | not applicable while no package runtime exists (H) |

---

## G. Manifest

### G.1 Fields

`package.json` stays the single manifest. The app's `build.rs` embeds it
(G.3); nothing reads it from disk at runtime.

| Field | ow-electron use | ow-tauri |
|---|---|---|
| `name` | app name fallback | `app.name` fallback; uid input when `productName` is absent |
| `productName` (top level) | app name; uid input [DOC] [OBS] | `<PN>` = `productName` if it is a non-empty string, else `name`. Used for `app.getName()`, analytics `app_name`, the guest `name`, the userData directory, the UA token and the default window title [OBS]. Checked against `tauri.conf.json` `productName` (mismatch = build warning) |
| `build.productName` | electron-builder only; the builder strips `build` from the packaged `package.json` [BUILDER] | **ignored**, as ow-electron ignores it at runtime [OBS] |
| `author` (string or `{ name }`) | uid input | same rule (G.2); a missing author is a build warning, because Overwolf's CLI requires one |
| `version` | app version | `<ver>`: `app.getVersion()`, analytics `app_ver`, guest `version`; checked against `tauri.conf.json` `version` |
| `main` | Electron entry, hashed for signing [BUILDER] | not an entry point (the main webview entry is `plugins.overwolf.main.url`); used as the `fileHashes` key when signing (G.4) |
| `overwolf.packages` | which packages load | recorded and reported in `PackagesSnapshot.listed`; no package loads while no runtime exists (H). Accepted names: `gep`, `overlay`, `recorder`, `utility`, `crn`; other strings are kept as they are |
| `overwolf.uid` | written by Overwolf's signing server into the signed `package.json` [BUILDER]; used verbatim [OBS] | honoured as the uid (G.2 rule 2) |
| `build.overwolf.disableAdOptimization` | builder: skip downloading the Windows optimisation helper [BUILDER] | default of the runtime ad-optimisation switch (`settings.disableOptimization`, D.2); nothing is downloaded either way (OQ-14) |
| `build.overwolf.enablePackageBundling` | builder: bundle `.owepk` packages | build warning that it has no effect while no package runtime exists |
| `build.overwolf.overridePackagesUrl` | builder: package list URL | build warning that it has no effect |
| `build.overwolf.requireSigning` | builder: Overwolf signing required; on by default for Windows builds [BUILDER] | the same gating in the signing step (G.4) |
| `build.overwolf.enableOWCertSigning` | builder: Authenticode-sign the app exe with Overwolf's certificate [BUILDER] | the same, through the signing step (G.4) |
| other `build.*` (NSIS, files, asar, ...) | electron-builder | ignored; the Tauri bundler uses `tauri.conf.json` (PORT-MAP) |

### G.2 App uid

Precedence:

1. `plugins.overwolf.uid` (or the `Builder` override) if set.
2. Else `overwolf.uid` from the manifest, used verbatim (console-signed
   builds, G.4) [OBS].
3. Else the computed uid [OBS] [BUILDER: `ow client calc-electron-uid`] [DOC]:

```
name   = productName (top level) if it is a non-empty string, else name
author = typeof author === "string" && author !== ""                       ? author
       : typeof author === "object" && author !== null
         && typeof author.name === "string" && author.name !== ""        ? author.name
       : "unknown"
s      = "{'author':'" + author + "','name':'" + name + ".electron'}"     // UTF-8, no escaping
d      = sha1(s)                                                          // 20 bytes
uid    = for each byte b of d: chr(97 + (b & 15)) + chr(97 + (b >> 4))    // 40 characters a..p
```

- `build.productName` is ignored (G.1).
- A string `author` is used verbatim: no npm-style `Name <email> (url)`
  parsing, nothing trimmed, non-ASCII hashed as UTF-8, quotes not escaped
  [OBS].
- A missing author, `{}`, `""`, `null`, a number, or an object with only
  `email` becomes `"unknown"` [OBS]: that value reproduces both observed
  fallback uids exactly. `author: { name: "" }` was not tested and is treated
  as `"unknown"` [INF].
- `<cuid>` (`app_cuid` in analytics) is always the rule 3 value. With
  `overwolf.uid` set, ow-electron reports the override as `app_id` and the
  computed uid as `app_cuid`, and InsertStats `Extra` is
  `<ver>.<override>.<os>.<PN>.<computed uid>` [OBS]. The rule 1 config
  override behaves the same way [DEC].
- `process.env.OVERWOLF_APP_UID` is the uid from the moment the main module
  loads [OBS] (B.1.1).

A uid from rule 1 or 2 must be 1 to 64 ASCII letters or digits after
trimming whitespace, because it names the state directory (F.1) and must
never contain a path separator or `..` [DEC]. Uids Overwolf assigns and
computed uids satisfy this. A manifest `overwolf.uid` that does not is a
build error (`embed_manifest`, G.3); at runtime an invalid uid from rule 1 or
2 is skipped and the next rule applies.

The `.electron` suffix is kept on purpose: the uid keys the developer console,
the ad configuration and the state directory, so a Tauri build of the same
app must keep it ([ADR 0007](adr/0007-state-file-continuity.md)).

Test vectors. Each is observed in ow-electron 42.11.4 and agrees with
`ow client calc-electron-uid` [OBS]; the crate's unit tests use all of them:

| # | `package.json` | uid |
|---|---|---|
| 1 | `name: "parity-harness"`, `author: { name: "Example Studio" }`; also with `author: "Example Studio"`, or with `build.productName: "Parity Build Name"` added | `binaioonkjpolnojeenpbmjmbfkbmffcekndbmdk` |
| 2 | as 1, plus `productName: "Parity Harness"` | `bijigndkghcikkfmhgkmicdkjpdehpjafgpmdhcc` |
| 3 | `name: "parity-harness"`, `author: "Overwolf Ltd."` | `djaoacjhpjaenfddlfmkeoiklmccgcgcgeknhmgj` |
| 4 | `productName: "Parity Harness"`, `author: "Overwolf Ltd."` | `aejkligdodglhcjinbhdcnlohocenfkpdihjacdg` |
| 5 | `name: "parity-harness"`, `author: "Example Studio <dev@example.com> (https://example.com)"` | `agmekflfehlhfcnofnhghbgohnigngnkddpkdbnc` |
| 6 | as 5, plus `productName: "Parity Harness"` | `cmbaaahkhdbkbbfcmenfbmmngmommpjjacllbgan` |
| 7 | `name: "parity-harness"`, `author: "Example Studio <dev@example.com>"` | `mcfopdapolegaeddgbbfedginnmcnmjldgdcbcjo` |
| 8 | `name: "parity-harness"`, author missing, `{}` or `""` (author `"unknown"`) | `nbhlaphlggihmjefpjdelbobckfhklbfkiicjaja` |
| 9 | `productName: "Parity Harness"`, author missing, `{}` or `""` (author `"unknown"`) | `fifpcfmoobnjlimjhefehejankadpajlfgbmpheo` |
| 10 | `productName: "Pârity Ünicode"`, `author: { name: "Exämple" }` | `mmfoflmmchoacblhjlimpanaijdnhgoalaloihjd` |
| 11 | `productName: "O'Brien Tools"`, `author: { name: "D'Arcy" }` | `khalfglcmeemfnjoldckbfmeeidgkoabeebkpbbl` |
| 12 | `productName: " Parity Harness "`, `author: { name: " Example Studio " }` | `cppaiialckdbmhdojecejpjafcblbingfdiffkdi` |
| 13 | `overwolf: { uid: "aaaabbbbccccddddeeeeffffgggghhhhiiiijjjj" }`, any name and author | `aaaabbbbccccddddeeeeffffgggghhhhiiiijjjj` |

### G.3 Build helper

```rust
// src-tauri/build.rs (as in examples/packages-sample)
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tauri_plugin_overwolf::build::embed_manifest("../package.json")?;
    let dir = std::env::var("CARGO_MANIFEST_DIR")?;
    let dir = Path::new(&dir);
    // The NSIS hooks of I.6; tauri.conf.json points installerHooks here.
    tauri_plugin_overwolf::build::write_nsis_installer_hooks(
        &dir.join("../package.json"),
        None,     // the plugin's `uid` override, if the app sets one
        "tauri",  // analytics.hostLabel
        &dir.join("windows/hooks.nsh"),
    )?;
    tauri_build::try_build(tauri_build::Attributes::new())?;
    Ok(())
}
```

`embed_manifest` parses and validates the fields above, emits
`cargo:rerun-if-changed`, prints `cargo:warning` lines for the conditions in
G.1, and writes `$OUT_DIR/ow-tauri-manifest.json`, which
`embedded_manifest!()` includes. Validation errors (missing `name` and
`productName`, `overwolf.packages` not an array of strings, non-boolean
`build.overwolf` flags) fail the build with the field path.

Build warnings, besides those in the table:

- The app name (`<PN>`) contains `bot` in any case. Overwolf documents that
  app names containing "bot" are refused, because ad partners see the name.
- `author` is missing or empty (the uid then uses `"unknown"`, G.2).
- A removed configuration key is present (A.1).

In debug builds `embed_manifest` also embeds `dev-app-update.yml` when the
file exists next to `package.json`, for `forceDevUpdateConfig` (I.1).

`EmbeddedManifest` (JSON, also in `HostSnapshot.manifest`):

```ts
interface EmbeddedManifest {
  name: string; productName: string; version: string; author: string;   // author after the G.2 rule
  overwolf: { packages: string[]; uid?: string };
  buildOverwolf: { disableAdOptimization: boolean; enablePackageBundling: boolean;
                   overridePackagesUrl?: string; requireSigning: boolean; enableOWCertSigning: boolean };
  raw: Record<string, unknown>;   // the packaged form of package.json (below)
}
```

`productName` here is `<PN>`. `raw` is the packaged form Overwolf's builder
produces [BUILDER]: `package.json` without `build`, `scripts`, `keywords`,
`devDependencies` and keys starting with `_`, with `build.extraMetadata`
merged in.

### G.4 Signing

Overwolf signs the gaming-package integrity and the developer signs the exe;
without both, GEP, overlay and recorder do not load, while unsigned builds
still run, and ads and analytics never depend on signing [DOC]
(https://dev.overwolf.com/ow-electron/guides/dev-tools/app-signing).
ow-tauri reproduces the published builder's flow wherever a Tauri build can
([ADR 0016](adr/0016-signing-approach.md)).

The signing step is `ow-tauri sign`, a Node CLI shipped in the `ow-tauri`
package that the app's build runs before `tauri build` (a `build.rs` hook may
call it through an environment flag). It runs when `OW_CLI_EMAIL`,
`OW_CLI_API_KEY` and `OW_BUILD_KEY` are set. Requests go to `OW_CLI_API_URL`
(default `https://console-be.overwolf.com`) with the headers
`Authorization: Key <email>:<apiKey>` and `x-ow-app-key: <OW_BUILD_KEY>`
[BUILDER].

| Step | What it does | Status |
|---|---|---|
| a | `POST /sign/electron` with `{ packageJson, fileHashes: { [main]: <sha256 hex> } }`. `packageJson` is the packaged form (`raw`, G.3). The `fileHashes` key is `main` when present, else the main-webview bundle's relative path; the value is the SHA-256 of that built file. `electronVersion` is **not** sent. The response `{ zip, integrityDllUrl, isOwCertificateEnabled }` carries the signed `package.json`, whose `overwolf.uid` becomes the uid (G.2 rule 2), and `_metadata.json`, which is embedded in the manifest and shipped as a bundle resource next to the exe | now, every OS |
| b | download `integrity.dll` from `integrityDllUrl` and ship it next to the exe as a Windows bundle resource | now (inert without a package runtime) |
| c | Windows PE resource `OWEINTEGRITY/OWE` = `{"appUid":"<uid>"}`, compiled in `build.rs` with `embed-resource` as its own resource type (no second `VERSIONINFO`) so it exists **before** Authenticode signing | now; MSVC and GNU links to be verified in CI [INF] |
| d | Authenticode with the developer's certificate through Tauri's `bundle.windows.signCommand`. With `enableOWCertSigning` and `isOwCertificateEnabled`, a `signCommand` script posts the **app exe only** to `/sign/electron-certificate` (multipart `file`) and replaces it with the exe from the returned zip, or keeps it on `isAlreadySigned` [BUILDER] | now |
| e | gating: a Windows release build with `requireSigning !== false` (or env `OW_REQUIRE_SIGNING`) whose credentials are missing or whose signing calls fail **fails the build**; elsewhere a warning [BUILDER] (the builder code fails; an older changelog text says it warns) | now |
| f | `/sign/asar` and the `OWEASARSIG` resource | **not done and not faked**: Tauri has no asar. An off-by-default option `signing.assetIntegrity: "none" \| "tauri-assets"` would write a distinct resource `OWEINTEGRITY/OWETAURIASSETS`, only once Overwolf defines a Tauri integrity target (OQ-09) |

Open with Overwolf (OQ-09): whether `/sign/electron` accepts a non-Electron
manifest and whether `electronVersion` is required, what `fileHashes` are
used for at runtime, a Tauri integrity target, and whether `integrity.dll`
has a host-agnostic interface.

---

## H. Packages

Scope cut ([ADR 0004](adr/0004-packages-backend-selection.md)): GEP, overlay,
recorder, utility and CRN are not implemented. Overwolf documents that only
ad services are supported on macOS and Linux and that packages are
Windows-only [DOC]
(https://dev.overwolf.com/ow-electron/guides/dev-tools/non-windows-dev).
ow-tauri behaves, on every OS, exactly as ow-electron 42.11.4 behaves where
packages are not available.

### H.1 Behaviour on every host today

Observed with `overwolf.packages: ["gep", "overlay"]` on macOS [OBS]:

| Member | ow-electron | ow-tauri |
|---|---|---|
| events (`loading`, `ready`, `failed-to-initialize`, `crashed`, `package-update-pending`, `updated`) | none emitted | none emitted |
| `hasPendingUpdates()` | `{ hasPendingUpdate: false, details: [] }`, returned synchronously (not a promise) | same |
| `getChannel(...)` | resolves `{}` | same |
| `getAvailableChannels(name)` | rejects asynchronously with `Error("getAvailableChannels - package 'gep' is not registered in this app")`, even for a listed name | same message with the first name passed |
| `setChannel(name)` | rejects asynchronously with `Error("setChannel - package 'gep' is not registered in this app")` | same, with the name passed |
| `relaunch()` | returns `undefined` | same |
| `app.overwolf.packages.<name>` | `undefined` | same |
| `logsFolderPath`, `phasePercent` | F.4, E.4 | same |

`failed-to-initialize` keeps its public signature `(event, packageName)`
[TYPES] for a future runtime. The `{ reason, version }` third argument and the
reasons `unsupported-host`, `no-native-runtime` and `packages-disabled` of
earlier drafts are removed: ow-electron emits nothing. The simulated backends
of earlier drafts are removed from the scope.

### H.2 `packagesBackend`

| Value | Result |
|---|---|
| `none` (default) | H.1 |
| `native` | reserved for a package runtime that implements Appendix P. No such runtime exists, so it behaves as `none` and logs one warning |

The command line switches and environment variables that a runtime would
consume (`--owepm-package-channel`, `--force-phased-package`,
`--owepm-packages-url`, the dev-mode credentials) are accepted and ignored
(A.1).

---

## I. Updater and distribution

Overwolf distributes ow-electron app updates through an electron-updater
**generic provider** feed per app ([DOC]:
https://dev.overwolf.com/ow-electron/developers-console/releases-management/release-management#setting-up-electron-auto-updates).
ow-tauri's update client reads that feed ([ADR 0008](adr/0008-updater-client.md)).

### I.1 Feed and configuration

**Overwolf's feed** [DOC] [OBS]:

- URL `https://electron-updates.overwolf.com/electron-updates/electron/<console app id>`,
  where the console app id is the uid. A console test channel is the
  `channel` option, read as `<channel>.yml`.
- `latest.yml` has `version`, `files[]: { url, sha512, size, blockMapSize, IsAdminRightsRequired }`,
  `releaseDate` and `releaseName`, and no top-level `path` or `sha512` [OBS].
- Files are served from `https://appsdl.overwolf.com/prod/apps/<id>/<ver>/setup.exe`
  [OBS].
- `latest-mac.yml` and `latest-linux.yml` return 404: the console serves
  Windows setup files only [OBS]. macOS and Linux builds use a self-hosted
  generic feed with the same YAML shape [DEC].
- Whether the console accepts a Tauri NSIS `setup.exe` upload and serves it in
  this feed is **open** (OQ-18). Testing it needs an upload to a console test
  channel, which is a publish action and needs the app owner's explicit
  approval.

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
2. Parsed fields: `version` (semver), `files[]: { url, sha512, size, blockMapSize, isAdminRightsRequired }`,
   `path`, `sha512` (legacy top-level), `releaseDate`, `releaseName`,
   `releaseNotes`, `stagingPercentage`. Field names inside `files[]` are
   matched case-insensitively, because Overwolf's feed spells
   `IsAdminRightsRequired` with a capital `I` [OBS].
3. `blockMapSize` is ignored: updates are always full downloads.
4. Update available when `version > current` (or `!=` with
   `allowDowngrade`); prereleases only with `allowPrerelease`.
5. `stagingPercentage`: available only if
   `stagingBucket < stagingPercentage`, where `stagingBucket` is derived from
   `ow-tauri.json` `stagingId` (0 to 99).
6. File choice per OS: Windows `.exe` (NSIS) else `.msi`; macOS `.zip`
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
| Windows | NSIS `.exe` | run `"<file>" /S /UPDATE` (`updater.installerArgs` overrides; without `isSilent` the `/S` is dropped), then exit. When the feed entry has `IsAdminRightsRequired: true`, the installer is started elevated (`runas`), as electron-updater does for `isAdminRightsRequired` [INF] |
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
(B.1).

### I.6 Installer parity (Tauri NSIS hooks)

Overwolf's NSIS installer does Overwolf work at install and uninstall time
[BUILDER]. A Tauri NSIS installer is "your own installer" in Overwolf's terms
[DOC]; ow-tauri ships `bundle.windows.nsis.installerHooks` that do the same
[DEC]:

- `NSIS_HOOK_POSTINSTALL`: write `SHELL_CONTEXT\Software\OverwolfElectron\<uid>`
  values `InstallLocation`, `version` and `ShortcutName` (`<shortcut>.exe`).
- `NSIS_HOOK_POSTUNINSTALL`, on a real uninstall only, never during an update
  (the hook checks the update flag of Tauri 2.12's NSIS template; Overwolf's
  builder fixed exactly this case, where consent was reset by an update)
  [BUILDER]:
  1. `RMDir /r "$APPDATA\ow-electron\<uid>"`;
  2. delete the `<uid>` registry key;
  3. `GET https://analyticssec.overwolf.com/analytics/Counter?Name=ow_<label>_app_uninstall&MUID=<HKCU\Software\OverwolfElectron MUID>&MUIDV2=<HKCU\Software\OverwolfPersist MUIDV2>&Extra={"app_id":"<uid>","app_version":"<ver>","app_name":"<PN>"}`,
     with the URL shape of the builder's tracking template. This feeds the
     console's "App Uninstalls" widget [DOC].
- The removal of Overwolf's elevated helper is skipped while no package
  runtime exists (commented in the hook).
- The runtime writes the two machine-id registry values (E.4), so step 3
  works for Tauri installs.

---

## Appendix P. Deferred design: package runtime interface

> **Deferred design, not binding.** Nothing in this appendix is implemented
> while the scope is limited to the ads system (section H,
> [ADR 0004](adr/0004-packages-backend-selection.md)). It records the
> interface ow-tauri proposes for a package runtime that Overwolf (or anyone)
> could ship for non-Electron hosts (OQ-21, OQ-33), so the work is not lost
> and can be reviewed. Contract tests do not cover it, and it may change
> completely before it is implemented. A runtime would be registered with
> `packagesBackend: "native"` (H.2).

This appendix is the whole design; a separate guide for runtime authors
would be written together with the implementation.

### P.1 Rust trait

```rust
/// A component that runs Overwolf packages for the host.
pub trait PackageRuntime: Send + Sync + 'static {
    /// Name and version reported in `PackagesSnapshot.runtime`.
    fn info(&self) -> RuntimeInfo;

    /// Called once at setup. The runtime keeps `host` to emit events, invoke
    /// callbacks and make host requests (windows, paths).
    fn initialize(&self, init: InitializeParams, host: HostHandle) -> BoxFuture<'_, Result<InitializeResult, RuntimeError>>;

    /// Load one listed package on its current channel. The result names the
    /// members the runtime provides and the initial sync state (P.3).
    fn load(&self, name: &str, channel: &str) -> BoxFuture<'_, Result<LoadedPackage, LoadFailure>>;

    /// Call a package member, e.g. ("gep", "setRequiredFeatures", [5426, null]).
    /// Arguments and the result use the remote-value encoding (P.1.1).
    fn call(&self, package: &str, method: &str, args: Vec<serde_json::Value>) -> BoxFuture<'_, Result<serde_json::Value, CallError>>;

    /// Call a method on a handle this runtime returned earlier (P.1.1).
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
    /// Initial values of the package's sync state (P.3).
    pub state: serde_json::Map<String, serde_json::Value>,
}
```

`HostHandle` lets the runtime emit `RuntimeEvent`s, call back into JS and use
host services:

```rust
pub enum RuntimeEvent {
    Manager(ManagerEvent),                       // loading, ready, failed-to-initialize, crashed, package-update-pending, updated
    Package { package: String, event: String, args: Vec<Value>, event_id: Option<u64>, actions: Vec<PackageEventAction> },
    State { package: String, path: String, value: Value },   // sync caches, P.3
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

#### P.1.1 Remote values

Package members take callbacks, take `BrowserWindow`s, and return objects with
methods. None of these are JSON, so arguments, results and event arguments use
this encoding between `ow-tauri/main` and the runtime (Rust passes it through
unchanged):

| Value | Encoded as | Notes |
|---|---|---|
| a function argument (callback) | `{ "$cb": <id> }` | `id` is unique per main-runtime session. The runtime invokes it with `package/callback { cbId, args }` (sidecar) or `HostHandle::invoke_callback`, which becomes a `package-callback` host message (P.7); JS calls the function with the decoded `args` and ignores its return value. The runtime releases it with `package/callbackRelease` once it will never call it again (after a one-shot callback ran; on `hotkeys.unregister`; when a later `setChannel` replaces a `ready`). JS keeps every unreleased callback alive |
| a `BrowserWindow` facade | `{ "$window": <id> }` | the Electron-style window id |
| a `WebContents` facade | `{ "$webContents": <id> }` | the owning window's id |
| an object with methods, returned or passed by the runtime | `{ "$handle": <id>, "kind": "<Kind>", "data": { ... } }` | JS materialises one facade per `(package, id)` and returns the same facade for the same id; methods call `package_handle_call` / `packages/handleCall`; JS sends `package_handle_release` when the facade can no longer be used (see kinds), and also from a `FinalizationRegistry` |
| a value-only object built in JS | `{ "$value": "<Kind>", "data": { ... } }` | materialised locally, no handle |
| an error (`data.error` of a failed call) | `{ "$kind": "RecorderError" \| "UtilityApiError" \| "Error", ... }` | rebuilt as in P.6 |

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

### P.2 JSON-RPC sidecar protocol

A native runtime can be a separate executable. ow-tauri starts it with
`--ow-tauri-runtime-protocol=1` and talks JSON-RPC 2.0 over its stdin and
stdout, framed like the Language Server Protocol:
`Content-Length: <bytes>\r\n\r\n<UTF-8 JSON>`. Stderr lines go to the
ow-tauri log at debug level. The trait methods map one-to-one:

| Direction | Method | Params | Result |
|---|---|---|---|
| host -> runtime | `initialize` | `InitializeParams` (below) | `{ runtime: { name, version }, packages: string[], protocolVersion: 1, pendingUpdates: PendingUpdatesResult }` |
| host -> runtime | `packages/load` | `{ name, channel }` | `{ version, members: string[], state: object }`, or error with `data.reason` |
| host -> runtime | `packages/call` | `{ package, method, args }` | any (P.1.1 encoding) |
| host -> runtime | `packages/handleCall` | `{ package, handle, method, args }` | any (P.1.1 encoding) |
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
| runtime -> host (request) | `host/createWindow`, `host/closeWindow`, `host/nativeWindowHandle`, `host/paths` | as P.1 | |

```ts
interface InitializeParams {
  protocolVersion: 1;
  host: { name: 'ow-tauri'; version: string; tauriVersion: string; os: string; arch: string };
  app: { uid: string; name: string; version: string; packages: string[]; channels: Record<string, string>;
         packagesUrl?: string; enablePackageBundling: boolean };
  switches: { packageChannel?: string; forcePhasedPackage?: string | true; packagesUrl?: string };
  devMode: { email?: string; apiKey?: string; devKey?: string };   // from the environment (A.1), debug builds only
  identity: { muid: string; phasePercent: number };
  paths: { packagesDir: string; logsDir: string; cacheDir: string };
}
```

Error objects use JSON-RPC `code` -32000 to -32099 with `data: { reason }`;
`packages/call` and `packages/handleCall` errors carry the package's own error
in `data.error` with a `$kind` tag (P.1.1), which ow-tauri rebuilds in JS
(P.6). The sidecar is killed if it does not answer `shutdown` within 5 s. A
crash of the sidecar emits `crashed` `(event, canRecover: true)`; unless a
listener calls `preventDefault()`, ow-tauri restarts it (at most 3 times per
session) and reloads the listed packages. Every callback and handle of the
crashed process is released on the JS side.

### P.3 Package state caches

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

### P.4 Actionable events and defaults

| Event | Actions | Default when settled without an action |
|---|---|---|
| packages `crashed` | `prevent-default` | relaunch the package |
| gep `game-detected` | `enable` | not enabled (no events for that game) |
| overlay `game-launched` (from detection or from `requestGameInjection`) | `inject(options?)`, `dismiss` | dismiss (OQ-16, deferred) |
| crn `before-notification` | `abort` | the notification shows |

Listeners may act asynchronously (ow-electron 39.x documents async
`game-detected` callbacks); ow-tauri sends `package_event_settled` after every
returned promise settles, or after 10 s.

### P.5 C ABI (sketch)

For runtimes shipped as a shared library (`OW_TAURI_PACKAGE_RUNTIME=<path to .dll/.dylib/.so>`).
The messages are exactly the JSON-RPC messages of P.2, without framing:

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


### P.6 Package objects in `ow-tauri/main`

With a runtime, `app.overwolf.packages` would behave as in ow-electron when
packages load: per listed package (`utility` included whenever any package is
listed, OQ-34) `loading`, then `ready(event, name, version)` or
`failed-to-initialize(event, name)`, and the manager's `crashed`,
`package-update-pending` and `updated` events. Package objects would keep the
exact upstream signatures from `@overwolf/ow-electron-packages-types` 1.1.12;
every member is forwarded with `package_call` (P.7), synchronous members are
served from the package's state cache (P.3), and callbacks, windows and
objects with methods use the remote-value encoding (P.1.1). `gep` events take
a leading synthetic `Event` (B.1.2); `overlay`, `utility`, `recorder` and
`crn` events do not, exactly as typed upstream. Actionable events add their
methods to the first argument (`enable()` on gep `game-detected`,
`inject(options?)` and `dismiss()` on overlay `game-launched`, `abort()` on
crn `before-notification`), and the runtime sends `package_event_settled`
after synchronous dispatch and after every promise a listener returned has
settled.

Proposed object lifecycle (the sample reads `packages.utility` in a
constructor and subscribes to `packages[name].on('ready')` before `ready`;
OQ-36):

| Phase | Object | Members |
|---|---|---|
| before `loading` | `undefined` | |
| from `loading` (defined before the `loading` listeners run) | defined, `version` `''` | event subscription works; async members reject `OwTauriError('not-ready')`; sync members return their empty value (`undefined`, `false`, `[]`, `{}`) |
| `ready` | same object, `version` set | the object emits its own `ready(version)` immediately before the manager's `ready`; members the runtime does not provide are absent if optional upstream, else reject `unsupported` |
| `failed-to-initialize` | stays defined | async members reject `not-ready`; listeners are kept |
| `crashed` | same object | async members reject `not-ready` until the package's next `ready`; listeners are kept |
| `updated` (hot update) | same object, `version` updated | listeners and remote handles are kept unless the runtime releases them |

Writes that are synchronous upstream (`hotkeys.update`,
`overlayOptions.<field> = ...`, `recorder.options[prop] = ...`,
`crn.allowNotifications`) would update the JS cache at once and be sent in
call order (B.1.6). Package errors would be rebuilt from `data.error`:

| `$kind` | Upstream type | Rejects with |
|---|---|---|
| `RecorderError` | `class RecorderError extends Error` (`code`, `codeStr`, `internalError?`) | an instance of the `RecorderError` class exported by `ow-tauri/main` (B.1.5), with `internalError` rebuilt as an `Error` from `{ name, message }` when present |
| `UtilityApiError` | `interface UtilityApiError` (`message`, `exitCode?`) | a plain frozen object `{ message, exitCode? }`, not an `Error`, as typed |
| `Error` or absent | | `Error` with the reported `name` and `message` |

### P.7 Commands, host messages and Rust API of the design

| Command | Arguments | Returns | Purpose |
|---|---|---|---|
| `package_call` | `{ package: string; method: string; args: unknown[] }` | `unknown` | generic call into the runtime; `method` is the typed member path, for example `setRequiredFeatures` or `hotkeys.register` |
| `package_handle_call` | `{ package: string; handle: number; method: string; args: unknown[] }` | `unknown` | a method on a remote object returned by the runtime (P.1.1) |
| `package_handle_release` | `{ package: string; handles: number[] }` | `void` | JS no longer references these handles |
| `package_event_action` | `{ eventId: number; action: 'enable' \| 'inject' \| 'dismiss' \| 'prevent-default' \| 'abort'; args?: unknown[] }` | `void` | the methods on event objects (P.4) |
| `package_event_settled` | `{ eventId: number }` | `void` | all listeners for an actionable event have returned |

`packages_set_channel` would take a `ready?: { $cb: number }` callback
reference, persist the channel in `ow-tauri.json` `packageChannels`, and
resolve `SetChannelResult`; `packages_get_available_channels` and
`packages_get_channel` would answer from the runtime.

| Host message `type` | Fields | When |
|---|---|---|
| `packages` | `{ event: 'loading' \| 'ready' \| 'failed-to-initialize' \| 'crashed' \| 'package-update-pending' \| 'updated', name?, version?, info?, canRecover?, eventId? }` | package manager lifecycle |
| `package-event` | `{ package, event, args, eventId?, actions? }` | an event emitted by a package object |
| `package-callback` | `{ cbId, args }` | the runtime invokes a callback reference (P.1.1) |
| `package-callback-release` | `{ cbIds }` | the runtime will never invoke these callbacks again |

`PackagesSnapshot` would gain `runtime: { name, version }`,
`packages: Record<string, { state, version }>`, `channels`, `members`
(per package, the member paths the runtime provides) and `packageState`
(P.3). The Rust `Builder` would gain `package_runtime(Arc<dyn PackageRuntime>)`
and `package_runtime_sidecar(path)`, and `OW_TAURI_PACKAGE_RUNTIME=<path>`
would register a sidecar executable or a shared library exporting
`ow_runtime_v1_entry` (P.5).
