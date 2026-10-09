# ADR 0021: Ship two crates and two npm packages, each with one job

- Status: Accepted
- Date: 2026-10-08

## Context

The first design shipped one npm package, `ow-tauri`, with many jobs:

- the Electron subset;
- the main-webview runtime;
- the renderer bootstrap;
- the guest scripts;
- the Node signing CLI.

Browser bundles and Node tooling shared one dependency graph. Every app paid
for code it never used.

After the Tauri-native rewrite ([ADR 0017](0017-tauri-native-pivot.md)) an app needs a
small browser API, a build-time CLI and the Rust plugin. Tauri's `unstable`
feature must reach the app's `tauri` crate on Windows and macOS only
([ADR 0023](0023-unstable-and-macos-input.md)).

## Decision

| Artifact | Kind | Job |
|---|---|---|
| `tauri-plugin-overwolf` | crate | The plugin, the config schema, the identity functions, the build step (`build` feature) and the Windows updater (`updater` feature). |
| `tauri-plugin-overwolf-unstable` | crate | A shim that depends on `tauri` with `features = ["unstable"]`. The plugin depends on it only for Windows and macOS targets, so `unstable` reaches the app's `tauri` only there. |
| `tauri-plugin-overwolf-api` | npm | Browser-only ESM: the command wrappers (`.`), the `<owadview>` element (`./adview`), the updater (`./updater`) and test helpers (`./testing`). No Node imports. |
| `tauri-plugin-overwolf-cli` | npm | Node CLI `ow-tauri`: `sign`, `sign-exe`, `migrate`, `init`, `doctor`. Installed as a local dev dependency and run with `npm exec`, never `npx`. |

The guest scripts are built from `packages/guest-shims` (private, not
published). The crate embeds the committed output in
`crates/tauri-plugin-overwolf/js/`, and CI fails when that output differs
from a fresh build.

All four artifacts share one version and are released together.

## Consequences

- A web bundle pulls in only `@tauri-apps/api` and the API package.
- The CLI's Node dependencies never reach the app.
- Linux builds and the `build` feature never enable `unstable`. CI checks
  this with `cargo tree`.
- Two crates must be published in order: the shim first.

## Alternatives considered

- **One npm package with conditional exports.** Browser and Node entry
  points would still share the dependency graph and the audit surface.
  Rejected.
- **Ask apps to enable `unstable` themselves for each target.** This is easy
  to get wrong, and a wrong setting silently breaks ads on one OS. It
  remains the documented fallback if Cargo ever rejects the shim.
