# ow-electron parity harness

A small ow-electron app plus scripts that watch what `@overwolf/ow-electron` does as a black box. ow-tauri uses it to match ow-electron's ad, consent, analytics and identity behaviour on the wire. The harness only observes: it records requests, files, cookies and the values ow-electron exposes. It never changes ow-electron.

It is not part of the npm workspace and nothing in ow-tauri imports it.

## What it records

Each run writes `captures/<run-id>/`. That folder is git-ignored and holds:

| File                                                         | Contents                                                                                                                                                                                                                                                                                                                                             |
| ------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `netlog.json`, `netlog-requests.json`                        | Chromium net log (`--log-net-log`, capture mode `Everything`) and one parsed record per request. A record has the URL, method, initiator, wire headers in order, cookies sent and stored, upload body (HTTP/1.1 and HTTP/2), status, response headers and body. It covers every process, including requests ow-electron makes from the main process. |
| `cdp-network.jsonl`, `cdp-bodies.jsonl`                      | DevTools protocol network events for each webContents, including the `<owadview>` guests. Response bodies for overwolf.com documents and XHR.                                                                                                                                                                                                        |
| `main-js-net.jsonl`                                          | Calls made from main-process JavaScript to `net.request`, `net.fetch`, `fetch`, `http` and `https`.                                                                                                                                                                                                                                                  |
| `webrequest.jsonl`                                           | `session.webRequest` events. Only written with `--webrequest`.                                                                                                                                                                                                                                                                                       |
| `overwolf.json`                                              | `app.overwolf` members over time, and the results of `isCMPRequired()` and friends.                                                                                                                                                                                                                                                                  |
| `packages.jsonl`                                             | `app.overwolf.packages` events.                                                                                                                                                                                                                                                                                                                      |
| `guest-<n>-<phase>.json`                                     | Each ad guest's `window.__overwolf__` (data fields, plus which members are native functions), `document.referrer`, UA, cookies and localStorage.                                                                                                                                                                                                     |
| `guest-messages.jsonl`, `page-events.jsonl`, `console.jsonl` | postMessages seen in guests, `<owadview>` element events in the host page, and console output from remote pages.                                                                                                                                                                                                                                     |
| `cmp-pages.jsonl`, `cookies-*.json`, `cookie-changes.jsonl`  | The consent page's native API surface, plus cookie jars at startup and at the end, and every cookie change with its cause.                                                                                                                                                                                                                           |
| `files/before`, `files/after`                                | The ow-electron state folder (`<appData>/ow-electron/<uid>`) and the app's userData, hashed and copied. `meta.json` has the diff.                                                                                                                                                                                                                    |
| `live-loads.jsonl`                                           | Every live ad load, counted against the cap.                                                                                                                                                                                                                                                                                                         |
| `ipc.jsonl`                                                  | Messages between the host and every page: `host->page` (`send`, internal sends, `postMessage`, `executeJavaScript`, `reload`, `loadURL`, `setAudioMuted`, ...) and `page->host` (`-ipc-message` / `-ipc-invoke` on the session, with the reply). The harness's own probes are left out.                                                            |
| `wc-events.jsonl`                                            | Other webContents events of the ad guests (navigation, crashes, load failures).                                                                                                                                                                                                                                                                      |
| `actions.jsonl`, `ticks.jsonl`                               | The scenario's timed actions with their results, and a periodic heartbeat from the harness.                                                                                                                                                                                                                                                          |
| `introspect.jsonl`, `listeners.jsonl`                        | Own member names of ow-electron objects and the IPC listener names on each webContents (names only).                                                                                                                                                                                                                                                 |
| `features.jsonl`                                             | Requests answered by the local feature-flag stand-in (`--features`), with headers in order.                                                                                                                                                                                                                                                          |
| `window-monitor.jsonl`                                       | macOS: every window the app owns as the window server sees it (alpha, on-screen, bounds), and an `everVisible` verdict at the end.                                                                                                                                                                                                                   |
| `emitter-trace.jsonl`                                        | Every `EventEmitter.emit` in the main process. Only with `emitterTrace` in a scenario config; large.                                                                                                                                                                                                                                                  |
| `screen-*.png`, `app.pid`                                    | Screenshots of the main display (only with `--screencapture`) and the app's pid.                                                                                                                                                                                                                                                                     |
| `report.md`, `report.json`                                   | Summary from `analyze.mjs`.                                                                                                                                                                                                                                                                                                                          |

