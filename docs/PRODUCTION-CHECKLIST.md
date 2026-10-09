# Production checklist

Go through this list before every release. `npm exec --no -- ow-tauri doctor`
checks several items for you; they are marked **doctor**.

## Identity

- [ ] **The uid is pinned.** `plugins.overwolf` sets `uid`, or both `author`
  and `name`. A release build without them fails:
  `plugins.overwolf: set "uid", or both "author" and "name", before a release build (the uid must not depend on defaults)`.
  **doctor** prints the uid the app will use.
- [ ] **The uid is the one Overwolf knows.** If the console assigned your app
  a uid, set it as `uid`
  ([OVERWOLF-ONBOARDING.md](OVERWOLF-ONBOARDING.md#2-get-the-uid)).
- [ ] **A migrated app keeps its ow-electron uid** (`ow-tauri migrate`,
  [MIGRATION.md](MIGRATION.md#2-keep-the-uid-with-ow-tauri-migrate)).

## Ads

- [ ] **Test ads are off.** Remove `ads.testAd` (**doctor**) and
  `Builder::test_ad(true)`; make sure your shortcuts and installer do not
  pass `--test-ad` or set `OW_TAURI_TEST_AD`.
- [ ] **Overwolf enabled live ads for your app**
  ([OVERWOLF-ONBOARDING.md](OVERWOLF-ONBOARDING.md)).
- [ ] **Users can open the ad privacy settings.** Show an entry that calls
  `openAdPrivacySettingsWindow()` when `isCMPRequired()` is `true`.
- [ ] **Ad containers stay visible** and are not covered, moved or made
  transparent while the ad shows.

## Permissions and security

- [ ] **Capabilities select `webviews`, never `windows`** (**doctor**; the build
  step warns).
- [ ] **Opt-in sets only where needed:** `overwolf:machine-id`,
  `overwolf:email-hashes`, `overwolf:analytics`, `overwolf:updater`. Never
  `overwolf:adview-guest` or `overwolf:cmp-window`
  ([api/permissions.md](api/permissions.md)).
- [ ] **No `remote.urls`** that covers Overwolf pages or every origin (the
  build step fails on it).
- [ ] **A strict CSP**, the asset protocol off or narrowly scoped, and the
  other items of
  [SECURITY.md, "What your app must do"](SECURITY.md#what-your-app-must-do).
- [ ] **No `lab` or `test-util` feature.** A release build with either one
  does not compile.

## Rust setup

- [ ] **`tauri-plugin-single-instance` is registered first**, and
  `tauri-plugin-log` before this plugin ([INTEROP.md](INTEROP.md)).
- [ ] **macOS: the terminate hook is wired** (**doctor**;
  [TROUBLESHOOTING.md](TROUBLESHOOTING.md#blank-ad-macos)).
- [ ] **No `get_webview_window`, `webview_windows` or `WebviewWindow`** in
  code that touches windows with ads (**doctor**;
  [TROUBLESHOOTING.md](TROUBLESHOOTING.md#get_webview_window-returns-none)).
- [ ] **The `tauri` crate and `@tauri-apps/api` are on the same minor**
  (**doctor**), and `tauri` is 2.12.1 or newer
  ([COMPATIBILITY.md](COMPATIBILITY.md)).
- [ ] **Privacy opt-outs that must cover the launch requests** are in the
  config, the Builder or the stored preference, not only in page code
  ([MIGRATION.md](MIGRATION.md#7-move-the-privacy-opt-outs)).

## Installers

- [ ] **Windows: build the NSIS target.** The build step writes the hooks
  that create and remove Overwolf's install record; point
  `bundle.windows.nsis.installerHooks` at
  `./gen/overwolf/installer-hooks.nsh` (**doctor**). MSI is not supported for
  Overwolf distribution: the updater refuses an `.msi` release with
  `MSI is not supported for Overwolf distribution`.
- [ ] **Sign the installer and the app** with your code-signing certificate
  (`bundle.windows.signCommand` or `certificateThumbprint`).
- [ ] **WebView2.** Keep Tauri's default `webviewInstallMode`, or ship a
  fixed-version runtime of 98.0.1108.44 or newer.
- [ ] **Test an update** over the previous release, and for a migrated app
  over the last ow-electron release.

## Overwolf signing (optional)

- [ ] With `signing.enabled`, `ow-tauri sign` runs after the frontend build,
  with `OW_CLI_EMAIL`, `OW_CLI_API_KEY` and `OW_BUILD_KEY` from your CI
  secrets (**doctor** checks the output). See
  [OVERWOLF-ONBOARDING.md](OVERWOLF-ONBOARDING.md#7-signing-optional).

## Updater (Windows, optional)

- [ ] `updater.publisherNames` (your installer's certificate subject) or
  `updater.pubkey` is set. A release build with the `updater` feature fails
  without one (**doctor**).
- [ ] Not together with `tauri-plugin-updater` on Windows (**doctor**;
  [INTEROP.md](INTEROP.md#tauri-plugin-updater)).

## Tools

- [ ] **Run the CLI as `npm exec --no -- ow-tauri`**, never `npx ow-tauri`,
  in scripts and CI. `npx` may fetch a package of the same name from the
  registry.
- [ ] **The three packages have the same version:** `tauri-plugin-overwolf`,
  `tauri-plugin-overwolf-api` and `tauri-plugin-overwolf-cli`.
