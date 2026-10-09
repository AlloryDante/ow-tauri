# Rust API

The crate `tauri-plugin-overwolf` is the native half of the plugin. The app's
`src-tauri` crate registers it, and its build script runs the build step.
rustdoc has every item; this page is the map. The source is
`crates/tauri-plugin-overwolf/src/`.

```toml
[dependencies]
tauri-plugin-overwolf = "1.0.0-rc.1"

[build-dependencies]
tauri-plugin-overwolf = { version = "1.0.0-rc.1", default-features = false, features = ["build"] }
```

## Contents

- [Cargo features](#cargo-features)
- [Registering the plugin](#registering-the-plugin)
- [`Builder`](#builder)
- [`OverwolfExt` and `Overwolf`](#overwolfext-and-overwolf)
- [The macOS terminate hook](#the-macos-terminate-hook)
- [The updater](#the-updater)
- [The build step](#the-build-step)
- [Errors](#errors)
- [Types](#types)
- [Constants](#constants)
- [Internal modules](#internal-modules)

## Cargo features

| Feature | Default | What it adds |
|---|---|---|
| `plugin` | on | the Tauri plugin |
| `ads` | on | the ad webviews behind `<owadview>`. On Windows and macOS it turns on Tauri's `unstable` feature (child webviews) through `tauri-plugin-overwolf-unstable`. On Linux ads are unsupported and Tauri stays stable. |
| `updater` | off | Overwolf's update client. Windows only; elsewhere the commands answer `unsupported`. |
| `build` | off | the build step, `build::run()`. Use it on the build dependency with `default-features = false`. |
| `test-util`, `lab` | off | test hooks and this repository's parity lab. A release build with either one fails to compile. Never enable them in an app you ship. |

## Registering the plugin

```rust
tauri::Builder::default()
    .plugin(tauri_plugin_overwolf::init())
    .run(tauri::generate_context!())
    .expect("error while running the app");
```

`init()` is `Builder::new().build()`. Everything else comes from
`plugins.overwolf` in `tauri.conf.json` ([CONFIG.md](../CONFIG.md)).

The plugin starts its network work at `RunEvent::Ready`. Nothing is written
to disk or sent before it.

## `Builder`

Every method takes and returns the builder.

| Method | Effect |
|---|---|
| `new()` | a builder with the defaults |
| `test_ad(enabled: bool)` | turns test ads on. Test ads are on when this, `ads.testAd`, the argument `--test-ad` or `OW_TAURI_TEST_AD=1` says so. |
| `disable_anonymous_analytics()` | only the mandatory analytics are sent, from this launch's first request |
| `disable_ads_optimization()` | `disableAdsOptimization()` from the start |
| `disable_ads_fpd()` | `disableAdsFPD()` from the start: no first-party data reaches the ads |
| `host_label(label, version: Option<String>)` | the host label and version the analytics and the ads report. Replaces `analytics.hostLabel` and `analytics.hostVersion`. `None` keeps the Tauri version. |
| `exclude_windows(globs)` | window label globs (`*`, `?`) that never count as visible app windows for the analytics. Added to `analytics.excludeWindows`. |
| `forwards_web_content_process_terminate()` | macOS only. Says the app calls `handle_web_content_process_terminate` from its own hook ([below](#the-macos-terminate-hook)). |
| `macos_key_fix(enabled: bool)` | macOS only. On by default: the plugin repairs keyboard input in Tauri's child webviews. Pass `false` if your app ships its own fix. See [TROUBLESHOOTING](../TROUBLESHOOTING.md#keyboard-input-on-macos). |
| `dev_update_config(yaml: &'static str)` | Windows with `updater`. The `dev-app-update.yml` a debug build reads instead of the configured feed. Ignored in release builds. |
| `build()` | builds the plugin |

```rust
let overwolf = tauri_plugin_overwolf::Builder::new()
    .test_ad(cfg!(debug_assertions))
    .exclude_windows(["tray-*", "splash"])
    .build();
```

The uid is configuration only. There is no Builder method for it, so the
build step and the app always resolve the same uid.

## `OverwolfExt` and `Overwolf`

`use tauri_plugin_overwolf::OverwolfExt;` adds `.overwolf()` to every Tauri
manager (`App`, `AppHandle`, `Window`, `Webview`, ...). It panics when the
plugin is not registered.

```rust
use tauri_plugin_overwolf::OverwolfExt;

let ow = app.overwolf();
println!("uid {}", ow.uid());
```

Identity and state:

| Method | Returns |
|---|---|
| `info() -> Info` | what `getInfo()` returns |
| `uid() -> &str` | the effective uid |
| `cuid() -> &str` | the computed uid, even when `uid` overrides it |
| `muid() -> &str` | the machine id the analytics send as `muid` |
| `muid_v2() -> &str` | `muidV2`. Equal to `muid()` except on Windows when the shared registry values differ. |
| `phase_percent() -> u8` | this machine's phase bucket, 0 to 99 |
| `utm_params() -> Option<&Value>` | the UTM parameters stored at install, if any |
| `is_test_ad() -> bool` | whether test ads are on |
| `config() -> &Config` | the validated `plugins.overwolf`, with the Builder's overrides |
| `state_dir() -> &Path` | `<appData>/ow-electron/<uid>`, the folder of `ow-electron.json` and `ow-tauri.json` |

Consent:

| Method | Notes |
|---|---|
| `async is_cmp_required() -> bool` | never fails; `true` when the answer is not known |
| `async open_ad_privacy_settings_window(CmpWindowOptions) -> Result<()>` | opens the ad privacy settings window. A Rust caller may pass any `https:` `cmp_url`. Errors: `invalid-argument`, `not-found` (unknown parent window), `unsupported`. |
| `async open_cmp_window(CmpWindowOptions) -> Result<()>` | the deprecated alias of the above |

Privacy switches and email hashes:

| Method | Notes |
|---|---|
| `disable_anonymous_analytics()` | only the mandatory events are sent from now on. Called before `RunEvent::Ready` (for example in `setup`), it also covers the launch burst. Later, one warning says the burst was already sent. |
| `disable_ads_optimization()` | as `disableAdsOptimization()` |
| `disable_ads_fpd()` | no first-party data reaches the ads from now on |
| `set_anonymous_analytics_preference(enabled) -> Result<()>` | stores the user's choice in `ow-tauri.json`. `false` applies from the next launch's burst. Error: `io`. |
| `set_analytics_user_enabled(enabled) -> Result<()>` | the app-level switch, only with `analytics.userSwitch`; stored in `ow-tauri.json`. Errors: `unsupported` without `userSwitch`, `io`. |
| `generate_user_email_hashes(email) -> EmailHashes` | hashes the normalised address in `emailHashes.encoding`, then sends and stores the hashes as `set_user_email_hashes` does |
| `set_user_email_hashes(&EmailHashes)` | sends the hashes to every ad and stores them as `eHashes` in `ow-electron.json`, as ow-electron does. Empty hashes are ignored, and so is every call after `disable_ads_fpd()` (one warning). |
| `clear_user_email_hashes() -> Result<()>` | forgets the hashes and removes `eHashes`. Errors: `io`, `backend`. |

Analytics:

| Method | Notes |
|---|---|
| `async set_external_payment_user_id(&Map<String, Value>) -> Result<()>` | one `<label>_sub_info` request. Build the map with `PaymentUserIdOptions::new(user_id).provider(..).payment(..).into_map()`. Resolves after the response or the failure. Error: `invalid-argument` when `userId` is missing or empty. |
| `set_window_name(window_label, name) -> Result<()>` | the name the analytics report for that window. Errors: `invalid-argument` unless `name` is 1 to 128 printable ASCII characters; `forbidden` for a plugin window; `not-found` for an unknown window. |
| `prepare_for_restart()` | ends the visible periods and sends the pending analytics now (at most 1.5 s). Every exit and `app.restart()` already does this, so you rarely need it. |

Updater (Windows, feature `updater`):

| Method | Notes |
|---|---|
| `updater() -> Result<Updater<R>>` | the update client with the configured feed |
| `updater_builder() -> UpdaterBuilder<R>` | a builder with the configured defaults |

## The macOS terminate hook

On macOS each webview's content runs in its own process. When that process
dies, the webview stays blank until something reloads it. Tauri reports this
only through the app-wide `tauri::Builder::on_web_content_process_terminate`,
which a plugin cannot install. So the app wires it, in one of two ways:

```rust
tauri::Builder::default()
    .plugin(tauri_plugin_overwolf::init())
    .on_web_content_process_terminate(tauri_plugin_overwolf::web_content_process_terminate_hook())
```

or, if the app has its own hook:

```rust
tauri::Builder::default()
    .plugin(
        tauri_plugin_overwolf::Builder::new()
            .forwards_web_content_process_terminate()
            .build(),
    )
    .on_web_content_process_terminate(|webview| {
        tauri_plugin_overwolf::handle_web_content_process_terminate(webview);
        // your own handling
    })
```

`handle_web_content_process_terminate` reloads an app webview, recovers an ad,
closes a hidden consent window and reloads the visible consent settings
window. Without either form, the plugin logs one warning at startup that
points to [TROUBLESHOOTING](../TROUBLESHOOTING.md#blank-ad-macos). Both
functions exist on macOS only; wrap the call in `#[cfg(target_os = "macos")]`
in a cross-platform app.

## The updater

Module `updater`, feature `updater`. The client types are exported on Windows
only. It reads Overwolf's update feed for the app's uid and does what
electron-updater does with it. See [INTEROP](../INTEROP.md) before you also
use `tauri-plugin-updater`.

`UpdaterBuilder` (from `app.overwolf().updater_builder()`):

| Method | Notes |
|---|---|
| `channel(name)` | `latest`, `beta`, ... Setting a channel also allows a downgrade, as electron-updater does; call `allow_downgrade(false)` after it to turn that off. |
| `allow_downgrade(bool)` | whether a lower version may be offered |
| `allow_prerelease(bool)` | whether pre-release versions may be offered |
| `header(name, value) -> Result<Self>` | a request header for the feed origin only. It is dropped on cross-origin redirects and never sent to the download host. Error: `invalid-argument`. |
| `connect_timeout(Duration)` | the connection timeout |
| `read_timeout(Duration)` | the longest wait for the next bytes, not a limit on the whole download |
| `build() -> Result<Updater<R>>` | errors: `unsupported` off Windows; `config` when neither `updater.publisherNames` nor `updater.pubkey` is set; `invalid-argument` for a bad channel, feed URL or key |

`Updater::check().await -> Result<Option<Update>>`. `None` means no update.

`Update` fields: `version`, `current_version`, `date`, `body` (the release
notes as one text), `raw` (the feed entry), `download_url`, `staged`
(the feed had a `stagingPercentage` and this install passed it).

| Method | Notes |
|---|---|
| `download(on_chunk, on_finish).await -> Result<DownloadedUpdate>` | downloads and verifies the installer. `on_chunk(length, total)` per chunk, `on_finish()` once. With `updater.installOnExit` (the default) the installer also starts silently when the app exits, unless `install` ran. Errors: `network`, `io`, `verification`. |
| `install(DownloadedUpdate) -> Result<()>` | verifies the installer again (hash and signer), starts it with `/UPDATE /R` (or `updater.installerArgs`, always with `/UPDATE`) and exits the app. Errors: `verification`, `io`, `not-found` when the download was already installed or replaced. |
| `download_and_install(on_chunk, on_finish).await -> Result<()>` | both |

```rust
use tauri_plugin_overwolf::OverwolfExt;

if let Some(update) = app.overwolf().updater()?.check().await? {
    update.download_and_install(|_, _| {}, || {}).await?;
}
```

Only NSIS installers are supported. An `.msi` release is refused (see
[PRODUCTION-CHECKLIST](../PRODUCTION-CHECKLIST.md#installers)).

## The build step

Module `build`, feature `build`. Call it from `src-tauri/build.rs` before
`tauri_build::build()`:

```rust
fn main() {
    tauri_plugin_overwolf::build::run().expect("tauri-plugin-overwolf build step failed");
    tauri_build::build();
}
```

`build::run() -> Result<(), BuildError>`:

- reads the merged Tauri configuration as `tauri-build` does
  (`tauri.conf.json`, the `tauri.<target>.conf.json` overlay, then
  `TAURI_CONFIG`);
- validates `plugins.overwolf`, with the release-only rules in a release
  build;
- resolves the uid the app will use at run time;
- checks the capabilities: a `remote.urls` entry that covers Overwolf pages
  fails the build, and a `windows` selector while `ads` is on warns;
- for a Windows target, writes `gen/overwolf/overwolf-hooks.nsh` and
  `gen/overwolf/installer-hooks.nsh`, the NSIS hooks that write and remove
  the install record `Software\OverwolfElectron\<uid>`. Point
  `bundle.windows.nsis.installerHooks` at `./gen/overwolf/installer-hooks.nsh`;
- with `signing.enabled`, a Windows release build links the signed-build
  resource from the `ow-tauri sign` output, and fails without it when
  `signing.requireSigning` is on (the default).

It prints one `cargo:warning` per finding.

`BuildError` variants: `Env` (a build-script variable is missing), `File`
(a configuration file could not be read or parsed), `TauriConfig`
(`TAURI_CONFIG` is not a JSON object), `Config` (`plugins.overwolf` is
invalid), `NoName` (no `plugins.overwolf.name`, `productName` or Cargo
package name), `Capability` (a capability lets Overwolf ad pages call the
app), `Signing` (the signed build cannot be completed).

`BuildIdentity` holds what the build resolved: `uid`, `cuid`, `name`,
`author`, `version`.

## Errors

`Error` is the error of every fallible method; `Result<T>` is
`std::result::Result<T, Error>`. `error.code()` returns an `ErrorCode`, and
`ErrorCode::as_str()` gives the wire spelling. JavaScript receives the same
code ([js.md](js.md#errors)).

| `ErrorCode` | Wire code |
|---|---|
| `Unsupported` | `unsupported` |
| `InvalidArgument` | `invalid-argument` |
| `NotFound` | `not-found` |
| `Forbidden` | `forbidden` |
| `Io` | `io` |
| `Network` | `network` |
| `Verification` | `verification` |
| `Backend` | `backend` |
| `Config` | `config` |
| `Tauri` | `tauri` |

`ErrorCode::ALL` lists the ten codes.

## Types

| Type | What it is |
|---|---|
| `Info` | `getInfo()`: `uid`, `app_cuid`, `phase_percent`, `utm_params`, `test_ad`, `ads_supported`, `name`, `version`, `host` |
| `HostInfo` | `label`, `version`, `ow_version` |
| `MachineIds` | `muid` (`muid_v2` when present, else the first-generation id, as ow-electron reports it) and `muid_v2` |
| `EmailHashes` | `sha1`, `md5`, `sha256`, each `Option<String>`; `is_empty()` |
| `HashEncoding` | `Hex` (default) or `Base64` |
| `CmpWindowOptions` | the privacy settings window options: `tab`, `modal`, `parent`, `center`, `background_color`, `pre_loader_spinner_color`, `width`, `height`, `x`, `y`, `cmp_url`, `language`. Builders: `new()`, `tab(CmpTab)`, `modal_to(parent)`, `cmp_url(url)`. |
| `CmpTab` | `Purposes`, `Features`, `Vendors` |
| `PaymentUserIdOptions` | `new(user_id)`, `provider(name)`, `payment(id)`, `into_map()` |
| `Config` and the `*Config` types | `plugins.overwolf`, in [CONFIG.md](../CONFIG.md) |
| `ConfigError` | a validation failure: `path` and `message` |

Module `identity` holds the pure functions behind the ids, for example
`identity::computed_uid(author, name)`, `is_valid_uid`, `phase_percent`,
`normalize_email` and `email_hashes`.

## Constants

| Constant | Value |
|---|---|
| `PLUGIN_NAME` | `"overwolf"` |
| `COMMAND_PREFIX` | `"plugin:overwolf\|"` |
| `VERSION` | the crate version |
| `COMMANDS` | the 25 command names, in wire spelling |

## Internal modules

`ads`, `analytics`, `consent` and `state` are public only for their tests.
They are hidden from rustdoc and not covered by semver. Do not use them.
