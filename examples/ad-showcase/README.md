# ow-tauri Ad Showcase

One window that shows every `<owadview>` ad format working, in test and in
live mode, with **the same HTML and JavaScript on ow-electron and on
ow-tauri**. It is built for a presenter: pick a page, watch the ads load and
read every event the ad element raises in the timeline on the right.

- `src/renderer/` is the window: plain TypeScript and CSS, no framework. It
  talks to the main process only through `window.showcase` (the preload).
- `src/main/main.ts` is the main process, written against the `electron` API.
  - On **ow-electron**, `electron` is the real module.
  - On **ow-tauri**, the bundler aliases `electron` to `ow-tauri/electron`
    and the main process runs in the hidden main webview, as in
    [`examples/packages-sample`](../packages-sample/README.md). `src-tauri/`
    is the same thin shell around `tauri-plugin-overwolf`.
- One rolldown config ([rolldown.config.mjs](rolldown.config.mjs)) builds both
  outputs; [scripts/stage.mjs](scripts/stage.mjs) writes them to `.stage/`
  (git-ignored).

## Prerequisites

- Node 22.12 or newer and the [Tauri 2 prerequisites](https://tauri.app/start/prerequisites/)
  for your OS (Rust, WebView2 on Windows, WebKitGTK on Linux).
- Install from the repository root, so `ow-tauri` links to `packages/ow-tauri`,
  and install ow-electron for the twin in `tools/parity-harness` (its own
  install, outside the workspace):

```shell
npm install
npm run build --workspace ow-tauri
(cd tools/parity-harness && npm install --workspaces=false)
```

The showcase runs ow-electron from there
([scripts/ow-electron.mjs](scripts/ow-electron.mjs)) instead of depending on
it: hoisted into the workspace next to ow-tauri, ow-electron's `electron`
types would merge with ow-tauri's ambient ones.

## Run both hosts

From `examples/ad-showcase`:

| Script                                | What it does                                                                       |
| ------------------------------------- | ---------------------------------------------------------------------------------- |
| `npm run start:electron:test`         | stage, then `ow-electron --test-ad .stage/electron` (test ads)                     |
| `npm run start:electron`              | the same without `--test-ad` (live ads)                                            |
| `npm run start:tauri:test`            | stage, then `tauri dev` with `--test-ad` (test ads)                                |
| `npm run start:tauri`                 | the same without `--test-ad` (live ads)                                            |
| `npm run build`                       | stage both hosts (`.stage/electron`, `.stage/tauri`)                               |
| `npm run typecheck` / `lint` / `test` | `tsc`, ESLint, vitest                                                              |
| `npm run check:rust`                  | `cargo fmt --check`, `cargo clippy -D warnings` with and without the `lab` feature |
| `npm run lab:smoke`                   | the invisible lab smoke run (macOS, agents; see [e2e/README.md](e2e/README.md))    |

ow-electron downloads its runtime on first use. `src-tauri/` is its own
Cargo workspace with its own `Cargo.lock`, like the packages sample.

## TEST vs LIVE

- **TEST** (`--test-ad`): Overwolf's test inventory. Every format fills,
  including high impact, interstitial and reward. The top bar shows a
  **TEST** badge.
- **LIVE** (no switch): real demand for the app's uid. The top bar shows a
  **LIVE** badge. Standard display and video fill once Overwolf has enabled
  the uid; a house ad fills on no-fill once one is set up. High impact,
  interstitial and reward are demand-gated: they do not fill until Overwolf
  qualifies the uid or attaches a demo campaign.
- The mode follows the `--test-ad` switch on both hosts. ow-tauri also
  accepts `OW_TAURI_TEST_AD=1`, but the badge only sees the switch, so use
  the `:test` scripts.
- **Restart in TEST / LIVE** in the top bar relaunches the app with or
  without `--test-ad` (`app.relaunch`) and comes back to the same page and
  choice (`--showcase-page=<page>[/<choice>]`, e.g. `layouts/tower` or
  `sizes/300x250`; the same switch opens the app on that page). Going LIVE
  asks for an inline confirmation first.

## Identity

The tracked [package.json](package.json) holds a placeholder identity
(`author: "Example Studio"`, `productName: "ow-tauri Ad Showcase"`), so a clean
clone runs in test mode with the formula uid (`docs/CONTRACT.md` G.2).

For a demo with your own app identity:

1. Copy [identity.example.json](identity.example.json) to
   `identity.local.json` (git-ignored) and fill in `author` and
   `productName` as registered with Overwolf. Set `uid` only when Overwolf
   assigned one in the console; it becomes `overwolf.uid`, which both hosts
   use as is.
2. Run any start script (or `npm run build`). `scripts/stage.mjs` merges the
   file into `.stage/package.json`, `.stage/electron/package.json` and
   `.stage/tauri.conf.json`; the Tauri build embeds the staged manifest.
   Nothing tracked changes: `git status` stays clean.

`--identity FILE` (`node scripts/stage.mjs --host all --identity FILE`)
stages another file. The stage script never prints the values. The window
shows the uid, cuid and muid masked (`abcd…wxyz`); **Reveal** shows one in
full on screen only, and timeline exports carry the masked uid.

## The window

- **Top bar**: host and version, the TEST/LIVE badge, the uid (masked), the
  consent state, the theme toggle and Restart.
- **Sidebar**: the nine pages (keys `1` to `9`).
- **Timeline** (right rail): every event of every `<owadview>` plus every
  action you take (`control:*` rows), with the time, the slot's `cid` and
  the time since the slot was created. Filter by slot or by family, pause,
  click a row to see its payload, and **Export JSON** to
  `<userData>/exports/timeline-<host>-<mode>-<time>.json`. The counts per
  event are at the bottom.

## Pages

1. **Sizes**: the seven documented sizes (970x90, 728x90, 160x600, 400x600,
   400x60, 400x300, 300x250), each with its own `cid`. A slot loads only
   once it is fully in view, and all seven do not fit one window, so the
   **Show** select picks a group: **Towers and rectangles** (160x600,
   400x600, 400x300, 300x250; the default), **Banners** (970x90, 728x90,
   400x60), **Below the fold** (a 300x250 in a scroll box that loads only
   when scrolled into view) or one size alone. Switching builds fresh
   containers. The timeline folds away on this page to make room. In LIVE
   mode the groups leave the 400x300 out (one video container per page;
   policy note on the page); it can still be shown alone.
2. **Layouts**: the eight recommended layouts at true size; switching or
   **Recreate** builds fresh containers. This is the live-mode proof page.
3. **High impact**: a 440 px wide, full-height zone with Tower Plus
   (400x60 + 400x600 with `adstyle="high-impact-ad;"`). On
   `high-impact-ad-loaded` the slot takes the zone and its sibling is hidden
   with `display: none`; on `high-impact-ad-removed` both come back.
4. **Interstitial**: Default, Red dim, Blur 3 and With `unit` each add a
   performance `<owadview>`. The **Click me** counter shows clicks passing
   through while the ad loads and blocked once it is shown. The DOM panel
   shows the live `owadview` count and the element's `pointer-events`.
   **Shrink to 900x500**, then adding an interstitial, shows the error path
   (`performance_ad_error`, then `shutdown`); **Restore size** undoes it.
   Shrinking while an interstitial is already shown does not end it. A no-fill ends with `shutdown`
   only: `performance_ad_no_fill` has not been seen to fire.
5. **Reward**: a coin shop. The 400x300 `adstyle="rewarded-ad;"` slot
   preloads, is hidden on `video_ad_ready`, and **Watch ad · +100 coins** shows
   it. On `complete` after a `play` of the same cycle the coins are granted
   once and the slot hides for the next preload. A 300x250 reward slot shows
   "unavailable" after 10 s without `video_ad_ready`. The grant is
   client-side: Overwolf documents no server verification or postback.
6. **House**: one 400x300 slot and its `house_ad_action` /
   `house-ad-action` events. House ads appear on no-fill once configured in
   the Dev Console for the uid.
7. **Controls**: one 400x300 video slot with a `customTracking` editor,
   mute (`setAudioMuted`), `display: none`, scroll out of view and back, hide
   the window for 3 s and minimize for 3 s.
8. **Consent & identity**: `isCMPRequired`, the ad privacy settings window,
   `generateUserEmailHashes` for the fixed address `player@example.com`, and
   the identity table (uid, cuid, muid masked; host, version, platform,
   test flag).
9. **Parity**: the parity harness's report (`parity-diff.json` copied to
   `<userData>/parity-report.json`), or the commands that make it.

