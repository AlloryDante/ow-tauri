# ADR 0011: Give each remote guest one scoped, rate-limited command

- Status: Accepted (amended 2026-10-08)
- Date: 2026-10-06

## Context

The architecture rule is that remote content gets no IPC. Overwolf's ad page
and consent page still have to talk to the host: the ad page reports events
through `window.__overwolf__.triggerEvent` and asks for mute and reload; the
consent page saves consent and closes its window. ow-tauri injects a small
shim into each guest that turns those calls into host messages.

The shim cannot hide its transport. The ad page's main frame runs third-party
ad scripts in the same JavaScript world, and they can call any command the
guest webview is allowed to call, with any arguments, as often as they like.

## Decision

- Each guest class gets exactly one command: `adview_event` for ad guests,
  `cmp_event` for the consent windows. Nothing else, no events, no core
  permissions.
- The capability that grants it is scoped to the page's own path,
  `https://www.overwolf.com/monsdk/electron/*` and
  `https://content.overwolf.com/monsdk/electron/*`, and to the guest webview
  labels (`owad-*`, `ow-cmp-startup`, `ow-cmp-default`, `ow-cmp`).
- Every value a guest sends is untrusted input: names are restricted to
  1 to 64 characters of `[A-Za-z0-9_:.-]`, data is capped at 16 KiB, and the
  caller's label (not a payload field) decides which element an event belongs
  to.
- Per-guest token buckets limit events per second, bytes per second and
  external browser opens per minute. One reported gesture allows one external
  open; on Windows popups also need WebView2's user-initiated flag. Only
  `http` and `https` URLs without credentials are opened.
- Consent strings are printable ASCII and at most 16 KiB; `cmpURL` is limited
  to the consent page's own path, so the scoped command always works there.

## Consequences

- The deviation from "no IPC" is one command per guest, documented, scoped and
  bounded; SECURITY.md describes the threat model.
- A malicious creative can forge events for its own slot (for example extra
  `impression` events in the app's logs) and can open at most a handful of
  `http(s)` pages per minute after a gesture. It cannot reach the IPC router,
  other slots, other webviews or any file or OS API.
- A flooding guest is dropped, then reloaded, then closed.

## Alternatives considered

- **No IPC; poll the guest with `eval` from Rust.** Tauri cannot return values
  from `eval`, so the guest would still need a way to send data. Rejected.
- **A custom URI scheme handler outside the ACL.** Equally reachable by page
  scripts and harder to audit than a single scoped command. Rejected.
- **Trust the shim's closure.** Page scripts can call the same command
  directly, so a closure protects only the shim's own messages. Rejected as a
  security boundary.

## Amendments

- 2026-10-06, parity revision: the hidden startup consent window
  `ow-cmp-startup` gets the same `cmp_event` capability as `ow-cmp`
  ([ADR 0015](0015-startup-consent-window.md)).
- 2026-10-06, harness round 2: the hidden default-consent window
  `ow-cmp-default` (opened by the first settings-window call, CONTRACT D.6.4)
  gets the same capability.
- 2026-10-08, [ADR 0020](0020-native-gesture-authority.md): a reported `__host:gesture` no longer allows an open. Only native user activation does, with a per-guest and a per-app cap. A JavaScript `cmpURL` must match `consent.allowedCmpOrigins`. The runtime capabilities name webviews (`owad-*` and `ow-cmp*`), never windows.
