# ADR 0024: Recreate a macOS ad guest on reload instead of reloading in place

- Status: Accepted
- Date: 2026-10-08

## Context

Overwolf's ad page reloads itself at intervals, and the host reloads it after
a crash. On macOS a `WKWebView` that reloads in place keeps memory from
earlier pages. Over a 30-minute idle run the largest guest kept growing. A
Chromium guest in ow-electron stays flat.

A prototype replaced the reload with a fresh `WKWebView` that has the same
label, configuration, geometry, z-order, mute, transparency and
pass-through state. Memory stopped growing (the largest guest went from
150 MB to 133 MB in the forced-reload run). The ad page's requests and
events did not change.

A fresh webview loses the page's `sessionStorage`, which the ad page may
read across reloads. Unbounded recreation could also be used to churn
processes.

## Decision

On macOS, with `ads.recreateOnReload` (default `true`), the following
reloads destroy the guest's `WKWebView` and build a new one through the same
guest builder:

- page-requested reloads (`__overwolf__.reload()`, `__host:reload`);
- host reloads;
- crash recovery.

- **`sessionStorage` carry.** Before closing the old view, the plugin reads
  the top frame's `sessionStorage`, waiting at most 200 ms. It does so only
  when the frame's origin is `https://www.overwolf.com`. A one-shot prelude
  (`js/session-restore.js`) restores it in the new view before any page
  script runs. A snapshot over 2,000,000 UTF-16 units is never truncated:
  the guest reloads in place instead.
- **Rate guard.** A reload within `ads.recreateMinIntervalMs` (30 s) of the
  guest's last recreate, or beyond `ads.recreateMaxPerHour` (30), reloads
  in place. A reload is never dropped or delayed.
- **Generations.** Each native webview gets an opaque generation id. Page
  loads and messages from a closed generation are dropped.
- **Same outcome as a reload.** There is no `did-attach` and no 400025. The
  `customTracking` and `__overwolf__` data are the same, and the page sends
  the same requests.
- If recreation fails, the plugin logs one warning, closes the guest and
  the element gets `did-fail-load`.

Windows and Linux reload in place, as ow-electron does.

## Consequences

- macOS guests stay flat in memory across ad-library reloads. Their absolute
  level is WebKit's, about 1.5 to 1.8 times a Chromium guest, and PARITY
  records it as a platform difference.
- `sessionStorage` in iframes inside the ad page is lost on a recreate.
- The ad page cannot tell a recreate from a reload. The harness compares its
  request stream on both hosts.

## Alternatives considered

- **Reload in place and accept the growth.** Long sessions would end with a
  guest of several hundred MB. Rejected.
- **Recreate on a timer.** That adds loads ow-electron does not make, which
  is a wire difference. Rejected.
