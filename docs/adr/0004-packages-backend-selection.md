# ADR 0004: Packages: report them as unavailable, defer the package runtime

- Status: Accepted (amended 2026-10-08)
- Date: 2026-10-06

## Context

`app.overwolf.packages` gives apps GEP, overlay, recorder, utility and CRN.
Overwolf's documentation states that these packages are delivered and run by
ow-electron's package manager, are Windows-only today, and do not load on
macOS or Linux, where ow-electron supports ads only
(https://dev.overwolf.com/ow-electron/guides/dev-tools/non-windows-dev). A
Tauri host cannot load packages built for ow-electron's internals, and
ow-tauri must not pretend it can.

Apps still need the API surface: the sample registers listeners on startup,
renders the package-channels page, and disables UI per package based on what
loaded.

The project owner's decision of 2026-10-06 narrows the scope to the
ads system first (ads, consent, analytics, identity, updates and
distribution), and asks for exact ow-electron parity everywhere.

## Decision

- **No package runtime and no simulated backends now.** On every OS,
  `app.overwolf.packages` behaves exactly as ow-electron 42.11.4 behaves on a
  host where packages are unavailable, as observed with the parity harness
  (CONTRACT H.1): no events at all; `hasPendingUpdates()` returns
  `{ hasPendingUpdate: false, details: [] }`; `getChannel()` resolves `{}`;
  `getAvailableChannels(name)` rejects with
  `getAvailableChannels - package '<name>' is not registered in this app`;
  package objects are `undefined`.
- `packagesBackend` keeps two values: `none` (the default, the behaviour
  above) and `native`, reserved for a future runtime and behaving as `none`
  until one exists.
- The package runtime interface (Rust trait, JSON-RPC sidecar, C ABI,
  remote values, package objects) is kept in CONTRACT as a clearly marked
  **deferred design** (Appendix P). It is not binding and not tested.

## Consequences

- Apps run unchanged on every OS and degrade exactly as they do under
  ow-electron on macOS: the code paths an ow-electron app already has for
  "no packages" are the ones that run.
- No fake gaming data can reach users, in any build.
- Developers cannot exercise gaming features on ow-tauri until a runtime
  exists; the sample's package pages show their "not available" state.
- The deferred design remains available for Overwolf to review (OQ-21,
  OQ-33) without costing implementation and maintenance now.

## Alternatives considered

- **Omit `packages` off Windows.** Breaks apps that subscribe at startup.
  Rejected.
- **Load ow-electron's package files in a Node sidecar.** Depends on
  undocumented internals and integrity checks we cannot satisfy. Rejected.
- **Simulated backends for development** (the original decision). Useful for
  demos, but not parity, and a large surface to maintain for behaviour no
  user receives. Removed by the scope cut.
- **`failed-to-initialize` with `{ reason: 'unsupported-host' }`** (the
  original decision). ow-electron emits no event at all on such hosts, so
  this would diverge from what app code is written against. Rejected.

## Amendments

- 2026-10-06, the project owner's scope cut: simulated backends removed;
  `packagesBackend` reduced to `none` and `native`; unavailable packages
  report the observed ow-electron results instead of `failed-to-initialize`;
  the runtime interface becomes a deferred design (CONTRACT Appendix P).
- 2026-10-08, Tauri-native rewrite ([ADR 0017](0017-tauri-native-pivot.md)): reviewed. 1.0 has no `packages` API at all. There is nothing to report as unavailable, and `packagesBackend` is removed. CONTRACT H states this. The runtime design that was Appendix P is no longer part of the contract.
