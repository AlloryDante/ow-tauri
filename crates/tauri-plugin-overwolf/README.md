# tauri-plugin-overwolf

Overwolf ads, consent and app analytics for Tauri 2 apps.

The plugin gives a Tauri app what ow-electron gives an Electron app: the
`<owadview>` ad element, Overwolf's consent flow, email hashes, the anonymous
app analytics, the app uid and machine ids, and Overwolf's update feed on
Windows. Overwolf receives the same data it receives from an ow-electron app,
with two differences: the analytics host label says `tauri` where
ow-electron says `electron`, and on macOS ad subresource requests do not
carry ow-electron's `Origin` and `x-ow-*` headers
([CONTRACT D.8.3](https://github.com/AlloryDante/ow-tauri/blob/main/docs/CONTRACT.md#d83-per-platform)).

The JavaScript half is the npm package `tauri-plugin-overwolf-api`, in
[`packages/api`](https://github.com/AlloryDante/ow-tauri/tree/main/packages/api).

This project is not affiliated with or endorsed by Overwolf.

## Platforms

Windows 10 22H2 and 11 x64 (WebView2 98.0.1108.44 or newer) and macOS 14 or newer on
Apple Silicon are supported. Windows arm64, Intel Macs and older macOS are
best effort. Linux builds, but ads report `unsupported`. Mobile builds answer
`unsupported` to every command. Requires `tauri` 2.12.1 or newer (below 3)
and Rust 1.90.

## Install

Not on crates.io yet. Add the crate from GitHub in `src-tauri/Cargo.toml`
(optionally pinned with `rev = "<commit>"`):

```toml
[dependencies]
tauri-plugin-overwolf = { git = "https://github.com/AlloryDante/ow-tauri" }

[build-dependencies]
tauri-plugin-overwolf = { git = "https://github.com/AlloryDante/ow-tauri", default-features = false, features = ["build"] }
```

The npm packages are built from a clone of the repository
([getting started](https://github.com/AlloryDante/ow-tauri/blob/main/docs/GETTING-STARTED.md#before-you-start)).

| Feature | Default | What it adds |
|---|---|---|
| `plugin` | on | the Tauri plugin |
| `ads` | on | the ads; turns on Tauri's `unstable` feature on Windows and macOS |
| `updater` | off | Overwolf's update client (Windows) |
| `build` | off | the build step, for `[build-dependencies]` |

## Use

`src-tauri/build.rs`:

```rust
fn main() {
    tauri_plugin_overwolf::build::run().expect("tauri-plugin-overwolf build step failed");
    tauri_build::build();
}
```

`src-tauri/src/lib.rs`:

```rust
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_overwolf::init());

    // macOS: lets the plugin recover an ad whose web content process died.
    #[cfg(target_os = "macos")]
    let builder = builder.on_web_content_process_terminate(
        tauri_plugin_overwolf::web_content_process_terminate_hook(),
    );

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

`src-tauri/tauri.conf.json`:

```json
{
  "plugins": {
    "overwolf": {
      "author": "Example Studio",
      "name": "Example App",
      "ads": { "testAd": true }
    }
  }
}
```

`author` and `name` give the app's Overwolf uid, as in ow-electron. A
release build needs them (or `uid`).

`src-tauri/capabilities/default.json`:

```json
{
  "identifier": "default",
  "webviews": ["main"],
  "permissions": ["core:default", "overwolf:default"]
}
```

Select **webviews**, not windows: an ad is a child webview inside your
window, and a capability that names the window would also cover the ad.

From Rust:

```rust
use tauri_plugin_overwolf::OverwolfExt;

let ow = app.overwolf();
println!("uid {}, test ads {}", ow.uid(), ow.is_test_ad());
```

## Documentation

- [Getting started](https://github.com/AlloryDante/ow-tauri/blob/main/docs/GETTING-STARTED.md)
- [Rust API](https://github.com/AlloryDante/ow-tauri/blob/main/docs/api/rust.md) (`cargo doc -p tauri-plugin-overwolf --open` builds the rustdoc locally)
- [Configuration](https://github.com/AlloryDante/ow-tauri/blob/main/docs/CONFIG.md)
- [Permissions](https://github.com/AlloryDante/ow-tauri/blob/main/docs/api/permissions.md)
- [Migrating from ow-electron](https://github.com/AlloryDante/ow-tauri/blob/main/docs/MIGRATION.md)
- [Troubleshooting](https://github.com/AlloryDante/ow-tauri/blob/main/docs/TROUBLESHOOTING.md)
- [Compatibility](https://github.com/AlloryDante/ow-tauri/blob/main/docs/COMPATIBILITY.md)

## License

MIT or Apache-2.0, at your option.
