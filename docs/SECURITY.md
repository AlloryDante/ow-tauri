# Security model

This page is for security reviewers and for teams shipping an app on
ow-tauri. It covers:

- what the plugin defends against and how;
- which permissions grant what;
- what the plugin writes to disk;
- what your app must do itself.

To report a vulnerability, see the [security policy](../SECURITY.md).

## The trust shift from ow-electron

In ow-electron, the Overwolf API runs in Electron's main process, which page
scripts cannot reach. In ow-tauri, your app's pages call the plugin through
Tauri commands. Script in an app page, including an XSS payload or a
compromised dependency, can therefore call any command its capability
grants.

The plugin limits that reach in three ways:

- the default permission set leaves out personal data and machine ids;
- every app command checks who is calling;
- ad and consent pages run in their own webviews with exactly one command
  each.

## Actors

| Actor | Runs in | Can | Cannot |
|---|---|---|---|
| Your app's pages | app webviews (any label without a reserved prefix, showing an app origin) | call the commands its capability grants; mount ads | call commands from a remote page; act as a guest |
| Overwolf's ad page **and every third-party creative in it** | an ad guest, a child webview labelled `owad-<n>` | call `adview_event` at the per-guest rate limits; open `http(s)` URLs in the system browser after a native click on its own guest | reach any app command, other guests, files or OS APIs; take keyboard focus; show dialogs; load an app-origin or `localhost` frame |
| Overwolf's consent pages | `ow-cmp-startup` (hidden, every launch), `ow-cmp-default` (hidden), `ow-cmp` (the settings window) | call `cmp_event` to save consent, toggle ad optimisation and close their window | anything else |
| A network attacker | between the app and Overwolf, or the update feed | cause TLS failures | change an update undetected |
| A local attacker (same OS user) | the user's account | read and change the state files, as with ow-electron | make the app load code from a state file |

## Mitigations

### Capabilities name webviews, never windows

Tauri matches a capability that names a **window** for every webview inside
that window, and ad guests live inside your windows. Grant the plugin with
`webviews`:

```json
{
  "identifier": "default",
  "webviews": ["main"],
  "permissions": ["core:default", "overwolf:default"]
}
```

The build step (`tauri_plugin_overwolf::build::run`) checks your
capabilities (`build/lint.rs`):

- **error**: a `remote.urls` entry that covers Overwolf pages, such as
  `https://*.overwolf.com/*`, `https://*`, `http://*` or `*`;
- **warning** when the `ads` feature is on: any capability that selects
  `windows`.

The plugin adds two runtime capabilities of its own (`capabilities.rs`).
Both are remote-only and select webviews by label:

| Capability | Webviews | Remote URL | Grants |
|---|---|---|---|
| `ow-tauri-adview-guest` | `owad-*` | `https://www.overwolf.com/monsdk/electron/*` | `overwolf:adview-guest` (`adview_event`) |
| `ow-tauri-cmp` | `ow-cmp*` | `https://content.overwolf.com/monsdk/electron/*` | `overwolf:cmp-window` (`cmp_event`) |

Never grant `overwolf:adview-guest` or `overwolf:cmp-window` yourself.

### Permission sets

`overwolf:default` grants (`permissions/default.toml`):

- the ads (`adview_mount`, `adview_update`, `adview_unmount`,
  `adview_command`);
- `set_window_name` and `get_info`;
- the consent queries and windows (`is_cmp_required`,
  `open_ad_privacy_settings_window`, `open_cmp_window`);
- the three switches that reduce what is sent (`disable_anonymous_analytics`,
  `disable_ads_optimization`, `disable_ads_fpd`).

Opt-in sets add data or change future launches. Grant them only to the
webviews that need them:

| Set | Commands | Why it is opt-in |
|---|---|---|
| `overwolf:machine-id` | `get_machine_ids` | machine ids Overwolf shares across apps (`muid`, `muidV2`) |
| `overwolf:email-hashes` | `generate_user_email_hashes`, `set_user_email_hashes`, `clear_user_email_hashes` | personal data, sent to Overwolf and stored |
| `overwolf:analytics` | `set_external_payment_user_id`, `set_analytics_user_enabled`, `set_anonymous_analytics_preference` | adds a payment id, or changes later launches |
| `overwolf:updater` | `updater_check`, `updater_download`, `updater_install`, `updater_download_and_install` | downloads and runs an installer (Windows, `updater` feature) |

Rust code (`app.overwolf()`) is trusted and needs no permission.

### Every app command checks its caller

Tauri's ACL runs first. Then every app command calls `require_app_webview`
(`commands/mod.rs`), which refuses with `forbidden`
(`tauri-plugin-overwolf: <label> is not a local app webview`) when:

- the caller's label starts with `owad-` or `ow-cmp` (reserved for the
  plugin); or
- the caller's page is not on an app origin. App origins are
  `tauri://localhost`, `http://tauri.localhost` and
  `https://tauri.localhost`; `build.devUrl` counts only in debug builds;
  each entry of `ads.allowedEmbedderOrigins` counts too.

