# ADR 0003: Implement `<owadview>` with a MutationObserver and native child webviews

- Status: Accepted (amended 2026-10-08)
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
  (at attach: own attribute-backed properties, and the methods on an inserted
  prototype, as on ow-electron's element). It tracks the element's rect (`ResizeObserver`, scroll,
  resize) and visibility (`IntersectionObserver` at 0.5, a 500 ms
  `checkVisibility` poll, document visibility).
- The plugin creates one native child webview per mounted element, labelled
  `owad-<embedder>-<n>`, positioned over the element, with `adview-host.js`
  injected as an initialization script.
- A zero-specificity default style (`:where(owadview) { display: inline-flex;
  width: 100%; height: 100% }`, a 0 x 0 block for `performance`) makes the
  element fill its container, as the sample expects; `performance` elements
  cover the embedder viewport.
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
- What ow-electron gets from the guest being part of the page has to be
  rebuilt natively per platform: transparency, the interstitial on top of
  other ads, and input passing through an interstitial that has not loaded.
  On Linux, where tauri-runtime-wry packs child webviews into a `GtkBox`,
  guests never overlap, so those three do not apply (a known gap).
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
- 2026-10-06, harness round 2: ow-electron upgrades the element after attach
  (an `OwAdViewElement` prototype with Electron's `<webview>` methods,
  `setPageUrl`, `sendCommand`, and own properties such as `pageUrl`). ow-tauri
  keeps instance members and adds `pageUrl`, `setPageUrl` and `sendCommand`;
  the `pageurl` attribute now feeds the guest's `pageUrl`. Electron's generic
  `<webview>` methods are not provided (undocumented for `<owadview>`, and they
  would hand app code control of remote content). The element also receives
  `render-process-gone`, navigation and console events. The host passes the
  guest ow-electron's four message types and its visibility and focus
  signals; guests are recovered without a cap (CONTRACT B.3, D.5, D.7).
- 2026-10-07, ad formats (wave 3e): the element follows ow-electron's
  lifecycle (an element removed or moved after attach is dead; a plain
  `destroyed` reaches it only when it is back in the document; a
  performance element leaves the document after `shutdown`, and a second one
  is removed at once), its DOM (`inline-flex` default; a performance element
  gets an overlay `div` and `pointer-events` instead of a shadow root),
  `Object.assign` payload copies, and the `sendCommand` / `setPageUrl`
  messages. Guests are transparent from creation (`ads.transparentGuests`),
  the newest performance guest is raised above the others after each mount,
  and it passes input through until its first `performance_ad_loaded`
  (CONTRACT B.3.4). The host now passes six message types (D.5).
- 2026-10-08, Tauri-native pivot ([ADR 0017](0017-tauri-native-pivot.md)): the app imports `tauri-plugin-overwolf-api/adview`, which registers the element; nothing is injected. Any app webview the capability allows can embed an ad. Guests are labelled `owad-<n>`, skipping labels already taken. Events reach the element over one Tauri `Channel` per mount. `unstable` is enabled only for Windows and macOS ([ADR 0023](0023-unstable-and-macos-input.md)); Linux reports `unsupported`. The element no longer receives `did-start-navigation`, `load-commit` or `console-message` (CONTRACT B.3.5).
