# Configuration

The plugin reads one block: `plugins.overwolf` in `tauri.conf.json`. Every key
is optional; the defaults behave as ow-electron does.

```json
{
  "productName": "My Game App",
  "version": "1.0.0",
  "plugins": {
    "overwolf": {
      "author": "Example Studio",
      "name": "My Game App",
      "uid": "<the uid the Overwolf console assigned>",
      "ads": { "testAd": true }
    }
  }
}
```

The source of truth is `crates/tauri-plugin-overwolf/src/config.rs`. Rust
code reads the parsed block with `app.overwolf().config()`; it never changes
at run time.

## Contents

- [Where the values come from](#where-the-values-come-from)
- [Identity](#identity)
- [`ads`](#ads)
- [`ads.guestLimits`](#adsguestlimits)
- [`analytics`](#analytics)
- [`consent`](#consent)
- [`emailHashes`](#emailhashes)
- [`updater`](#updater)
- [`signing`](#signing)
- [`state`](#state)
- [Validation](#validation)
- [Removed keys](#removed-keys)

## Where the values come from

The plugin, its build step (`build::run` in `build.rs`) and the `ow-tauri`
CLI read the same merged configuration, as `tauri build` does:

1. `tauri.conf.json`;
2. the target's overlay, `tauri.windows.conf.json`, `tauri.macos.conf.json`
   or `tauri.linux.conf.json`;
3. `TAURI_CONFIG` (what `tauri build --config` passes).

Each layer is a JSON merge patch over the one before. So the uid the Windows
installer records is always the uid the app runs with.

Some values can also be set at run time. The on/off switches below only
turn something on: test ads, or an opt-out. None of them turns a setting
off that the config turned on. The Rust host label replaces the config
value.

| Setting | Config | Rust `Builder` | Other |
|---|---|---|---|
| Test ads | `ads.testAd` | `.test_ad(true)` | `--test-ad` argument, `OW_TAURI_TEST_AD=1` (or `true`) |
| No anonymous analytics | `analytics.disableAnonymous` | `.disable_anonymous_analytics()` | the user's stored preference ([js.md](api/js.md#analytics-switches)) |
| No ad optimisation | `ads.disableOptimization` | `.disable_ads_optimization()` | |
| No first-party ad data | `ads.disableFpd` | `.disable_ads_fpd()` | |
| Host label | `analytics.hostLabel`, `analytics.hostVersion` | `.host_label(label, version)` | |
| Uncounted windows | `analytics.excludeWindows` | `.exclude_windows(globs)` (added to the config list) | |

The uid has no Rust setter on purpose: the build step must see it.

## Identity

| Key | Type | Default | Meaning |
|---|---|---|---|
| `author` | string | `"unknown"` (debug builds only) | an input of the uid formula |
| `name` | string | `productName` | the ow-electron app name: an input of the uid formula, the `app_name` of the analytics, the name the ad pages receive |
| `uid` | string | computed | the app uid; overrides the formula. 1 to 64 ASCII letters or digits |

When `uid` is not set, the plugin computes it as ow-electron does: the SHA-1
of `{'author':'<author>','name':'<name>.electron'}`, written with the
letters `a` to `p`. The `.electron` suffix is part of the formula, so a
Tauri app and its ow-electron predecessor share the uid.

Once the Overwolf console assigns your app a uid, set it as `uid`
([OVERWOLF-ONBOARDING.md](OVERWOLF-ONBOARDING.md#2-get-the-uid)). A release
build fails unless `uid`, or both `author` and `name`, are set:

```text
plugins.overwolf: set "uid", or both "author" and "name", before a release build (the uid must not depend on defaults)
```

The analytics also send the computed uid (`app_cuid`), even when `uid` is
set. `npm exec --no -- ow-tauri doctor` prints both.

## `ads`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `testAd` | boolean | `false` | Overwolf test ads instead of live ads |
| `disableOptimization` | boolean | `false` | ow-electron's `build.overwolf.disableAdOptimization` |
| `disableFpd` | boolean | `false` | no first-party data for the ads, from the first request on |
| `transparentGuests` | boolean | `true` | ad webviews have a transparent background, so an empty slot shows your container |
| `browserArgs` | string[] | `[]` | Windows: extra WebView2 switches for the ad webviews, after the ones ow-electron uses. Each must start with `--`; `--user-data-dir` is refused, and remote debugging is refused in release builds |
| `allowedEmbedderOrigins` | string[] | `[]` | extra page origins that may host ads, for example a `tauri-plugin-localhost` origin. Never an Overwolf origin |
| `loadErrorRetryMs` | integer (ms) | `5000` | retry delay after an ad page failed to load |
| `maxRecoveries` | integer or null | `null` | most crash recoveries per ad; `null` is unlimited, as in ow-electron |
| `recreateOnReload` | boolean | `true` | macOS: an ad page that asks for a reload gets a fresh webview, which keeps memory flat; ignored on Windows |
| `recreateMinIntervalMs` | integer (ms) | `30000` | macOS: a reload sooner than this after the last fresh webview reloads in place |
| `recreateMaxPerHour` | integer | `30` | macOS: fresh webviews per ad and hour; beyond it, reloads are in place |
| `requestShaping` | boolean | `true` | sends the ad requests with ow-electron's headers. A debug switch: keep it on |
| `owVersionOverride` | string | none | overrides the runtime version the ad pages receive. A debug switch |
| `macPrivateHeaderApi` | boolean | `false` | macOS prototype; keep it off |
| `guestLimits` | object | see below | limits per ad webview |

Example:

```json
"ads": { "testAd": true, "allowedEmbedderOrigins": ["http://localhost:9527"] }
```

A page on an origin in `allowedEmbedderOrigins` gets every command of the
permission sets its capability grants. List only origins you control.

### `ads.guestLimits`

Limits on what one ad page may send and open.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `eventsPerSecond` | integer | `50` | ad events per second |
| `eventBurst` | integer | `100` | event burst |
| `bytesPerSecond` | integer | `262144` | event bytes per second |
| `externalOpensPerMinute` | integer | `20` | browser pages one ad may open per minute |
| `externalOpensPerMinuteApp` | integer | `20` | browser pages all ads together may open per minute |
| `activationWindowMs` | integer (ms) | `5000` | how long a user click on an ad allows one browser page to open |

All must be greater than 0.

## `analytics`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `hostLabel` | string | `"tauri"` | the host label in the analytics and the user agent, where ow-electron says `electron`. 1 to 32 characters: lower-case letters, digits, `_`, starting with a letter |
| `hostVersion` | string | the Tauri version | the version sent with the host label |
| `disableAnonymous` | boolean | `false` | the same as ow-electron's `disableAnonymousAnalytics()` before the app is ready: only the mandatory events are sent, from the first launch request on |
| `excludeWindows` | string[] | `[]` | window label globs (`*`, `?`) that never count as visible app windows, for splash screens or overlays |
| `userSwitch` | boolean | `false` | enables `setAnalyticsUserEnabled()` |
| `muidStrategy` | `"machine-id"` or `"per-install"` | `"machine-id"` | `machine-id` is ow-electron's machine id, shared by every Overwolf app on the machine. `per-install` uses a random id per install instead (a difference from ow-electron) |

The plugin's own windows (`ow-cmp*`) are never counted.

## `consent`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `cmpUrl` | string (https) | Overwolf's consent page | replaces the consent page |
| `allowedCmpOrigins` | string[] | `["https://content.overwolf.com"]` | origins a JavaScript `cmpURL` option must match |
| `readyTimeoutMs` | integer (ms) | `30000` | how long the consent page may take to report ready |
| `euOnlyTimeoutMs` | integer (ms) | `60000` | how long the "is consent required" request may take. On expiry consent counts as required, as when the request fails |
| `hostCookieFallback` | `"auto"` or `"never"` | `"auto"` | whether the consent cookies are checked when the consent page saved nothing |

Both timeouts must be 1 to 600000. ow-electron has no timeout on the "is
consent required" request; the 60 s bound is a documented difference that
only shows when the request hangs.

## `emailHashes`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `encoding` | `"hex"` or `"base64"` | `"hex"` | the text form of the hashes. `hex` (lower case) is what ow-electron sends |

## `updater`

Read only with the crate's `updater` feature, on Windows:

```toml
# src-tauri/Cargo.toml
tauri-plugin-overwolf = { git = "https://github.com/AlloryDante/ow-tauri", features = ["updater"] }
```

| Key | Type | Default | Meaning |
|---|---|---|---|
| `publisherNames` | string[] | none | the subject names of **your** installer's Authenticode certificate. This or `pubkey` is required in a release build |
| `pubkey` | string | none | a minisign public key the installer must verify against |
| `endpoint` | string (https) | Overwolf's feed for the uid | the feed URL. `http://` on a loopback host is allowed in debug builds |
| `channel` | string | `"latest"` | the feed channel |
| `allowPrerelease` | boolean | `false` | accept pre-release versions |
| `allowDowngrade` | boolean | `false` | accept a lower version |
| `allowJsDowngrade` | boolean | `false` | let JavaScript's `channel` / `allowDowngrade` options cause a downgrade |
| `installOnExit` | boolean | `true` | install a downloaded update when the app exits |
| `installerArgs` | string[] | none | installer arguments. `/UPDATE` is always kept; with `/S` it is added (one warning) |
| `connectTimeoutMs` | integer (ms) | `30000` | connect timeout |
| `readTimeoutMs` | integer (ms) | `60000` | time allowed between two received bytes |
| `dangerousSkipPublisherCheck` | boolean | `false` | skip the publisher check. Debug builds only |

The default feed is
`https://electron-updates.overwolf.com/electron-updates/electron/<uid>/`, the
one Overwolf hosts for ow-electron apps. Only NSIS installers are accepted.

```json
"updater": { "publisherNames": ["Example Studio Ltd"] }
```

A release build with the `updater` feature and neither `publisherNames` nor
`pubkey` fails:

```text
plugins.overwolf.updater: set publisherNames (your installer's certificate subject) or pubkey before a release build
```

Do not also register `tauri-plugin-updater` for Windows
([INTEROP.md](INTEROP.md)).

## `signing`

Read by the build step and the CLI only. Overwolf signing is optional; see
[OVERWOLF-ONBOARDING.md](OVERWOLF-ONBOARDING.md#7-signing-optional).

| Key | Type | Default | Meaning |
|---|---|---|---|
| `enabled` | boolean | `false` | signing is on |
| `requireSigning` | boolean | `true` | with `enabled`: a Windows release build without the `ow-tauri sign` output fails |
| `owCertSigning` | boolean | `false` | ow-electron's `enableOWCertSigning`: `ow-tauri sign-exe` asks Overwolf to sign the app exe |
| `entry` | string | none | the file whose hash Overwolf signs; `ow-tauri sign --main <file>` overrides it |

## `state`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `appDataDir` | string (path) | the OS app data folder | replaces the folder that holds `ow-electron/<uid>/`. For tests; refused in release builds |

## Validation

The plugin checks the block when the app starts; a wrong value stops the
start with a message that names the key. The build step runs the same checks
at compile time, plus the release-only rules, so most mistakes show up in
`cargo build`.

| Rule | Message |
|---|---|
| unknown key | `plugins.overwolf.<path>: unknown field "<key>", expected one of ...` |
| wrong type | `plugins.overwolf.<path>: invalid type: ...` |
| `uid` | `plugins.overwolf.uid: must be 1 to 64 ASCII letters or digits` |
| release build without a pinned uid | `plugins.overwolf: set "uid", or both "author" and "name", before a release build (the uid must not depend on defaults)` |
| `analytics.hostLabel` | `plugins.overwolf.analytics.hostLabel: must be 1 to 32 lower-case letters, digits or _ starting with a letter` |
| `analytics.excludeWindows[i]` empty, or a plugin label | `plugins.overwolf.analytics.excludeWindows[i]: must be a non-empty label glob` |
| `consent.cmpUrl` | `plugins.overwolf.consent.cmpUrl: must be an https URL` |
| `consent.allowedCmpOrigins[i]` | `plugins.overwolf.consent.allowedCmpOrigins[i]: must be an https origin (scheme://host[:port], no path)` |
| `ads.allowedEmbedderOrigins[i]` | `plugins.overwolf.ads.allowedEmbedderOrigins[i]: must be an http(s) origin that is not an Overwolf origin` |
| `consent.readyTimeoutMs`, `consent.euOnlyTimeoutMs` | `...: must be 1 to 600000` |
| `ads.guestLimits.*`, `ads.recreateMaxPerHour` = 0 | `...: must be greater than 0` |
| `ads.browserArgs[i]` | `plugins.overwolf.ads.browserArgs[i]: must be a --switch other than --user-data-dir (and no remote debugging in release builds)` |
| `updater.endpoint` | `plugins.overwolf.updater.endpoint: must be an https URL` |
| `updater.pubkey` empty | `plugins.overwolf.updater.pubkey: must not be empty when set` |
| release, `updater` feature, no publisher data | `plugins.overwolf.updater: set publisherNames (your installer's certificate subject) or pubkey before a release build` |
| release and `updater.dangerousSkipPublisherCheck` | `plugins.overwolf.updater.dangerousSkipPublisherCheck: not allowed in release builds` |
| release and `state.appDataDir` | `plugins.overwolf.state.appDataDir: only allowed in debug builds` |

The build step also checks the capability files:

- **error**: a `remote.urls` entry that covers Overwolf pages
  (`*.overwolf.com`) or every origin (`https://*`);
- **warning**, while the `ads` feature is on: a capability that selects
  `windows`. Select `webviews` instead
  ([api/permissions.md](api/permissions.md#select-webviews-not-windows)).

## Removed keys

Keys of the pre-1.0 previews are refused with a pointer to the migration
guide:

| Key | Use instead |
|---|---|
| `main` | your own pages and Rust code ([MIGRATION.md](MIGRATION.md#no-main-webview)) |
| `ipc` | Tauri commands ([MIGRATION.md](MIGRATION.md#no-ipc-bridge)) |
| `shell`, `fs` | the official Tauri plugins ([MIGRATION.md](MIGRATION.md#official-plugins)) |
| `webview` | `ads.browserArgs` ([MIGRATION.md](MIGRATION.md#browser-args)) |
| `packagesBackend` | nothing yet ([MIGRATION.md](MIGRATION.md#packages)) |
| `logging` | the `log` crate ([MIGRATION.md](MIGRATION.md#logging)) |
| `ads.gestureWindowMs` | nothing: clicks use the OS's user activation ([MIGRATION.md](MIGRATION.md#gesture-window)) |
| `ads.guestHeartbeatTimeoutMs` | the macOS terminate hook ([MIGRATION.md](MIGRATION.md#crash-recovery)) |
| `ads.recreateOnReload: "auto"` / `"never"` | `true` / `false` ([MIGRATION.md](MIGRATION.md#recreate-on-reload)) |

The message reads:

```text
plugins.overwolf.main: removed in ow-tauri 1.0 (there is no hidden main webview; app code runs in the app's own webviews); see docs/MIGRATION.md#no-main-webview
```
