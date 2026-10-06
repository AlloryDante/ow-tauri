# ADR 0005: Keep ow-electron's test/live ad semantics

- Status: Accepted (amended 2026-10-06)
- Date: 2026-10-06

## Context

ow-electron serves live ads unless the app is started with `--test-ad`
(documented; the official sample has a `start-ad` script for it). Test
inventory can also be forced from the ad page's own `localStorage`
(`owAdTestAd`). The reference implementation instead forced test ads unless
live mode was confirmed in its UI, which is safe for a demo but differs from
what developers expect.

Live ads also depend on Overwolf enabling the app's uid on its backend, and a
Tauri host needs Overwolf's approval to serve them in production (OQ-20).

The parity harness shows that test and live mode differ in exactly one input
the host controls: `__overwolf__.testAd`. The request shaping, the consent
flow and the analytics are identical; only the demand the ad page picks
changes [OBS].

## Decision

- ow-tauri follows ow-electron: live unless `--test-ad`, `OW_TAURI_TEST_AD=1`,
  `ads.testAd: true` or `Builder::test_ad(true)`.
- Test and live mode use the same wire behaviour (CONTRACT D.8); test mode
  only sets `testAd: true` in `__overwolf__`.
- As a safety guard, test mode also rewrites a non-empty `unit` to
  `"testAd"`, so a test build cannot request a live performance ad. This was
  not observed in ow-electron (R2-13) and is ow-tauri's own guard.
- Live mode does not touch the guest's `localStorage`.
- **Labs** (owner decision, 2026-10-06): agents and automated labs may load
  live ads to verify parity, under fixed rules: at most 10 live loads per
  run, each one logged; ads are never clicked and no input is sent to a
  guest; windows are never visible (hidden, or shown at alpha 0 when an ad
  must fill, with the dock icon hidden). Test ads remain the default for
  every lab run that does not need live demand.
- The example keeps a `start-ad` script, and the README and the example's
  scripts default to `--test-ad` for development.

## Consequences

- Developers get the behaviour they know from ow-electron.
- The labs can show that live demand fills on ow-tauri, which test ads alone
  cannot, without generating invalid traffic: no clicks, a small logged load
  count.
- Shipping live ads in production remains a deliberate step that needs
  Overwolf's approval for the app and host (OQ-20).
- A misconfigured development build can request live ads; the scripts'
  `--test-ad` default is the mitigation, as in ow-electron.

## Alternatives considered

- **Test unless confirmed live (the reference implementation's rule).**
  Safer, but diverges from ow-electron and needs a UI or config step every app
  would have to add. Rejected.
- **Test in debug builds automatically.** Convenient, but makes debug and
  release behave differently in a way ow-electron does not. Rejected; the
  example's scripts pass `--test-ad` instead.
- **Test ads only in every lab** (the original rule). It cannot show live
  fill, which is the point of parity for monetisation. Replaced by the
  capped, logged, no-click rule above.

## Amendments

- 2026-10-06, owner round 2: labs may load live ads under the rules above
  (previously: test ads only in every lab); test and live are documented as
  identical on the wire, as observed.
