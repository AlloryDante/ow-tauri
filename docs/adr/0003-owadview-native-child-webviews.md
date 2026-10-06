# ADR 0003: Implement `<owadview>` with a MutationObserver and native child webviews

- Status: Accepted
- Date: 2026-10-06

## Context

ow-electron apps show ads by creating an `owadview` element
(`document.createElement('owadview')`), setting attributes such as `cid`,
`slotsize`, `adstyle` and `customTracking`, and listening for DOM events such as
`impression` and `high-impact-ad-loaded`. The ad itself is Overwolf's page
`https://www.overwolf.com/monsdk/electron/latest/adview.html`, which reads its
configuration from `window.__overwolf__`.

Two constraints shape the port:

1. A custom element name must contain a hyphen, so `customElements.define('owadview', ...)`
   throws in every engine.
2. Tauri webviews cannot embed a cross-origin page with host-injected scripts
   inside the DOM (no `<webview>` tag). An `<iframe>` would not receive
   `__overwolf__` and would be subject to the app's CSP and third-party cookie
   rules.

The reference implementation showed that a native child webview per slot
(Tauri `Window::add_child`, behind the `unstable` feature) loads the ad page,
renders test creatives and reports events.

## Decision

- `ow-tauri/renderer` wraps `document.createElement` and watches the document
  with a `MutationObserver`, and upgrades each `OWADVIEW` element in place
  (properties and methods defined on the instance). It tracks the element's rect (`ResizeObserver`, scroll,
  resize) and visibility (`IntersectionObserver` at 0.5, a 500 ms
  `checkVisibility` poll, document visibility).
- The plugin creates one native child webview per mounted element, labelled
  `owad-<embedder>-<n>`, positioned over the element, with `adview-host.js`
  injected as an initialization script.
- A zero-specificity default style (`:where(owadview) { display: block;
  width: 100%; height: 100% }`) makes the element fill its container, as the
  sample expects; `performance` elements cover the embedder viewport.
- Guest events come back as `adview-event` host messages and are dispatched
  on the element as plain non-bubbling `Event`s with the data as own
  properties, as ow-electron does (CONTRACT B.3.5), in both spellings where the
  ecosystem uses two (`ad-clicked` / `ad_clicked`, `house_ad_action` /
  `house-ad-action`).
- The plugin enables Tauri's `unstable` feature itself (`add_child` needs it
  on every platform); Cargo feature unification turns it on for the app.

## Consequences

- App ad code runs unchanged, including the sample's high-impact zone logic.
- Native webviews paint above HTML. Anything that must overlap an ad has to
  hide the element; hidden ancestors hide the guest automatically.
- CSS transforms and clipping on ancestors are not reflected.
- Geometry updates are asynchronous (one IPC hop per animation frame at most),
  so very fast scrolling can show the ad a frame late.
- `unstable` APIs may change in a Tauri 2.x minor release. The workspace
  allows the newest 2.x, so a scheduled CI job builds against it to catch
  breakage early; the supported range is documented in `Cargo.toml`.
- Guest mute on macOS uses WebKit's private `_setPageMuted:` selector, guarded
  by `respondsToSelector:`. It can disappear in a macOS release and may draw
  App Store review questions; without it guests stay unmuted on macOS.

## Alternatives considered

- **A hyphenated custom element (`ow-adview`).** Breaks every existing app.
  Rejected; the MutationObserver keeps the original tag name.
- **`<iframe>`.** No host-injected `__overwolf__`, cross-site cookie and CSP
  problems. Rejected.
- **A separate borderless window per slot glued to the app window.** Avoids
  `unstable`, but breaks z-order, focus, dragging, multi-monitor DPI and
  occlusion. Rejected.
- **The reference implementation's explicit layout call (`ad_layout` with a
  list of rects).** Works, but every app would have to replace its
  `<owadview>` code. Kept only as the internal mechanism.

## Amendments

- 2026-10-06, contract review: default element style; the plugin (not each
  app) enables `unstable`; the `unstable` and macOS private-selector risks.
- 2026-10-06, parity revision: events are plain `Event` objects with own
  properties instead of `CustomEvent`s; the element gets ow-electron's open
  shadow root; guests run in the ads environment with ow-electron's request
  shaping ([ADR 0013](0013-request-shaping-per-os.md)).
