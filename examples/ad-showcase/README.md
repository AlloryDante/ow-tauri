# ow-tauri ad showcase

A Tauri 2 app that shows every Overwolf `<owadview>` ad format and the consent
flow through `tauri-plugin-overwolf`. The same page also runs on ow-electron,
so you can put the two hosts side by side and compare them. Pick a page, watch
the ads load, and read every event the ad elements raise in the timeline on
the right. It is for developers who want to see the plugin work before they
add ads to their own app, and for anyone who presents the project.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="../../docs/images/showcase/layouts-dark.webp">
  <img alt="The showcase in test mode on the Layouts page: the numbered page list on the left, the Tall Duo layout with a 160x600 test ad and a 400x600 slot playing a video test ad in the middle, and the event timeline with its counts on the right." src="../../docs/images/showcase/layouts-light.webp">
</picture>

To run it, start with [Run it in test mode](#run-it-in-test-mode). To present
it, read [Showing it to someone](#showing-it-to-someone).

## Run it in test mode

You need:

- Node.js 22.12 or newer;
- Rust 1.90 or newer and the rest of the
  [Tauri 2 prerequisites](https://tauri.app/start/prerequisites/) for your OS:
  the Xcode command line tools on macOS, the WebView2 Runtime and the MSVC
  build tools on Windows, WebKitGTK on Linux;
- Windows 10/11 or macOS 14 or newer to see ads. On Windows the ads need the
  WebView2 Runtime 98.0.1108.44 or newer. On Linux the app builds and runs,
  but ads report `unsupported`.

From a fresh clone, install at the repository root first, then run the
example:

```sh
git clone https://github.com/AlloryDante/ow-tauri
cd ow-tauri
npm install
npm run build --workspace tauri-plugin-overwolf-api
cd examples/ad-showcase
npm run start:tauri:test
```

The plugin is not on crates.io or npm yet, and the example does not need
them. It is an npm workspace, so `tauri-plugin-overwolf-api` links to
`packages/api`, and `src-tauri/` takes the plugin by path from
`crates/tauri-plugin-overwolf`. If `packages/api/dist` is missing, the stage
script builds it for you.

`start:tauri:test` stages the page, builds a debug app with the page embedded
(`tauri build --debug --no-bundle`) and runs the binary with `--test-ad`.
[scripts/run.mjs](scripts/run.mjs) does this on every OS (the binary is an
`.exe` on Windows, and `CARGO_TARGET_DIR` is honoured). Arguments after `--`
go to the app, and `--no-build` reuses the last binary:

```sh
npm run start:tauri:test -- --showcase-page=layouts/tower
node scripts/run.mjs --no-build -- --test-ad
```

For hot reload, run `npm run dev:tauri:test` instead. It uses `tauri dev`,
where the page comes from the Tauri CLI's dev server. That server stops when
the first process exits, so Restart in TEST/LIVE cannot work there: the app
refuses the restart and the banner tells you to use `npm run start:tauri`.

Stay on the `:test` scripts while you explore. They show Overwolf's test ads,
which fill every format. Live ads are covered in
[Test mode and live mode](#test-mode-and-live-mode).

## What you see

The window is 1280x860 (at least 1100x700), large enough for every format's
documented minimum.

- The top bar has the app name, a TEST or LIVE badge, the host and its
  version, the uid (masked; click it to reveal it on screen), a consent chip,
  the Theme button and Restart in LIVE or Restart in TEST. The consent chip
  reads `checking…`, `EU rules apply`, `not required` or `could not check`;
  its tooltip names `isCMPRequired()`.
- The sidebar lists nine pages. Keys `1` to `9` open them.
- The timeline on the right lists every event of every `<owadview>` and every
  action you take (`control:*` rows), with the time, the slot's `cid` and the
  time since the slot was created. This page (the default) shows the slots of
  the current visit plus the app and control rows; All pages shows
  everything. The counts at the bottom and the slot filter follow that
  choice. You can filter by slot or event family, pause, hide the rail and
  click a row to see its payload. Export JSON writes
  `<userData>/exports/timeline-<host>-<mode>-<time>.json`. Paths in the
  window and in the export show your home folder as `~`.

The pages follow, in sidebar order.

### 1. Sizes

The seven documented sizes (970x90, 728x90, 160x600, 400x600, 400x60,
400x300, 300x250), each with its own `cid`. Look at how a slot loads only
once at least half of it is in view.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="../../docs/images/showcase/sizes-dark.webp">
  <img alt="The Sizes page with the Towers and rectangles group: a 160x600 test ad, a 400x600 slot with a test banner and a video test ad, a 400x300 video test ad and a 300x250 test ad, each with its status chip." src="../../docs/images/showcase/sizes-light.webp">
</picture>

All seven do not fit one window, so the Show select picks a group: Towers
and rectangles (160x600, 400x600, 400x300, 300x250; the default), Banners
(970x90, 728x90, 400x60), Below the fold (a 300x250 in a scroll box that
loads only when you scroll it into view) or one size alone. Switching builds
fresh containers. The timeline folds away on this page to make room. In live
mode the groups leave out the 400x300, because a page may have only one
video container; you can still show it alone.

### 2. Layouts

Overwolf's eight recommended layouts at true size: Combo Classic, Tall Duo,
Tower Plus, Studio Tower, Tower, Studio, Studio Plus and PopUp Studio Plus.
Each has at most one video container, so this is the page to use in live
mode. Switching, or Recreate, builds fresh containers. The picture at the top
of this file shows Tall Duo.

### 3. High impact

A 440 px wide, full-height zone with Tower Plus (400x60 and 400x600, the
400x600 with `adstyle="high-impact-ad;"`). Watch the State chip: on
`high-impact-ad-loaded` the slot takes the whole zone and its sibling gets
`display: none`; on `high-impact-ad-removed` both come back.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="../../docs/images/showcase/high-impact-dark.webp">
  <img alt="The High impact page during a takeover: a test ad fills the 440 px zone on the left, the State chip reads Takeover, and the timeline ends with high-impact-ad-loaded." src="../../docs/images/showcase/high-impact-light.webp">
</picture>

### 4. Interstitial

Default, Red dim, Blur 3 and With unit each add a performance `<owadview>`
to the page. The Click me counter shows clicks passing through while the ad
loads and blocked once it is shown.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="../../docs/images/showcase/interstitial-dark.webp">
  <img alt="The Interstitial page with a test interstitial over the page and its close button, and performance_ad_loaded as the last timeline row." src="../../docs/images/showcase/interstitial-light.webp">
</picture>

The DOM panel shows the live `owadview` count and the element's
`pointer-events`. Shrink to 900x500, then add an interstitial, to see the
error path (`performance_ad_error`, then `shutdown`); Restore size undoes
it. Shrinking while an interstitial is already shown does not end it. A
no-fill ends with `shutdown` only: `performance_ad_no_fill` has not been
seen to fire.

### 5. Reward

A small coin shop. Press Watch ad · +100 coins and follow the steps from
Preloading to Granted +100.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="../../docs/images/showcase/reward-playing-dark.webp">
  <img alt="The Reward page while the rewarded video test ad plays: the shop shows 0 coins, with Preloading and Ready checked and Playing as the current step." src="../../docs/images/showcase/reward-playing-light.webp">
</picture>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="../../docs/images/showcase/reward-granted-dark.webp">
  <img alt="The Reward page after the grant: 100 coins, every step through Granted +100 checked, the slot hidden for the next preload and the 300x250 slot marked unavailable." src="../../docs/images/showcase/reward-granted-light.webp">
</picture>

The 400x300 `adstyle="rewarded-ad;"` slot preloads, hides on
`video_ad_ready`, and Watch ad shows it. On `complete` after a `play` of the
same cycle the coins are granted once, and the slot hides for the next
preload. Hide 2 s during play hides the slot while it plays. A second,
300x250 reward slot shows "unavailable" after 10 s without
`video_ad_ready`. The grant happens in the page: Overwolf documents no
server verification or postback for reward ads.

### 6. House

One 400x300 slot and a log of its `house_ad_action` and `house-ad-action`
events. House ads appear on no-fill once one is set up in the Dev Console
for the app's uid.

### 7. Controls

One 400x300 video slot with a `customTracking` editor, mute
(`setAudioMuted`), `display: none`, scroll out of view and back, hide the
window for 3 s and minimize it for 3 s. Each action is a `control:` row in
the timeline.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="../../docs/images/showcase/controls-dark.webp">
  <img alt="The Controls page with a playing 400x300 video test ad, the customTracking editor and the sound, visibility and window buttons." src="../../docs/images/showcase/controls-light.webp">
</picture>

### 8. Consent & identity

`isCMPRequired()`, a button that opens the ad privacy settings window,
`generateUserEmailHashes()` for the fixed address `player@example.com`, and
the identity table (uid, cuid and muid masked; host, version, platform and
test flag).

### 9. Parity

The parity harness's report (`parity-diff.json` copied to
`<userData>/parity-report.json`), or the commands that make it. Both hosts
read the same file, because they use the same userData folder for the same
product name. See
[Rerun the proof](../../tools/parity-harness/README.md#rerun-the-proof).

## Showing it to someone

Run the demo in normal, visible windows, ideally on Windows. macOS has a
known gap in the request headers of ad subresources
([ARCHITECTURE 6.6](../../docs/ARCHITECTURE.md#66-request-shaping)).

Before a demo that includes live ads, make sure that:

- Overwolf has enabled your demo uid for live ads;
- a house ad, with an event name, is set up in the Dev Console for that uid;
- Overwolf has qualified the uid, or attached a demo campaign, for high
  impact, interstitial and reward. Without that, show these three in test
  mode only.

Then:

1. Start `npm run start:electron:test` and `npm run start:tauri:test` side by
   side.
2. Open Sizes, then Layouts, and point at the timeline as the slots load.
3. Walk High impact, Interstitial, Reward, House and Controls on both
   hosts. Close the interstitial yourself.
4. On Consent & identity, show the consent chip, open the ad privacy
   settings and generate the email hashes.
5. Export both timelines and open Parity.
6. Press Restart in LIVE on both, with your own identity (below). Show
   Layouts filling, and a house ad on no-fill if one is set up. Say that high
   impact, interstitial and reward do not fill until Overwolf qualifies the
   uid; the timeline still shows each slot's ad page loading on both hosts.
7. Minimize the window: the ads hide. Close the app: no ad process is left.

Clicking an ad is for test mode only, and only when someone asks to see it:
each click opens one browser window and nothing crashes. Never click a live
ad.

Do not claim:

- in-stream ads (ow-electron has no API for them);
- macOS request-header parity for subresources and `x-ow-*` headers, or
  request shaping on Linux;
- a server-verified reward (none exists);
- that `performance_ad_no_fill` fires (ow-electron sends `shutdown` only).

## Test mode and live mode

- Test mode (`--test-ad`, TEST badge) uses Overwolf's test inventory. Every
  format fills, including high impact, interstitial and reward.
- Live mode (no `--test-ad`, LIVE badge) uses real demand for the app's uid.
  Standard display and video fill once Overwolf has enabled the uid, and a
  house ad fills on no-fill once one is set up. High impact, interstitial and
  reward do not fill until Overwolf qualifies the uid or attaches a demo
  campaign.

The mode follows the `--test-ad` switch on both hosts: the plugin reads it,
and `showcase_info` reports it to the page.

Restart in TEST or Restart in LIVE relaunches the app with or without
`--test-ad` and comes back to the same page and choice
(`--showcase-page=<page>[/<choice>]`, for example `layouts/tower` or
`sizes/300x250`; the same switch opens the app on that page). Going LIVE
asks you to confirm in a banner first. On Tauri the new process starts once
the old one has exited (`RunEvent::Exit`), after the plugin has sent its
pending analytics and the single-instance lock is free.

Live ads fill only for an app registered with Overwolf
([OVERWOLF-ONBOARDING.md](../../docs/OVERWOLF-ONBOARDING.md)). The tracked
[package.json](package.json) holds a placeholder identity
(`author: "Example Studio"`, `productName: "ow-tauri Ad Showcase"`), so a
clean clone runs with the formula uid
([CONTRACT G.2](../../docs/CONTRACT.md#g2-app-uid)) and test ads. For live
mode, give it your own app identity:

1. Copy [identity.example.json](identity.example.json) to
   `identity.local.json`, which is git-ignored, and fill in `author` and
   `productName` as registered with Overwolf. Set `uid` only when Overwolf
   assigned one in the console; it becomes `overwolf.uid`, which both hosts
   use as is.
2. Run any start script, or `npm run build`. `scripts/stage.mjs` merges the
   file into `.stage/package.json`, `.stage/electron/package.json` and
   `.stage/tauri.conf.json` (product name, version, and the `plugins.overwolf`
   author, name and uid). The Tauri scripts pass that config with `--config`.
   Nothing tracked changes, so `git status` stays clean.

To stage another file, run `node scripts/stage.mjs --host all --identity FILE`.
The stage script never prints the identity values. The window shows the uid,
cuid and muid masked (`abcd…wxyz`); Reveal shows one in full on screen only,
and timeline exports carry the masked uid.

Never click live ads. The LIVE confirmation banner says the same.

## Compare with ow-electron

The ow-electron twin answers the same page API as the Tauri app, over
Electron IPC with `app.overwolf`. Install ow-electron once, in the parity
harness folder (its own install, outside the workspace):

```sh
cd tools/parity-harness
npm install --workspaces=false
```

Then, from `examples/ad-showcase`:

```sh
npm run start:electron:test   # test ads
npm run start:electron        # live ads
```

Both stage the ow-electron app into `.stage/electron` and run it with
[scripts/ow-electron.mjs](scripts/ow-electron.mjs), which takes ow-electron
from the harness install. ow-electron downloads its runtime on first use.

## Scripts

Run these from `examples/ad-showcase`.

| Script                        | What it does                                                                                       |
| ----------------------------- | -------------------------------------------------------------------------------------------------- |
| `npm run start:tauri:test`    | stage, debug build with the page embedded, run it with `--test-ad` (test ads)                      |
| `npm run start:tauri`         | the same without `--test-ad` (live ads)                                                            |
| `npm run dev:tauri:test`      | `tauri dev` with hot reload and `--test-ad`; Restart is refused here                               |
| `npm run dev:tauri`           | the same without `--test-ad`                                                                       |
| `npm run start:electron:test` | stage, then `ow-electron --test-ad .stage/electron` (test ads)                                     |
| `npm run start:electron`      | the same without `--test-ad` (live ads)                                                            |
| `npm run build`               | stage both hosts (`.stage/electron`, `.stage/tauri`)                                               |
| `npm run stage`               | the same as `build`                                                                                |
| `npm run typecheck`           | `tsc`                                                                                              |
| `npm run lint`                | ESLint                                                                                             |
| `npm run test`                | vitest                                                                                             |
| `npm run check:rust`          | `cargo fmt --check`, and `cargo clippy -D warnings` with and without the `lab` feature             |
| `npm run lab:smoke`           | the invisible lab smoke run (macOS; see [e2e/README.md](e2e/README.md))                            |
| `npm run screenshots`         | the documentation images, in the invisible lab with test ads (see [Lab and tests](#lab-and-tests)) |

## How it is built

- `src/renderer/` is the page: plain TypeScript and CSS, no framework. It
  talks to its host only through `window.showcase` (`ShowcaseApi` in
  [src/shared/ipc.ts](src/shared/ipc.ts)).
- The Tauri app is [src-tauri/](src-tauri/) and [src/tauri/](src/tauri/), a
  plain Tauri 2 app. `src/tauri/install.ts` builds `window.showcase` from
  [`tauri-plugin-overwolf-api`](../../packages/api/README.md) (consent,
  email hashes) and the app's own commands
  ([src-tauri/src/showcase.rs](src-tauri/src/showcase.rs): host info,
  exports, the parity report, restart, window actions). It imports
  `tauri-plugin-overwolf-api/adview` for the `<owadview>` element. The window
  is a normal `WebviewWindow` labelled `main`. Its capability
  ([src-tauri/capabilities/default.json](src-tauri/capabilities/default.json))
  grants `overwolf:default` and `overwolf:email-hashes`.
- The ow-electron twin is [src/main/main.ts](src/main/main.ts) and
  [src/preload/preload.ts](src/preload/preload.ts).
  [types/electron.d.ts](types/electron.d.ts) types the part of `electron` the
  twin uses. The showcase runs ow-electron from the harness install instead
  of depending on it, so the workspace install does not download the
  Electron runtime.
- One rolldown config ([rolldown.config.mjs](rolldown.config.mjs)) builds
  both hosts, and [scripts/stage.mjs](scripts/stage.mjs) writes them to
  `.stage/` (git-ignored).
- `src-tauri/` is its own Cargo workspace with its own `Cargo.lock`, like
  the packages sample.

## Lab and tests

`npm run typecheck`, `npm run lint`, `npm run test` and `npm run check:rust`
are the checks to run before you commit.

`e2e/` drives the showcase headlessly on both hosts: macOS, invisible
windows, test ads. See [e2e/README.md](e2e/README.md). The Tauri app has the
lab only with its `lab` Cargo feature, which is off by default.

`npm run screenshots` records the documentation images. It runs the lab
tour with stills in the dark and the light theme, with test ads only and the
tracked placeholder identity. It refuses to use `identity.local.json` and
checks that the app reported the placeholder's formula uid. Output goes to
`e2e/out/screenshots/<theme>/` (git-ignored); `--out DIR` also copies each
still to `DIR/<still>-<theme>.png`. See
[scripts/screenshots.mjs](scripts/screenshots.mjs).

For results that compare the two hosts, see
[docs/PARITY.md](../../docs/PARITY.md).

## Troubleshooting

- Restart does nothing, or the restart banner says it needs a built app. You
  are under `tauri dev`. Use `npm run start:tauri:test`.
- `ow-electron is not installed`. Run
  `cd tools/parity-harness && npm install --workspaces=false` once. If
  install scripts are blocked, run
  `node node_modules/@overwolf/ow-electron/install.js` in that folder.
- Nothing fills in live mode. Overwolf has not enabled the uid yet, or the
  format needs qualification. Use the `:test` scripts.
- The 970x90 test creative is often blank. It is blank on ow-electron too.
- An interstitial ends with `performance_ad_error` and `shutdown`. The
  window is smaller than the ad's minimum; press Restore size.

For problems with the plugin itself, see
[docs/TROUBLESHOOTING.md](../../docs/TROUBLESHOOTING.md).
