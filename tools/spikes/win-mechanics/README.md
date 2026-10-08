# W0c spike B — Windows + Cargo mechanics

Standalone Cargo workspace (NOT part of the root workspace); never published,
never bundled. Answers DESIGN-v2 §10 W0c items B1–B8 for the Tauri-native
Overwolf ads port. Loopback fixture guests only — never an ad, never a click
on an ad.

## Layout

- `app/` — one Tauri app that stands up ad-guest child webviews over a
  **loopback** fixture page (`app/web/guest-fixture.html`, served from
  `127.0.0.1` by `app/src/loopback.rs`) and runs the runtime probes
  (`app/src/win.rs`, Windows COM), writing a JSON verdict per item to
  `SPIKE_OUT`. The app asks for plain `tauri`; `unstable` arrives only through
  `spike-miniplugin`'s Windows/macOS target dependency on `unstable-shim`.
- `miniplugin/` + `unstable-shim/` — the shipped B7 pattern: a plugin crate
  turns on tauri's `unstable` feature on Windows and macOS only, via a tiny
  shim crate it depends on from a `cfg(any(windows, target_os = "macos"))`
  target table under its `ads` feature.
- `b7-variants/` — the two B7 alternatives, for the record: `v1-*` is the
  design's renamed `tauri-unstable = { package = "tauri" }` dependency; `v2-*`
  is the same `tauri` name in a target table.
- `b8-rewrap/` — B8: a crate holding TWO `webview2-com` versions (wry's 0.39.1
  and a different 0.38.2) that re-wraps a raw COM controller pointer with the
  alternate set; `cargo build` answers whether that compiles and links.
- `cargo-tree.sh` — B7 resolution matrix (`cargo tree` + `cargo check
  --unit-graph`); nothing is compiled. Prints one `VERDICT` line per case.

## Run

- B7 (local, no build): `./cargo-tree.sh`
- B1–B6, B8 (Windows): the `spike-win-mechanics` workflow (`workflow_dispatch`)
  builds `app` and `b8-rewrap` on `windows-2025` and runs the probes.

Use your own `CARGO_TARGET_DIR` outside the repo; delete it after.
