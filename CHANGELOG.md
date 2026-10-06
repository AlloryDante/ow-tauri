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
  `ads.owVersionOverride`, `ads.macPrivateHeaderApi`,
  `consent.hostCookieFallback`, `logging.enabled`; `Builder::uid` and
  `Builder::host_label`.
- Harness round 2 in the contract: ow-electron's four host-to-guest messages
  (`consent`, `customTracking`, `eHashes`, `window-hidden`) and the guest
  visibility and focus signals (D.5); `setExternalPaymentUserId` reporting
  `<label>_sub_info` (E.2); the guest crash report and Kind 400024 shape
  (E.1, E.2); the settings window's shape, query and promise, and the hidden
  default-consent window `ow-cmp-default` (D.6.4); `<owadview>` `pageUrl`,
  `setPageUrl` and `sendCommand`, and the `pageurl` attribute (B.3);
  `render-process-gone`, navigation and console events on the element
  (B.3.5); `__settings__.adsOptimization` (B.1.1); OQ-38.
- `docs/PARITY.md`: round-2 results, round-3 list, a deviations table.

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
- Harness round 2 corrections: the startup consent window opens when the
  `cmp-eu-only` request completes, `isCMPRequired()` resolves at its page's
  load and has no timeout (D.6.1, D.6.2); the settings-window promise
  resolves on creation (A.2.2); `window_closed` is sent per visible period
  and the window name comes from the URL at first show, ignoring the `name`
  option (E.2); guest recovery has no cap (`ads.maxRecoveries` defaults to
  `null`) and load errors retry every 5 s on the main frame only (D.7);
  `getIsAdOptimizationEnabled()` defaults to `false` (D.6.6); package-manager
  rejections are asynchronous (H.1); `app_cuid` stays the computed uid when
  `overwolf.uid` is set (G.2); the 3 s ad wait is measured from the mount
  (D.6.5). The test-mode `unit` guard is now a documented deviation.

### Removed

- Simulated package backends and `packagesBackend` values `auto` and
  `simulated`; the `failed-to-initialize` reasons `unsupported-host`,
  `no-native-runtime` and `packages-disabled`.
- Configuration keys `analytics.hostFields`, `ads.experimentalElementApi`,
  `ads.exposeEmailHashesToGuest`, `consent.gateAdsOnConsent` and
  `consent.cmpRequired`.
- Configuration key `ads.legacyHostMessages` (the observed messages are
  always sent).
- The consent cookie write from the ad shim (the consent page writes them).

### Planned documentation

`docs/MIGRATION.md`, `docs/api/`,
`examples/packages-sample/CHANGES-FROM-UPSTREAM.md` and
`examples/packages-sample/.env.example` (see CONTRIBUTING.md, "Documentation
checklist").