## Safety

- **No visible windows.** The app hides its dock icon first. `browser-window-created` and guards on every `BrowserWindow` show, focus and fullscreen method keep each window hidden (`--present hidden`) or at opacity 0, ignoring the mouse and not focusable (`--present transparent`). Every call to those methods is logged.
- **Windows are pinned invisible before they exist on screen.** `browser-window-created` runs before the constructor applies its options (the `cmp` scenario calibrates this first and drops its window actions if it fails). The handler sets opacity 0, ignores the mouse and makes the window not focusable. Later calls to `setOpacity`, `setIgnoreMouseEvents` and `setFocusable` are pinned to those values and logged as `pinned-call`. This is what makes it safe to call `openAdPrivacySettingsWindow()` and `openCMPWindow()`, which show their window from JavaScript.
- **Proof.** `--window-monitor` (macOS) polls the window server for the app's windows and records `everVisible`. Treat a run with `everVisible: true` as a failure. Under `taskpolicy -b` it samples about every 190 ms.
- **No clicks.** The harness never sends input to a page.
- **Live ads are opt-in and capped.** `--mode live` needs `--live-ok`. The run removes every `<owadview>` once `--max-live-loads` loads have happened (default 10), and every live load is logged in `live-loads.jsonl`.
- **Offline probes.** `uid-matrix.mjs` and the muid experiment launch with `--proxy-server=127.0.0.1:9`, so throwaway app identities send no analytics.
- **Isolated home.** By default each run gets a fresh home: `CFFIXED_USER_HOME` on macOS, `HOME` and `XDG_CONFIG_HOME` on Linux. The real profile, consent and cookies are not touched. Windows has no isolation, so use `--home real` there knowingly. `--use-mock-keychain` keeps Chromium out of the login keychain.
- **Quiet machine.** Launches wait, for at most 10 minutes, until the 1-minute load average is below 8.
- **Raw machine identifiers are never printed.** `muid-probe.mjs` reports only which derivation matched.

- **Screenshots are opt-in.** `screencapture` actions do nothing without `--screencapture`. On recent macOS the first capture from a terminal can raise a system screen-recording prompt. Do not answer it from an automated run.

## Setup

```sh
cd tools/parity-harness
npm install --workspaces=false
# Install scripts may be blocked (ignore-scripts). If so, fetch the binary yourself:
node node_modules/@overwolf/ow-electron/install.js
```

Keep `@overwolf/ow-electron` and `@overwolf/ow-cli` on their `latest` dist-tag. Check with `npm view @overwolf/ow-electron dist-tags`.

### App identity

The uid that ow-electron derives depends on `package.json` `author.name` and `productName`. By default the harness runs as a neutral example app ("Example Studio" / "Parity Harness").

To watch a registered app's real ad setup, copy `local.identity.example.json` to `local.identity.json`, which is git-ignored, and fill it in:

```json
{ "name": "my-app", "productName": "My App", "version": "1.0.0", "author": { "name": "My Studio" } }
```

You can also pass `--identity <file>` to any run.

## Usage

