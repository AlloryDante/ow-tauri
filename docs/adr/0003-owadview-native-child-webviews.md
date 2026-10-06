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

- `ow-tauri/renderer` watches the document with a `MutationObserver` and
  upgrades each `OWADVIEW` element in place (properties and methods defined on
  the instance). It tracks the element's rect (`ResizeObserver`, scroll,
  resize) and visibility (`IntersectionObserver` at 0.5, a 500 ms
  `checkVisibility` poll, document visibility).
- The plugin creates one native child webview per mounted element, labelled
  `owad-<embedder>-<n>`, positioned over the element, with `adview-host.js`
  injected as an initialization script.
- Guest events come back as `overwolf://adview-event` and are dispatched on
  the element as non-bubbling `CustomEvent`s, in both spellings where the
  ecosystem uses two (`ad-clicked` / `ad_clicked`, `house_ad_action` /
  `house-ad-action`).
- The plugin requires Tauri's `unstable` feature and documents it.

## Consequences

- App ad code runs unchanged, including the sample's high-impact zone logic.
- Native webviews paint above HTML. Anything that must overlap an ad has to
  hide the element; hidden ancestors hide the guest automatically.
- CSS transforms and clipping on ancestors are not reflected.
- Geometry updates are asynchronous (one IPC hop per animation frame at most),
  so very fast scrolling can show the ad a frame late.
- Every consuming app enables `tauri/unstable`.

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
