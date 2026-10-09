# `<owadview>` reference

`<owadview>` is Overwolf's ad element. In a Tauri app it has the same
attributes, members and DOM events as in ow-electron, so ow-electron ad HTML
and ad code move unchanged.

```ts
import 'tauri-plugin-overwolf-api/adview'; // once per page
```

```html
<div style="width: 400px; height: 300px">
  <owadview cid="main-mrec" slotsize="400x300"></owadview>
</div>
```

The import installs the element runtime in the page. Each element then gets
a native ad webview (an "ad guest", label `owad-<n>`) placed over its box. The
page's webview needs `overwolf:default` ([permissions.md](permissions.md)).

How to use each ad format (display, video, high impact, interstitial,
reward) is in [AD-FORMATS.md](../AD-FORMATS.md). The behaviour below is
specified in [CONTRACT.md](../CONTRACT.md), section B.3; the source is
`packages/api/src/adview/`.

<!-- image: docs/images/owadview/element-over-page -->

## Contents

- [Attributes](#attributes)
- [Properties and methods](#properties-and-methods)
- [Events](#events)
- [Lifecycle](#lifecycle)
- [Layout and visibility](#layout-and-visibility)
- [Performance (interstitial) elements](#performance-interstitial-elements)
- [Frameworks](#frameworks)
- [Not supported](#not-supported)
- [The `adview` object](#the-adview-object)

## Attributes

HTML stores attribute names in lower case. The runtime reads these seven:

| Attribute | Meaning | A change after mount |
|---|---|---|
| `cid` | Container id reported with the ad. Trimmed, at most 20 characters. | remounts the ad |
| `slotsize` | Requested inventory, `"WxH"`. Overwolf documents `400x300`, `400x600`, `300x250`, `160x600`, `728x90`, `970x90` and `400x60`. Other values pass through. | remounts the ad |
| `adstyle` | Style tokens, for example `"high-impact-ad;"` or `"rewarded-ad;"`. Passed to the ad page unchanged. | remounts the ad |
| `customtracking` | A JSON object as text. Invalid JSON clears it. | reaches the running ad page; no remount |
| `performance` | Present for a performance (interstitial) ad. See [below](#performance-interstitial-elements). | remounts the ad |
| `unit` | Ad unit override. | remounts the ad |
| `pageurl` | The ad page's `pageUrl` value. Read at mount. | applies at the next ad page load |

Any other attribute is ignored. `setAttribute('customTracking', ...)` works:
HTML stores it as `customtracking`.

## Properties and methods

Before the ad attaches, the element is a plain `HTMLElement`, as in
ow-electron. At attach it gets these members:

| Member | Behaviour |
|---|---|
| `cid`, `slotsize`, `unit`, `adstyle` | read and write the attribute |
| `performance` | reads and writes the `performance` attribute (boolean) |
| `pageUrl` | reads and writes `pageurl` (`""` when absent) |
| `customTracking` | reads and writes `customtracking` |
| `setAudioMuted(muted: boolean): void` | mutes or unmutes the ad. Ads start muted. |
| `reload(): void` | reloads the ad page |
| `setPageUrl(url: string): void` | sets `pageurl` and sends the new URL to the running ad page |
| `sendCommand(...args: unknown[]): void` | sends the arguments (as JSON) to the running ad page. Ignored before attach. |

The methods return nothing and never throw. The type of an attached element
is `OwAdViewElement`, exported by `tauri-plugin-overwolf-api/adview`;
`document.querySelector('owadview')` and `document.createElement('owadview')`
return it in TypeScript.

Electron's generic `<webview>` methods (`getURL`, `executeJavaScript`, ...)
are not provided. Overwolf does not document them for `<owadview>`, and they
would give page code control over remote ad content.

## Events

Events are plain DOM `Event`s dispatched on the element. They do not bubble
and cannot be cancelled. Their data is copied onto the event as own
properties, the way ow-electron copies it (`Object.assign(event, data)`):

```ts
ad.addEventListener('house_ad_action', (event) => {
  console.log((event as Event & { action?: string }).action);
});
```

An object payload gives its fields. A string payload (as
`performance_ad_error` sends) gives one property per character: `event[0]`
is its first character. `detail` stays empty.

### Ad page events

The runtime forwards every event the ad page sends, in order, with no list
of names. Names Overwolf documents or the parity lab observed:

| Event | Notes |
|---|---|
| `display_ad_loaded` | a display ad filled; usually arrives twice per fill, as in ow-electron |
| `impression` | |
| `player_loaded`, `video_ad_ready`, `play`, `pause`, `ended`, `complete` | video ads |
| `high-impact-ad-loaded`, `high-impact-ad-removed` | high-impact ads |
| `house_ad_action` / `house-ad-action` | house ads; carries `action` |
| `ad_clicked` / `ad-clicked` | a click inside the ad |
| `performance_ad_loaded`, `performance_ad_error`, `performance_ad_dismiss`, `performance_ad_clicked`, `performance_ad_video_complete`, `performance_ad_video_skipped` | performance ads |
| `shutdown` | a performance ad ended; the element leaves the document after it |

Overwolf also documents `performance_ad_no_fill`. No lab run has seen it: a
performance ad without fill sent only `shutdown`.

Four names come in two spellings. The runtime dispatches both, the received
spelling first:

| Received | Also dispatched |
|---|---|
| `ad_clicked` | `ad-clicked` |
| `ad-clicked` | `ad_clicked` |
| `house_ad_action` | `house-ad-action` |
| `house-ad-action` | `house_ad_action` |

### Host events

The plugin adds these lifecycle events:

| Event | When | Own properties |
|---|---|---|
| `did-attach` | the ad webview exists | none |
| `dom-ready` | the ad page's `DOMContentLoaded` | none |
| `did-finish-load` | the ad page finished loading | none |
| `did-fail-load` | a load failed | `errorCode`, `errorDescription`, `validatedURL`, `isMainFrame`, `frameProcessId`, `frameRoutingId` |
| `render-process-gone` | the ad page's process crashed; the plugin recovers it | `details` (`{ reason, exitCode }`) |
| `ad-clicked` | a click in the ad opened the system browser | `url` |
| `destroyed` | the ad of a moved element closed (see [Lifecycle](#lifecycle)) | none |

One click gives one `ad_clicked` / `ad-clicked` pair: the host `ad-clicked`
is skipped when the ad page sent a click spelling for that element in the
last second.

ow-electron also forwards Electron's other `<webview>` events
(`did-start-navigation`, `console-message`, `media-*`, ...). Tauri has no
equivalent for them, so they are not dispatched.

### Click-outs

A click inside the ad opens the system browser only when the operating
system reports a real user action on that ad (WebView2 user activation on
Windows, a native mouse or key event on the ad on macOS). A script in the ad
page cannot open a browser on its own. Each ad, and the app as a whole, may
open at most 20 pages per minute (`ads.guestLimits` in
[CONFIG.md](../CONFIG.md#adsguestlimits)).

## Lifecycle

| What the page does | What happens |
|---|---|
| inserts the element, with a non-empty box (or `performance`) | the ad mounts |
| resizes, scrolls or moves the box | the ad follows, once per animation frame |
| hides the box (`display: none`, a hidden ancestor, scrolled out) | the ad is told it is hidden; it is not destroyed |
| changes `cid`, `slotsize`, `adstyle`, `performance` or `unit` | the ad closes and mounts again |
| removes the element, or moves it to another parent | the ad closes for good |
| navigates or unloads the page | every ad of the page closes |

**Do not reuse elements.** An element that was attached and is then removed
or moved never attaches again, as in ow-electron. Create a new element. A
moved element gets `destroyed` once its ad has closed; an element removed
for good gets nothing.

Events that arrive before the mount completes are dispatched right after it.
Events after `destroyed` are dropped.

## Layout and visibility

- **Size the container, not the element.** The runtime's default style is
  `:where(owadview) { display: inline-flex; width: 100%; height: 100%; }`.
  `:where()` has zero specificity, so any rule of yours wins.
- **The ad paints above the page.** A native webview always covers the HTML
  under it. A menu or modal that must cover an ad has to hide the element
  (the runtime hides the ad when an ancestor is hidden).
- **CSS transforms and `clip-path`** on ancestors are not reflected in the
  ad's position.
- **Visible** means all of: the window is shown and not minimized; at least
  half of the box is inside the viewport (ow-electron's measured boundary);
  `checkVisibility()` passes, which includes `opacity` and `visibility`
  (checked every 500 ms); `document.visibilityState` is `visible`. Overwolf's
  policy asks that containers stay visible and that ads are not moved or made
  transparent.
- **Page zoom** is taken into account. On Windows the plugin compares
  `devicePixelRatio` with the window's scale factor; on macOS it compares
  the webview's width with `innerWidth`.
- **Ad backgrounds are transparent** by default, so an empty slot shows your
  container's background (`ads.transparentGuests` in
  [CONFIG.md](../CONFIG.md#ads)).
- **Closing a window**: the ads of a window that closes are told they are
  hidden first, as ow-electron does. A close that your app prevents leaves
  them as they were. Hiding the window (close to tray) sends the normal
  hidden message.

## Performance (interstitial) elements

```js
const ad = document.createElement('owadview');
ad.setAttribute('performance', '');
document.body.append(ad);
```

- The element's own box is ignored. The ad covers the window's content area
  and follows its size.
- One per window. A second `performance` element that would attach while
  one is mounted is removed from the document at once, with no event, as in
  ow-electron.
- Until its first `performance_ad_loaded` the ad lets clicks through to your
  page, so an empty interstitial never blocks the app.
- On `shutdown` the runtime dispatches the event, closes the ad, and removes
  the element in the next task. No `destroyed` follows.
- The ad page itself answers a window smaller than 500 x 500 with
  `performance_ad_error` and `shutdown`. Overwolf's guidance is a 1000 x 600
  minimum.

## Frameworks

- **React**: import `tauri-plugin-overwolf-api/adview` once before rendering,
  and `import type {} from 'tauri-plugin-overwolf-api/jsx'` for the JSX type.
  Attach listeners with a `ref` and `addEventListener`; React's `on*` props
  do not map underscore event names. StrictMode's mount, unmount and mount
  yields one ad. Example:
  [examples/quickstart-react](../../examples/quickstart-react).
- **Preact, Solid**: declare the element with the exported
  `OwAdViewAttributes` type (see the comment in
  `packages/api/src/jsx.ts`).
- **Several bundles in one page**: the runtime installs once per page,
  however many copies of the package the page loads.

## Not supported

- Elements inside shadow roots or iframes. The runtime watches the top
  document only; such an element stays inert.
- `-webkit-app-region` on the element. Use Tauri's `data-tauri-drag-region`
  for window dragging.
- Linux: the element mounts nothing; `adview_mount` rejects with
  `unsupported` (`getInfo().adsSupported` is `false`).
- A page that is not a local app page (Tauri's app origin, the dev server in
  a debug build, or an origin in `ads.allowedEmbedderOrigins`): the plugin
  refuses with `forbidden`.

## The `adview` object

`tauri-plugin-overwolf-api/adview` also exports `adview`, for tests and
unusual setups:

| Member | What it does |
|---|---|
| `adview.version` | the package version of the copy that installed the runtime |
| `adview.upgrade(el)` | registers an element the runtime could not observe |
| `adview.elements()` | the tracked `<owadview>` elements, in discovery order |

The runtime stays inert outside a Tauri webview and in the plugin's own
webviews (`owad-*`, `ow-cmp*`).