```sh
# Test ads, sample slot sizes, 90 s, window shown at opacity 0 so ads fill.
node run.mjs --mode test --present transparent --duration 90

# One slot size, never shown (ads load but do not fill: "<owadview> is not visible").
node run.mjs --mode test --layout 300x250 --duration 60

# Live ads, at most 10 loads, never clicked.
node run.mjs --mode live --live-ok --max-live-loads 10 --present transparent --duration 90

# Second launch with stored consent: run twice against the same profile.
node run.mjs --home profile:p1 --present transparent --duration 40 --run-id first
node run.mjs --home profile:p1 --present transparent --duration 40 --run-id second

# Long session (heartbeat cadence), then app.quit() with the window open.
node run.mjs --present transparent --duration 360 --quit-style quit

# Opt-out, packages and webRequest variants.
node run.mjs --disable-analytics --present transparent
node run.mjs --packages gep,overlay
node run.mjs --webrequest --present transparent

# Round-2 scenarios: an option preset plus a timed action script (lib/scenarios.mjs).
node run.mjs --scenario messages --window-monitor             # host <-> page messages, email hashes, payment id
node run.mjs --scenario crash --window-monitor                # guest crash recovery and analytics
node run.mjs --scenario block --window-monitor                # first ad page load refused: retry timing
node run.mjs --scenario cmp --window-monitor                  # consent windows, pinned invisible
node run.mjs --scenario cmp-required --features empty-object  # isCMPRequired() against a local stand-in
node run.mjs --scenario windows --window-monitor              # window analytics names (offline)
node run.mjs --scenario windows-urls --window-monitor         # names for remote, about:, data: URLs (offline)
node run.mjs --scenario offscreen --window-monitor            # ad window at opacity 0 and off-screen
node run.mjs --scenario packages                              # package manager surface (offline)
node run.mjs --scenario introspect                            # own member and listener names
nohup taskpolicy -b node run.mjs --scenario long --allow-long --caffeinate --no-cdp &   # 13 h hidden session

node analyze.mjs captures/<run-id>        # report.md + report.json
node lib/netlog-parse.mjs <netlog.json>   # parse any net log
node uid-matrix.mjs                       # author shapes x name fields -> uid, checked with `ow client calc-electron-uid`
node muid-probe.mjs                       # which machine-id derivation gives the observed muid
node muid-probe.mjs --experiment          # macOS: stand-in ioreg on PATH, checks muid follows it
```

Run `node run.mjs --help` for every option. Prefix long runs with `taskpolicy -b` on macOS, or `nice` on Linux, to keep the machine responsive.

## Notes and pitfalls

- `<owadview>` guests are webContents of type `owadview`, and `app.on('web-contents-created')` does not report them. The harness finds them by polling `webContents.getAllWebContents()`.
- Attaching the DevTools protocol to a guest before its first `dom-ready` breaks ow-electron's guest preload. The guest then reloads, with `Cannot destructure property 'preloadScripts'`. The harness attaches late. `--no-cdp` is the control run: the net log still records everything.
- Do not style `<owadview>`. A sized block style changes the slot the guest reports and the ad library rejects it. Size the parent instead, as Overwolf's sample does.
- `--webrequest` adds `session.webRequest` listeners to the default session, where ow-electron also shapes ad requests. Compare with a run without it before you trust header details from such a run.
- In Electron 42, messages from a page to the main process arrive as `-ipc-message` and `-ipc-invoke` events on the page's **session**, not on its webContents. The harness wraps `Session.prototype.emit` to see them.
- `session.webRequest.onBeforeRequest` registered by the app is not called for `<owadview>` guest navigations, and `debugger` network emulation does not reach the guests. To make a guest load fail, the `block` scenario rewrites the guest's `loadURL` to a closed local port.
- macOS clamps window positions: a window moved to -20000,-20000 lands just off the left edge of the display (for example x -960). It is still fully off-screen.
- `--offline` blocks the consent page too. Add `--offline-allow content.overwolf.com` when a scenario needs it to load.
- The net log has no HTTP/3 upload bodies. Only some ad-page requests use h3. Host analytics go over HTTP/2.