## Measured results (lab, macOS, 2026-10-07)

Invisible lab runs (`e2e/`, alpha-0 windows, never frontmost, no process
left), ow-tauri 0.1.0 on Tauri 2.12.1 and ow-electron 42.11.4 on one Mac
(1280x837 window). TEST is the full tour (47 steps, every page and button);
LIVE used a lab app identity whose uid is not enabled for live demand.
"Same events" means the same ad event names per slot over the run, guest
lifecycle left out (`e2e/compare.mjs`).

**TEST mode** (tours `B2-T-tour-2` against `B2-E-tour-4`)

| Format (page)                             | ow-tauri                                                             | ow-electron            | Notes                                                                                                                                                           |
| ----------------------------------------- | -------------------------------------------------------------------- | ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Standard, seven sizes (1)                 | all fill, 2 to 34 s                                                  | all fill, 3 to 7 s     | 970x90 test creative is often blank on both                                                                                                                     |
| Below the fold (1)                        | waits, fills about 2 s after scrolling in                            | same                   |                                                                                                                                                                 |
| Eight layouts (2)                         | 8/8 fill, 2.5 to 3.7 s                                               | 8/8 fill, 2.5 to 5.1 s | which slot gets a video creative varies per load                                                                                                                |
| High impact (3)                           | takeover, then removed and restored                                  | same                   | `high-impact-ad-loaded` / `-removed`, sibling `display: none`                                                                                                   |
| Interstitial (4)                          | pass-through while loading, modal after                              | same                   | probe: "Click me" 0 to 1 while loading, the ad on top once shown                                                                                                |
| Interstitial, small window (4)            | `performance_ad_error`, `shutdown`                                   | same                   |                                                                                                                                                                 |
| Interstitial, red dim / blur 3 / unit (4) | load / load / `shutdown`                                             | same                   |                                                                                                                                                                 |
| Reward (5)                                | ready, play, granted once, next ready                                | same                   | `complete` came after 11 s (ow-tauri) and 87 s (ow-electron) after a 2 s hide during play; one of three ow-tauri runs reloaded the guest on that hide (see Lab) |
| House slot (6)                            | test video plays                                                     | same                   | no house ad configured                                                                                                                                          |
| Controls (7)                              | tracking, mute, display, scroll, hide, minimize: a new ad after each | same                   |                                                                                                                                                                 |
| Consent and identity (8)                  | CMP required, three hashes, uid masked                               | same                   | the privacy settings window is skipped in lab runs                                                                                                              |

