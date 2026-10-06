# ADR 0013: Shape ad guest requests like ow-electron, per OS

- Status: Accepted
- Date: 2026-10-06

## Context

The parity harness shows that ow-electron shapes every request of an
`<owadview>` guest (CONTRACT D.8):

- the ad document `https://www.overwolf.com/monsdk/electron/latest/adview.html`
  goes out with `Referer: https://www.overwolf.com/<uid>` and
  `Origin: https://www.overwolf.com`;
- every subresource, from any frame and any initiator, gets
  `Origin: https://www.overwolf.com`, replacing any existing value;
- the ad library `owads.min.js` instead gets `x-ow-uid`, `x-ow-phase` and
  `x-ow-window`;
- guests run with web security off and insecure content allowed, with the
  app's user agent, in the default session (so they carry its cookies);
- the app's own request hooks cannot undo any of it.

The owner's decision (Q05) is to replicate exactly what ow-electron sends, on
macOS too. The three Tauri engines expose very different request APIs.

## Decision

- Request shaping is on by default and not an app hook (`ads.requestShaping`
  exists only to switch it off while debugging).
- Ad guests and the consent windows run in a separate **ads environment**
  (CONTRACT A.1.1): on Windows a WebView2 environment with its own user data
  folder and `--disable-web-security --allow-running-insecure-content`; on
  Linux per-webview settings; on macOS the default data store.
- Per OS:
  - **Windows (WebView2):** a `WebResourceRequested` handler on each guest
    sets the document's `Referer` and `Origin`, forces `Origin` on every other
    request, and appends the `x-ow-*` headers to `owads.min.js`. Full parity.
  - **macOS (WKWebView):** the first navigation is
    `WKWebView.load(URLRequest)` with `Referer` and `Origin`. Subresource
    `Origin`, the `x-ow-*` headers and turning web security off have no public
    API: a documented **gap**, reported to Overwolf (OQ-05). No private API by
    default; a prototype behind `ads.macPrivateHeaderApi` (off) may be
    evaluated and reported.
  - **Linux (WebKitGTK):** the first navigation is
    `webkit_web_view_load_request` with the two headers; subresource shaping
    needs a web-process extension (`send-request`) and is deferred.
- The lab checks in PARITY.md verify on the wire that each platform sends
  what this ADR claims, and compare macOS fill with ow-electron's live
  baseline.

## Consequences

- Windows, the platform with most Overwolf users, matches ow-electron on the
  wire.
- macOS ads may fill or attribute differently from ow-electron; the reference
  implementation filled test ads without any shaping, and the lab check
  measures the live difference. The uid, phase and window name still reach
  the ad server in the `owads.min.js` query string.
- Web security is off only in webviews that run no app code and hold one
  scoped command (ADR 0011); app windows keep the platform defaults.
- On Windows the ads environment has its own browser profile, so its cookies
  are separate from the app windows' cookies (ow-electron shares one
  session). Nothing in the app windows reads the `.overwolf.com` cookies, so
  no observable behaviour changes.

## Alternatives considered

- **No shaping (the original interim).** Simple and portable, but not what
  ow-electron sends; rejected by the owner's Q05 decision.
- **Proxy all guest traffic through Rust.** Would give identical shaping on
  every OS, but means terminating TLS for third-party ad partners inside the
  host. Rejected: a security and policy risk far larger than the gap.
- **Private WebKit API by default on macOS.** Closes the gap, but can break in
  any macOS release and raises App Store review questions. Kept as an
  off-by-default prototype only.