`adview_update`, `adview_unmount` and `adview_command` only find elements
that the calling webview mounted. Another webview's element is `not-found`.

`ads.allowedEmbedderOrigins` is for apps that serve their pages from
`tauri-plugin-localhost` or a remote origin. Every origin you list gets the
commands your capability grants, so list only origins you control.

A window or webview your app creates with a reserved prefix is logged once
as an error and is never tracked or accepted as an embedder.

### Ad guests are isolated

- **One command.** A guest can only call `adview_event`. The command checks
  that the caller's label starts with `owad-` and that its page is Overwolf's
  ad page (`commands/ads.rs`). Names and data are validated, data is capped
  at 16 KiB, and token buckets limit each guest
  (`ads.guestLimits`: 50 events/s, bursts of 100, 256 KiB/s).
- **No app-origin frames.** A guest or consent window may load only
  `http`, `https`, `about`, `data` and `blob` URLs, never on a `localhost`
  or `*.localhost` host (`ads/rules.rs` `frame_url_allowed`). This keeps any
  frame inside a guest from passing Tauri's ACL as a local page:
  - Windows cancels other navigations in `NavigationStarting`, and for every
    frame through `FrameCreated` and the frame's own `NavigationStarting`;
  - macOS applies the same rule in the navigation hook, which wry calls for
    every frame.
- **Top-level navigation.** A guest that navigates away from Overwolf's ad
  page is sent back. The target opens in the system browser only after a
  native click (below).
  - On Windows the navigation is cancelled before it loads.
  - On macOS it is caught when the foreign page starts loading. That page
    briefly exists but has no IPC.
- **Native click authority.** A popup or off-Overwolf navigation opens the
  system browser only when the OS reports that the user clicked or pressed
  a key on that guest ([ADR 0020](adr/0020-native-gesture-authority.md)).
  - The guest's own `__host:gesture` message is ignored as an authority.
  - Opens are capped at 20 per minute per guest and 20 per minute per app.
  - Only `http` and `https` URLs open.
- **No keyboard.** Guests are created with `focused(false)`, so a user's
  typing never reaches an ad page.
- **No dialogs.** `alert`, `confirm` and `prompt` in a guest return at once
  without showing anything.
- **Crash and reload loops are bounded.** Recovery and recreation have
  limits: `ads.maxRecoveries`, `ads.recreateMinIntervalMs` (30 s) and
  `ads.recreateMaxPerHour` (30).
- **Windows ads environment.** Guests and consent windows run in their own
  WebView2 environment with ow-electron's ads arguments, which include
  `--disable-web-security`. Your app's webviews keep their own environment
  and arguments.

### The asset protocol caveat (Windows)

On Windows the ads environment runs with web security off. The frame guard
stops a guest from **navigating** any frame to `http://asset.localhost/`, but
the plugin cannot stop a **subresource request** (an image or a `fetch`) to
that host: wry's protocol handler answers it first.

If your app enables `app.security.assetProtocol`, an ad page in that app can
read files inside the asset scope. Keep the asset protocol off, or keep its
scope to files that are safe to expose, such as your own bundled media.
Never scope it to the user's home, documents or app data. The build step
does not check this.

### Consent windows

- `cmp_event` accepts only calls from `ow-cmp*` webviews on Overwolf's
  consent path. Consent strings are printable ASCII, at most 16 KiB.
- A JavaScript `cmpURL` (`openCMPWindow({ cmpURL })`) must have an origin in
  `consent.allowedCmpOrigins` (default `https://content.overwolf.com`), or
  the call fails with `invalid-argument`. The page receives the uid, muid and
  muidV2 in its query. Rust callers may pass any `https` URL.
- `cmp-eu-only` has a client timeout (`consent.euOnlyTimeoutMs`, 60 s), so
  a hung server cannot hold the consent round open for the whole session.

### Updates (Windows, `updater` feature)

- HTTPS only, including redirects (at most 10). Custom headers go to the feed
  host only and are dropped on redirects to other hosts.
- The download is size-capped (after decompression) and checked against the
  feed's SHA-512.
- The installer must then pass the publisher check, and the check fails
  closed:
  - its Authenticode signer subject is in `updater.publisherNames`; or
  - its detached minisign signature verifies against `updater.pubkey`.
- A release build with the `updater` feature fails to build unless one of
  the two is set. There is no default publisher.
- `dangerousSkipPublisherCheck` is refused in release builds.
- The installer is checked again right before it runs.
- Downgrades are off. JavaScript cannot turn them on unless
  `updater.allowJsDowngrade` is set.
- The feed endpoint is set in config only.
- NSIS `.exe` installers only; `.msi` is `unsupported`.

### Release guards

- `lab` and `test-util` are development features. Enabling either in a
  release build is a `compile_error!` (`src/lib.rs`).
- Release builds also refuse `state.appDataDir`, loopback updater
  endpoints, `--remote-debugging*` in `ads.browserArgs` and
  `updater.dangerousSkipPublisherCheck`.
