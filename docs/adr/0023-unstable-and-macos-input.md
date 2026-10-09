# ADR 0023: Use Tauri's `unstable` child webviews, and repair macOS key input

- Status: Accepted
- Date: 2026-10-08

## Context

An ad is a native webview placed over the app's page, the way ow-electron
places a child view. Tauri offers child webviews (`Window::add_child`) only
behind its `unstable` feature, and that feature switches every webview in
the app to wry's child mode.

A spike on Tauri 2.12.1 with a text-input page beside an ad guest found:

- **macOS, tauri-apps/tauri#10194**: arrow keys insert U+001C and U+001D in
  text fields, with or without a guest.
- **macOS**: a new window's page gets no keys until the first click.
- **Windows**: a child built with `focused(true)` takes the keyboard while
  the user types, and the user's keys reach the ad page.
- **Both**: `get_webview_window` returns `None` for a window that hosts a
  guest.
- **Linux**: `unstable` rebuilds every window as a child-webview window.

Windows is otherwise unaffected.

## Decision

- `unstable` is enabled only for Windows and macOS targets, through the
  `tauri-plugin-overwolf-unstable` shim crate
  ([ADR 0021](0021-package-split.md)). Linux builds stay on stable Tauri
  APIs, and ads report `unsupported` there.
- **Guests never take focus.** The guest builder sets `focused(false)`, and
  a unit test pins it.
- **macOS responder splice** (`platform/input.rs`). For every webview, right
  after it joins its window, the plugin inserts one small `NSResponder`
  between the `WKWebView` and its parent view. Its `keyDown:` only offers
  the event to the main menu, as wry's stable-mode parent view does. It is
  per webview and idempotent, with no class swizzling.
- **macOS focus at open.** App webviews (never guests or consent webviews)
  get `set_focus()` when their window is created and is key.
- `Builder::macos_key_fix(false)` turns the splice and the focus off for
  apps that ship their own fix.
- Plugin code looks up windows with `get_window` and `get_webview`, never
  `get_webview_window`.
- The root cause is reported upstream. The splice is removed once a fixed
  wry ships.

## Consequences

- The spike's 53 macOS key cases match a stable-mode build, and its 59
  Windows cases stay green. CI and a weekly job re-run them against the
  newest Tauri 2.x.
- Tauri has no hook for `Webview::reparent`, so a reparented app webview
  loses the splice. This is a documented limit.
- Real IME (CJK) input and macOS older than 14 are not covered by the lab.
- Apps that call `get_webview_window` on a window hosting an ad get `None`.
  `ow-tauri doctor` flags these calls.

## Alternatives considered

- **A transparent top-level window per ad.** This works without `unstable`,
  but the ad no longer moves, clips and stacks with its page as an
  ow-electron child view does. Rejected.
- **Swizzle `WKWebView` key handling.** It changes every webview in the
  process, including ones the plugin does not own. Rejected.
- **Wait for upstream.** Text input would be broken on macOS until then.
  Rejected.
