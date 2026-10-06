# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). The Rust crate and
the npm package share one version number.

## [Unreleased]

### Added

- Upstream `ow-electron-packages-sample` imported verbatim at commit `8a27053`
  into `examples/packages-sample` (MIT, Overwolf Ltd.).
- Cargo workspace with the `tauri-plugin-overwolf` crate scaffold.
- npm workspace with the `ow-tauri` package scaffold (`./main`, `./electron`,
  `./renderer`) and the shared `OwTauriError` / `OwTauriUnsupportedError` types.
- Tooling: rustfmt, clippy (pedantic), ESLint (strict type-checked), Prettier,
  Vitest, TypeDoc, EditorConfig, CI on macOS, Windows and Linux.
- Contract documents: architecture, contract, ADRs, open questions, port map.
- Contract review: per-webview IPC channels instead of events (ADR 0010),
  main-webview liveness and lifecycle (ADR 0009), scoped and rate-limited guest
  commands (ADR 0011), one injected JS runtime per webview (ADR 0012), a
  remote-value protocol for package callbacks and handles (now CONTRACT
  P.1.1),
  typings (CONTRACT B.4), `<owadview>` default style and creation-time
  upgrade, `shell.openPath` checks, fail-closed update signatures, CSP
  baseline, threat model, open questions OQ-32 to OQ-37 and an index.
- Error code `ipc-overloaded`; `OwTauriError` `instanceof` works across
  runtime copies.
- CI: actions pinned by SHA, committed `package-lock.json`, cargo-deny,
  `npm audit`, Overwolf dist-tag check, weekly newest-toolchain and soak jobs,
  Dependabot; `.gitattributes` for LF line endings.

- `docs/PARITY.md`: the parity definition, the harness scenarios, the parity
  matrix, lab checks, the harness round-2 list, and how to re-run it.
- ADR 0013 (request shaping per OS), ADR 0014 (machine-id parity), ADR 0015
  (startup consent window), ADR 0016 (signing approach).
- CONTRACT: the startup consent window, `cmp-eu-only`, consent cookies and
  the cookie fallback (D.6); request shaping and the ads environment (A.1.1,
  D.8); the 31-key `window.__overwolf__` (D.2); exact analytics request shapes
  and order (E.1, E.2); the machine id per OS with test vectors (E.4); the
  exact `ow-electron.json` encoding (F.2); the uid rule with 13 test vectors
  (G.2); signing with `ow-tauri sign` (G.4); Overwolf's update feed and the
  NSIS hooks (I.1, I.6).
- Configuration: `analytics.hostLabel`, `analytics.hostVersion`,
  `ads.owVersionOverride`, `ads.legacyHostMessages`,
  `ads.macPrivateHeaderApi`, `consent.hostCookieFallback`,
  `logging.enabled`; `Builder::uid` and `Builder::host_label`.

### Changed

- Parity revision of the contract: ow-tauri replicates what ow-electron does,
  as observed with the parity harness, and differs only in the host label
  (default `"tauri"`).
- Analytics are ow-electron's events and requests with `<label>` in the
  names and `<label>-<hostVersion>` as the Overwolf version (ADR 0006).
- `analytics.muidStrategy` defaults to `machine-id` (ADR 0014);
  `ads.requestShaping` defaults to on (ADR 0013).
- `<owadview>` events are plain `Event` objects with the data as own
  properties; the element gets an open shadow root; `did-attach` and
  `did-fail-load` carry ow-electron's properties (CONTRACT B.3).
- `adview_mount` no longer waits for consent; each guest's first navigation
  waits for the startup consent window instead (CONTRACT D.6.5).
- `app.overwolf.packages` reports packages as unavailable on every OS: no
  events, `getChannel()` resolves `{}`, `getAvailableChannels()` rejects with
  ow-electron's message (CONTRACT H, ADR 0004).
- The package runtime interface moved to CONTRACT Appendix P as a deferred
  design.
- Logging is off by default (CONTRACT F.4).
- ADR 0005: labs may load live ads under fixed rules; test and live modes are
  identical on the wire.
- ADR 0007: the state file uses ow-electron's exact encoding.
- OPEN-QUESTIONS: every question now states whether it is answered, decided
  or still open, with its source.

### Removed

- Simulated package backends and `packagesBackend` values `auto` and
  `simulated`; the `failed-to-initialize` reasons `unsupported-host`,
  `no-native-runtime` and `packages-disabled`.
- Configuration keys `analytics.hostFields`, `ads.experimentalElementApi`,
  `ads.exposeEmailHashesToGuest`, `consent.gateAdsOnConsent` and
  `consent.cmpRequired`.
- `adview_command` actions `setPageUrl` and `sendCommand`.
- The consent cookie write from the ad shim (the consent page writes them).

### Planned documentation

`docs/MIGRATION.md`, `docs/api/`,
`examples/packages-sample/CHANGES-FROM-UPSTREAM.md` and
`examples/packages-sample/.env.example` (see CONTRIBUTING.md, "Documentation
checklist").
