# ADR 0019: Track app windows from native Tauri events and name them like ow-electron

- Status: Accepted
- Date: 2026-10-08

## Context

Overwolf receives window data in several places:

- the heartbeat with `hasVisibleWindow: true`;
- `<label>_window_closed` with `name`, `title` and `length`;
- the guest config fields `windowName` and `windowTitle`;
- the `x-ow-window` request header.

In ow-electron these come from `BrowserWindow` objects. The name is derived
from the page URL (`index.html` → `index`). The title is the page title.

The first ow-tauri design tracked only its own `bw-*` windows. In a plain
Tauri app every window is the app's own, and its pages load from
`tauri://localhost/` or `http(s)://tauri.localhost/`. Tauri drops
`index.html` from those URLs. Tauri also emits no shown, hidden or minimized
events, and `show()` on an unfocused window emits nothing at all.

## Decision

- **Every app window is tracked** by its Tauri label (`host/windows.rs`).
  Exceptions:
  - the plugin's consent windows (`ow-cmp*`);
  - windows whose label takes a reserved prefix without the plugin creating
    them (logged once, never tracked).

  Windows that match `analytics.excludeWindows` are tracked for their ads
  but not counted for analytics.
- **Visibility is polled.** One ticker polls every tracked window every
  250 ms, all in one main-thread hop. It parks only when no window is
  tracked (a tray-only app).
- **Name.** The naming webview is the one whose label equals the window
  label, or else the first non-reserved webview in the window. Its last
  finished page URL goes through ow-electron's derivation. For an app-origin
  URL whose path is empty or ends in `/`, the path is first read as
  `<path>index.html`, so `tauri://localhost/` gives `index`
  (`analytics/mod.rs` `window_analytics_name`). The name is fixed the first
  time the window is seen visible with a loaded page, as in ow-electron.
- **Title.** The title declared in `tauri.conf.json`, else the native title
  at registration. Tauri's placeholder `Tauri App` reports the product name
  instead.
- **`set_window_name`** overrides the name. ow-electron has no such call, so
  it is documented as a non-parity escape hatch and the examples never use
  it.

## Consequences

- Single-page apps that use history routing (`/route`) report `route` where
  a hash-routed ow-electron app reports `index`. MIGRATION says so.
- A window shown without focus is seen within one tick, so heartbeat and
  `window_closed` timings match ow-electron to within 250 ms.
- An idle app with a hidden tracked window still wakes four times a second.
  This is one main-thread hop per tick, with a measured CPU budget (PARITY).

## Alternatives considered

- **Native show and occlusion notifications.** These would let the ticker
  park while windows are hidden, but each OS needs its own subclassing.
  Deferred until a harness re-proof.
- **Require apps to name every window.** That moves an ow-electron
  derivation into app code and gets it wrong by default. Rejected.
