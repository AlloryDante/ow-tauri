# ADR 0015: Run ow-electron's hidden startup consent window on every launch

- Status: Accepted
- Date: 2026-10-06

## Context

The original draft wrote the consent cookies from the ad guest's shim and
opened a consent window only when the app called `openCMPWindow`. The parity
harness shows something different (CONTRACT D.6):

- on **every launch**, ow-electron opens a hidden 1 x 32 window on
  `https://content.overwolf.com/monsdk/electron/latest/cmp/22.3.27/ow-cmp-v2.html`
  with the query `unifiedcmp, muid, uid, muidv2, oweVersion, appVersion`;
- on the first launch the page generates a default Full consent and reports
  it with its own analytics; on later launches it reuses the stored consent;
- the page stores consent through native `window.cmp` / `window.privacy`
  globals, writes `euconsent-v2` and `acconsent` on `.overwolf.com` itself
  (365 days), and closes itself about a second after loading;
- the cookies exist before the first ad document is requested, and the ad
  page reads consent from them (`__overwolf__.consent` is always `""`).

The owner's decision: behave exactly like ow-electron's consent flow.

## Decision

- The plugin opens the startup consent window `ow-cmp-startup` at
  `RunEvent::Ready` on every launch: never shown, not focusable, in the ads
  data store, with the composed user agent and the observed URL and query.
- The consent shim provides the same globals; Rust stores what the page saves
  in `ow-electron.json` with ow-electron's encoding (ADR 0007).
- Consent cookies are written by the page only. Rust writes them, with the
  same attributes, only when they are missing after the window has closed
  (`consent.hostCookieFallback: "auto"`), for platforms whose cookie policy
  blocks the page.
- Each ad guest's first navigation waits until the startup window has closed
  or 3 s have passed, which makes ow-electron's observed ordering
  deterministic. `consent.gateAdsOnConsent` and the ad shim's cookie write are
  removed.
- `isCMPRequired()` follows the observed source (`cmp-eu-only`), and the
  window opens whether or not it has resolved, as in ow-electron.

## Consequences

- The consent page sees, stores and reports exactly what it does under
  ow-electron, including its first-launch Full consent and its analytics.
- Every launch loads one Overwolf page in a hidden webview, as ow-electron
  does; it costs about a second of background work.
- The first ad may wait up to 3 s for the consent window on a slow network;
  in ow-electron the same ordering happened by timing.
- If WebKit's tracking prevention blocks the page's cookie write, the
  fallback keeps ads consented; the lab check records whether it fired.

## Alternatives considered

- **Write cookies from the ad shim (the original draft).** Not what
  ow-electron does; the cookie values, expiry and timing would differ, and the
  consent page's own logic and analytics would never run. Rejected.
- **Open the consent page only when `isCMPRequired()` is true.** Plausible,
  but not observed; ow-electron opened it before the answer arrived. Kept as
  an open item (R2-7).
