# ADR 0005: Keep ow-electron's test/live ad semantics

- Status: Accepted
- Date: 2026-10-06

## Context

ow-electron serves live ads unless the app is started with `--test-ad`
(documented; the official sample has a `start-ad` script for it). Test
inventory can also be forced from the ad page's own `localStorage`
(`owAdTestAd`). The reference implementation instead forced test ads unless
live mode was confirmed in its UI, which is safe for a demo but differs from
what developers expect.

Live ads also depend on Overwolf enabling the app's uid on its backend, and a
Tauri host needs Overwolf's approval to serve them.

## Decision

- ow-tauri follows ow-electron: live unless `--test-ad`, `OW_TAURI_TEST_AD=1`,
  `ads.testAd: true` or `Builder::test_ad(true)`.
- Test mode sets `testAd: true` in `__overwolf__` and rewrites a non-empty
  `unit` to `"testAd"` so a performance ad cannot turn live.
- Live mode does not touch the guest's `localStorage`.
- The example keeps a `start-ad` script, and every automated lab runs in test
  mode only; tests assert that a live configuration is never generated.

## Consequences

- Developers get the behaviour they know from ow-electron.
- Shipping live ads remains a deliberate step that needs Overwolf's approval
  for the app and host (OQ-20).
- A misconfigured development build can request live ads; the README and the
  example's scripts default to `--test-ad` for development.

## Alternatives considered

- **Test unless confirmed live (the reference implementation's rule).**
  Safer, but diverges from ow-electron and needs a UI or config step every app
  would have to add. Rejected for the plugin; labs keep the stricter rule.
- **Test in debug builds automatically.** Convenient, but makes debug and
  release behave differently in a way ow-electron does not. Rejected; the
  example's scripts pass `--test-ad` instead.
