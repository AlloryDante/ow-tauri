# ow-electron parity harness

This folder records what an app does on ow-electron and on Tauri with
`tauri-plugin-overwolf`, and compares the two. It is a small ow-electron app
plus scripts that watch `@overwolf/ow-electron` as a black box: they record
requests, files, cookies and the values ow-electron exposes, and never change
ow-electron. `tauri-app/` runs the same scenarios on the plugin, and
`parity-diff.mjs` lists every difference. ow-tauri uses the harness to match
ow-electron's ad, consent, analytics and identity behaviour on the wire.

It is not part of the npm workspace, and nothing in ow-tauri imports it. To
check the parity results yourself, start with
[Rerun the proof](#rerun-the-proof). The rest of this file is for
contributors who add scenarios or read captures.

## Rerun the proof

You run one scenario on ow-electron (the baseline), the same scenario on
Tauri with the plugin, and diff the two captures: host and ad page requests,
consent, cookies, state files, guest state and `<owadview>` events.

You need macOS (the Tauri run needs the window monitor), Node.js 22.12 or
newer, and Rust with the [Tauri 2 prerequisites](https://tauri.app/start/prerequisites/).
Install at the repository root, then the harness on its own:

```sh
npm install
npm run build --workspace tauri-plugin-overwolf-api
cd tools/parity-harness
npm install --workspaces=false
```

Record the baseline and a Tauri run with test ads and invisible windows, then
diff them. The first Tauri run builds `tauri-app` in debug with the plugin's
`lab` feature.

```sh
node run.mjs --mode test --present transparent --window-monitor --run-id E1
node run.mjs --host tauri --mode test --present transparent --window-monitor --run-id T1
node parity-diff.mjs captures/E1 captures/T1
```

To compare one scenario, add the same `--scenario <name>` to both runs, for
example `--scenario sizes`. Each run writes `captures/<run-id>/`
(git-ignored). The diff writes `parity-diff.md` and `parity-diff.json` into
the Tauri run's folder and exits 1 while a `BUG` row remains. An
`intended:*` row is a documented difference (the host label, a platform gap,
an optimisation or a deviation), a `variance` row also differs between two
ow-electron runs, and a `BUG` row is any other difference. See
[the class table](#comparing-the-hosts-parity-diffmjs) and
[docs/PARITY.md](../../docs/PARITY.md) for the results.

## What it records

Each run writes `captures/<run-id>/`. That folder is git-ignored and holds:

| File                                                         | Contents                                                                                                                                                                                                                                                                                                                                                          |
| ------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `netlog.json`, `netlog-requests.json`                        | Chromium net log (`--log-net-log`, capture mode `Everything`) and one parsed record per request. A record has the URL, method, initiator, wire headers in order, cookies sent and stored, upload body (HTTP/1.1 and HTTP/2), status, response headers and body. It covers every process, including requests ow-electron makes from the main process.              |
| `cdp-network.jsonl`, `cdp-bodies.jsonl`                      | DevTools protocol network events for each webContents, including the `<owadview>` guests. Response bodies for overwolf.com documents and XHR.                                                                                                                                                                                                                     |
| `main-js-net.jsonl`                                          | Calls made from main-process JavaScript to `net.request`, `net.fetch`, `fetch`, `http` and `https`.                                                                                                                                                                                                                                                               |
| `webrequest.jsonl`                                           | `session.webRequest` events. Only written with `--webrequest`.                                                                                                                                                                                                                                                                                                    |
| `overwolf.json`                                              | `app.overwolf` members over time, and the results of `isCMPRequired()` and friends.                                                                                                                                                                                                                                                                               |
| `packages.jsonl`                                             | `app.overwolf.packages` events.                                                                                                                                                                                                                                                                                                                                   |
| `guest-<n>-<phase>.json`                                     | Each ad guest's `window.__overwolf__` (data fields, plus which members are native functions), `document.referrer`, UA, cookies and localStorage.                                                                                                                                                                                                                  |
| `guest-messages.jsonl`, `page-events.jsonl`, `console.jsonl` | postMessages seen in guests, `<owadview>` element events in the host page, and console output from remote pages.                                                                                                                                                                                                                                                  |
| `cmp-pages.jsonl`, `cookies-*.json`, `cookie-changes.jsonl`  | The consent page's native API surface, plus cookie jars at startup and at the end, and every cookie change with its cause.                                                                                                                                                                                                                                        |
| `files/before`, `files/after`                                | The ow-electron state folder (`<appData>/ow-electron/<uid>`) and the app's userData, hashed and copied. `meta.json` has the diff.                                                                                                                                                                                                                                 |
| `live-loads.jsonl`                                           | Every live ad load, counted against the cap.                                                                                                                                                                                                                                                                                                                      |
| `ipc.jsonl`                                                  | Messages between the host and every page: `host->page` (`send`, internal sends, `postMessage`, `executeJavaScript`, `reload`, `loadURL`, `setAudioMuted`, ...) and `page->host` (`-ipc-message` / `-ipc-invoke` on the session, with the reply). The harness's own probes are left out.                                                                           |
| `wc-events.jsonl`                                            | Other webContents events of the ad guests (navigation, crashes, load failures).                                                                                                                                                                                                                                                                                   |
| `actions.jsonl`, `ticks.jsonl`                               | The scenario's timed actions with their results, and a periodic heartbeat from the harness.                                                                                                                                                                                                                                                                       |
| `introspect.jsonl`, `listeners.jsonl`                        | Own member names of ow-electron objects and the IPC listener names on each webContents (names only).                                                                                                                                                                                                                                                              |
| `features.jsonl`                                             | Requests answered by the local feature-flag stand-in (`--features`), with headers in order.                                                                                                                                                                                                                                                                       |
| `window-monitor.jsonl`                                       | macOS: every window the app owns as the window server sees it (alpha, on-screen, bounds), and an `everVisible` verdict at the end.                                                                                                                                                                                                                                |
| `adformats.md`, `adformats.json`                             | From `lib/adformat-report.mjs`: what the app set on each `<owadview>`, the attach parameters ow-electron forwarded, the ad library `options` requested on the wire, and one time-ordered table of element events (payloads are own properties of the `Event`), guest-to-host trigger events, host-to-guest messages, element removal and ad-format console lines. |
| `emitter-trace.jsonl`                                        | Every `EventEmitter.emit` in the main process. Only with `emitterTrace` in a scenario config; large.                                                                                                                                                                                                                                                              |
| `screen-*.png`, `app.pid`                                    | Screenshots of the main display (only with `--screencapture`) and the app's pid.                                                                                                                                                                                                                                                                                  |
| `report.md`, `report.json`                                   | Summary from `analyze.mjs`.                                                                                                                                                                                                                                                                                                                                       |

## Safety

- **No visible windows.** The app hides its dock icon first. `browser-window-created` and guards on every `BrowserWindow` show, focus and fullscreen method keep each window hidden (`--present hidden`) or at opacity 0, ignoring the mouse and not focusable (`--present transparent`). Every call to those methods is logged.
- **Windows are pinned invisible before they exist on screen.** `browser-window-created` runs before the constructor applies its options (the `cmp` scenario calibrates this first and drops its window actions if it fails). The handler sets opacity 0, ignores the mouse and makes the window not focusable. Later calls to `setOpacity`, `setIgnoreMouseEvents` and `setFocusable` are pinned to those values and logged as `pinned-call`. This is what makes it safe to call `openAdPrivacySettingsWindow()` and `openCMPWindow()`, which show their window from JavaScript.
- **Proof.** `--window-monitor` (macOS) polls the window server for the app's windows and records `everVisible`. Treat a run with `everVisible: true` as a failure. Under `taskpolicy -b` it samples about every 190 ms.
- **No clicks on ads.** The harness never sends input to an ad or app page. The one exception is `gesture-timing`: it refuses outside test mode, first replaces the first ad guest's page with a loopback fixture page (`http://127.0.0.1:<port>/fixture`, never an ad), sends `sendInputEvent` clicks and keys only to that page, and the page only ever opens `owparity-canary://` URLs, a scheme with no registered handler. The scenario also sets `stubOpenExternal`, which replaces `shell.openExternal`, `openPath` and `showItemInFolder` with recorders (`open-external.jsonl`) that open nothing.
- **Live ads are opt-in and capped.** `--mode live` needs `--live-ok`. `--max-live-loads N` lets N loads run (default 10); a load beyond N removes every `<owadview>` as it starts (so a run logs at most N + 1), and every live load is logged in `live-loads.jsonl`.
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

# Record-first observations (TEST mode, hidden; ow-electron).
node run.mjs --scenario email-hashes-clear --window-monitor   # setUserEmailHashes(undefined | null | {} | '' | no argument) after a set
node run.mjs --scenario last-window-during-consent --window-monitor  # app window closed while the startup consent window is open
node run.mjs --scenario last-window-before-consent --window-monitor  # closed before the consent window exists
node run.mjs --scenario last-window-consent-saved --window-monitor   # closed just after the consent was saved
node run.mjs --scenario last-window-after-consent --window-monitor   # control: closed after 10 s
node run.mjs --scenario corrupt-state --home profile:cs --window-monitor   # seed a profile first (normal launch)
node run.mjs --scenario corrupt-state --home profile:cs --corrupt-state garbage --window-monitor
node run.mjs --scenario gesture-timing --window-monitor       # click-out gesture timing on a loopback fixture (see Safety)
nohup taskpolicy -b node run.mjs --scenario long --allow-long --caffeinate --no-cdp &   # 13 h hidden session

# Round-3 ad formats (docs: monetization/advertising/*). Test ads unless --mode live --live-ok.
node run.mjs --scenario sizes --window-monitor             # every documented standard container size
node run.mjs --scenario perf --window-monitor              # performance (interstitial) ad, docs example
node run.mjs --scenario perf-sample --window-monitor       # performance ad as the official sample creates it
node run.mjs --scenario perf-unit --window-monitor         # `unit` on a performance ad and a standard slot
node run.mjs --scenario perf-small --window-monitor        # window below the documented 1000x600 minimum
node run.mjs --scenario perf-twice --window-monitor        # two performance ads in one window
node run.mjs --scenario perf-remove --window-monitor       # the app removes a running performance ad
node run.mjs --scenario perf-with-standard --window-monitor  # performance ad over a running standard slot
node run.mjs --scenario reward --window-monitor            # reward ad: preload, opt-in by showing the slot, play ... complete
node run.mjs --scenario reward-two-slots --window-monitor  # two rewarded slots, preload only
node run.mjs --scenario high-impact --window-monitor        # the documented high-impact ad zone
node run.mjs --scenario high-impact-small-zone --window-monitor
node run.mjs --scenario house --window-monitor             # only Overwolf hosts reachable: partner no-fill path
node run.mjs --scenario adstyle-probe --window-monitor     # which attribute values switch the ad library options
node run.mjs --scenario send-command-probe --window-monitor  # how sendCommand()/setPageUrl() reach the guest

# Lab checks (both hosts; AD-FORMATS-SPEC section 7). Test ads unless named -live.
node run.mjs --scenario lab-layers        # L1-L4: transparency, z-order, pass-through, blur (hit probes)
node run.mjs --scenario audio             # L5: setAudioMuted timeline per guest
node run.mjs --scenario standard-remove   # L6/L9: remove, re-add and move a standard slot
node run.mjs --scenario reward-optin      # L7: reward opt-in hidden for 1 frame / 50 ms / 500 ms / 2 s
node run.mjs --scenario perf-minimize     # L8: minimize and restore under a performance ad
node run.mjs --scenario owadtestad        # L11: localStorage.owAdTestAd inside the guest
node run.mjs --scenario tower-plus        # 400x600 + 400x60
node run.mjs --scenario high-impact-only  # the high-impact zone without the small zone
node run.mjs --scenario inview-probe      # 300x250 slots 25/50/75/100 % in view, vertical and horizontal, plus a 2.5-3 % sweep
node run.mjs --scenario inview-fine       # one 300x250 swept 1 % at a time across 50 %, top edge then left edge

node analyze.mjs captures/<run-id>        # report.md + report.json
node lib/adformat-report.mjs captures/<run-id>   # adformats.md + adformats.json: attach params, ad-library options, event/IPC timeline
node lib/netlog-parse.mjs <netlog.json>   # parse any net log
node uid-matrix.mjs                       # author shapes x name fields -> uid, checked with `ow client calc-electron-uid`
node muid-probe.mjs                       # which machine-id derivation gives the observed muid
node muid-probe.mjs --experiment          # macOS: stand-in ioreg on PATH, checks muid follows it
```

Run `node run.mjs --help` for every option. Prefix long runs with `taskpolicy -b` on macOS, or `nice` on Linux, to keep the machine responsive.

## Tauri edition (`--host tauri`)

`tauri-app/` is the same harness app on ow-tauri, written the way a Tauri app is: plain Tauri windows and webviews, `tauri-plugin-overwolf` built with its `lab` feature (debug only), the plugin's Rust API for the calls an ow-electron app makes in its main process, and in the ad window's page the plugin's `<owadview>` runtime and JavaScript API (`tauri-plugin-overwolf-api`). It runs the scenarios `app/` runs, step by step, and writes the same capture files. `run.mjs --host tauri` builds it once in debug (`tauri-app/build.mjs`: `web/harness.js` bundled with rolldown, the shared `app/page.js`, the pages the `open-window` actions load, then `cargo build -j 4` at background priority) and runs one scenario. `--no-build` reuses the last binary. macOS only, because the window monitor is, except on a CI runner (see the Windows lab below).

```sh
taskpolicy -b node run.mjs --host tauri --identity local.identity.example.json --mode test --present transparent --layout 400x600 --duration 90 --run-id T-A
taskpolicy -b node run.mjs --host tauri --no-build --identity local.identity.example.json --scenario messages --run-id T-messages
```

- **The app (`tauri-app/src-tauri/src`).** `driver.rs` runs the scenario (snapshots, calls, the ad window `main`, the timed actions, the quit flow); `windows.rs` builds plain windows, hidden and not focusable, sized as Electron sizes a `BrowserWindow` (frame size); `observe.rs` probes the ad guests (`owad-*`: `guest-<n>-<phase>.json` at each new ad page document and 10 s later; `guest-requests.jsonl`, every resource each guest's page and its same-origin frames list in their resource timing, read every second), the consent pages (`ow-cmp*`: `cmp-pages.jsonl`) and the cookies (`cookie-changes.jsonl`, with `wall`); `probe.rs` with `native.rs` / `native_win.rs` answers `hit-probe`. The ad window's page (`web/index.html`) loads `web/harness.js`, which answers the driver's `pageEval` and `owCall` requests through the app commands `harness_reply`, `harness_page_event` and `harness_page_info`.
- **Calls.** An `ow-call` goes through the JavaScript API in the ad window's page when it exists (the scenario's `app.overwolf.<fn>` names are the API's names), else through the Rust API (`noWindow` scenarios, and `isCMPRequired` at startup). `cmp-open` uses the Rust API. `overwolf.json` snapshots are Tauri-native (`surface: "tauri-plugin-overwolf-api"`): the data members ow-electron exposes on `app.overwolf` (`uid`, `muid`, `phasePercent`, `utmParams`); `parity-diff.mjs` compares those and reports the API surface (`apiSurface`).
- **Lab mode.** The plugin's `lab` feature is off by default and must never ship. With it, `OW_TAURI_LAB_DIR=<run dir>` turns on the plugin's trace and `OW_TAURI_LAB_INVISIBLE=1` keeps every window invisible. The app runs as an accessory app (no Dock icon, no Cmd-Tab) that never activates (`macos_lab.rs` turns `activate` into a no-op and `makeKeyAndOrderFront:` into `orderFrontRegardless` before any window exists). A harness window is built hidden and not focusable and is shown only after it has alpha 0 and ignores the mouse, at an on-screen position (an ad slot must be on screen to fill); the plugin's own windows stay hidden. `run.mjs` always sets both. The trace files (see `crates/tauri-plugin-overwolf/src/lab.rs`) use the ow-electron capture shapes, and `lib/tauri-host.mjs` turns the host requests into `netlog-requests.json`, so `analyze.mjs` and `parity-diff.mjs` read both hosts.
- **Proof of invisibility.** The window monitor runs on every Tauri run and the app is killed the moment it reports a visible window. A run with `everVisible: true` is a failure.
- **What the lab cannot see.** Headers WebKit adds to ad pages (only the fields the host sets are recorded), HTTP/2 pseudo-header order (reconstructed in the `h2` crate's order), the protocol of a request that never completed, and requests of cross-origin frames. The guest probe's `labResources` lists the requests of the ad page and its same-origin frames (where the fill impression is sent), repeats included, so the live fill count (lab check 6) matches the net log's.
- **Steps not mirrored.** `pkg-call` and the startup `packages.*` calls (ow-tauri has no package manager), `listeners` and `introspect` (Electron internals), `window` `emit` (Tauri cannot raise a window event), `cookie-set`, and `guest-fixture` / `gesture-case` (the plugin sends a guest's top-level navigation off Overwolf back to the ad page, so the loopback fixture cannot replace it) are recorded as `action-unsupported` and reported as `not-mirrored`. `screencapture` never runs. `crash-guests` ends each guest's web content process (`kill -9` of the `WKWebView`'s process; macOS).
- **Offline presets.** A scenario preset's `--offline` is Electron's proxy; the Tauri run ignores it and goes online. Record the ow-electron twin with `--online` so both hosts see the same network (an offline ow-electron also writes `logs/ow-electron.log` and stores no consent).
- **Native probe.** `hit-probe` asks `probe.rs` (`native.rs` on macOS) for the native z-order of the window's webviews, a native hit test at each point and one snapshot per webview (`WKWebView takeSnapshot`, which needs no screen recording). `lib/adformat-report.mjs` composites the snapshots bottom to top. In test mode it may also send a synthesized click to the app's own control (never to an ad guest); WebKit does not turn such events into DOM events in the invisible window, so the native hit test is the routing proof.
- **Front app.** `lib/front-monitor.mjs` samples the front app (`lsappinfo front`) every 200 ms on both hosts and writes `front-monitor.jsonl`. A run whose app ever became frontmost is a failure: an invisible app must never take the keyboard.
- The app identity comes from `local.identity.json` (or `--identity <file>`) at run time (`PARITY_HARNESS_PACKAGE_JSON`): the app sets its name, version and `plugins.overwolf.{name, author, uid}` from it before the plugin starts, so it never enters the build or a tracked file.

## The DESIGN §5.2 checks

`SECTION_5_2` in `lib/scenarios.mjs` lists each check, the scenarios that run it and where (`both`: the macOS invisible lab and the Windows CI lab; `macos`; `windows-ci`), plus what the harness still lacks for the ow-tauri run (`missing`); `scenarios.test.mjs` keeps the list whole. Scenario options added for them, on both hosts unless noted:

- `setupTitle`: the app retitles its window right after creating it (`title-set-in-setup`).
- `windowUserAgent`: the ad window's own user agent (`custom-ua`).
- `closeHandler`: the app's own close handler on its window, off while the run quits: `tray` (prevent, hide), `tray-js` (ow-tauri: the page's `onCloseRequested`), `delay-destroy` (destroy 500 ms later), `confirm-5s` (show again 5 s later).
- `--quit-style exit` (`app.exit(0)`) and `terminate` (macOS `[NSApp terminate:]`; ow-electron `app.quit()`, the same call).
- `overwolfConfig` (ow-tauri): merged into `plugins.overwolf` (`recreate-reload`, `no-analytics-config`).
- `crashHook: false` (ow-tauri, macOS): the web content termination hook left unwired (`crash-fallback`).
- `hosts` / `windowsOnly`: `run.mjs` refuses the scenario elsewhere. `dialog-probe` is ow-tauri only: ow-electron shows a native dialog for a guest's `alert()`.

## Windows lab on CI (`ci/windows-lab.mjs`)

`.github/workflows/windows-lab.yml` runs the lab on a GitHub Windows runner (on pushes to `main` that touch the plugin, the API or guest-shim packages or the harness, weekly as the drift lab, and on demand with a list of lab entries). One job builds `tauri-app` with the `lab` feature; eight shards then run, per lab entry, ow-electron and ow-tauri one after the other on the same runner, run `parity-diff.mjs` on the pair and evaluate the Windows checks of `lib/windows-checks.mjs`. The entries (`LAB_SCENARIOS` in `ci/windows-lab.mjs`) are the round-2 base runs, every round-3 ad-format scenario, the record-first scenarios (email-hash clears, the last window during consent, every `corrupt-state` kind, gesture timing) and every DESIGN §5.2 check that applies to Windows. Captures, window copies and diffs are uploaded as the `windows-lab-captures-<shard>` artifacts; each shard's table, with every BUG row under it, is in the job summary and in the `report` job.

- **Lab entries.** A scenario name, or `corrupt-state:<kind>` for one corruption (`corrupt-state` in `--scenarios` means all seven). `corrupt-state` and `no-analytics-persisted` need an earlier launch: the driver runs a seed launch per host on the same state first and compares only the second. A scenario that runs on ow-tauri only (`hosts`, `dialog-probe`) is recorded and listed as `n/a` without a comparison. `build-identity` (an installer) is its own job.
- **Other jobs of the workflow.** `unstable-input` (the key-input spike: stable 58/58, unstable without and with a child webview 58/58 and 59/59), `webview2-minimum` (ow-tauri on fixed-version WebView2 Runtimes: `sizes` and `local-frame` on the floor's Edge 98 build with ads working, checked by `ci/webview2-min.mjs`, and Edge 97 below it with no ad guest), `build-identity` (DESIGN §5.2 #20: the quickstart's NSIS installer with an overridden version installs, runs, updates and uninstalls with the merged-config uid; Overwolf's hosts resolve to 127.0.0.1 there) and, weekly or with `drift: true`, the newest ow-electron `latest`, the newest Tauri 2.x for the spike and a macOS subset (`MACOS_DRIFT_SCENARIOS`, invisible lab on a hosted runner); a failed weekly run opens or updates an issue.
- **Test ads only.** The driver passes `--mode test` to every run and never `--live-ok`.
- **Neutral identity.** The harness default (`Example Studio` / `Parity Harness`, formula uid). The driver refuses to run when a `local.identity.json` is present.
- **Visible on the runner.** `run.mjs --ci-visible` (refused unless `GITHUB_ACTIONS=true`) runs the Tauri app without the invisible lab mode (which on Windows only keeps windows hidden, and a hidden slot does not fill); the runner's desktop is nobody's screen. ow-electron keeps its opacity-0 windows. No window monitor runs there.
- **Fresh state per run.** Windows reads the app folders from the shell, not from `HOME`, so the driver ends any WebView2 process the previous run left behind, then removes the ow-electron state folder, the app's userData (with the ads data store) and the Tauri app's WebView2 folder before each launch.
- **Native probe (`tauri-app/src-tauri/src/native_win.rs`).** `hit-probe` reads each webview's container window (the WebView2 controller's parent window): the z-order of the window's child windows, each container's window region (`GetWindowRgn`: `empty` while input passes through, `none` when the guest takes input), the controller's default background colour and the guest's `IsMuted`. A native hit test (`WindowFromPoint`) gives the routing. The composed window is copied with `PrintWindow(PW_RENDERFULLCONTENT)` (and, to compare, the same desktop rectangle), sampled at the points and kept as `hit-<label>-print.bmp` / `-screen.bmp`. In test mode one `SendInput` click goes to the app control, and only when the hit test there names the app's own webview.
- **Windows checks.** L1-W: the composed window shows the app's colour under a guest with no content, as ow-electron's does (the red container under the ready reward slot). L2: the performance guest's container is the top child window. L3-W: while the interstitial loads, its container's region is empty and the click reaches the app; once the modal has loaded (`performance_ad_loaded`; the probe at 16 s) the region is gone and the click is refused (it would reach the ad). L5: each guest's mute state read back natively equals ow-electron's `isAudioMuted()` at the same moments (`audio` probes at 5, 12 and 47 s).

```sh
# On a runner (the workflow's lab step):
node ci/windows-lab.mjs --shard 1/8 --scenarios all
node ci/windows-lab.mjs --scenarios lab-layers,audio,corrupt-state:array
# Anywhere: print a shard's plan (entries and estimated seconds) without running it.
node ci/windows-lab.mjs --list --suite windows --shard 3/8
```

## Comparing the hosts (`parity-diff.mjs`)

```sh
node parity-diff.mjs captures/<ow-electron run> captures/<ow-tauri run> [--tolerance-ms 1500] [--burst-ms 250] [--allow-scenario-mismatch]
node --test parity-diff.test.mjs
```

Both runs must use the same scenario definition, layouts and mode (`options.scenarioDef`, `layouts` and `mode` in each `meta.json`). When they differ, for example a baseline recorded before the scenario was edited, every difference of definition would read as a BUG, so the diff refuses with exit 2 and names the fields. `--allow-scenario-mismatch` runs it anyway and puts a warning at the top of `parity-diff.md` (`scenarioMismatch` in `parity-diff.json`). A capture's folder name can differ from its `runId`; pick the baseline by its `meta.json`.

Current ow-electron baselines (TEST mode, recorded locally, captures are not in git): `R3-reward-test` for `reward`, `R4-inview-test` for `inview-probe` and `R4-inview-fine-test` for `inview-fine`. A scenario edited after its baseline needs a new baseline run before it can be diffed.

It compares two captures of the same scenario after normalising volatile values (timestamps, session ids, consent strings, cache-busters, the host label) and lists every observable difference: host requests (set, order, query and `Extra` fields, body, header order and values, cookies, status, protocol), requests sent together, ad document headers, consent pages and cookies, the state file, each guest's `__overwolf__` and page state, element events and API, host-to-guest messages and the visibility sequence. Each difference is classified:

| Class                 | Meaning                                                           |
| --------------------- | ----------------------------------------------------------------- |
| `intended:host-label` | the labelling rule (`tauri_*`, `tauri-<tv>`, `Tauri/<tv>`)        |
| `intended:os-gap`     | a documented platform gap (WebKit, macOS)                         |
| `intended:optimised`  | documented "optimised, same outcome"                              |
| `intended:deviation`  | a documented ow-tauri decision (PARITY deviations)                |
| `variance`            | differs between two ow-electron runs too (ad content, HTTP cache) |
| `not-mirrored`        | a harness step the Tauri edition cannot run                       |
| `BUG`                 | anything else                                                     |

Ad-format runs are also compared through `lib/adformat-report.mjs` facts: per element the event names, order, counts, payload keys and removal timings, the DOM state (display, pointer-events, inline style) before and after the first `display_ad_loaded`, the ad library options on the wire (as sets, keys sorted at every depth), each guest's mute sequence, the lab hit probes (page routing, ow-tauri native routing, composited colour class, performance pointer-events, clicks) and the front app. A colour at a point that lands on a served creative (`std-slot`) is variance; the red container, the app control and the bare corner carry the transparency and z-order checks.

The requests the ad page itself sends (CONTRACT E.5: the `owads_*` and `oam_*` Counters, InsertStats Kind 400051, the `ad_uid` session ids) are compared from ow-electron's net log and ow-tauri's `guest-requests.jsonl` (Windows: the plugin's `guest-network.jsonl`): each name sent by both hosts, the per-document names once per `owads_first_load`, the same InsertStats posts and as many session ids per document. Counts of names driven by the ads served (waterfall, video states, rewarded flow) are variance, and so are more documents (and their teardown names, `owads_shutdown`, `owads_ad_container_duration`) when the element events' `dom-ready` counts differ the same way. The `gesture-timing` cases (§5.2 #12) are compared by the external opens each case led to and what its page saw `window.open` return; a case ow-tauri could not run is reported once, as `action-unsupported`.

The ad page asks its host for a reload on its own after it is told `hidden` (about 4.5 s later, depending on its ad state, and not once it is shown again first). Fewer ow-tauri guest loads are variance only when both hosts told the guest the same visibility sequence and ow-tauri loaded the page once plus once per reload request it received (ow-electron's `GUEST_ADVIEW_RELOAD` requests, when recorded, must account for its loads the same way). Each host re-sends `customTracking` after every guest load, so a message sequence that differs only by as many re-sends as the loads differ is variance too.

The results go to `parity-diff.json` and `parity-diff.md` in the Tauri run. The exit status is 1 while a `BUG` remains.

## Notes and pitfalls

- Scenario `elementSpec` (round 3) lets a scenario build elements the way a format's docs do: `{parent: 'body'}` appends an unsized `<owadview>` to `<body>` (performance ads), `{zone: 'high-impact'}` builds the documented 440 px ad zone with its listeners, and `at` delays creation. `page.js` also samples each element's layout (`layout-sample` in `page-events.jsonl`) and reports removal (`owadview-removed`).
- `hook-guest-frames` adds a `message` listener to the ad guest's same-origin child frames (the ad library frame) and logs what arrives there as `__PARITYF__` console lines. `guest-eval` runs code in every ad guest. Both are the harness's own calls and stay out of `ipc.jsonl`.

- `<owadview>` guests are webContents of type `owadview`, and `app.on('web-contents-created')` does not report them. The harness finds them by polling `webContents.getAllWebContents()`.
- Attaching the DevTools protocol to a guest before its first `dom-ready` breaks ow-electron's guest preload. The guest then reloads, with `Cannot destructure property 'preloadScripts'`. The harness attaches late. `--no-cdp` is the control run: the net log still records everything.
- Do not style `<owadview>`. A sized block style changes the slot the guest reports and the ad library rejects it. Size the parent instead, as Overwolf's sample does.
- `--webrequest` adds `session.webRequest` listeners to the default session, where ow-electron also shapes ad requests. Compare with a run without it before you trust header details from such a run.
- In Electron 42, messages from a page to the main process arrive as `-ipc-message` and `-ipc-invoke` events on the page's **session**, not on its webContents. The harness wraps `Session.prototype.emit` to see them.
- `session.webRequest.onBeforeRequest` registered by the app is not called for `<owadview>` guest navigations, and `debugger` network emulation does not reach the guests. To make a guest load fail, the `block` scenario rewrites the guest's `loadURL` to a closed local port.
- macOS clamps window positions: a window moved to -20000,-20000 lands just off the left edge of the display (for example x -960). It is still fully off-screen.
- `--offline` blocks the consent page too. Add `--offline-allow content.overwolf.com` when a scenario needs it to load.
- The net log has no HTTP/3 upload bodies. Only some ad-page requests use h3. Host analytics go over HTTP/2.
