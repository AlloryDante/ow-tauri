# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The two crates (`tauri-plugin-overwolf`, `tauri-plugin-overwolf-unstable`)
and the two npm packages (`tauri-plugin-overwolf-api`,
`tauri-plugin-overwolf-cli`) are released together and share one version.
[docs/RELEASING.md](docs/RELEASING.md) describes how a release is made.

## [Unreleased]

## [1.0.0-rc.1] - Unreleased

First public release candidate.

ow-tauri brings Overwolf's ow-electron SDK features to Tauri 2 as a Tauri
plugin: ads (`<owadview>`), consent, analytics, app identity, the Overwolf
update feed and signing. Your app keeps Tauri's own windows, webviews,
commands, events and permissions. There is no Electron API.

Overwolf receives the same data from an ow-tauri app as from an ow-electron
app: the same analytics events and fields, consent outcome, uid and muid, and
ad events. Two differences are by design or open with Overwolf:

- The analytics host label is `tauri` (ow-electron sends `electron`).
- On macOS, ad subresource requests do not carry ow-electron's `Origin` and
  `x-ow-*` headers (see Known limits).

### Added

**Crate `tauri-plugin-overwolf`**

- The Tauri plugin: `tauri_plugin_overwolf::init()` or
  `tauri_plugin_overwolf::Builder`, configured in `tauri.conf.json` under
  `plugins.overwolf`. Requires `tauri` 2.12.1 or a later 2.x release and
  Rust 1.90 or later.
- Ads: `<owadview>` with the same attributes, properties, methods and DOM
  events as in ow-electron. Each ad runs in its own native child webview.
  Test ads: the `--test-ad` switch, `OW_TAURI_TEST_AD=1`,
  `plugins.overwolf.ads.testAd` or `Builder::test_ad(true)`.
- Consent: the consent round on every launch, `isCMPRequired()`,
  `openAdPrivacySettingsWindow()` and `openCMPWindow()`. The state file is
  ow-electron's `ow-electron.json`, so an app that moves from ow-electron
  keeps its consent and first-launch state.
- Analytics: the launch events at `RunEvent::Ready`, heartbeats,
  `window_closed`, ad guest events, and the uninstall event from the NSIS
  installer hooks. Switches: `disableAnonymousAnalytics()`,
  `disableAdsOptimization()`, `disableAdsFPD()`, and
  `setAnonymousAnalyticsPreference()`, which stores the choice in
  `ow-tauri.json` for the next launch.
- Identity: the uid from ow-electron's formula (`plugins.overwolf.author`
  and `name`, or `productName`) or a fixed `plugins.overwolf.uid`; `getInfo()`
  (uid, phase, UTM parameters, test-ad flag, whether ads are supported);
  machine ids with `getMachineIds()`; email hashes with
  `generateUserEmailHashes()`, `setUserEmailHashes()` and
  `clearUserEmailHashes()`.
- Rust API: `OverwolfExt` on `AppHandle`, `Window` and `Webview`, and
  `build::run()` for the app's `build.rs` (feature `build`): it validates the
  merged configuration and writes the NSIS hooks and the signed-build
  resource.
- Permissions: `overwolf:default` (ads, consent, read-only identity, window
  name and the three switches) and the opt-in sets `overwolf:machine-id`,
  `overwolf:email-hashes`, `overwolf:analytics` and `overwolf:updater`.
- Updater (feature `updater`, Windows): Overwolf's update feed behind an API
  shaped like `tauri-plugin-updater` (`check()`, `download()`, `install()`,
  `downloadAndInstall()`). A release build with the updater must set
  `updater.publisherNames` or `updater.pubkey`.
- macOS: `Builder::macos_key_fix` (on by default) keeps key events and
  shortcuts working in app webviews next to an ad;
  `handle_web_content_process_terminate` for
  `tauri::Builder::on_web_content_process_terminate` recovers crashed web
  content; ad guests are recreated on a page-requested reload
  (`ads.recreateOnReload`, on by default).

