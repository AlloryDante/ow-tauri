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
- Install from the repository root, so `ow-tauri` links to `packages/ow-tauri`
  and `@overwolf/ow-electron` is installed for the twin:

```shell
npm install
npm run build --workspace ow-tauri
```

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

`ow-electron` downloads its runtime on first use. `src-tauri/` is its own
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
  without `--test-ad` (`app.relaunch`). Going LIVE asks for an inline
  confirmation first.

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

1. **Sizes**: all seven sizes (970x90, 728x90, 160x600, 400x600, 400x60,
   400x300, 300x250) with unique `cid`s, plus a 300x250 in a scroll box below
   the fold that loads only when scrolled into view. The timeline folds away
   on this page to make room; at the default 1280x860 the 300x250 sits just
   below the others. In LIVE mode the 400x300 is left out (policy note on the
   page).
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
   **Shrink to 900x500** shows the error path (`performance_ad_error`, then
   `shutdown`); **Restore size** undoes it. A no-fill ends with `shutdown`
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
