# ADR 0015: Run ow-electron's hidden startup consent window on every launch

- Status: Accepted (amended 2026-10-08)
- Date: 2026-10-06

## Context

The original draft wrote the consent cookies from the ad guest's shim and
opened a consent window only when the app called `openCMPWindow`. The parity
harness shows something different (CONTRACT D.6):

- on **every launch**, once its `cmp-eu-only` feature request has completed,
  ow-electron opens a hidden 1 x 32 window on
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

- The plugin starts the `cmp-eu-only` request at `main_ready` (first written
  as `RunEvent::Ready`; moved so it leaves with the startup analytics, CONTRACT
  E.2) and opens
  the startup consent window `ow-cmp-startup` as soon as it completes, on
  every launch: never shown, not focusable, in the ads data store, with the
  composed user agent and the observed URL and query.
- The consent shim provides the same globals; Rust stores what the page saves
  in `ow-electron.json` with ow-electron's encoding (ADR 0007).
- Consent cookies are written by the page only. Rust writes them, with the
  same attributes, only when they are missing after the window has closed
  (`consent.hostCookieFallback: "auto"`), for platforms whose cookie policy
  blocks the page.
- Each ad guest's first navigation waits until the startup window has closed
  or 3 s have passed since the guest was mounted, which makes ow-electron's
  observed ordering deterministic. `consent.gateAdsOnConsent` and the ad
  shim's cookie write are removed.
- `isCMPRequired()` follows the observed source (`cmp-eu-only`): one request
  per launch, no client timeout, resolved when the startup page has loaded,
  `true` for every response observed; a `{}` body disables the cache and
  opens a new window per call, as in ow-electron.
- Each consent save also sends the guests ow-electron's `consent` messages
  (CONTRACT D.5).

## Consequences

- The consent page sees, stores and reports exactly what it does under
  ow-electron, including its first-launch Full consent and its analytics.
- Every launch loads one Overwolf page in a hidden webview, as ow-electron
  does; it costs about a second of background work.
- The first ad may wait up to 3 s for the consent window on a slow network;
  in ow-electron the same ordering happened by timing.
- A hung feature request delays the consent window and `isCMPRequired()` as
  long as it hangs, as in ow-electron; the ads are bounded by the 3 s rule.
- If WebKit's tracking prevention blocks the page's cookie write, the
  fallback keeps ads consented; the lab check records whether it fired.

## Alternatives considered

- **Write cookies from the ad shim (the original draft).** Not what
  ow-electron does; the cookie values, expiry and timing would differ, and the
  consent page's own logic and analytics would never run. Rejected.
- **Open the consent page only when `isCMPRequired()` is true.** Plausible,
  but not observable: every response served to ow-electron resolved `true`.
  Asked of Overwolf (OQ-06, OQ-07).

## Amendments

- 2026-10-06, harness round 2: the window opens after the `cmp-eu-only` response,
  not in parallel with it; `isCMPRequired()` resolves at the page's load and
  has no timeout; the `{}` body case; the 3 s bound is measured from the guest
  mount; consent saves send `consent` messages. The settings window's first
  call also opens a hidden default-consent window (`ow-cmp-default`) that
  writes a fresh default consent, copied from ow-electron and asked of
  Overwolf (OQ-38).
- 2026-10-08, Tauri-native pivot ([ADR 0018](0018-lifecycle-ready-exit.md)): the round starts at `RunEvent::Ready`, with the launch burst. `cmp-eu-only` now has a client timeout, `consent.euOnlyTimeoutMs` (60 s). A timeout counts as a failed request: `isCMPRequired()` resolves `true` and the startup window still opens. This is a listed deviation (PARITY). A JavaScript `cmpURL` must match `consent.allowedCmpOrigins`. The last-window rule of this amendment is replaced by the next one.
- 2026-10-09, harness observation: consent windows never keep the app alive, as in ow-electron, which quits at `window-all-closed`. When the last app window closes, a startup window whose page has saved closes, an unsaved one is discarded at once (its consent is lost and nothing is written), and a round whose window does not exist yet is skipped. Tauri's own exit flow then runs; the plugin never prevents or forces the exit (CONTRACT D.6.1).