**Crate `tauri-plugin-overwolf-unstable`**

- Turns on Tauri's `unstable` feature (child webviews) for the plugin's
  `ads` feature on Windows and macOS only. Apps do not depend on it directly.

**npm package `tauri-plugin-overwolf-api`**

- The browser-only JavaScript API: the functions above, `setWindowName()`,
  `tauri-plugin-overwolf-api/adview` (registers `<owadview>`),
  `tauri-plugin-overwolf-api/updater`, React JSX types
  (`tauri-plugin-overwolf-api/jsx`) and a fake plugin for unit tests
  (`tauri-plugin-overwolf-api/testing`).

**npm package `tauri-plugin-overwolf-cli`** (command `ow-tauri`, Node.js
22.12 or later)

- `ow-tauri init`: adds `plugins.overwolf`, the `overwolf:default`
  capability and the NSIS hooks to a Tauri app.
- `ow-tauri migrate --from <package.json>`: prints the `plugins.overwolf`
  block that keeps an ow-electron app's uid.
- `ow-tauri doctor`: read-only checks of the uid, capabilities, versions and
  test ads.
- `ow-tauri sign` and `ow-tauri sign-exe`: Overwolf signing of a Tauri build.

**Examples**

- `examples/quickstart-vanilla` and `examples/quickstart-react`: one window,
  one ad, test ads on.
- `examples/ad-showcase`: every `<owadview>` ad format.
- `examples/packages-sample`: Overwolf's ow-electron packages sample as a
  Tauri app.

### Platform support

| Platform | Ads | Consent, analytics, identity | Updater | Status |
|---|---|---|---|---|
| Windows 10 22H2 and 11, x64 | yes | yes | yes | supported |
| macOS 14 or later, Apple Silicon | yes | yes | no | supported |
| Windows 11 arm64 | yes | yes | yes | best effort (not tested) |
| macOS on Intel, macOS before 14 | yes | yes | no | best effort (not tested) |
| Linux (WebKitGTK 4.1) | no | yes | no | builds; ads unsupported |
| Android, iOS | no | no | no | builds; every command is `unsupported` |

On Windows, ads need WebView2 98.0.1108.44 or later. With an older runtime,
ads report `unsupported`.

### Known limits

- **Overwolf packages are not available.** GEP (game events), the overlay
  and the recorder have no API on Tauri in this release.
- **No ads on Linux.** The plugin builds and consent, analytics and identity
  work. Mounting an ad fails with `unsupported: ads are not available on
  Linux`, and `getInfo()` returns `adsSupported: false`.
- **The updater is Windows-only.** On macOS and Linux `check()` fails with an
  `unsupported` error; use `tauri-plugin-updater` there. Only NSIS installers
  are supported; a release that offers only an `.msi` is refused.
- **macOS request headers ([OQ-05](docs/OPEN-QUESTIONS.md#oq-05-request-shaping-for-the-ad-page)).**
  On macOS only the ad document request carries ow-electron's `Referer` and
  `Origin`. Subresource requests have no forced `Origin` and no `x-ow-*`
  headers, and ad guests run with web security on. The uid, phase and window
  name still reach Overwolf in the `owads.min.js` query string. Whether this
  affects fill or attribution is open with Overwolf.
- **Live ads need Overwolf's approval ([OQ-20](docs/OPEN-QUESTIONS.md#oq-20-live-ads-from-a-tauri-host)).**
  Overwolf enables live ads for an app after its QA. The approval path for an
  app built on ow-tauri is open with Overwolf. Test ads work without it.
- **Tauri's `unstable` feature.** On Windows and macOS the `ads` feature turns
  on Tauri's `unstable` feature for the whole app (child webviews).

[Unreleased]: https://github.com/AlloryDante/ow-tauri/commits/main
[1.0.0-rc.1]: https://github.com/AlloryDante/ow-tauri/commits/main
