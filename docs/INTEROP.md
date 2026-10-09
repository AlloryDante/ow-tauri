# Using the plugin with other Tauri plugins

The plugin works with Tauri's official plugins. A few need an order or a
setting. The full example is
[examples/quickstart-vanilla/src-tauri/src/lib.rs](../examples/quickstart-vanilla/src-tauri/src/lib.rs).

| Plugin | Rule |
|---|---|
| [`tauri-plugin-single-instance`](#tauri-plugin-single-instance) | register it first |
| [`tauri-plugin-window-state`](#tauri-plugin-window-state) | filter out the `ow-cmp` windows |
| [`tauri-plugin-log`](#tauri-plugin-log) | register it before this plugin |
| [`tauri-plugin-updater`](#tauri-plugin-updater) | not on Windows together with the `updater` feature |
| [`tauri-plugin-process`](#tauri-plugin-process) | nothing to do |
| [`tauri-plugin-localhost`](#tauri-plugin-localhost-and-remote-pages) | add the origin to `ads.allowedEmbedderOrigins` |
| [`tauri-plugin-opener`, `-shell`, `-fs`](#opener-shell-and-fs-plugins) | nothing to do |

## `tauri-plugin-single-instance`

Register it before every other plugin. A second launch then ends in its
setup. The Overwolf plugin writes nothing to disk and sends nothing before
`RunEvent::Ready`, so the second process leaves no trace.

```rust
use tauri::Manager;

tauri::Builder::default()
    .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        // get_window, not get_webview_window: a window that shows an ad has two webviews.
        if let Some(window) = app.get_window("main") {
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    }))
    .plugin(tauri_plugin_overwolf::init())
```

## `tauri-plugin-window-state`

The plugin opens Overwolf's consent page in windows labelled `ow-cmp...`.
Their size and position are not yours to restore:

```rust
.plugin(
    tauri_plugin_window_state::Builder::new()
        .with_filter(|label| !label.starts_with("ow-cmp"))
        .build(),
)
```

Ads are webviews inside your windows, not windows, so they need no filter.

## `tauri-plugin-log`

Register it before this plugin, so the messages the plugin logs during its
setup reach it. The targets are `tauri_plugin_overwolf` and
`tauri_plugin_overwolf::updater`.

## `tauri-plugin-updater`

On Windows, use one updater. The plugin's `updater` feature reads Overwolf's
update feed and can install a downloaded update when the app exits;
`tauri-plugin-updater` does the same with its own feed. With both, two
installers could start at exit. `ow-tauri doctor` warns:

```text
both tauri-plugin-updater and the overwolf "updater" feature are on: two updaters would race at exit on Windows; register only one there (docs/INTEROP.md)
```

The plugin's updater runs on Windows only. On macOS and Linux,
`tauri-plugin-updater` is the one to use. Split them with target-specific
dependencies:

```toml
[target.'cfg(windows)'.dependencies]
tauri-plugin-overwolf = { git = "https://github.com/AlloryDante/ow-tauri", features = ["updater"] }

[target.'cfg(not(windows))'.dependencies]
tauri-plugin-overwolf = { git = "https://github.com/AlloryDante/ow-tauri" }
tauri-plugin-updater = "2"
```

## `tauri-plugin-process`

`relaunch()` and `app.restart()` are covered: before the process ends, the
plugin sends the pending analytics (at most 1.5 seconds), as on any exit.
`prepare_for_restart()` is not needed. Under `tauri dev`, see
[TROUBLESHOOTING.md](TROUBLESHOOTING.md#restart-does-nothing-under-tauri-dev).

## `tauri-plugin-localhost` and remote pages

The plugin answers only your app's own pages: Tauri's app origin, and the
`devUrl` origin in a debug build. An app served from another origin, such as
`http://localhost:<port>` with `tauri-plugin-localhost`, lists it:

```json
"plugins": { "overwolf": { "ads": { "allowedEmbedderOrigins": ["http://localhost:9527"] } } }
```

Never list an Overwolf origin; the configuration check refuses it. A remote
page also needs a capability with `remote.urls` that names its origin. A
`remote.urls` entry that covers `*.overwolf.com` or every origin fails the
build ([api/permissions.md](api/permissions.md#select-webviews-not-windows)).

## Opener, shell and fs plugins

No interaction. The plugin opens ad clicks and consent links in the system
browser itself; it does not need these plugins registered or granted.