Step by step, 240 of 305 compare rows are the same and 19 differ only in
guest lifecycle (ow-electron reports a `did-fail-load` per guest). The
remaining rows are timing: a video's `play` lands one step earlier or later.

**LIVE mode** (9 ad loads in total, never clicked)

| Run (page)               | Loads | ow-tauri                                                    | ow-electron                                             |
| ------------------------ | ----- | ----------------------------------------------------------- | ------------------------------------------------------- |
| Layout Combo Classic (2) | 2 + 2 | auctions sent (`gampad/ads`, prebid bidders), no fill event | auctions sent, slot impression ping sent, no fill event |
| 300x250 alone (1)        | 1 + 1 | auctions sent, no fill event                                | auctions sent, slot impression ping sent, no fill event |
| Reward (5)               | 2     | video auctions sent, no `video_ad_ready`                    | not run                                                 |
| Interstitial (4)         | 1     | `shutdown` (no fill)                                        | not run                                                 |

No format filled live on either host for this uid: the requests go out the
same way, demand does not answer. The ow-tauri guest probe lists only the
guest page's own requests, not those of its frames, so the impression ping
is not visible there.

## Run of show (presenter)

A person runs this, with a visible window, ideally on Windows (macOS has a
known request-header gap, `docs/ARCHITECTURE.md` section 6).

1. Start `npm run start:electron:test` and `npm run start:tauri:test` side by
   side.
2. Walk pages 1 to 8 on both. Close the interstitial yourself. If Overwolf's
   QA asks for it, click an ad a few times: each click opens one browser
   window and nothing crashes.
3. Export both timelines; show them and the parity report (page 9).
4. **Restart in LIVE** on both with the demo identity. Show page 2 filling
   and a house ad on no-fill (if one is set up). Say plainly that high
   impact, interstitial and reward will not fill until Overwolf qualifies
   the uid; the timeline shows the live ad requests going out on both
   hosts.
5. Minimize: the ads hide. Close the app: no ad process is left.

## Before the meeting (with Overwolf)

- [ ] The demo uid is enabled for live ads on Overwolf's backend.
- [ ] A house ad (with an event name) is set up in the Dev Console for it.
- [ ] A demo campaign or test qualification for high impact, interstitial
      and reward on the demo uid; without it these three are shown in TEST
      mode only.
- [ ] A Windows run of the full ow-tauri test suite is green.
- [ ] A live rehearsal on the Windows machine.

## Do not claim

- In-stream ads: ow-electron has no API for them.
- Live fill of high impact, interstitial or reward without qualification.
- macOS request-header parity for subresources and `x-ow-*` headers, or
  Linux request shaping.
- A server-verified reward: none exists.
- That `performance_ad_no_fill` fires: ow-electron sends `shutdown` only.

## Lab

`e2e/` drives the showcase headlessly on both hosts (macOS, invisible
windows, test ads): see [e2e/README.md](e2e/README.md). The Tauri shell has
the lab only with its `lab` Cargo feature, which is off by default.

Open lab notes: in one of three ow-tauri tours the reward guest asked the
host to reload (`__host:reload`) when its slot came back from a 2 s
`display: none` during play, so that video never completed; the other two
and both ow-electron tours completed. In one ow-tauri tour the banner slot
of four layouts and the controls slot got no test fill (guest visible,
sized and requesting ads); the tour before and the ow-electron tours filled
them.
