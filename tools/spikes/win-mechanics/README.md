# Spike: Windows and Cargo mechanics

A standalone Cargo workspace, not part of the root workspace. It is never
published or bundled. It checks eight Windows and Cargo mechanisms the
Tauri-native plugin relies on, items B1 to B8 (each a key in the JSON
verdict). Guests load only loopback fixture pages: never
an ad, and never a click on an ad.

## Layout

- `app/`: one Tauri app that stands up ad-guest child webviews over a
  loopback fixture page (`app/web/guest-fixture.html`, served from
  `127.0.0.1` by `app/src/loopback.rs`) and runs the runtime probes
  (`app/src/win.rs`, Windows COM), writing a JSON verdict per item to
  `SPIKE_OUT`. The app asks for plain `tauri`; `unstable` arrives only through
  `spike-miniplugin`'s Windows/macOS target dependency on `unstable-shim`.
- `miniplugin/` and `unstable-shim/`: the shipped B7 pattern: a plugin crate
  turns on tauri's `unstable` feature on Windows and macOS only, via a tiny
  shim crate it depends on from a `cfg(any(windows, target_os = "macos"))`
  target table under its `ads` feature.
- `b7-variants/`: the two B7 alternatives, for the record: `v1-*` is the
  design's renamed `tauri-unstable = { package = "tauri" }` dependency; `v2-*`
  is the same `tauri` name in a target table.
- `b8-rewrap/` (B8): a crate holding two `webview2-com` versions (wry's 0.39.1
  and a different 0.38.2) that re-wraps a raw COM controller pointer with the
  alternate set; `cargo build` answers whether that compiles and links.
- `cargo-tree.sh`: the B7 resolution matrix (`cargo tree` and `cargo check
  --unit-graph`); nothing is compiled. Prints one `VERDICT` line per case.

## Run

- B7 (local, no build): `./cargo-tree.sh`
- B1 to B6 and B8 (Windows): the `spike-win-mechanics` workflow (`workflow_dispatch`)
  builds `app` and `b8-rewrap` on `windows-2025` and runs the probes.

Use your own `CARGO_TARGET_DIR` outside the repo; delete it after.
