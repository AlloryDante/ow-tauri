# ADR 0004: Select the packages backend: native, simulated or none

- Status: Accepted
- Date: 2026-10-06

## Context

`app.overwolf.packages` gives apps GEP, overlay, recorder, utility and CRN.
Overwolf's documentation states that these packages are delivered and run by
ow-electron's package manager, are Windows-only today, and do not load on
macOS or Linux, where ow-electron supports ads only. A Tauri host cannot load
packages built for ow-electron's internals, and ow-tauri must not pretend it
can.

Apps still need the API surface: the sample registers listeners on startup,
renders the package-channels page, and disables UI per package based on what
loaded.

## Decision

The package manager is implemented in full on the host (events, channels,
pending updates, logs folder, per-package objects). Package behaviour comes
from a backend chosen by `packagesBackend` (`auto`, `native`, `simulated`,
`none`; config, environment or command line):

- `native`: a runtime that implements the `PackageRuntime` contract, either
  in-process (Rust trait or C ABI) or as a JSON-RPC sidecar. This is the
  interface we propose Overwolf implements.
- `simulated`: development backends that behave like the packages from the
  app's point of view and say so (`version: "0.0.0-simulated"`): GEP driven by
  Overwolf's public game-events status data and recorded scenarios, overlay as
  real always-on-top windows with desktop global shortcuts, a recorder state
  machine that writes no media, utility and CRN driven by scenarios.
- `none`: every listed package fails to initialise.
- `auto`: native if registered; else simulated in debug builds; else every
  listed package emits `loading` then `failed-to-initialize` with
  `{ reason: 'unsupported-host', version }`, the same shape ow-electron apps
  already handle when a package fails.

## Consequences

- Apps run unchanged on every OS; release builds without a native runtime
  degrade exactly like a failed package in ow-electron.
- Developers can build and test gaming features on macOS and in CI.
- Simulated behaviour is never shipped by accident: `auto` excludes it from
  release builds.
- Overwolf gets a precise, versioned interface (CONTRACT H) rather than a
  request to support Tauri internals.

## Alternatives considered

- **Omit `packages` off Windows.** Breaks apps that subscribe at startup.
  Rejected.
- **Load ow-electron's package files in a Node sidecar.** Depends on
  undocumented internals and integrity checks we cannot satisfy. Rejected.
- **Simulated backends in release builds by default.** Would show fake gaming
  data to users. Rejected.
