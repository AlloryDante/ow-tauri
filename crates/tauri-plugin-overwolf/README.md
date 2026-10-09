# tauri-plugin-overwolf

The Rust half of the Overwolf plugin for Tauri 2. Add it to a Tauri app that
shows Overwolf ads.

The plugin gives a Tauri app what ow-electron gives an Electron app: the
`<owadview>` ad element, Overwolf's consent flow, email hashes, the anonymous
app analytics, the app uid and machine ids, and an update client for
Overwolf's update feed on Windows. Overwolf receives the same data it
receives from an ow-electron app, with two differences in normal use: the
analytics host label says `tauri` where ow-electron says `electron`, and on
macOS ad subresource requests do not carry ow-electron's `Origin` and
`x-ow-*` headers
([CONTRACT D.8.3](https://github.com/AlloryDante/ow-tauri/blob/main/docs/CONTRACT.md#d83-per-platform)).
Edge cases and platform gaps are listed in
[PARITY](https://github.com/AlloryDante/ow-tauri/blob/main/docs/PARITY.md#deviations).

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/AlloryDante/ow-tauri/main/docs/images/showcase/sizes-dark.webp">
  <img alt="The Sizes page of the ad showcase example: Overwolf test ads in 160x600, 400x600, 400x300 and 300x250 containers, each an owadview element with its load state above it." src="https://raw.githubusercontent.com/AlloryDante/ow-tauri/main/docs/images/showcase/sizes-light.webp">
</picture>

The JavaScript half is the npm package `tauri-plugin-overwolf-api`, in
[`packages/api`](https://github.com/AlloryDante/ow-tauri/tree/main/packages/api).
Your pages use it for the ad element and the plugin's calls. To see both
working before you add anything, run the
[ad showcase example](https://github.com/AlloryDante/ow-tauri/tree/main/examples/ad-showcase).
If you are moving an ow-electron app, start with the
[migration guide](https://github.com/AlloryDante/ow-tauri/blob/main/docs/MIGRATION.md):
`ow-tauri migrate` keeps the app's uid.

This project is not affiliated with or endorsed by Overwolf.

## Platforms

Supported: Windows 10 22H2 and 11 x64 (ads need WebView2 98.0.1108.44 or
newer), and macOS 14 or newer on Apple Silicon. Windows arm64, Intel Macs and
older macOS are best effort. Linux builds, but ads report `unsupported`.
Mobile builds answer `unsupported` to every command.

The crate needs `tauri` 2.12.1 or newer (below 3) and Rust 1.90 or newer.

## Install

Not on crates.io or npm yet. The planned crates.io name is
`tauri-plugin-overwolf`. Until then, add the crate from GitHub in
`src-tauri/Cargo.toml`. You can pin it with `rev = "<commit>"`; use the
commit you build the npm packages from, so the Rust and JavaScript halves
match:

```toml
[dependencies]
tauri-plugin-overwolf = { git = "https://github.com/AlloryDante/ow-tauri" }

[build-dependencies]
tauri-plugin-overwolf = { git = "https://github.com/AlloryDante/ow-tauri", default-features = false, features = ["build"] }
```

The second entry is the build step your `build.rs` calls. You build the npm
packages from a clone of the repository; see
[getting started](https://github.com/AlloryDante/ow-tauri/blob/main/docs/GETTING-STARTED.md#before-you-start).

| Feature | Default | What it adds |
|---|---|---|
| `plugin` | on | the Tauri plugin |
| `ads` | on | the ads; turns on Tauri's `unstable` feature on Windows and macOS (Linux keeps stable Tauri) |
| `updater` | off | Overwolf's update client (Windows) |
| `build` | off | the build step, for `[build-dependencies]` |

`test-util` and `lab` are for this repository's tests. A release build that
enables them fails to compile.

## Use

Call the build step in `src-tauri/build.rs`, before `tauri_build::build()`:

```rust
fn main() {
    tauri_plugin_overwolf::build::run().expect("tauri-plugin-overwolf build step failed");
    tauri_build::build();
}
```

Register the plugin in `src-tauri/src/lib.rs`. On macOS, also install the
hook that lets the plugin recover an ad whose web content process died:

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

Configure it in `src-tauri/tauri.conf.json`:

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

`author` and `name` give the app's Overwolf uid, as in ow-electron. A release
build fails without them, unless you set `uid` instead. Remove `testAd` before
you ship.

Grant the permission in `src-tauri/capabilities/default.json`:

```json
{
  "identifier": "default",
  "webviews": ["main"],
  "permissions": ["core:default", "overwolf:default"]
}
```

Select webviews, not windows. An ad is a child webview inside your window, so
a capability that names the window would also cover the ad. `default` leaves
out machine ids, email hashes, the external payment user id, the persisted
analytics preferences and the updater. Their opt-in sets are `overwolf:machine-id`,
`overwolf:email-hashes`, `overwolf:analytics` and `overwolf:updater`
([permissions](https://github.com/AlloryDante/ow-tauri/blob/main/docs/api/permissions.md)).

Read the plugin's state from Rust:

```rust
use tauri_plugin_overwolf::OverwolfExt;

let ow = app.overwolf();
println!("uid {}, test ads {}", ow.uid(), ow.is_test_ad());
```

The page side (the `<owadview>` element, consent and the other calls) is in
the [`tauri-plugin-overwolf-api` README](https://github.com/AlloryDante/ow-tauri/tree/main/packages/api#readme).

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
