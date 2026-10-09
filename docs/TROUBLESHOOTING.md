# Troubleshooting

Register `tauri-plugin-log` before the plugin to see its messages. The
plugin logs under the targets `tauri_plugin_overwolf` and
`tauri_plugin_overwolf::updater`. `npm exec --no -- ow-tauri doctor` checks
the most common setup mistakes without running the app.

## Contents

- [No ad fills](#no-ad-fills)
- [A command fails with `forbidden`](#a-command-fails-with-forbidden)
- [A command fails with a permission error](#a-command-fails-with-a-permission-error)
- [Ads report `unsupported`](#ads-report-unsupported)
- [The ad covers my menu](#the-ad-covers-my-menu)
- [I hear no ad audio](#i-hear-no-ad-audio)
- [A consent window opens on every launch](#a-consent-window-opens-on-every-launch)
- [Blank ad on macOS](#blank-ad-macos)
- [Keyboard input on macOS](#keyboard-input-on-macos)
- [`get_webview_window` returns `None`](#get_webview_window-returns-none)
- [Restart does nothing under `tauri dev`](#restart-does-nothing-under-tauri-dev)
- ["too late for this launch's burst"](#too-late-for-this-launchs-burst)
- [Analytics lost at Windows log off](#analytics-lost-at-windows-log-off)
- [Updater errors](#updater-errors)
- [Configuration errors at build or start](#configuration-errors-at-build-or-start)

## No ad fills

Check, in this order:

1. **Test ads are on.** Live ads fill only after Overwolf approved your app
   ([OVERWOLF-ONBOARDING.md](OVERWOLF-ONBOARDING.md)). Turn test ads on with
   `ads.testAd`, `OW_TAURI_TEST_AD=1`, `--test-ad` or
   `Builder::test_ad(true)`. `(await getInfo()).testAd` says whether they
   are on.
2. **The runtime is imported.** The page imports
   `tauri-plugin-overwolf-api/adview` before the element is added.
3. **The element has a size.** Size its container. An element with an empty
   box does not mount (a `performance` element is the exception).
4. **The ad is visible.** The plugin tells the ad page it is hidden unless
   the window is shown and not minimized and at least half of the element is
   inside the viewport.
5. **The webview has the permission.** Its capability grants
   `overwolf:default` and selects the webview by label
   ([api/permissions.md](api/permissions.md)).
6. **The page is a local app page.** See
   [`forbidden`](#a-command-fails-with-forbidden).
7. **The platform supports ads.** See [`unsupported`](#ads-report-unsupported).

A fill can take a few seconds on the first launch: an ad that mounts while
the consent check runs waits for it, up to 3 seconds. Without fill the
element simply stays empty; it shows your container's background.

## A command fails with `forbidden`

```text
tauri-plugin-overwolf: <label> is not a local app webview
```

The plugin answers only pages from your app: Tauri's app origin
(`tauri://localhost`, `http://tauri.localhost`, `https://tauri.localhost`),
the `devUrl` origin in a debug build, and origins you list in
`ads.allowedEmbedderOrigins`. A remote page, or an app served by
`tauri-plugin-localhost`, needs its origin in that list. Overwolf origins are
refused there.

The plugin's own labels (`owad-*`, `ow-cmp*`) are reserved. A window you
create with such a label logs
`label "<label>" is reserved for tauri-plugin-overwolf; the window is not tracked and cannot host ads`.
Rename it.

## A command fails with a permission error

Tauri refuses a command its capability does not allow, with a message that
names the command, for example `overwolf.get_machine_ids not allowed`. Add the
set that contains it ([api/permissions.md](api/permissions.md)):
`getMachineIds()` needs `overwolf:machine-id`, the email hash functions need
`overwolf:email-hashes`, `setAnonymousAnalyticsPreference`,
`setAnalyticsUserEnabled` and `setExternalPaymentUserId` need
`overwolf:analytics`, and the updater needs `overwolf:updater`.

## Ads report `unsupported`

`getInfo().adsSupported` is `false`, and mounting an ad rejects with one of:

| Message | Cause |
|---|---|
| `ads are not available on Linux` | Linux builds have no ads in 1.0 |
| ``ads are not available in this build (feature `ads` is off)`` | the crate was added with `default-features = false` and without `ads` |
| `ads need the WebView2 Runtime 98.0.1108.44 or newer` | Windows with an older WebView2 Runtime. Evergreen WebView2 updates itself; a fixed-version runtime must be updated by you. |

## The ad covers my menu

Each ad is a native webview above your page; HTML cannot paint over it. Hide
the element (or an ancestor) while a menu or modal is open. The ad is told it
is hidden and comes back when you show it again. See
[api/owadview.md](api/owadview.md#layout-and-visibility).

## I hear no ad audio

Ads start muted, as in ow-electron. Call `element.setAudioMuted(false)` after
the user asked for sound.

## A consent window opens on every launch

That is expected. On every launch the plugin opens Overwolf's consent page in
a hidden window (`ow-cmp-startup`), as ow-electron does. The user sees a
window only when Overwolf asks for consent. If you use
`tauri-plugin-window-state`, filter the `ow-cmp` labels out
([INTEROP.md](INTEROP.md#tauri-plugin-window-state)).

<a id="blank-ad-macos"></a>

## Blank ad on macOS

On macOS each webview's content runs in its own process. When that process
dies, the webview stays blank until it is reloaded. Tauri reports this only
to an app-wide hook, so your app has to wire it. Without it, the plugin logs
at startup:

```text
tauri-plugin-overwolf: ad guests cannot be recovered after a crash on macOS; wire tauri_plugin_overwolf::web_content_process_terminate_hook() (docs/TROUBLESHOOTING.md#blank-ad-macos)
```

Add the hook:

```rust
#[cfg(target_os = "macos")]
let builder = builder.on_web_content_process_terminate(
    tauri_plugin_overwolf::web_content_process_terminate_hook(),
);
```

An app that already has its own hook calls
`tauri_plugin_overwolf::handle_web_content_process_terminate(webview)` from it
and adds `Builder::new().forwards_web_content_process_terminate()`
([api/rust.md](api/rust.md#the-macos-terminate-hook)).

## Keyboard input on macOS

Tauri's child webviews (its `unstable` feature, which ads need) break some
keyboard input on macOS: arrow keys insert characters, and keys do nothing
until the first click. The plugin repairs this for every webview of the app.
It gives each webview a key responder that offers key shortcuts to the main
menu, and it focuses the app's webview when its window opens. Ad webviews
never take the focus.

If your app ships its own fix, turn the plugin's off:

```rust
let overwolf = tauri_plugin_overwolf::Builder::new()
    .macos_key_fix(false)
    .build();
```

`macos_key_fix` exists on macOS only; wrap it in
`#[cfg(target_os = "macos")]` in a cross-platform app.

## `get_webview_window` returns `None`

While a window shows an ad, it has two webviews: yours and the ad's. Tauri's
`WebviewWindow` helpers expect one, so `app.get_webview_window("main")`
returns `None`, `app.webview_windows()` leaves the window out, and a command
argument typed `WebviewWindow` fails with
`current webview is not a WebviewWindow`. Use `app.get_window(label)`,
`app.get_webview(label)`, and `Window` or `Webview` command arguments.
`ow-tauri doctor` lists the old helpers in `src-tauri/src`.

## Restart does nothing under `tauri dev`

`app.restart()` (or `relaunch()` from `tauri-plugin-process`) ends the
process and starts a new one. Under `npm run tauri dev`, the Tauri CLI
stops when the first process ends, and takes its dev server with it. The new
process then has no page to load and shows no window. This is how the Tauri
CLI works, not the plugin.

To test a restart, build a debug app with the assets inside and run the
binary:

```sh
npm run tauri build -- --debug --no-bundle
```

Then start the binary from `src-tauri/target/debug/`. The
[ad showcase](../examples/ad-showcase) example has a `start:tauri` script
that does both.

## "too late for this launch's burst"

```text
too late for this launch's burst; use plugins.overwolf.analytics.disableAnonymous, the Builder, or setAnonymousAnalyticsPreference(false)
```

`disableAnonymousAnalytics()` was called after the launch analytics were
sent. It applies from now on. To also cover the launch requests, use one of
the options the message names ([MIGRATION.md](MIGRATION.md#7-move-the-privacy-opt-outs)).

## Analytics lost at Windows log off

On exit the plugin waits up to 1.5 seconds to send the pending analytics.
When Windows ends the session (log off, shut down), Tauri delivers no exit
event, so those requests are lost, as after a crash.

## Updater errors

The updater runs on Windows only. Elsewhere `check()` rejects with
`unsupported`; use `tauri-plugin-updater` there ([INTEROP.md](INTEROP.md)).

| Message | Fix |
|---|---|
| `set publisherNames (your installer's certificate subject) or pubkey before checking for updates` | set `updater.publisherNames` or `updater.pubkey` ([CONFIG.md](CONFIG.md#updater)) |
| `The update installer is signed by another publisher.` | `publisherNames` must match the subject of the certificate that signed the installer |
| `The update installer has no valid signature.` | sign the installer, or use `pubkey` with a minisign signature |
| `The signature check could not run.` | the check runs `Get-AuthenticodeSignature` through Windows PowerShell; a policy that blocks PowerShell blocks it. Use `pubkey` instead. |
| `The update file's SHA-512 does not match the feed.` | the feed and the uploaded installer differ; upload them again |
| `MSI is not supported for Overwolf distribution` | build the NSIS target ([PRODUCTION-CHECKLIST.md](PRODUCTION-CHECKLIST.md#installers)) |

## Configuration errors at build or start

A wrong `plugins.overwolf` value stops the build (the build step runs the same
checks) or the app start, with a message that names the key, for example:

```text
plugins.overwolf: set "uid", or both "author" and "name", before a release build (the uid must not depend on defaults)
```

[CONFIG.md](CONFIG.md#validation) lists every message. A key from a preview
release says `removed in ow-tauri 1.0` and links its entry in
[MIGRATION.md](MIGRATION.md#keys-removed-in-10).
