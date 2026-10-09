# ADR 0020: Let only native user activation open the system browser from an ad

- Status: Accepted
- Date: 2026-10-08
- Amends: [ADR 0011](0011-remote-guest-ipc.md)

## Context

An ad click usually opens the advertiser's page. It arrives in one of two
ways: as a popup (`window.open`, `target=_blank`) or as a top-level
navigation away from Overwolf's ad page. The host then opens the URL in the
system browser.

ADR 0011 let the guest shim report a click with `__host:gesture`. Any script
in the ad page, including third-party creatives, can send that message. A
malicious creative could open browser tabs at will, up to the rate limit.

## Decision

- `__host:gesture` is still accepted from the shim, but it is never an
  authority. A guest popup or off-Overwolf navigation opens the system
  browser only when the OS reports that the user acted on that guest
  (`platform/gesture.rs`):
  - **Windows**: WebView2's `IsUserInitiated` on new-window requests and on
    top-level navigations. For a script navigation that follows a click
    straight away, the plugin also checks how recently native input reached
    the guest.
  - **macOS**: one `NSEvent` local monitor. A left mouse-down arms guest G
    only if all of these hold:
    - the window's content view hit-tests the event to G's `WKWebView`;
    - G is shown;
    - G does not pass input through;
    - the window is key.

    A Return, Space or keypad Enter key-down arms G only when G's webview is
    the first responder. A script cannot create an `NSEvent`.
  - **Linux**: there are no ads.
- Arming opens an activation window of `guestLimits.activationWindowMs`
  (5000 ms, Chromium's transient activation length). The first open
  consumes it.
- Opens are capped twice: `guestLimits.externalOpensPerMinute` (20) per guest
  and `guestLimits.externalOpensPerMinuteApp` (20) per app. The app budget
  survives guest recreation.
- Each open dispatches `ad-clicked` on the element (CONTRACT B.3.5). No
  setting turns JS-reported gestures back on.

## Consequences

- A creative cannot open the browser without a real click or key press on
  its own guest.
- Clicks that ow-electron would honour still open, including script
  navigations that follow a click straight away. The harness's
  `gesture-timing` scenario compares both hosts, and PARITY lists any
  difference.
- The macOS monitor sees every event in the process. It only reads the
  event's location and the key code, and it never consumes an event.

## Alternatives considered

- **Keep `__host:gesture` with a shorter window.** It is still forgeable.
  Rejected.
- **Read `WKNavigationAction.navigationType` on macOS.** wry owns the
  navigation delegate, so the plugin cannot read it.
- **Block all click-outs.** Ads would lose clicks that ow-electron delivers,
  which is a wire difference for advertisers. Rejected.
