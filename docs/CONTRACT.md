# Overwolf wire contract

This document says what Overwolf receives from an app that uses
`tauri-plugin-overwolf`: every request, header, query, cookie, file, id and
event, and every value Overwolf's ad and consent pages can read. It is the
specification the plugin is tested against. When the code and this document
disagree, one of them is changed in the same pull request.

Contract for `tauri-plugin-overwolf` 1.0 and `tauri-plugin-overwolf-api` 1.0
on Tauri 2.12. Parity reference: ow-electron 42.11.4, observed with the
parity harness ([PARITY.md](PARITY.md)).

**The rule.** Wherever Overwolf can see a difference, the plugin sends what
ow-electron sends. The one deliberate difference is the host label
([section 0](#host-label)). Where a platform cannot reproduce a behaviour,
this document says so and names the gap.

Other documents cover the rest:

| Topic | Document |
|---|---|
| Every configuration key and its default | [CONFIG.md](CONFIG.md) |
| The JavaScript and Rust APIs | [api/js.md](api/js.md), [api/rust.md](api/rust.md), [api/owadview.md](api/owadview.md) |
| Permission sets | [api/permissions.md](api/permissions.md) |
| How the plugin is built | [ARCHITECTURE.md](ARCHITECTURE.md) |
| Threat model | [SECURITY.md](SECURITY.md) |
| Ad formats and their events | [AD-FORMATS.md](AD-FORMATS.md) |
| Moving an ow-electron app | [MIGRATION.md](MIGRATION.md) |

| Section | Covers |
|---|---|
| [0. Conventions](#0-conventions) | sources, the host label, placeholders, what is not claimed |
| [A. Plugin surface](#a-plugin-surface) | configuration that changes the wire, commands, errors, start and exit |
| [B. JavaScript API](#b-javascript-api) | the API package and the `<owadview>` element |
| [D. Guest pages](#d-guest-pages) | what the ad page and the consent pages get, consent, request shaping |
| [E. Analytics](#e-analytics) | user agent, request shapes, events and their order, opt-outs, machine ids |
| [F. State files](#f-state-files) | paths and the exact encoding of `ow-electron.json` |
| [G. Identity and signing](#g-identity-and-signing) | the app name, the uid rule with test vectors, signing |
| [H. Packages](#h-packages) | GEP, overlay and the other packages |
| [I. Updates and installer](#i-updates-and-installer) | Overwolf's update feed, verification, the installer hooks |

Section C (the IPC protocol of the earlier design) no longer exists; see
[Removed sections](#removed-sections).

---

## 0. Conventions

- **Plugin name** `overwolf`. JavaScript invokes commands as
  `plugin:overwolf|<command>`. Permissions are `overwolf:<set>` and
  `overwolf:allow-<command-kebab>`.
- **Casing.** Command arguments and JSON payloads use `camelCase` keys.
  String values use the ow-electron spelling.
- **Labels.** Tauri webview labels the plugin owns: `owad-<n>` (ad guests,
  `n` from 1, a taken label is skipped), `ow-cmp-startup` and
  `ow-cmp-startup-<n>` (hidden startup consent windows), `ow-cmp-default`
  (the hidden default-consent window) and `ow-cmp` (the ad privacy settings
  window). An app window with one of these labels is not tracked, cannot
  host ads and cannot call the plugin's app commands
  ([ARCHITECTURE 3.1](ARCHITECTURE.md#31-window-classes)).
- **Time** values are milliseconds unless a name says otherwise. Timestamps
  are Unix epoch milliseconds, except where ow-electron uses seconds
  (F.2 `cmp.timeStamp`).
- **Platforms.** Ads and consent windows run on Windows and macOS. Linux
  builds compile; ad guests are not created there, and `getInfo()` reports
  `adsSupported: false`. Mobile builds register every command and answer
  `unsupported`.

### Sources

Each behaviour copied from ow-electron names its source:

| Tag | Source |
|---|---|
| [DOC] | Overwolf's documentation (`https://dev.overwolf.com/ow-electron/`) and Overwolf's legal and privacy pages |
| [TYPES] | the published ow-electron 42.11.4 typings |
| [SAMPLE] | the official `ow-electron-packages-sample` |
| [BUILDER] | the published JavaScript of `@overwolf/app-builder-lib` 26.9.3 (including its NSIS templates) and `@overwolf/ow-cli` 0.1.10 |
| [OBS] | observed: a running ow-electron 42.11.4 app under the parity harness, or Overwolf's public update feed ([PARITY.md](PARITY.md)) |
| [DEC] | a plugin decision where the sources above do not pin the behaviour down |
| [INF] | an inference not verified yet; a lab check confirms it ([PARITY.md, "Lab checks"](PARITY.md#lab-checks)) |

Markers:

- **Unknown (R3-n)**: not settled; harness item `R3-n`
  ([PARITY.md](PARITY.md#harness-rounds)) observes it. The text gives the
  interim behaviour.
- **Plugin option**: a behaviour ow-electron does not have. Off by default
  and documented as non-parity.

### Host label

Wherever ow-electron names itself ("electron", its version), the plugin says
"tauri" instead ([ADR 0006](adr/0006-analytics-labelling.md)). All such
values come from two settings, so Overwolf can change them in one place:

- `analytics.hostLabel`, default `"tauri"` (written `<label>` below);
- `analytics.hostVersion`, default the Tauri crate version (`<tv>`, for
  example `2.12.1`).

`<owVersion>` is `<label>-<hostVersion>` (for example `tauri-2.12.1`), or
`<hostVersion>` alone when `<label>` is `electron`. So `hostLabel: "electron"`
with `hostVersion: "42.11.4"` reproduces ow-electron's values exactly [DEC].

| Where | ow-electron sends [OBS] | The plugin sends |
|---|---|---|
| Counter `Name` (E.2) | `electron_app_first_launch`, `electron_app_start`, `electron_app_heartbeat`, `electron_window_closed`, `electron_owadview_crashed`, `electron_sub_info` | `<label>_app_first_launch`, `<label>_app_start`, `<label>_app_heartbeat`, `<label>_window_closed`, `<label>_owadview_crashed`, `<label>_sub_info` |
| uninstall Counter (I.6) [BUILDER] | `ow_electron_app_uninstall` | `ow_<label>_app_uninstall` |
| Counter `owver` | `42.11.4` | `<owVersion>` (`tauri-2.12.1`) |
| InsertStats `owver` | `42_11_4` | `<owVersion>` with `.` replaced by `_` (`tauri-2_12_1`) |
| guest `__overwolf__.owVersion` (D.2) | `"42.11.4"` | `"<owVersion>"`, or `ads.owVersionOverride` when set |
| consent page query `oweVersion` (D.6) | `42.11.4` | `<owVersion>`, or `ads.owVersionOverride` when set |
| user agent token (E.1) | `Electron/42.11.4` | `<Label>/<hostVersion>`, first letter upper-cased (`Tauri/2.12.1`) |

Never substituted, because they are Overwolf's identifiers:

- the URLs `https://www.overwolf.com/monsdk/electron/latest/adview.html`,
  `https://content.overwolf.com/monsdk/electron/latest/cmp/...`,
  `https://electron-updates.overwolf.com/electron-updates/electron/<uid>/`
  and the signing endpoints `/sign/electron*` (G.3);
- the `.electron` suffix in the uid formula (G.2);
- the state directory and file `<appData>/ow-electron/<uid>/ow-electron.json`
  (F);
- the registry keys `HKCU\Software\OverwolfElectron`,
  `HKCU\Software\OverwolfPersist` and `Software\OverwolfElectron\<uid>` (E.4,
  I.6);
- cookie names, and the analytics Overwolf's own pages send (E.5).

**Risk (OQ-03, OQ-04).** Console dashboards may filter on the `electron_`
names or on a numeric `owver`. The ad and consent pages derive their own
`ClientVer` / `CurrentVersion` values from `owVersion` / `oweVersion`. The lab
check in [PARITY.md](PARITY.md#lab-checks) confirms that the ad page fills
and reports with `tauri-<tv>`. If Overwolf needs a numeric value,
`ads.owVersionOverride` gives the guest and the consent page one.

### Placeholders

| Placeholder | Meaning |
|---|---|
| `<uid>` | the app uid (G.2) |
| `<cuid>` | the computed uid (G.2) |
| `<muid>`, `<muidV2>` | machine ids (E.4) |
| `<PN>` | the app name (G.1) |
| `<PNNS>` | `<PN>` with spaces removed (user agent token) |
| `<ver>` | the app version (Tauri `version`) |
| `<tv>` | the Tauri crate version, for example `2.12.1` |
| `<label>`, `<owVersion>` | the host label values (above) |
| `<UA>` | the composed user agent (E.1) |
| `<locale>` | the app locale in BCP 47 form, for example `en-US` |
| `<appData>` | the OS configuration directory (F.1) |

### What this contract does not claim

These limits are part of the contract. No document of this project claims
otherwise.

- **No in-stream ads.** There is no `<owadview>` API for them on either host.
- **Formats that need demand.** High-impact, interstitial (performance) and
  reward ads fill in test mode. Live fill comes from direct deals after
  Overwolf qualifies the app, on either host
  ([AD-FORMATS.md](AD-FORMATS.md)).
- **macOS request headers.** The ad document gets ow-electron's `Referer`
  and `Origin`. Subresource requests do not get the forced `Origin`, and the
  ad library does not get the `x-ow-*` headers (D.8.3).
- **Linux.** No ad guests and no request shaping.
- **Rewards.** The ad page reports events to the app. There is no
  server-verified reward.
- **`performance_ad_no_fill`.** Overwolf documents it; no lab run has seen
  it. A performance ad without fill sends `shutdown` only (B.3.5).

---

## A. Plugin surface

### A.1 Configuration

Configuration is `plugins.overwolf` in `tauri.conf.json` (merged with the
`tauri.<target>.conf.json` overlay and `TAURI_CONFIG`, as Tauri merges them).
Every key and default is in [CONFIG.md](CONFIG.md). These keys change what
Overwolf receives:

| Key | Default | Effect on the wire |
|---|---|---|
| `uid` | none | the app uid (G.2) |
| `author`, `name` | `"unknown"`, the Tauri `productName` | the computed uid and `<PN>` (G.1, G.2) |
| `ads.testAd` | `false` | `testAd: true` for every guest (D.7) |
| `ads.disableOptimization` | `false` | `settings.disableOptimization` (D.2), as ow-electron's `build.overwolf.disableAdOptimization` |
| `ads.disableFpd` | `false` | `disableAdsFPD()` before the launch (D.5) |
| `ads.requestShaping` | `true` | D.8; switch it off only while debugging |
| `ads.owVersionOverride` | none | guest `owVersion` and consent `oweVersion` (section 0) |
| `ads.browserArgs` | `[]` | Windows: appended to the ads environment's browser arguments (D.8.1) |
| `analytics.hostLabel`, `analytics.hostVersion` | `"tauri"`, `<tv>` | section 0 |
| `analytics.muidStrategy` | `"machine-id"` | `"per-install"` is a plugin option (E.4) |
| `analytics.disableAnonymous` | `false` | `disableAnonymousAnalytics()` before the launch (E.3) |
| `analytics.excludeWindows` | `[]` | label globs never counted in E.2 #5 and #7 |
| `analytics.userSwitch` | `false` | exposes `setAnalyticsUserEnabled` (E.3, plugin option) |
| `consent.cmpUrl` | none | settings window page (D.6.4); `https:` only |
| `consent.allowedCmpOrigins` | `["https://content.overwolf.com"]` | origins a JavaScript `cmpURL` may use (D.6.4) |
| `consent.euOnlyTimeoutMs` | `60000` | the `cmp-eu-only` timeout (D.6.2) |
| `consent.readyTimeoutMs` | `30000` | hidden consent windows close after this (D.6.1) |
| `consent.hostCookieFallback` | `"auto"` | D.6.3 |
| `emailHashes.encoding` | `"hex"` | `"base64"` is a plugin option (A.2.2) |
| `updater.*` | | I |

Switches read at setup:

| Switch | Effect |
|---|---|
| `--test-ad` (a process argument, exactly) | same as `ads.testAd: true` |
| `OW_TAURI_TEST_AD=1` (or `true`) | same as `ads.testAd: true` |

ow-electron's package switches (`--owepm-package-channel`,
`--force-phased-package`, `--owepm-packages-url`) mean nothing here (H).

### A.2 Commands

The plugin registers 25 commands. The app's own webviews call 23 of them
through `tauri-plugin-overwolf-api` (B.1). Ad guests and consent windows get
one command each, through capabilities the plugin adds at run time (A.2.6,
A.2.7).

Every app command first checks its caller. The webview must not be a plugin
webview (`owad-*`, `ow-cmp*`) and must show a local app page:
`tauri://localhost`, `http://tauri.localhost` or `https://tauri.localhost`,
`build.devUrl`'s origin in a debug build, or an
`ads.allowedEmbedderOrigins` entry. Otherwise the command fails with
`forbidden` and the message
`tauri-plugin-overwolf: <label> is not a local app webview`.

"Errors" below lists codes (A.4) beyond `forbidden` (caller or permission)
and `invalid-argument` (malformed arguments).

#### A.2.1 Command table

| Command | Set | Arguments | Returns | Errors |
|---|---|---|---|---|
| `adview_mount` | `default` | `request: AdviewMount`, `onEvent: Channel` | `{ guestLabel }` | `unsupported` (no ads on this platform) |
| `adview_update` | `default` | `request: AdviewUpdate` | `null` | `not-found` |
| `adview_unmount` | `default` | `elementId` | `null` | `not-found` |
| `adview_command` | `default` | `elementId`, `command`, `args?` | `null` | `not-found` |
| `set_window_name` | `default` | `name` | `null` | `not-found` |
| `get_info` | `default` | none | `Info` | |
| `get_machine_ids` | `machine-id` | none | `{ muid, muidV2 }` | |
| `is_cmp_required` | `default` | none | `boolean` | |
| `open_ad_privacy_settings_window` | `default` | `options?: CMPWindowOptions` | `null` | `backend`, `not-found` (parent) |
| `open_cmp_window` | `default` | `options?` | `null` | as above |
| `disable_anonymous_analytics` | `default` | none | `null` | |
| `disable_ads_optimization` | `default` | none | `null` | |
| `disable_ads_fpd` | `default` | none | `null` | |
| `generate_user_email_hashes` | `email-hashes` | `email` | `{ sha1, md5, sha256 }` | |
| `set_user_email_hashes` | `email-hashes` | `hashes: { value? }` | `null` | |
| `clear_user_email_hashes` | `email-hashes` | none | `null` | `io` |
| `set_external_payment_user_id` | `analytics` | `options?` | `null` | `network` |
| `set_analytics_user_enabled` | `analytics` | `enabled` | `null` | `unsupported` (without `analytics.userSwitch`), `io` |
| `set_anonymous_analytics_preference` | `analytics` | `enabled` | `null` | `io` |
| `updater_check` | `updater` | `options?` | update or `null` | `unsupported` (not Windows, or no `updater` feature), `network`, `config` |
| `updater_download` | `updater` | `rid`, `onEvent` | `null` | `network`, `verification`, `not-found` |
| `updater_install` | `updater` | `rid` | `null` | `not-found`, `backend` |
| `updater_download_and_install` | `updater` | `rid`, `onEvent` | `null` | as the two above |
| `adview_event` | `adview-guest` (runtime only) | `slotId?`, `name`, `data?` | `null` | `not-found` |
| `cmp_event` | `cmp-window` (runtime only) | `name`, `data?` | `null` | `not-found` |

`overwolf:default` grants the twelve commands marked `default`. The
`machine-id`, `email-hashes`, `analytics` and `updater` sets are opt-in.
`adview-guest` and `cmp-window` are never granted by an app
([api/permissions.md](api/permissions.md)).

#### A.2.2 Overwolf API

These commands mirror `app.overwolf` of ow-electron.

- **`get_info`** returns `{ uid, appCuid, phasePercent, utmParams, testAd,
  adsSupported, name, version, host: { label, version, owVersion } }`. It
  carries no machine id. `utmParams` is `null` when `ow-electron.json` has
  none (ow-electron reports `undefined`) [OBS].
- **`get_machine_ids`** returns `{ muid, muidV2 }` (E.4). `muid` is
  `muidV2` when that is not empty, as ow-electron's `app.overwolf.muid`
  answers [OBS].
- **`is_cmp_required`** never fails for an app webview (D.6.2).
- **Email hashes.** `generate_user_email_hashes(email)` trims and lower-cases
  the address and, for `gmail.com`, removes dots and any `+suffix` from the
  local part. It returns the SHA-1, MD5 and SHA-256 of the result in
  `emailHashes.encoding` (`hex`, as ow-electron), then sends and stores them
  as `set_user_email_hashes` does. Empty or blank input gives empty hashes.
  Shared vectors: `crates/tauri-plugin-overwolf/tests/fixtures/email-hashes.json`.
- **`set_user_email_hashes({ value })`** does what ow-electron's
  `setUserEmailHashes(value)` does [OBS]. The JavaScript API passes its
  argument as `value`; a missing `value` is `undefined`, and `null` stays
  `null`.
  - A `value` is written to `ow-electron.json` as `eHashes` exactly as
    given (F.2): `null`, `{}`, `""`, missing or extra keys, in the app's
    key order.
  - A missing `value` removes `eHashes`, so the file returns to its bytes
    from before the set.
  - Every existing ad guest gets one `eHashes` message (D.5) whose data is
    the value, or `{}` when the value is missing or falsy.
  - After `disableAdsFPD()` a `value` is ignored (one warning). A missing
    `value` still removes `eHashes` and sends `{}`.
  - A failed write is logged and the command still succeeds.
- **`clear_user_email_hashes()`** (plugin option) is the missing-`value`
  case: it removes `eHashes` and sends `{}` to every existing guest, also
  after `disableAdsFPD()`. It fails with `io` when the file cannot be
  written.
- **Switches.** `disable_anonymous_analytics` (E.3),
  `disable_ads_optimization` (D.2) and `disable_ads_fpd` (D.5) only reduce
  what is sent. `set_anonymous_analytics_preference(false)` stores the choice
  in `ow-tauri.json`; it applies from the next launch's burst.
- **`set_external_payment_user_id(options)`** sends `<label>_sub_info`
  (E.2 #10) and resolves after the response.

#### A.2.3 Window names

`set_window_name(name)` names the caller's own window for analytics and for
the ad guests it embeds (`windowName`, `x-ow-window`). The name is 1 to 128
printable ASCII characters. ow-electron has no such call: it derives the
name from the page URL (E.2 #7). The command exists because a Tauri app with
client-side routing has no other way to keep a stable name. It is a
non-parity escape hatch; without it the plugin derives the name as
ow-electron does.

#### A.2.4 Packages

No command. See H.

#### A.2.5 Ads commands

`adview_mount` creates one guest webview for one `<owadview>` element. The
element runtime (B.3) sends:

```ts
interface AdviewMount {
  elementId: string;                 // runtime-assigned, unique per webview
  attributes: {
    cid: string;                     // trimmed, at most 20 characters
    slotsize: string;                // "WxH"
    adstyle: string;
    customTracking: object | null;   // parsed JSON object
    performance: boolean;
    unit: string | null;
    pageurl: string;                 // "" when absent
  };
  rect: { x: number; y: number; width: number; height: number }; // CSS px
  visible: boolean;
  documentTitle?: string;            // at most 1024 characters are used
  devicePixelRatio: number;          // (0, 16]
  innerWidth: number;                // (0, 65536]
  runtimeVersion?: string;
}
```

Unknown keys are refused. The rectangle must be finite, not negative and at
most 16384 per side. The mount waits until the launch burst has started
(A.6), so 400025 never precedes it, and returns `{ guestLabel }`.

`adview_update` carries only what changed: `rect` (with the page's new
`devicePixelRatio` and `innerWidth`), `visible`, or changed `attributes`
(`customTracking: null` clears it). `adview_unmount(elementId)` closes the
guest. `adview_command(elementId, command, args)` runs an element method:
`setAudioMuted`, `reload`, `setPageUrl` or `sendCommand` (B.3.3). An element
mounted by another webview is never found (`not-found`).

The mount's `onEvent` channel carries `{ name, data?, source }`, with
`source` `"guest"` (an ad page event) or `"host"` (a lifecycle event,
B.3.5).

#### A.2.6 `adview_event`

The ad guest's one command, granted only to webviews labelled `owad-*` while
they show a page under `https://www.overwolf.com/monsdk/electron/`.

- `slotId` is the label the shim claims; the caller's label decides.
- `name` is 1 to 64 characters of `[A-Za-z0-9_:.-]`.
- `data` whose JSON encoding exceeds 16 KiB becomes
  `{ truncated: true, bytes }`.
- Each guest has a token bucket (`ads.guestLimits`): `eventsPerSecond` (50)
  with bursts of `eventBurst` (100), and `bytesPerSecond` (262144) of encoded
  data. Messages over the limit are dropped and counted in the log once a
  minute. A guest that stays over the limit for 10 s is reloaded once, then
  closed.
- Names starting with `__host:` are internal (D.4) and never reach the
  element.

#### A.2.7 `cmp_event`

The consent windows' one command, granted only to webviews labelled
`ow-cmp*` while they show a page under
`https://content.overwolf.com/monsdk/electron/`, and only for windows the
plugin created. `name` is `ready`, `saveConsent`, `saveUnifiedConsent`,
`enableAdOptimization` or `close`; `data` is `{ consent?: string,
enabled?: boolean }`. A consent string is at most 16 KiB of printable ASCII
(`0x21` to `0x7E`). D.6.6 lists what each name does.

#### A.2.8 Updater

`updater_check`, `updater_download`, `updater_install` and
`updater_download_and_install` drive the update client (I). They answer
`unsupported` outside Windows and without the cargo feature `updater`.

### A.4 Errors

Every command error reaches JavaScript as
`{ code, message, data? }`. `data` is `{ status }` only for a `network` error
with an HTTP status.

| `code` | Meaning |
|---|---|
| `unsupported` | not available on this platform or build |
| `invalid-argument` | a malformed or refused argument |
| `not-found` | no such element, window, update or guest |
| `forbidden` | the caller is not allowed (wrong webview class or origin) |
| `io` | a state file could not be read or written |
| `network` | a request failed |
| `verification` | a downloaded update failed a check (I.3) |
| `backend` | a platform or webview call failed |
| `config` | the configuration is invalid for this call |
| `tauri` | Tauri itself returned an error |

A missing permission is Tauri's own rejection, before the command runs.

### A.5 Rust API

`app.overwolf()` (trait `OverwolfExt`) offers the same calls as the commands,
plus `uid()`, `cuid()`, `muid()`, `muid_v2()`, `phase_percent()`,
`utm_params()`, `is_test_ad()`, `config()`, `state_dir()`,
`prepare_for_restart()` and, on Windows with `updater`, `updater()` and
`updater_builder()`. A Rust caller may pass any `cmp_url` to the settings
window; the `allowedCmpOrigins` check applies to JavaScript only.
`Builder` methods (`test_ad`, `disable_anonymous_analytics`,
`disable_ads_optimization`, `disable_ads_fpd`, `host_label`,
`exclude_windows`) set the same values as the configuration
([api/rust.md](api/rust.md)).

### A.6 Start and exit

The plugin writes nothing and sends nothing during setup. Calls made in the
app's setup closure (for example `disable_anonymous_analytics()`) therefore
land before the launch burst, as calls at module load do in ow-electron.

At `RunEvent::Ready` the plugin, in order:

1. writes the state it found missing (machine ids on Windows, a per-install
   muid) and moves a corrupt `ow-tauri.json` aside (F.1);
2. applies the persisted anonymous-analytics preference;
3. marks the launch started;
4. reads the app webview's user agent, waiting at most 2.5 s (E.1);
5. sends the launch burst (E.2) and writes `firstLaunch`;
6. starts the consent round (D.6);
7. starts the window ticker (E.2 #5, #7, #9).

`RunEvent::ExitRequested` is never prevented, and the plugin never exits the
app. At `RunEvent::Exit` (and before `AppHandle::restart`) it ends every
visible period (E.2 #7) and drains the queued requests for at most 1.5 s.
ow-electron's main-webview liveness rules have no equivalent: there is no
hidden main webview.

---

## B. JavaScript API

### B.1 The API package

`tauri-plugin-overwolf-api` runs in the app's own webviews only (it is
browser-only). Each function calls one command:

| Function | Command |
|---|---|
| `getInfo()` | `get_info` |
| `getMachineIds()` | `get_machine_ids` |
| `isCMPRequired()` | `is_cmp_required` |
| `openAdPrivacySettingsWindow(options?)`, `openCMPWindow(options?)` | `open_ad_privacy_settings_window`, `open_cmp_window` |
| `generateUserEmailHashes(email)`, `setUserEmailHashes(hashes?)`, `clearUserEmailHashes()` | the `*_user_email_hashes` commands |
| `disableAnonymousAnalytics()`, `disableAdsOptimization()`, `disableAdsFPD()` | the `disable_*` commands |
| `setAnonymousAnalyticsPreference(enabled)` | `set_anonymous_analytics_preference` |
| `setExternalPaymentUserId(options?)` | `set_external_payment_user_id` |
| `setAnalyticsUserEnabled(enabled)` | `set_analytics_user_enabled` |
| `setWindowName(name)` | `set_window_name` |
| `check(options?)` and `Update` | the `updater_*` commands |
| `<owadview>` | the `adview_*` commands (B.3) |

Errors are `OverwolfError` with the A.4 `code`. With `app.withGlobalTauri`
the same functions are on `window.__TAURI__.overwolf`. The full API is in
[api/js.md](api/js.md).

### B.3 `<owadview>`

#### B.3.1 Element model

`owadview` has no hyphen, so it cannot be a custom element
([ADR 0003](adr/0003-owadview-native-child-webviews.md)). The element runtime
of `tauri-plugin-overwolf-api` makes it work in the app's webviews:

1. **Default style.** A constructed style sheet with
   `:where(owadview) { display: inline-flex; width: 100%; height: 100%; }`
   and a 0 x 0 `block` for `owadview[performance]`, as ow-electron's element
   computes [OBS]. `:where()` has zero specificity, so app rules win.
2. **Upgrade.** Elements created with `createElement('owadview')` (any case)
   are tracked at once; a `MutationObserver` picks up parsed and inserted
   ones. Properties and methods appear at attach (B.3.3), because
   ow-electron's element is a plain `HTMLElement` until then [OBS].
3. **Mounting.** An element mounts when it is connected and its box is not
   empty, or it has `performance`.

Elements inside shadow roots and iframes are not supported.

#### B.3.2 Attributes

HTML lower-cases attribute names. The runtime reads `cid`, `slotsize`,
`adstyle`, `customtracking`, `performance`, `unit` and `pageurl`.

| Attribute | Meaning | Change after mount |
|---|---|---|
| `cid` | container id reported with the ad; trimmed, at most 20 characters [DOC] | remount |
| `slotsize` | requested inventory `"WxH"`. Overwolf documents `400x300`, `400x600`, `300x250`, `160x600`, `728x90`, `970x90` and `400x60` [DOC]; all seven fill in test mode on ow-electron [OBS]. Other values pass through | remount |
| `adstyle` | style tokens such as `"high-impact-ad;"` and `"rewarded-ad;"`, passed through; the ad page matches them itself [OBS] | remount |
| `customTracking` / `customtracking` (also the property) | a JSON object string; invalid JSON clears it [DOC] | sent live as a `customTracking` message (D.5) and again after every later guest reload [OBS]; no remount |
| `performance` | an interstitial (performance) ad; one per window. A second one that would attach while one is mounted is removed from the document, as ow-electron removes it [OBS] | remount |
| `unit` | ad unit override, passed through in test and live mode [OBS] | remount |
| `pageurl` | the guest's `__overwolf__.pageUrl` (D.2) at mount [OBS] | applies at the next guest load |

#### B.3.3 Properties and methods

At attach the element gets ow-electron's own accessors `cid`, `slotsize`,
`pageUrl`, `performance`, `unit`, `adstyle` and `customTracking` (attribute
backed), and these methods on an inserted prototype [OBS]:

| Method | Behaviour |
|---|---|
| `setAudioMuted(muted)` | mutes or unmutes the guest; guests start muted [DOC] |
| `reload()` | reloads the guest |
| `setPageUrl(url)` | sets `pageurl` and, on an attached element, sends the running page `{ type: 'setPageUrl', data: [url] }` (D.5) [OBS] |
| `sendCommand(...args)` | sends the running page `{ type: 'sendCommand', data: [...args] }` with the arguments as JSON (D.5) [OBS]; ignored before attach |

Electron's generic `<webview>` methods (`executeJavaScript`, `send`, ...) are
not provided: Overwolf does not document them for `<owadview>`, and they
would give app code control over remote ad content [DEC].

#### B.3.4 Lifecycle and geometry

| Trigger | Command |
|---|---|
| connected, and a non-empty box or `performance` | `adview_mount` |
| resize, scroll, or a layout change of an ancestor (one per animation frame) | `adview_update { rect }` |
| visibility change | `adview_update { visible }` |
| a "remount" attribute changes | `adview_unmount`, then `adview_mount` |
| removed, or moved (removed and inserted in one task) | `adview_unmount` |
| the embedder document unloads | the plugin closes that webview's guests |

**Position.** The guest is a native child webview at the element's rectangle.
The plugin converts CSS pixels with the page zoom: on macOS the app
webview's native width divided by `innerWidth`, on Windows
`devicePixelRatio / scaleFactor`.

**Visibility** [OBS]. ow-electron signals a guest `hidden` when its element
is `display: none`, scrolled out of view, or its window is hidden or
minimized; a resize signals nothing. It turns `visible` from exactly half of
a slot inside the viewport, on either axis. The plugin reports a guest
visible only when all hold: the window is shown and not minimized; at least
half of the element is in the viewport; `checkVisibility({ opacityProperty:
true, visibilityProperty: true })` (polled every 500 ms); the embedder
document is visible. A performance element ignores the box conditions. A
hidden guest is hidden, not destroyed. The guest's own document follows
(D.5).

About 2 s after `hidden` the ad page gives up its ad and calls
`__overwolf__.reload()`; the host reloads the guest and the page waits until
it is visible again [OBS]. Chromium runs a hidden page's long timers only at
whole-second wake-ups; the shim aligns the guest main frame's timeouts of
1 s or more the same way while it reads `hidden`, and the host holds a reload
asked for while hidden until 2.5 s after `hidden` [DEC]. With this, a slot
hidden for 2 s keeps its ad, as on ow-electron [OBS: lab, reward-optin].

**Performance element** [OBS]. At attach it gets no shadow root, the inline
style `pointer-events: none;` and one child `div` styled
`position: fixed; top: 0px; left: 0px; width: 100vw; height: 100vh; background: transparent; z-index: 999999;`.
It turns `pointer-events: auto` just before its first
`performance_ad_loaded`. Until then the plugin lets input pass through the
guest (Windows an empty window region, macOS a `hitTest:` that returns
`nil`), so the page under an empty interstitial stays usable. The guest
covers the window's content area. After `shutdown` the runtime closes the
guest and removes the element in the next task; no `destroyed` follows.
The ad page itself answers a content area smaller than 500 x 500 with
`performance_ad_error` and `shutdown` [OBS].

**Z-order.** Native guests paint above the page. After each mount the
plugin raises the window's newest performance guest above the other guests,
as ow-electron's `z-index` does [OBS]. HTML that must cover an ad needs the
app to hide the element.

**Transparency.** With `ads.transparentGuests` (default) guests are
transparent, so an empty slot shows the app's container, as on ow-electron
[OBS].

**Removal.** An element removed or moved after attach is dead: its guest
closes and it never attaches again. It gets a plain `destroyed` event only
when it is in the document again by then (a move), as ow-electron's late
`destroyed` reaches only an attached element [OBS].

#### B.3.5 DOM events

Each guest message that is not internal (D.4) is dispatched on the element
exactly as ow-electron dispatches it [OBS]:

```js
const event = new Event(name, { bubbles: false, cancelable: false });
Object.assign(event, data);   // own properties; detail stays null
element.dispatchEvent(event);
```

`data` is copied as `Object.assign` copies it: an object gives its fields, an
array its indexes, a string one property per character (seen with
`performance_ad_error`). Keys the event already has are skipped. Names pass
unchanged and in order; `display_ad_loaded` arrives twice per fill and is
passed twice [OBS]. Names Overwolf documents in two spellings are also
dispatched in the other one, the received one first: `ad_clicked` and
`ad-clicked`, `house_ad_action` and `house-ad-action`.

Names seen from the ad page [OBS] or documented [DOC]: `display_ad_loaded`,
`impression`, `play`, `pause`, `ended`, `player_loaded`, `video_ad_ready`,
`complete`, `house-ad-action` (`{ action }`), `high-impact-ad-loaded`,
`high-impact-ad-removed`, `shutdown`, `performance_ad_loaded`,
`performance_ad_error` (a string), `performance_ad_dismiss`,
`performance_ad_clicked`, `performance_ad_video_complete`,
`performance_ad_video_skipped`, and the documented `performance_ad_no_fill`,
which no lab run has seen. Orders per format are in
[AD-FORMATS.md](AD-FORMATS.md).

Host lifecycle events (`source: "host"`):

| Event | When | Own properties |
|---|---|---|
| `did-attach` | the guest webview exists | none |
| `dom-ready` | the guest document's `DOMContentLoaded` | none |
| `did-finish-load` | the guest's main frame finished loading (held until `dom-ready`, so the two keep ow-electron's order) | none |
| `did-fail-load` | a load failed: the main frame on both platforms; sub-frames on Windows | `errorCode`, `errorDescription`, `validatedURL`, `isMainFrame`, `frameProcessId: 0`, `frameRoutingId: 0` |
| `render-process-gone` | the guest's web content process ended (D.7); ow-electron sends no `crashed` event [OBS] | `details: { reason, exitCode }` |
| `ad-clicked` | a click-out opened the system browser (D.7) | `url` |
| `destroyed` | B.3.4, "Removal" | none |

The host `ad-clicked` is dispatched only if no guest click spelling was
dispatched for that element in the previous 1000 ms.

ow-electron forwards all of Electron's `<webview>` events, among them
`did-start-navigation`, `load-commit`, `console-message`, `did-frame-*` and
`media-*` [OBS]. The plugin emits the rows above only (a known gap).

### B.4 Typings

The package exports the TypeScript types of every function, `OverwolfError`,
`OverwolfErrorCode`, the `<owadview>` event names, and JSX typings for
`owadview` (`tauri-plugin-overwolf-api/jsx`).

---

## D. Guest pages

The ad page is `https://www.overwolf.com/monsdk/electron/latest/adview.html`.
The consent pages are under
`https://content.overwolf.com/monsdk/electron/latest/cmp/22.3.27/`:
`ow-cmp-v2.html` for the hidden windows, `cmp.html` for the settings window
[OBS]. The plugin never changes those pages. It loads them with the inputs
ow-electron gives them (URL, query, headers, cookies, globals), so they
behave and report as they do under ow-electron.

The plugin injects one script into each: `adview-host.js` into ad guests,
`cmp.js` into consent windows. Both are built from `packages/guest-shims`,
committed under `crates/tauri-plugin-overwolf/js/` with a CI drift check, and
embedded in the crate ([ADR 0012](adr/0012-js-runtime-singleton.md)).

Guest pages are remote content. They get one command each (A.2.6, A.2.7),
and everything they send is untrusted
([ADR 0011](adr/0011-remote-guest-ipc.md)). Any script in the ad page's main
frame can call the command; the limits of A.2.6 and D.7 bound it, not the
shim.

### D.1 Injection rules

- Main frame only, at document start, before page scripts.
- The ad shim does nothing unless `location.origin` is
  `https://www.overwolf.com`; the consent shim, unless the page is under
  `https://content.overwolf.com/monsdk/electron/`.
- A second run in the same document does nothing.
- Rust replaces the token `/*__OW_TAURI_ADVIEW_CONFIG__*/null` (ads) or
  `/*__OW_TAURI_CMP_CONFIG__*/null` (consent) with JSON in which `<`, U+2028
  and U+2029 are escaped.
- The shim keeps the transport (`window.__TAURI_INTERNALS__.invoke`) in a
  closure at startup. Messages wait in a queue of at most 200, retried every
  250 ms, until it exists.
- Functions the shim gives the page are bound functions, frozen, with
  `length` 0, so their source reads `function () { [native code] }` like
  ow-electron's [OBS].

**Ad guest configuration.** Rust builds one object per guest. The page never
sees the object itself.

| Key | Use |
|---|---|
| `muid` ... `customTracking` | the D.2 data keys, in D.2 order (`customTracking` only when the attribute holds a JSON object) |
| `slotId` | the guest label `owad-<n>`, sent back in every `adview_event` |
| `visibilityState` | `visible` or `hidden` at document start |
| `hostKey` | the name of a non-enumerable, read-only window property that holds the host API (D.5): `_` and 32 random hex digits, new for every guest |
| `documentReferrer` | Windows only: the value the shim answers for `document.referrer` (D.8.3) |

### D.2 `window.__overwolf__` data

Defined with `writable: false, configurable: false, enumerable: true` and
deep-frozen [OBS]. Keys appear in this order, which is the order the page
enumerates in ow-electron [OBS]:

| # | Key | Value |
|---|---|---|
| 1 | `muid` | `<muid>` (E.4) |
| 2 to 10 | `setMute`, `triggerEvent`, `applySetting`, `crash`, `reload`, `getSystemInformation`, `getCustomTracking`, `hasWindowFocus`, `onmessage` | D.3 |
| 11 | `uid` | `<uid>` |
| 12 | `name` | `<PN>` |
| 13 | `owVersion` | `<owVersion>`, or `ads.owVersionOverride` |
| 14 | `version` | `<ver>` |
| 15 | `windowName` | the embedder window's analytics name (E.2 #7; `"index"` for the app's start page) |
| 16 | `windowTitle` | the embedder's `document.title` at mount (at most 1024 characters) |
| 17 | `windowFocused` | embedder focus at document start (`false` observed) |
| 18 | `testAd` | `true` in test mode (D.7) |
| 19 | `consent` | `cmp.unifiedConsentString` from `ow-electron.json` at launch (URL-encoded, `cmp%3D...`), or `""` [OBS]. Later saves reach running guests as messages (D.5) and new guests through cookies (D.6.3) |
| 20 | `consentFull` | the same value as `consent` [OBS] |
| 21 | `slotSize` | element `slotsize` |
| 22 | `containerId` | element `cid` |
| 23 | `systemInfo` | below |
| 24 | `settings` | `{ disableOptimization, anonymous: false }`. `disableOptimization` is `true` after `disableAdsOptimization()` or with `ads.disableOptimization`; `anonymous` stays `false` even after `disableAnonymousAnalytics()` [OBS] |
| 25 | `muidV2` | `<muidV2>` (E.4) |
| 26 | `phasePercent` | E.4 |
| 27 | `pageUrl` | element `pageurl` at mount, `""` when absent, or the last `setPageUrl()` value after a reload [OBS] |
| 28 | `performanceAd` | element has `performance` |
| 29 | `adStyle` | element `adstyle`, or `""` |
| 30 | `unit` | element `unit`, or `""`, unchanged in test mode [OBS] |
| 31 | `customTracking` | the parsed attribute at mount; the key is absent without the attribute, as on ow-electron [OBS] |

ow-electron exposes no `runTimeInfo` and no `emailHashes` key; email hashes
reach the page only as `eHashes` messages (D.5) [OBS].

**`systemInfo`** (`getSystemInformation()` returns a copy):

```json
{"gpus":[{"name":"","model":"","driverVersion":"","vendor":""}],
 "cpu":"<CPU brand string>",
 "displays":[{"name":"Built-in Retina Display","isMain":true,"position":[0,0],"resolution":[1470,956],"dpi":192}]}
```

| Field | Value |
|---|---|
| `cpu` | the CPU brand string: macOS `machdep.cpu.brand_string`; Windows `HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0\ProcessorNameString` [INF] |
| `gpus` | macOS: one entry of four empty strings [OBS]. Windows: one entry per DXGI adapter, `driverVersion` as `a.b.c.d` and the other fields empty [OBS: Windows lab] |
| `displays` | one per display: `name` (macOS `NSScreen.localizedName`; Windows the monitor's friendly name [OBS: Windows lab]), `isMain`, `position` and `resolution` in logical pixels, `dpi` = `round(96 x scaleFactor)` |

There are no `os`, `arch` or `scaleFactor` keys [OBS]. Overwolf's privacy
policy covers this data [DOC] (OQ-19).

### D.3 `window.__overwolf__` functions and `window.gc`

| Function | Behaviour |
|---|---|
| `setMute(muted)` | `__host:setMute { muted }`; Rust mutes or unmutes the guest |
| `triggerEvent(name, ...args)` | an `adview_event` `name` with the single argument, or the argument array when there are several; `message` and `messageerror` are dropped |
| `applySetting(setting)` | `__host:applySetting`; logged once, no effect. The ad page calls it with `{ enableHashes: true }` on every load [OBS] |
| `crash()` | `__host:crash`; handled as a crash (D.7) |
| `reload()` | `__host:reload`; Rust reloads the guest 70 ms later, as ow-electron does [OBS], or holds it while hidden (B.3.4) |
| `getSystemInformation()` | a copy of `systemInfo` [OBS] |
| `getCustomTracking()` | a copy of the current `customTracking` [OBS] |
| `hasWindowFocus()` | the embedder window's focus [OBS] |
| `onmessage(handler)` | registers a handler (at most 16); host messages (D.5) reach every handler as a fresh copy; handler errors are swallowed |

`window.gc` is a no-op function when the page has none, as in
ow-electron's guest [OBS].

### D.4 Guest to host

`adview_event { slotId, name, data }` (A.2.6). Before sending, the shim drops
functions, DOM nodes and `Window` objects, cuts cycles, flattens `Event`
objects to `{ type }`, and applies the 16 KiB rule. Rust applies the same
rules, because page scripts can call the command without the shim.

Internal names (handled by Rust, never dispatched):

| Name | Data | Effect |
|---|---|---|
| `__host:ready` | `{ href, testAd, visibilityState, pageUrl }` | the shim started |
| `__host:domReady` | none | the document's `DOMContentLoaded`; Rust dispatches `dom-ready`, then a held `did-finish-load` (B.3.5) |
| `__host:focus` | `{ focused }` | guest focus; recorded only |
| `__host:setMute`, `__host:applySetting`, `__host:crash`, `__host:reload` | D.3 | |
| `__host:gesture` | any | ignored: user activation comes from the OS (D.7) |

Any other `__host:` name is dropped. Every other name goes to the element
(B.3.5).

### D.5 Host to guest

Rust calls the guest's host API with `webview.eval` and a script of the form
`(function(h){h&&h.deliver(<json>)})(window["<hostKey>"])`. A document that
has not run the shim, or of another origin, ignores it. `deliver` checks
`{ type: string, data? }` and passes it to the `onmessage` handlers.

ow-electron passes the page exactly these messages [OBS] (OQ-13), and the
plugin sends the same:

| `type` | `data` | Sent when |
|---|---|---|
| `consent` | string | a consent page saves (D.6.6): **twice**, first the TCF string (`saveConsent`), then the stored, URL-encoded unified string `cmp%3D...` (`saveUnifiedConsent`). To every existing guest, also one still loading; not resent after a reload [OBS] |
| `customTracking` | object or `null` | the element's `customTracking` changed, and again after every later reload of that guest [OBS] |
| `eHashes` | the value passed to `setUserEmailHashes(value)` as given, or `{}` when it is `undefined` or falsy; `{ sha1, md5, sha256 }` from `generateUserEmailHashes()` | every `setUserEmailHashes()`, `generateUserEmailHashes()` and `clearUserEmailHashes()` call; to every existing guest; not resent after a reload or to a new guest [OBS]. After `disableAdsFPD()` only the `{}` of a clear is sent |
| `window-minimized` | none | the embedder window was minimized, after the guest document turned `hidden` [OBS: both labs] |
| `window-hidden` | none | the embedder window was hidden; on macOS also right after `window-minimized`; on Windows a minimize sends `window-minimized` only [OBS: both labs]. Nothing is sent when the window is shown again |
| `sendCommand` | array of the arguments | `element.sendCommand(...args)` (B.3.3) [OBS] |
| `setPageUrl` | `[url]` | `element.setPageUrl(url)` (B.3.3) [OBS] |
| `ad-clicked` | URL string | a click-out opened the system browser (D.7); plugin only [DEC; OQ-17] |

`disableAdsFPD()`, `disableAdsOptimization()` and resizes send nothing
[OBS]. A running performance ad dismisses itself after a minimize
(`performance_ad_dismiss`) in every macOS run on both hosts; on Windows
ow-electron's dismisses in some runs only [OBS].

**State signals** [OBS]. On every guest load ow-electron mutes the guest and
signals its visibility and focus; later it signals focus when the embedder
window gains or loses it. The plugin does this inside the shim, never through
`onmessage`:

- `setVisibility('visible' | 'hidden')` overrides `document.visibilityState`
  and `document.hidden` and fires `visibilitychange`. Like ow-electron's
  guest, the document first sees events that still read the old state: one
  on a hide, three on a show [OBS: lab]. A repeated state fires nothing. The
  engine's own `visibilitychange` is stopped before any page listener, so the
  host alone drives it.
- `setEmbedderFocus(<bool>)` drives `hasWindowFocus()` and
  `document.hasFocus()`.
- `setNextPageUrl(<url>)` stores the `pageUrl` of the next load (at most
  2048 characters) in `sessionStorage`.

On Windows a minimize also hides the guest webviews natively until the
restore, because a minimized ow-electron window has an empty client area
[OBS].

### D.6 Consent

ow-electron's consent flow opens a hidden startup consent window on every
launch, and the settings window that `openAdPrivacySettingsWindow()` opens;
the first settings call of a launch also opens a hidden default-consent
window [OBS] [DOC]. Consent reaches ads through cookies the consent page
writes in the ads data store, and through `consent` messages to running
guests (D.5) [OBS].

#### D.6.1 Startup consent window

At `RunEvent::Ready`, after the launch burst, the plugin sends
`cmp-eu-only` (D.6.2). When that request completes, whatever its outcome
(any status, an invalid body, a dropped connection, the timeout), it creates
`ow-cmp-startup` [OBS]:

- 1 x 32 logical pixels, centred, title `<PN>`, never shown, not focusable,
  no decorations, not in the taskbar; ads data store and `<UA>` (D.8.1).
- URL:
  `https://content.overwolf.com/monsdk/electron/latest/cmp/22.3.27/ow-cmp-v2.html?unifiedcmp=<X>&muid=<muid>&uid=<uid>&muidv2=<muidV2>&oweVersion=<owVersion>&appVersion=<ver>`,
  keys in that order. `<X>` is `""` when nothing is stored; otherwise the
  stored `cmp.unifiedConsentString` (already URL-encoded) encoded **once
  more**, so `cmp%3D...` becomes `cmp%253D...`.
- A plain navigation: no extra headers; no Origin, Referer or cookie on a
  first launch [OBS].
- The page closes itself with `window.close()` (`cmp_event close`) about 0.5
  to 1.5 s after it loads [OBS]. The plugin closes it after
  `consent.readyTimeoutMs`, or when its load fails [DEC].
- `isCMPRequired()` resolves when this page finishes loading, or right after
  its load fails [OBS].
- When consent is not required (D.6.2) the window still opens, on
  `ow-cmp-v2.html?clear=true` [OBS: Windows lab].
- It sends no analytics of its own (E.2).

What the page does [OBS]:

| | First launch (no stored consent) | Later launches |
|---|---|---|
| consent | generates a default **Full** consent | reuses the stored consent |
| fetches | `https://content.overwolf.com/cmp/v3/vendor-list.json`, `.../cmp/v3/gac/gac.json` | `gac.json` only |
| page analytics | Counter `electron_cmp_accept_full_launch` | none |
| state file (D.6.6) | `cmp` written | `cmp` rewritten; only `timeStamp` changes |
| cookies | `euconsent-v2`, `acconsent` inserted | both rewritten with a new expiry |

**Last window.** When the last app window is destroyed while the startup
window is open, the plugin keeps it until its page has saved or asked to
close, or until `consent.readyTimeoutMs` [DEC; awaits a lab observation,
see [OPEN-QUESTIONS](OPEN-QUESTIONS.md)].

#### D.6.2 `isCMPRequired()`

- One `GET https://features.overwolf.com/experiments/cmp-eu-only` per launch,
  with the host request headers of E.1 plus `cache-control: no-cache`, sent
  after the launch burst [OBS]. The observed response is `{"params":[]}`.
- Every `isCMPRequired()` call waits for it and for the startup page's load
  (D.6.1); later calls resolve at once. The result is not stored: the next
  launch asks again [OBS].
- **Timeout.** ow-electron sets none: with the server hung for 45 s the call
  resolved after 45.8 s [OBS]. The plugin stops waiting after
  `consent.euOnlyTimeoutMs` (60 s) and treats it as a failed request
  (consent required) [DEC].
- The result is `true` for every response ow-electron was given [OBS]:
  `{"params":[]}`, other `params` values, HTTP 500 and 404, invalid JSON and
  a dropped connection.
- **`{"params":["no-cmp"]}`** (the answer outside the consent region) [OBS:
  Windows lab]: `false`. The startup window loads
  `ow-cmp-v2.html?clear=true`, whose page calls `saveConsent("")` and
  `saveUnifiedConsent("")`; `cmp` is stored as
  `{"cmpString":"","timeStamp":0,"unifiedConsentString":""}`; no consent
  cookies exist; the settings window URL carries `cmpRequired=false`; no
  default-consent window opens. The plugin answers `false` only when
  `params` is an array holding the string `"no-cmp"`.
- **A `{}` body** (no `params`) [OBS]: the result is not cached, so every
  call sends a new request and opens a new startup window
  (`ow-cmp-startup-<n>`). The plugin does the same.

#### D.6.3 Consent cookies

- `euconsent-v2=<cmpString>` and `acconsent=<AC>`; domain `.overwolf.com`,
  path `/`, `Secure`, `SameSite=None`, not `HttpOnly`, expiring 365 days
  after they are written [OBS].
- **The consent page writes them**, on every launch, while the startup window
  is open, before the first ad document request [OBS].
- The plugin does not write them, unless the fallback applies: with
  `consent.hostCookieFallback: "auto"` (default), if both cookies are missing
  from the ads data store after the startup window closed, Rust writes them
  with the attributes above from the stored `cmp` values. `"never"` turns
  this off [DEC].

#### D.6.4 Settings window

`openAdPrivacySettingsWindow()` and `openCMPWindow()` behave the same [OBS].

- **Window** [OBS]: label `ow-cmp`, title `"CMP"`, 800 x 800 and centred
  unless the options say otherwise, not resizable, not maximizable,
  minimizable, background `#0D0D0D` (or `backgroundColor`). A `modal`
  window without a `parent` is parented to the caller's window. Ads data
  store and `<UA>`.
- **Content**: a local preloader (spinner in `preLoaderSpinnerColor`), then
  the consent page [OBS].
- **URL**: `consent.cmpUrl`, else the options' `cmpURL`, else
  `https://content.overwolf.com/monsdk/electron/latest/cmp/22.3.27/cmp.html`
  [OBS], with the query
  `uid=<uid>&appName=<PN>&tabName=<tab>&lang=<language>&firstRun=<b>&cmpRequired=<b>&muid=<muid>&muidv2=<muidV2>&oweVersion=<owVersion>&appVersion=<ver>`
  in that order [OBS]. `tabName` defaults to `purposes`, `lang` to `en`.
  `appName` is not URL-encoded, so a space reaches the wire as `%20` [OBS].
  `firstRun` is `true` on the app's first launch, `cmpRequired` is the
  `isCMPRequired()` result [INF; **Unknown (R3-5)** on later launches].
- **`cmpURL` from JavaScript** must be `https:` and its origin must be in
  `consent.allowedCmpOrigins`; otherwise `invalid-argument`. The typings
  accept any URL [TYPES]; the check is a plugin decision [DEC]. The consent
  globals and `cmp_event` exist only under
  `https://content.overwolf.com/monsdk/electron/`.
- **Promise** [OBS]: resolves once the window exists, not when it closes. A
  second call while it is open focuses it and resolves.
- **Close** [OBS]: writes nothing and sends no host analytics. The page
  fetches `https://features.overwolf.com/get-supported-consent` itself.
- **Default-consent window** [OBS]: the first call of a launch, when consent
  is required, also opens a hidden 1 x 32 window `ow-cmp-default` on
  `https://content.overwolf.com/monsdk/electron/latest/cmp/22.3.27/ow-cmp-v2.html?unifiedcmp=&firstRun=true`.
  That page generates a **new default consent** and saves it, overwriting
  `cmp`, both cookies, and sending `consent` messages (D.5). The plugin copies
  this; Overwolf is asked whether it is intended (OQ-38). It follows the
  startup window's rules.

#### D.6.5 Ads and consent order

ow-electron attaches guests at once. When consent is required, a guest
mounted while the startup window is open loads its ad document within 3 ms
of that window's close [OBS: macOS lab]. The plugin does the same, with a
bound [DEC]:

1. A guest's first navigation waits until the startup window has closed, or
   until 3 s after the mount, whichever comes first.
2. When `cmp-eu-only` says consent is not required, guests navigate as soon
   as the clearing page opens.
3. If the startup window fails to load, guests navigate at once.

`adview_mount` itself never waits and never fails because of consent.

#### D.6.6 Consent page globals and storage

`cmp.js` defines, in the consent windows and only under the scope above,
frozen functions with `length` 0; the page gets no `window.overwolf` [OBS]:

| Global | Behaviour |
|---|---|
| `window.cmp.saveConsent(value)` | `cmp_event saveConsent { consent }` (a TCData object is reduced to its `tcString`; `""` clears). Rust stores `cmp.cmpString` and `cmp.timeStamp` = now in seconds (F.2) |
| `window.cmp.saveUnifiedConsent(value)` | `cmp_event saveUnifiedConsent { consent }`; Rust stores `cmp.unifiedConsentString`, URL-encoded (F.2) |
| `window.privacy.enableAdOptimization(enabled)` | `cmp_event enableAdOptimization { enabled }`; stored as `adOptimization` in `ow-tauri.json` |
| `window.privacy.getIsAdOptimizationEnabled()` | the stored value; before one is stored, `true` on Windows and `false` on macOS, as ow-electron answers [OBS] |
| `window.close()` | `cmp_event close`; Rust closes the window |

After each save Rust writes the state file atomically and sends the matching
`consent` message to every existing guest (D.5) [OBS]. Popups of a consent
page open in the system browser (`https:` only).

### D.7 Test mode, click-outs and recovery

- **Test mode** (`--test-ad`, `OW_TAURI_TEST_AD=1`, `ads.testAd`, or
  `Builder::test_ad(true)`): `testAd: true`; every attribute passes through
  unchanged (D.2). Otherwise ads run live, as ow-electron does without
  `--test-ad` ([ADR 0005](adr/0005-ads-test-live-parity.md)). Request shaping
  is the same in both modes [OBS].
- **Click-outs** [DEC; OQ-17]. A guest popup is never opened in the app. A
  top-level navigation of a guest away from `https` Overwolf hosts is
  cancelled. Either one opens in the system browser only when the OS reports
  a user action on that guest, within `guestLimits.activationWindowMs`
  (5000 ms); one action allows one open:
  - Windows: WebView2's `IsUserInitiated`, or native input over the guest just
    before a script-started navigation;
  - macOS: a mouse-down that hits the shown guest in the key window, or
    Return, Space or keypad Enter while the guest has focus.

  The page's own messages never count. At most
  `guestLimits.externalOpensPerMinute` (20) opens per guest and
  `externalOpensPerMinuteApp` (20) per app; only `http` and `https` URLs
  without credentials. Each open sends `ad-clicked` to the guest (D.5) and
  the element (B.3.5). Everything else is dropped and logged.
- **Mute.** Guests start muted [DOC].
- **Crash recovery** [OBS] (OQ-28). A guest whose web content process ends
  is reloaded in place, with no cap (six crashes in a row all recovered).
  The element gets `render-process-gone`; the crash is reported (E.2 #8).
  `ads.maxRecoveries` (default none) is a plugin option that closes the
  guest after that many recoveries. On macOS the app wires the terminate hook
  ([api/rust.md](api/rust.md#the-macos-terminate-hook)); without it a crashed
  guest stays blank and a warning is logged at setup.
- **Load errors** [OBS]: a failed main-frame load is retried every
  `ads.loadErrorRetryMs` (5000 ms), with no cap and no analytics.
- **Reload on macOS.** A reload the page asks for recreates the guest
  webview (`ads.recreateOnReload`, at most one per 30 s and 30 per hour,
  otherwise an in-place reload), carrying the page's `sessionStorage` over.
  This is how WebKit releases the old page's memory; ow-electron's renderer
  does not grow across reloads [OBS].

### D.8 Request shaping and guest webviews

#### D.8.1 Guest webviews

| Setting | ow-electron [OBS] | The plugin |
|---|---|---|
| data store | the default session | the **ads data store**, shared by the consent windows and every ad guest. Windows: user data folder `<appData>/<PN>/EBWebView-ow`; macOS: the default `WKWebsiteDataStore` (shared with the app, as ow-electron's guests share the default session [OBS]) |
| web security | off | Windows: `--disable-web-security` in the ads environment; macOS: no public API, so it stays on (a known gap) |
| insecure content | allowed | Windows: `--allow-running-insecure-content`; macOS: platform default |
| background throttling | off for guests | Windows: the three `--disable-*-throttling/backgrounding` switches below |
| user agent | the app UA | `<UA>` (E.1) |
| `window.gc` | a function | D.3 |
| audio | muted at start | muted at start |

The Windows ads environment's browser arguments are exactly:

```
--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection
--disable-background-timer-throttling --disable-renderer-backgrounding
--disable-backgrounding-occluded-windows --disable-web-security
--allow-running-insecure-content [<ads.browserArgs>...]
```

App webviews keep Tauri's defaults. Web security is off only in the ads
environment, whose webviews run no app code and hold one command each
([ARCHITECTURE 5.3](ARCHITECTURE.md#53-remote-content-rules)). Frames inside
guests may load `http`, `https`, `about`, `data` and `blob` URLs only, and
never a `*.localhost` host, so a guest cannot reach the app's own origin.

#### D.8.2 Wire shape

**Document request** [OBS]: `GET https://www.overwolf.com/monsdk/electron/latest/adview.html`,
no query string, headers in this order:

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
Header order inside HTTP/2 and HTTP/3 frames is up to the engine.

**Subresources** from the guest, in any frame and from any initiator, get
`Origin: https://www.overwolf.com`, replacing any value [OBS]. Exceptions
carry **no** `Origin` [OBS]:

- the ad library
  `https://content.overwolf.com/libs/ads/latest/owads.min.js?uid=<uid>&phase=<phasePercent>&window=<windowName>`,
  which instead gets `x-ow-uid: <uid>`, `x-ow-phase: <phasePercent>` and
  `x-ow-window: <windowName>`, appended last;
- keepalive beacons sent after unload.

Apps get no hook into guest requests. The consent windows get no shaping:
ow-electron does not change their requests [OBS].

#### D.8.3 Per platform

| Platform | Document `Referer` and `Origin` | Subresource `Origin` | `x-ow-*` on `owads.min.js` |
|---|---|---|---|
| Windows (WebView2) | a `WebResourceRequested` handler on the guest | the same handler | the same handler |
| macOS (WKWebView) | the first navigation is a `URLRequest` carrying both headers | **gap**: no public API | **gap**: no public API; uid, phase and window still reach the server in the query string |
| Linux | no ad guests | | |

- **Windows.** WebView2 puts the changed headers on the wire with
  ow-electron's values [OBS: Windows lab]. **Gap:** WebView2 lets the host
  change a request once, not at each redirect hop; after a cross-origin
  redirect Chromium sends `Origin: null` on the next hops (cookie-sync pixels,
  about 1 to 2 % of a guest's requests), where ow-electron sets
  `https://www.overwolf.com` again [OBS: Windows lab]. WebView2 leaves
  `document.referrer` empty after a host navigation, so the shim answers it
  from `documentReferrer` while the platform's value is empty.
- **macOS.** The subresource `Origin` and the `x-ow-*` headers cannot be set
  with public WebKit API, and web security cannot be turned off. Test ads
  fill without them; the macOS lab check compares live fill against
  ow-electron ([PARITY.md](PARITY.md#lab-checks)). The gap is reported to
  Overwolf (OQ-05). `ads.macPrivateHeaderApi` (off) is reserved for a
  prototype of WebKit's private per-request header API; the default build uses
  no private API.

---

## E. Analytics

ow-electron sends anonymous app analytics by default;
`disableAnonymousAnalytics()` reduces them to a mandatory set [DOC]. The
plugin sends the same requests, in the same order, with the same fields, as
ow-electron 42.11.4 [OBS], with the host label in place of "electron"
(section 0). Rust makes every host request.

### E.1 User agent and request shape

**User agent** (`<UA>`). ow-electron uses one UA for host requests, ad guests
and consent windows:
`Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) <PNNS>/<ver> Chrome/148.0.7778.280 Electron/42.11.4 Safari/537.36`
[OBS]. The plugin composes `<UA>` from the platform webview's own UA, which
it reads from the first app webview after `RunEvent::Ready` (at most 2.5 s;
until then a per-OS template):

- With a ` Chrome/<x>` token (WebView2): `<PNNS>/<ver> ` goes right before
  `Chrome/`, and ` <Label>/<hostVersion>` right after the `Chrome/<x>`
  token, where Electron puts its tokens.
- Otherwise (WKWebView): ` <PNNS>/<ver> <Label>/<hostVersion>` is appended.
  When the UA has `AppleWebKit/<w>` but no `Safari/` token and the installed
  Safari's version is known, Safari's own tokens are added the way Electron
  keeps Chromium's: ` <PNNS>/<ver> Version/<major.minor> <Label>/<hostVersion> Safari/<w>`
  [DEC]. A WKWebView UA without browser tokens is rated an unknown browser by
  ad stacks.
- The engine part is never faked: no `Chrome/` token is added to a WebKit
  UA. Overwolf's ad policy forbids invalid traffic [DOC].
- The same `<UA>` is used for every host request, every ad guest and every
  consent window. App windows keep the platform default.

**Host request headers** [OBS], in this order (HTTP/2 pseudo-headers come
first):

```
[content-length: N]               (POST only)
[content-type: application/json]  (POST only)
[cache-control: no-cache]         (cmp-eu-only only)
sec-fetch-site: none
sec-fetch-mode: no-cors
sec-fetch-dest: empty
user-agent: <UA>
accept-encoding: gzip, deflate, br, zstd
accept-language: <locale>
priority: u=4, i
```

- **No cookies.** ow-electron's host requests send no cookie and store none
  [OBS]. The plugin's client has no cookie jar.
- **No `accept`**, no `Origin`, no `Referer` [OBS]. The client (hyper) adds
  no header of its own; over HTTP/1.1 it adds `host`.
- `accept-language` is the app locale in Chromium's form (`en-US`) [OBS].
- Responses in gzip, deflate, brotli and zstd are decoded.
- HTTP/2 by ALPN, as observed. Idle connections stay open until the server
  closes them: ow-electron sends the close counter on its existing session
  after 90 s or more of silence [OBS].
- One attempt, 30 s timeout (60 s for `cmp-eu-only`, D.6.2), no retry, no
  persistence. Failures are logged at debug level.

**Counter** [OBS]:
`GET https://analyticsnew.overwolf.com/analytics/Counter?Name=<event>&MUID=<muid>&MUIDV2=<muidV2>&owver=<owVersion>&Extra=<json>`

- Query keys in the order `Name, MUID, MUIDV2, owver, Extra`, encoded like
  `URLSearchParams` (space becomes `+`). No body. The response is `200 {}`.
- `Extra` is compact JSON with keys in this order: `app_ver` (`<ver>`),
  `app_id` (`<uid>`), `os` (`darwin`, `win32` or `linux`), `os_ver` (what
  Node's `os.release()` returns: the Darwin kernel release on macOS,
  `10.0.<build>` on Windows), `app_name` (`<PN>`, spaces kept), `app_cuid`
  (`<cuid>`), then the event fields in E.2 order.

**InsertStats** [OBS]:
`POST https://tracking.overwolf.com/tracking/InsertStats?Stats=true&owver=<owVersion with "." replaced by "_">`,
`content-type: application/json`.

- Body, compact JSON in this key order:
  `{"Kind":<n>,"Extra":"<app_ver>.<uid>.<os>.<PN>.<cuid>"}`.
- In each value `.` and `:` become `_`; spaces are kept.
- Kind 400024 puts the reason first:
  `"Extra":"<reason>.<app_ver>.<uid>.<os>.<PN>.<cuid>"` [OBS].

### E.2 Events and order

| # | Trigger | Counter `Name` (fields, in order) | InsertStats Kind | With `disableAnonymousAnalytics()` |
|---|---|---|---|---|
| 1 | launch, `firstLaunch` absent from `ow-electron.json` | `<label>_app_first_launch` | 400022 | Counter **kept**, Kind dropped |
| 2 | launch, after the burst | `GET https://features.overwolf.com/experiments/cmp-eu-only` (D.6.2) | none | kept |
| 3 | every launch | `<label>_app_start` | none | dropped |
| 4 | every launch | `<label>_app_heartbeat` (`hasVisibleWindow`: `false`) | 400023 | kept (both) |
| 5 | the first counted window becomes visible | `<label>_app_heartbeat` (`hasVisibleWindow`: `true`) | 400023 | kept (both) |
| 6 | an ad guest attaches (one per `<owadview>`, test and live) | none | 400025 | dropped |
| 7 | a visible period of a counted window ends | `<label>_window_closed` (`name`, `title`, `length`) | none | dropped |
| 8 | an ad guest's process ends, unless within 10 s of its last load or recovery | `<label>_owadview_crashed` (`sessionTS`, `reason`) | 400024 | dropped |
| 9 | hourly check: 12 h since the last heartbeat | as #4, `hasVisibleWindow` = now | 400023 | kept |
| 10 | `setExternalPaymentUserId(options)` | `<label>_sub_info` (below) | none | kept [DEC; **Unknown (R3-3)**] |

All [OBS] unless marked.

**Order and timing.** ow-electron sends #1, #2, #3, the #4 Counter, 400022 and
400023 in that order within about 100 ms of Electron's `ready`; #5 follows
when the first window is shown [OBS]. The plugin sends #1, #3, #4, 400022
and 400023 at `RunEvent::Ready`, once the UA is known (A.6), then #2 on the
same lane. Calls made in the app's setup closure therefore apply to the
burst, as calls at module load do in ow-electron. #6 leaves when the guest
webview exists, never before the burst. Host requests leave in the order
they are made.

**Second launch:** #1 and 400022 are not sent [OBS].

**#7 fields** [OBS]:

- `name`: the window's analytics name, fixed when the window is first seen
  visible with a page loaded. It is the last path segment of the page URL,
  decoded, without query or fragment; a trailing `.html` or `.htm` (any
  case) is removed and other extensions are kept; only letters, digits, `-`,
  `_` and `.` remain; no truncation. An empty segment gives the host name;
  `about:blank` gives `blank` [OBS]. For the app's own origin an empty or
  `/`-ending path counts as `index.html`, because Tauri serves `index.html`
  at `/` where ow-electron loads `file://.../index.html` [DEC]. A name set
  with `setWindowName` (A.2.3) wins.
- `title`: the title the window is configured with in `tauri.conf.json`, else
  its native title; Tauri's placeholder `Tauri App` or an empty title gives
  `<PN>`, as an Electron window shows the app name [DEC from OBS: ow-electron
  reports the `title` constructor option, else `<PN>`].
- `length`: whole seconds visible, rounded down. A period shorter than 1 s
  sends nothing.
- A period ends when the window is hidden, minimized, destroyed, or at exit
  while visible. Showing it again starts a new one [OBS].
- Never counted: windows that were never shown, ad guests, every `ow-cmp*`
  window (ow-electron sends nothing for its settings window either [OBS]),
  and labels matching `analytics.excludeWindows`.

**#8 fields** [OBS]: `sessionTS` is whole seconds since the guest's last load
or recovery; `reason` is Electron's `render-process-gone` reason (`killed`
observed; the plugin maps WebView2 and WebKit terminations to `killed`,
`crashed`, `oom`, `abnormal-exit` or `launch-failed` [DEC]). Crashes 2 s
after a recovery were recovered but not reported; the threshold lies between
3 and 20 s: **Unknown (R3-4)**, interim 10 s [DEC]. The Counter goes before
the reload, 400024 after it.

**#9** [OBS: a 13-hour session]: ow-electron sent one heartbeat Counter and
one 400023 after 12 h and nothing else. The plugin checks hourly and sends
once 12 h have passed since the session's last heartbeat. ow-electron's
request went out as a conditional request from Chromium's HTTP cache; the
plugin has no HTTP cache and sends the plain request to the same server.

**#10 fields** [OBS]: the Counter only. `Extra` holds the six base fields,
then the options' own keys in the order the app passed them, then
`providerName: "tebex"` when the options have none (or an empty one).

Not sent: events of the Windows ad-optimisation helper, which the plugin does
not ship (OQ-14). Email hash calls send no host request [OBS]. There are no
other host requests.

### E.3 Opt-outs and switches

| Switch | Effect |
|---|---|
| `disableAnonymousAnalytics()` (or `analytics.disableAnonymous`, or `Builder::disable_anonymous_analytics`) before `RunEvent::Ready` | only the mandatory set for the session: #1 Counter, #2, #4, #5, #9 [OBS], and #10 [DEC]. Dropped: #3, 400022, #6, #7, #8 |
| the same call after `RunEvent::Ready` | the mandatory set from then on; one warning says the burst already went out |
| `setAnonymousAnalyticsPreference(false)` | stored; acts as the first row from the next launch |
| `setAnalyticsUserEnabled(false)` (**plugin option**, `analytics.userSwitch`) | nothing at all, stored; stricter than ow-electron |

The ad and consent pages keep sending their own analytics in every case
(E.5) [OBS].

### E.4 Machine ids (`muid`, `muidV2`, `phasePercent`)

`analytics.muidStrategy`: `machine-id` (default, parity) or `per-install`
(**plugin option**: a random upper-case UUID v4 stored as `muid` in
`ow-tauri.json`, with `muidV2 = muid`). If the OS id cannot be read, the
plugin logs a warning and uses the per-install muid
([ADR 0014](adr/0014-machine-id-parity.md)).

**macOS** [OBS]:

```
id     = IOPlatformUUID of IOPlatformExpertDevice, read through IOKit
h      = lowercase hex of sha256(lowercase(id))
muid   = h[0..8] + "-" + h[8..12] + "-" + h[12..16] + "-" + h[16..20] + "-" + h[20..32]
muidV2 = muid
```

**Windows** [BUILDER] for the registry, [OBS] for `MUIDV2`:

1. Read `HKCU\Software\OverwolfElectron` value `MUID` and
   `HKCU\Software\OverwolfPersist` value `MUIDV2`. When present, use them, so
   the app shares the ids of any ow-electron app on the machine.
2. Otherwise derive a missing `muid` with the macOS formula from
   `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid` [INF], create a missing
   `muidV2` as a lower-case random UUID v4, as ow-electron does [OBS: Windows
   lab], and write both values at `RunEvent::Ready`. Overwolf's uninstaller
   reads them (I.6).

**Linux** [INF]: the macOS formula over `/etc/machine-id`, else
`/var/lib/dbus/machine-id`.

**`phasePercent`** [OBS]: the sum of the character codes of the lower-case
hex MD5 of `muid` without `-`, modulo 100.

Test vectors (stand-in platform UUIDs) [OBS]:

| `IOPlatformUUID` | `muid` (= `muidV2`) | `phasePercent` |
|---|---|---|
| `2D59BF70-9641-826A-F003-C362834EC045` | `5bd79133-f3bf-be27-e448-a4581ab5f3cd` | 80 |
| `DA3889E5-CB8A-8A15-CD1B-DCE6B5A71203` | `601860a3-90c7-b77b-a42e-636035921a81` | 51 |
| `D668AFF2-C8FD-39A3-6B92-D57DED8E5461` | `5d841b98-54cb-5f57-73bc-297706f34221` | 62 |
| `58468E7A-3E77-8816-5D62-7371174B102C` | `cbec68c3-9e97-b465-f479-f8493f973f32` | 24 |

Ad guests and consent pages get `muid` = `MUID` and `muidV2` = `MUIDV2`.
Overwolf keys unique users, installs and staged rollouts on the machine id
[DOC].

### E.5 Analytics that Overwolf's pages send

The consent page sends `electron_cmp_accept_full_launch` (D.6.1); the ad page
sends `owads_*`, `oam_*` and Kind 400051, with `ClientVer` / `CurrentVersion`
derived from `oweVersion` / `owVersion` [OBS]. The plugin gets these by
loading the same pages with the same inputs and never renames or filters
them.

---

## F. State files

### F.1 Location

`<appData>` is the OS configuration directory: `%APPDATA%` on Windows,
`~/Library/Application Support` on macOS, `$XDG_CONFIG_HOME` or `~/.config`
on Linux ([ADR 0007](adr/0007-state-file-continuity.md)).

| Path | Owner | Content |
|---|---|---|
| `<appData>/ow-electron/<uid>/ow-electron.json` | shared with ow-electron | F.2; the only file ow-electron writes there [OBS] |
| `<appData>/ow-electron/<uid>/ow-tauri.json` | the plugin | F.3 |
| `<appData>/<PN>/EBWebView-ow` | the plugin (Windows) | the ads data store (D.8.1). Cookies and web storage do not move between engines |

Nothing is written before `RunEvent::Ready`. An `ow-tauri.json` that does
not parse is moved to `ow-tauri.json.corrupt-<Unix ms>` at
`RunEvent::Ready`; the newest 3 copies are kept. A corrupt `ow-electron.json`
is reset without a copy (F.2). [SECURITY.md](SECURITY.md#what-is-written-to-disk)
lists every file and registry value the plugin writes.

### F.2 `ow-electron.json`

Exact shape and encoding, as ow-electron writes it [OBS]:

```json
{"firstLaunch":true,"cmp":{"cmpString":"<TCF v2 string>","timeStamp":1791302123,"unifiedConsentString":"cmp%3D<tcf>%26ac%3D<ac>"}}
```

| Key | Meaning and encoding |
|---|---|
| `firstLaunch` | `true`: the first launch was already reported (E.2 #1). Written on the first launch, never reset |
| `cmp.cmpString` | the TCF v2 string from `saveConsent` (D.6.6) |
| `cmp.timeStamp` | **Unix seconds**, refreshed on every launch by the startup consent page |
| `cmp.unifiedConsentString` | stored **URL-encoded**: `cmp%3D<tcf>%26ac%3D<ac>` |
| `utmParams` | written by Overwolf's installer; absent for apps installed any other way [OBS] [TYPES] |
| `eHashes` | the last value the app passed to `setUserEmailHashes(value)` (A.2.2), stored as given (`{ sha1, md5, sha256 }` from `generateUserEmailHashes()`). Written after `cmp`, replaced by every later call, absent until the first [OBS]. Removed by `setUserEmailHashes()` without a value and by `clearUserEmailHashes()` |

Rules:

- Compact JSON. The plugin reads `firstLaunch`, `cmp.*` and `utmParams`, and
  writes `firstLaunch`, `cmp.*` and `eHashes`. It never writes `utmParams`,
  removes no key but `eHashes`, and keeps unknown keys, their values and
  their positions.
- Writes are read-modify-write under a process lock, to a temp file in the
  same directory that is renamed over the original.
- A file that is not a valid state object is reset, as ow-electron resets
  an unparseable file [OBS]: garbage, a truncated or empty file, `null`,
  `[]`, or a `firstLaunch` that is not a boolean, a `cmp` that is not an
  object, or a `utmParams` that is neither an object nor `null`. It reads
  as a first launch, so `app_first_launch` is sent again and the consent
  page saves again. The next write starts a new object; no copy is kept.
  One warning is logged. ow-electron never repairs `[]` or wrong-typed
  keys; resetting them once is a listed deviation
  ([PARITY](PARITY.md#deviations), OQ-40).

### F.3 `ow-tauri.json`

The plugin's own file. Overwolf never reads it.

```jsonc
{
  "schema": 1,
  "stagingId": "...",              // updater staging bucket (I.2)
  "adOptimization": true,          // consent page toggle (D.6.6)
  "anonymousAnalytics": false,     // setAnonymousAnalyticsPreference (E.3)
  "analyticsUserEnabled": true,    // only with analytics.userSwitch
  "muid": "...",                   // per-install muid (E.4)
  "createdBy": "ow-tauri <version>"
}
```

Unknown keys are kept. A newer `schema` is never lowered on write.

---

## G. Identity and signing

### G.1 App name and version

| Value | Source |
|---|---|
| `<PN>` | `plugins.overwolf.name`, else the Tauri `productName`, else the Cargo package name (as Tauri falls back). Used for analytics `app_name`, the guest `name`, the consent `appName`, the UA token, the `EBWebView-ow` folder and the uid formula |
| `<ver>` | the Tauri `version` |
| author | `plugins.overwolf.author`; missing or empty is `"unknown"` |

ow-electron reads these from `package.json` (top-level `productName`, else
`name`; `author` as a string or `{ name }`) [OBS]. `ow-tauri migrate` writes
the values an ow-electron `package.json` resolves to into
`plugins.overwolf` ([MIGRATION.md](MIGRATION.md)). Overwolf documents that
app names containing "bot" are refused, because ad partners see the name
[DOC].

### G.2 App uid

1. `plugins.overwolf.uid` when set: 1 to 64 ASCII letters or digits after
   trimming. It names the state directory, so it never holds a path
   separator. Set it to the uid the Overwolf console assigned, and set it
   before signing (G.3).
2. Else the computed uid [OBS] [BUILDER: `ow client calc-electron-uid`]
   [DOC]:

```
s   = "{'author':'" + author + "','name':'" + <PN> + ".electron'}"   // UTF-8, no escaping
d   = sha1(s)                                                          // 20 bytes
uid = for each byte b of d: chr(97 + (b & 15)) + chr(97 + (b >> 4))    // 40 characters a..p
```

- `author` is used verbatim: no `Name <email> (url)` parsing, nothing
  trimmed, non-ASCII hashed as UTF-8, quotes not escaped [OBS].
- `<cuid>` (`app_cuid`) is always the formula's value. With a configured uid,
  `app_id` is the configured uid and `app_cuid` the computed one, as
  ow-electron reports a signed `overwolf.uid` [OBS].
- A release build needs `uid`, or both `author` and `name`, so the uid never
  depends on defaults; the build step and setup fail otherwise.
- The `.electron` suffix stays: the uid keys the developer console, the ad
  configuration and the state directory
  ([ADR 0007](adr/0007-state-file-continuity.md)).
- ow-electron also takes a uid from the signed `package.json`'s
  `overwolf.uid`. The plugin reads no `package.json`; `ow-tauri sign
  --write-uid` writes the signed uid into `plugins.overwolf.uid` (G.3).

Test vectors as (author, `<PN>`). Each is observed in ow-electron 42.11.4
from the `package.json` noted, and agrees with `ow client calc-electron-uid`
[OBS]. The crate's unit tests use them:

| # | ow-electron `package.json` | author | `<PN>` | uid |
|---|---|---|---|---|
| 1 | `name: "parity-harness"`, `author: { name: "Example Studio" }` (also as a string, or with `build.productName` added, which is ignored) | `Example Studio` | `parity-harness` | `binaioonkjpolnojeenpbmjmbfkbmffcekndbmdk` |
| 2 | as 1, plus `productName: "Parity Harness"` | `Example Studio` | `Parity Harness` | `bijigndkghcikkfmhgkmicdkjpdehpjafgpmdhcc` |
| 3 | `name: "parity-harness"`, `author: "Overwolf Ltd."` | `Overwolf Ltd.` | `parity-harness` | `djaoacjhpjaenfddlfmkeoiklmccgcgcgeknhmgj` |
| 4 | `productName: "Parity Harness"`, `author: "Overwolf Ltd."` | `Overwolf Ltd.` | `Parity Harness` | `aejkligdodglhcjinbhdcnlohocenfkpdihjacdg` |
| 5 | `name: "parity-harness"`, `author: "Example Studio <dev@example.com> (https://example.com)"` | as written | `parity-harness` | `agmekflfehlhfcnofnhghbgohnigngnkddpkdbnc` |
| 6 | as 5, plus `productName: "Parity Harness"` | as written | `Parity Harness` | `cmbaaahkhdbkbbfcmenfbmmngmommpjjacllbgan` |
| 7 | `name: "parity-harness"`, `author: "Example Studio <dev@example.com>"` | as written | `parity-harness` | `mcfopdapolegaeddgbbfedginnmcnmjldgdcbcjo` |
| 8 | `name: "parity-harness"`, author missing, `{}` or `""` | `unknown` | `parity-harness` | `nbhlaphlggihmjefpjdelbobckfhklbfkiicjaja` |
| 9 | `productName: "Parity Harness"`, author missing, `{}` or `""` | `unknown` | `Parity Harness` | `fifpcfmoobnjlimjhefehejankadpajlfgbmpheo` |
| 10 | `productName: "Pârity Ünicode"`, `author: { name: "Exämple" }` | `Exämple` | `Pârity Ünicode` | `mmfoflmmchoacblhjlimpanaijdnhgoalaloihjd` |
| 11 | `productName: "O'Brien Tools"`, `author: { name: "D'Arcy" }` | `D'Arcy` | `O'Brien Tools` | `khalfglcmeemfnjoldckbfmeeidgkoabeebkpbbl` |
| 12 | `productName: " Parity Harness "`, `author: { name: " Example Studio " }` | ` Example Studio ` | ` Parity Harness ` | `cppaiialckdbmhdojecejpjafcblbingfdiffkdi` |

### G.3 Build step and signing

**Build step.** `tauri_plugin_overwolf::build::run()` in the app's
`build.rs` reads the merged Tauri configuration as `tauri-build` does,
validates `plugins.overwolf` with the release rules, and resolves the same
identity the plugin resolves at run time. For a Windows target it writes the
installer hooks of I.6 to `gen/overwolf/`.

**Signing** ([ADR 0016](adr/0016-signing-approach.md)). Overwolf signs the
gaming-package integrity and the developer signs the exe; ads and analytics
never depend on signing [DOC]. Signing is opt-in
(`plugins.overwolf.signing.enabled`). `ow-tauri sign` (npm package
`tauri-plugin-overwolf-cli`) runs after the frontend build. It reads
`OW_CLI_EMAIL`, `OW_CLI_API_KEY` and `OW_BUILD_KEY`, and sends requests to
`OW_CLI_API_URL` (default `https://console-be.overwolf.com`) with
`Authorization: Key <email>:<apiKey>` and `x-ow-app-key: <OW_BUILD_KEY>`
[BUILDER].

| Step | What it does |
|---|---|
| a | `POST /sign/electron` with `{ packageJson, fileHashes: { <entry>: <sha256 hex> } }`. `packageJson` is synthesised from the merged configuration (`name`, `productName`, `version`, `author`, `overwolf.uid` when configured, `main`). The entry is `--main`, else `signing.entry`. `electronVersion` is not sent. The response ZIP carries the signed `package.json` and `_metadata.json`. If the signed uid differs from the configured one, nothing is written unless `--write-uid` pins it in `tauri.conf.json` |
| b | downloads `integrity.dll` from `integrityDllUrl`; the app ships it next to the exe as a bundle resource (inert without a package runtime) |
| c | the build step links the Windows PE resource `OWEINTEGRITY/OWE` = `{"appUid":"<uid>"}` after checking the signing output against the merged uid, before Authenticode signing |
| d | `ow-tauri sign-exe %1` as Tauri's `bundle.windows.signCommand`: with `owCertSigning` (or `OW_ENABLE_CERT_SIGNING`) and the service's consent, it posts the **app exe only** to `/sign/electron-certificate` (multipart `file`) and replaces it with the signed copy, or keeps it on `isAlreadySigned`; other files go to the developer's own signing command [BUILDER] |
| e | gating: a Windows release build with `signing.requireSigning` (default `true`, or env `OW_REQUIRE_SIGNING`) fails when credentials are missing or a signing call fails [BUILDER] |
| f | `/sign/asar` and `OWEASARSIG`: **not done and not faked**; Tauri has no asar (OQ-09) |

Open with Overwolf (OQ-09): whether `/sign/electron` accepts a synthesised
manifest, what `fileHashes` are used for at run time, and a Tauri integrity
target.

---

## H. Packages

GEP, overlay, recorder, utility and CRN are out of scope for 1.0
([ADR 0004](adr/0004-packages-backend-selection.md)). Overwolf documents that
packages are Windows-only and that macOS and Linux get ad services only [DOC].

### H.1 What the plugin does

Nothing: no package is downloaded, loaded or reported, and no command or
JavaScript object exists for them. The installer hooks skip the removal of
Overwolf's elevated package helper (I.6). An app that needs packages stays on
ow-electron for now.

---

## I. Updates and installer

Overwolf distributes ow-electron app updates through an electron-updater
**generic provider** feed per app ([DOC]:
`https://dev.overwolf.com/ow-electron/developers-console/releases-management/release-management`).
The plugin's update client reads that feed
([ADR 0008](adr/0008-updater-client.md)). It runs on **Windows only**, with
the cargo feature `updater`; elsewhere the commands answer `unsupported` and
apps use `tauri-plugin-updater`.

### I.1 Feed

- URL `https://electron-updates.overwolf.com/electron-updates/electron/<uid>/`,
  or `updater.endpoint` [DOC] [OBS].
- `latest.yml` has `version`, `files[]: { url, sha512, size, blockMapSize, IsAdminRightsRequired }`,
  `releaseDate` and `releaseName`, and no top-level `path` or `sha512` [OBS].
- Files are served from `https://appsdl.overwolf.com/prod/apps/<id>/<ver>/setup.exe`
  [OBS].
- The console serves Windows setup files only; `latest-mac.yml` and
  `latest-linux.yml` return 404 [OBS].
- Whether the console accepts a Tauri NSIS `setup.exe` upload is **open**
  (OQ-18).

### I.2 Feed handling

1. `GET <base><channel>.yml?noCache=<12 hex>`; `channel` is `updater.channel`
   or the JavaScript `channel`, default `latest`, 1 to 64 letters, digits,
   `.`, `_` or `-`.
2. Parsed fields: `version`, `files[]`, `path`, `sha512`, `releaseDate`,
   `releaseName`, `releaseNotes`, `stagingPercentage`,
   `minimumSystemVersion`. Field names in `files[]` match case-insensitively,
   because Overwolf's feed spells `IsAdminRightsRequired` with a capital `I`
   [OBS]. Every other key is kept for the app. The feed is at most 1 MiB.
3. Updates are always full downloads; `blockMapSize` is ignored.
4. electron-updater 6's rules, in order: an equal version is never an
   update; a prerelease only with `allowPrerelease`; `minimumSystemVersion`
   above the OS release is unsupported; the staged rollout; then newer, or
   older with `allowDowngrade`. A JavaScript `channel` or `allowDowngrade`
   turns a downgrade on only with `updater.allowJsDowngrade`.
5. `stagingPercentage`: offered only if the bucket (0 to 99, from
   `ow-tauri.json` `stagingId`) is below it.
6. The file is the release's NSIS `.exe`; a feed with only an `.msi` is
   `unsupported`. Relative URLs resolve against the feed.

### I.3 Download and verification

- HTTPS only (loopback `http` in debug builds). At most 10 redirects, each to
  HTTPS. Custom headers go to the feed origin only.
- The file streams to `<appCache>/ow-tauri-updater`; then its size and
  base64 SHA-512 must match the feed entry.
- **Publisher.** Nothing is trusted by default: release builds need
  `updater.publisherNames` (Authenticode subjects the installer must be signed
  by) or `updater.pubkey` (a minisign key; the client checks
  `<file url>.sig`). `dangerousSkipPublisherCheck` works in debug builds only.
- Every check fails closed with `verification` and deletes the file. The
  installer is checked again right before it starts.
- Progress events: `Started { contentLength? }`, `Progress { chunkLength }`,
  `Finished`.

### I.4 Install

The installer starts with `/S /UPDATE` when it installs at exit
(`updater.installOnExit`, default `true`) and with `/UPDATE /R` from an
app's `install()`. `updater.installerArgs` replaces the arguments; `/UPDATE`
is always kept, so an update never runs the uninstall work of I.6. When the
feed entry has `IsAdminRightsRequired: true` the installer starts elevated,
as electron-updater does [INF].

### I.6 Installer hooks

Overwolf's NSIS installer does Overwolf work at install and uninstall time
[BUILDER]. A Tauri NSIS installer is "your own installer" in Overwolf's terms
[DOC]. The build step generates hooks that do the same, for
`bundle.windows.nsis.installerHooks`:

- **Install** (also on updates, as Overwolf's install section runs on every
  install): `Software\OverwolfElectron\<uid>` values `InstallLocation`,
  `version` and `ShortcutName` (`<main binary>.exe`), under `HKLM` for an
  all-users install and `HKCU` otherwise [OBS].
- **Uninstall**, only when Tauri's `$UpdateMode` is not set (never during an
  update; Overwolf's builder fixed exactly this case, where an update reset
  consent) [BUILDER]:
  1. delete the `<uid>` registry key;
  2. `RMDir /r "$APPDATA\ow-electron\<uid>"` (the current user's folder);
  3. `GET https://analyticssec.overwolf.com/analytics/Counter?Name=ow_<label>_app_uninstall&MUID=<HKCU\Software\OverwolfElectron MUID>&MUIDV2=<HKCU\Software\OverwolfPersist MUIDV2>&Extra=<json>`,
     where `Extra` is `{"app_id":"<uid>","app_version":"<ver>","app_name":"<PN>"}`,
     form-encoded, sent with `curl.exe` (else PowerShell), 5 s timeout. This
     feeds the console's "App Uninstalls" widget [DOC].
- The removal of Overwolf's elevated helper is skipped (H).
- The runtime writes the machine-id registry values (E.4), so step 3 carries
  real ids.

---

## Removed sections

Earlier versions of this contract described a hidden main webview, an
Electron API subset and a package runtime. Those designs were replaced by
the Tauri-native plugin ([ADR 0017](adr/0017-tauri-native-pivot.md)).
Comments and old documents that cite these numbers mean:

| Old section | Now |
|---|---|
| A.1.1 Webview environments | D.8.1 |
| A.2.1, A.2.3, A.2.4 main-webview and package commands | gone; A.2.1 is the command table |
| A.3 Host messages | gone; ad events use the mount's channel (A.2.5) |
| A.6 Main-webview liveness | A.6 Start and exit |
| B.1 `ow-tauri/main`, B.2 `ow-tauri/electron` | B.1 The API package; no Electron API |
| B.3.6 Window dragging | gone; use Tauri's `data-tauri-drag-region` |
| C. IPC routing protocol | gone; Tauri commands |
| F.4 Logs, F.5 Migration | gone; [MIGRATION.md](MIGRATION.md) |
| G.1 Manifest fields, G.3 Build helper, G.4 Signing | G.1, G.3 |
| I.5 `autoUpdater` | I; JavaScript `check()` and `Update` ([api/js.md](api/js.md#updater)) |
| Appendix P | gone ([ADR 0004](adr/0004-packages-backend-selection.md)) |
