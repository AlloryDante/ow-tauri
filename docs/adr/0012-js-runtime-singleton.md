# ADR 0012: One injected JS runtime per webview, with thin npm facades

- Status: Accepted
- Date: 2026-10-06

## Context

The plugin must inject JavaScript into webviews that the app does not control
at creation time: the renderer bootstrap into every UI window (so `<owadview>`
works without an import), the snapshot and runtime into `ow-main`, and the
guest shims into remote pages. The same webviews also load the app's own
bundles, which import `ow-tauri/electron` and `ow-tauri/renderer` through the
alias.

Two copies of the runtime in one webview would mean two `ipcRenderer`s with
separate sequence counters (the router would see duplicate or missing
numbers), two listener registries, two `<owadview>` observers, and two
`OwTauriError` classes, so `instanceof` would fail across them.

The crate also needs the built JavaScript at compile time (`include_str!`),
but the Rust CI job and `cargo publish` never run npm.

## Decision

- The plugin injects a versioned bootstrap that installs
  `globalThis.__OW_TAURI_RUNTIME__` (non-writable, non-configurable) before
  any page script. It owns the IPC channel, the sequence counters, the
  listener registries, the `<owadview>` runtime and the `process` shim.
- The npm entry points are facades: they attach to that object on first use
  and fail with a clear error when its contract version differs.
- Errors carry a `Symbol.for` brand and the classes implement
  `Symbol.hasInstance`, so `instanceof` works across copies.
- The bootstrap and the guest shims are TypeScript in
  `packages/ow-tauri/src/{bootstrap,guest}/`, linted, type-checked and tested
  with Vitest (happy-dom) like the rest of the package. Their built output is
  committed under `crates/tauri-plugin-overwolf/js/`; CI rebuilds it and fails
  on any difference (`git diff --exit-code`).

## Consequences

- One source of truth per webview, however many bundles import the package.
- `cargo build` works without Node; the committed bundle is reviewable in
  pull requests.
- Contributors who change bootstrap or guest code must run the build and
  commit the output; CI catches it if they forget.

## Alternatives considered

- **Let the app bundle be the only runtime.** Windows that never import the
  package would get no `<owadview>` and no IPC, and remote-page shims cannot
  come from the app bundle. Rejected.
- **Build the JS from `build.rs`.** Makes every Rust build depend on Node and
  npm. Rejected.