- A release build must pin the uid inputs: set `uid`, or both `author` and
  `name`.

### Supply chain

- CI runs `cargo-deny` and `npm audit`. It also checks that every
  `@overwolf/*` dependency is on its `latest` dist-tag.
- Third-party GitHub Actions are pinned by commit SHA.
- The guest scripts embedded in the crate are rebuilt in CI and must match
  the committed output.
- Run the CLI from your local dev dependency (`npm exec --no -- ow-tauri`),
  never as `npx ow-tauri`. `npx` without a local install downloads whatever
  package is named `ow-tauri`.
- `tauri` must be 2.12.1 or newer. That release includes the fix for
  GHSA-w28w-mhc8-qvjv, where Tauri's channel-data fetch skipped the ACL.

## What is written to disk

ow-tauri writes what ow-electron writes for the same uid, in the same
places, plus one file of its own. All file writes are atomic (a temporary
file renamed over the original). Nothing is written before
`RunEvent::Ready`.

| Where | What | Written when | How to clear |
|---|---|---|---|
| `<appData>/ow-electron/<uid>/ow-electron.json` | shared with ow-electron, byte for byte: `firstLaunch`, `cmp` (consent strings and time), `eHashes` (the last email hashes the app set); `utmParams` is read, never written | first launch, each consent save, each `setUserEmailHashes` | `clearUserEmailHashes()` removes `eHashes`; uninstall removes the folder |
| `<appData>/ow-electron/<uid>/ow-tauri.json` | ow-tauri's own: `schema`, `stagingId`, `adOptimization`, `anonymousAnalytics`, `analyticsUserEnabled`, `muid` (only with `analytics.muidStrategy: "per-install"`), `createdBy` | first launch; when a switch or preference changes | uninstall removes the folder |
| `ow-tauri.json.corrupt-<ms>` | a copy of an `ow-tauri.json` that could not be parsed (the newest 3 are kept). A corrupt `ow-electron.json` is reset without a copy, as ow-electron does | at `RunEvent::Ready` after a parse failure | delete it |
| `<appData>/<productName>/EBWebView-ow` (Windows) | the ads WebView2 profile: cookies, including the consent cookies on `.overwolf.com` | while ads and consent pages run | uninstall, or delete the folder while the app is closed |
| macOS default website data store | the ad and consent pages' cookies and storage, shared with the app's webviews as in ow-electron's default session | while ads and consent pages run | the app's own data-clearing |
| HKCU `Software\OverwolfElectron` `MUID`, HKCU `Software\OverwolfPersist` `MUIDV2` (Windows) | machine ids shared by every Overwolf app on the machine, as ow-electron writes them | the first launch on a machine where they are missing | they are shared; the uninstaller leaves them, as ow-electron's does |
| `Software\OverwolfElectron\<uid>` under HKCU (HKLM for per-machine installs) | the install record: `InstallLocation`, `version`, `ShortcutName` | by the NSIS installer, on install and on update | removed by a real uninstall |
| `<appCache>/ow-tauri-updater` (Windows, `updater` feature) | one downloaded installer | after `download()` | replaced by the next download |

Email hashes are SHA-256, SHA-1 and MD5 of the normalised address. The
plugin never stores or logs the address itself and never scans user data
for addresses. The hashes are persisted as `eHashes`, exactly as ow-electron
persists them, and passed to the ad pages (CONTRACT D.5).

The plugin writes no log file. Logs go through the `log` crate under the
`tauri_plugin_overwolf` target. They never contain email addresses, hashes or
cookie values.

## What your app must do

1. **Grant `overwolf:default` with `webviews`**, never `windows`. Grant the
   opt-in sets only to webviews that need them.
2. **Never add a `remote.urls` capability** that covers Overwolf pages or
   any `https` host.
3. **Ship a strict CSP.** For example:

   ```text
   default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src ipc: http://ipc.localhost; frame-ancestors 'self'
   ```

4. **Keep `app.security.assetProtocol` off** or narrowly scoped (see the
   caveat above).
5. **Declare your own Tauri commands** with
   `tauri_build::AppManifest::commands` and grant them with your own
   capability. Otherwise every local webview may call them.
6. **On macOS, wire the terminate hook**
   (`tauri_plugin_overwolf::web_content_process_terminate_hook()`), so a
   crashed ad is recovered instead of staying blank.
7. **With the updater**, set `updater.publisherNames` to your installer's
   certificate subject, or set `updater.pubkey`.
8. **Register `tauri-plugin-single-instance` first**, before this plugin.
9. **Never ship `lab` or `test-util`.**

## Out of scope

- Vulnerabilities in Overwolf's ad pages, consent pages or services: report
  them to Overwolf.
- A compromised OS account or machine.
- The `uid` setting can name any app's uid. Binding a uid to its owner is
  Overwolf's console and signing flow
  ([OPEN-QUESTIONS](OPEN-QUESTIONS.md#oq-09-signing-and-integrity-for-tauri-builds)).
