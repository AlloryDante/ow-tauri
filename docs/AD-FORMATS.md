# Ad formats

How to show each Overwolf ad format from an app on ow-tauri, what events to
expect, and what not to expect. The app code is the same as on ow-electron:
every format is an `<owadview>` element, and ow-tauri passes its attributes
to Overwolf's ad page unchanged, so the ad page picks the format exactly as it
does on ow-electron.

Sources: Overwolf's advertising documentation
(<https://dev.overwolf.com/ow-electron/monetization/advertising/overview> and
the pages under it), the official sample app, and what ow-electron 42.11.4
was observed to do in the parity lab ([PARITY.md](PARITY.md#ad-formats)).
The wire-level rules are in [CONTRACT.md](CONTRACT.md) section B.3 (the
element) and section D (the ad guest). The
[ad showcase](../examples/ad-showcase/README.md) shows every format on both
hosts with the same HTML and JavaScript.

## At a glance

| Format | How the app asks for it | Main events | Test mode | Live mode |
|---|---|---|---|---|
| [Standard display](#standard-display) | `<owadview>` in a sized container | `display_ad_loaded` (twice per fill) | fills, all seven sizes | fills once Overwolf enables the uid |
| [Standard video](#standard-video) | the same, in a 400x300 or 400x600 container | `player_loaded`, `play`, `impression`, `complete` | plays a test video | as standard display |
| [House ads](#house-ads) | nothing in code; set up in the Dev Console | `house_ad_action` / `house-ad-action` | not served | fills on no-fill once set up |
| [High impact](#high-impact) | `adstyle="high-impact-ad;"` on one slot of a 440 px ad zone | `high-impact-ad-loaded`, `high-impact-ad-removed` | fills | demand-gated |
| [Interstitial (performance)](#interstitial-performance-ads) | `<owadview performance>` appended to `<body>` | `performance_ad_loaded`, `shutdown` | fills | demand-gated |
| [Reward](#reward) | `adstyle="rewarded-ad;"` on a slot of at least 400x300 | `video_ad_ready`, `play`, `complete` | plays a test video | demand-gated |
| [In-stream](#what-not-to-expect) | no `<owadview>` API | none | none | none |

"Demand-gated" means the format comes from direct deals after Overwolf's
DevRel team qualifies the app. An unqualified app gets no live fill for it on
either host; test mode shows it.

Per platform: Windows and macOS are verified in the parity lab against
ow-electron (test mode, every format). macOS has a request-header gap
([CONTRACT.md](CONTRACT.md) D.8.3) that can lower live fill. On Linux the
ad guests cannot overlap the page (tauri-runtime-wry packs child webviews
side by side), so ad positions, the interstitial overlay and its input
pass-through do not work there yet.

## Before you start

### The element

```js
const container = document.querySelector('#tower'); // a div sized 400x600
const ad = document.createElement('owadview');
ad.setAttribute('cid', 'main-tower'); // at most 20 characters, unique per window
ad.setAttribute('slotsize', '400x600'); // optional
container.appendChild(ad);
ad.addEventListener('display_ad_loaded', () => console.log('filled'));
```

- Size the **container**, not the element. The element fills its container
  by default (`display: inline-flex; width: 100%; height: 100%` with zero
  specificity, so any rule of yours wins). A sized style on the element
  itself changes the slot the ad page reports.
- Events are plain `Event`s dispatched on the element. They do not bubble.
  Their data is copied onto the event as own properties
  (`event.ad_uid`, not `event.detail.ad_uid`), the way `Object.assign` copies
  it: a string payload, such as the one `performance_ad_error` carries, is
  spread into `event[0]`, `event[1]`, ...
- `customTracking` (attribute or property) takes a JSON string; a change
  reaches the running ad page.
- Guests start muted. `ad.setAudioMuted(false)` unmutes one.
- `ad.reload()`, `ad.setPageUrl(url)` and `ad.sendCommand(...args)` exist as
  on ow-electron. The last two are passed to the ad page as messages; the
  test ad page does nothing visible with them.
- With TypeScript, `document.createElement('owadview')` is typed as
  `overwolf.AdviewTag` once `ow-tauri/types` is in your `types`.

### Lifecycle rules

- **Do not recycle elements.** An element that was attached and is then
  removed, or moved to another parent, is dead: its ad closes and it never
  attaches again, on ow-electron and on ow-tauri. Create a new element
  instead. A moved element gets a plain `destroyed` event once its ad has
  closed; an element removed for good hears nothing.
- **Hide with `display: none`.** A hidden slot is paused, not destroyed, and
  resumes when shown again. An ad hidden for longer than a few seconds
  reloads itself when it comes back (the ad page decides this; a 2 s hide
  keeps the ad).
- A slot loads only while it is in view: in a shown window, at least half of
  it inside the viewport (measured on ow-electron: 50 % loads, 49 % waits,
  vertically and horizontally; ow-tauri draws the same line), and not hidden
  by CSS. A window that is never
  shown never fills, even in test mode.
- Changing `cid`, `slotsize`, `adstyle`, `performance` or `unit` on a live
  element replaces its ad. `pageurl` applies at the next load.

### Test mode and live mode

Live ads are the default, as on ow-electron. Test mode serves Overwolf's test
inventory for every format; the request shaping, consent and analytics are
the same in both modes.

| Switch | Where |
|---|---|
| `--test-ad` on the command line | both hosts; with Tauri, `tauri dev -- -- --test-ad` |
| `OW_TAURI_TEST_AD=1` | ow-tauri |
| `"plugins": { "overwolf": { "ads": { "testAd": true } } }` in `tauri.conf.json` | ow-tauri |
| `tauri_plugin_overwolf::Builder::new().test_ad(true)` | ow-tauri |
| `localStorage.owAdTestAd = true` in the ad page's origin (`https://www.overwolf.com`) | the ad page itself; both hosts give the same result in test mode |

Every attribute, `unit` included, reaches the ad page unchanged in both
modes.

Live fill needs:

- Overwolf to have enabled the app's uid for ads;
- consent handled (ow-tauri runs Overwolf's consent window exactly as
  ow-electron does);
- the window shown and on screen (a window moved off-screen gets no live
  fill);
- for high impact, interstitial and reward, a qualified app (above).

### Policy reminders

From Overwolf's advertising documentation: keep containers visible and do
not move ads or make them transparent; at most one video-capable container
per page; a unique `cid` when a window has two identical units; never fake
impressions or reload pages to get more ads. Overwolf's own QA step clicks
an ad five times and expects five browser windows: each user gesture in an
ad opens one browser window, up to 20 per ad per minute on ow-tauri.

## Standard display

Put an `<owadview>` in a container of one of the documented sizes:

| Container | Creatives it can show | Video |
|---|---|---|
| 400x300 | 336x280, 300x250, 250x250 | yes |
| 400x600 | 336x280, 300x600, 300x250, 250x250 | yes |
| 300x250 | 300x250, 250x250 | no |
| 160x600 | 160x600, 120x600 | no |
| 728x90 | 728x90, 468x60, 234x60, 320x50, 300x50, 400x60 | no |
| 970x90 | the 728x90 set plus 970x90 | no |
| 400x60 | 400x60 | no |

Overwolf's recommended layouts (Combo Classic, Tall Duo, Tower Plus, Studio
Tower, Tower, Studio, Studio Plus, PopUp Studio Plus) combine these; the
showcase's Layouts page builds all eight.

Events: `display_ad_loaded` arrives **twice** per fill, and the slot
refreshes about every 30 s. `impression` arrives with the bid details
(`pos`, `cpm`, `bidder`, ...). A slot with no ad is transparent: your
container's own background shows through, so a fallback image or colour
behind the slot works as on ow-electron.

Test mode: all seven sizes fill, 970x90 included. Its test creative is often
blank: `display_ad_loaded` fires and the guest paints nothing, on ow-electron
(its in-process capture of the guest is fully transparent) as on ow-tauri, so
an empty 970x90 in test mode is the creative, not the host [OBS: showcase
lab, both hosts]. A slot below the fold waits until it is scrolled into view.

## Standard video

The 400x300 and 400x600 containers can play outstream video; the ad page
decides when. Events, in order: `player_loaded` (`sourceIsOwAdIframe`,
`ad_uid`, `isPlayerEvent`), `play`, `impression`, then `complete` 40 to 70 s
later with the test creative. The guest starts muted; call
`setAudioMuted(false)` to let the user hear it. Keep one video-capable
container per page in live mode.

## House ads

Your own promotions, shown when no paid ad fills. There is nothing to add in
code: set them up in the Dev Console (images, an optional link, an optional
event name). When an event name is set, the element receives
`house_ad_action` and `house-ad-action`, both with `{ action: '<event
name>' }`; listen to either. A house ad's link opens in the system browser
like any ad click, and the element gets `ad-clicked`.

Test mode does not serve house ads (the configuration request goes out and
returns nothing for an app with none set up; whether test mode can ever serve
one is [OQ-A4](OPEN-QUESTIONS.md#oq-a4-house-ads-in-test-mode)). House ads
do not show while an ad blocker is active.

## High impact

A takeover of an ad zone. Build a zone 440 px wide and at least 670 px tall
(the full window height), holding the containers of a layout such as Tower
Plus (a 400x60 and a 400x600). Give exactly one element
`adstyle="high-impact-ad;"`:

```js
const big = document.createElement('owadview');
big.setAttribute('cid', 'hi-tower');
big.setAttribute('adstyle', 'high-impact-ad;');
towerContainer.appendChild(big);

big.addEventListener('high-impact-ad-loaded', () => {
  towerContainer.classList.add('takeover'); // 100% x 100% of the zone
  bannerContainer.style.display = 'none'; // hide the siblings
});
big.addEventListener('high-impact-ad-removed', () => {
  towerContainer.classList.remove('takeover');
  bannerContainer.style.display = '';
});
```

Events: `display_ad_loaded` twice, then `high-impact-ad-loaded`
(`sourceIsOwAdIframe`, `isOwAdOrigin`, `ad_uid`), then
`high-impact-ad-removed` (`sourceIsOwAdIframe`, `ad_uid`) about 15 s later.
After that the slot is a normal 400x600. The ad follows the container as it
grows; nothing is remounted.

**Hide siblings with `display: none`.** Overwolf's page suggests removing the
small container and appending it again; on ow-electron 42.11.4 that kills
the small slot for good, and ow-tauri does the same
([OQ-A9](OPEN-QUESTIONS.md#oq-a9-re-appending-a-removed-element)).

Live: demand-gated.

## Interstitial (performance) ads

A full-window ad above the app. Overwolf now calls it "interstitial"; the
attribute is still `performance`:

```js
const ad = document.createElement('owadview');
ad.setAttribute('performance', '');
// optional: the dim behind the creative; "background-blur: -1" turns blur off
ad.setAttribute('adstyle', 'background-color: rgba(0, 0, 0, 0.6); background-blur: -1;');
// optional: ad.setAttribute('unit', '<unit>');
document.body.appendChild(ad);

ad.addEventListener('performance_ad_loaded', () => pauseTheApp());
ad.addEventListener('shutdown', () => resumeTheApp()); // the element is removed right after
```

What happens:

1. The element takes no room in the page. While the ad loads, the page under
   it keeps taking clicks (the element is `pointer-events: none`).
2. `display_ad_loaded` (twice), then `performance_ad_loaded`. From that
   event the ad is modal: it covers the window, takes the input and stays
   until the user closes it. There is no timeout.
3. When it ends, `shutdown` fires and the element is removed from the
   document right after your `shutdown` listeners ran. Create a new element
   for the next interstitial.

Other paths:

- **No fill** (live without demand, or an unknown `unit`): `shutdown` alone,
  about 2 to 3 s after the element was added. Treat "`shutdown` without
  `performance_ad_loaded`" as no fill.
- **Window too small**: under 500 x 500 the ad page sends
  `performance_ad_error` (a string payload, spread per character), then
  `shutdown`. Overwolf asks for a window of at least 1000 x 600; the host
  enforces neither size.
- **A second interstitial** while one is up is removed at once, with no ad
  and no event.
- **Minimize**: the ad may dismiss itself (`performance_ad_dismiss`), then
  shuts down. ow-electron varies here too.
- Documented events that need a user's click and were never seen in the lab:
  `performance_ad_dismiss` (on a close), `performance_ad_clicked`,
  `performance_ad_video_complete`, `performance_ad_video_skipped`.

`adstyle` sets the dim colour and blur; without it the ad page uses
`rgba(0, 0, 0, 0.4)` and blur 2. The blur cannot blur your app's content on
either host. Overwolf also asks: no other ad loading in the background, and
never during gameplay. Live: demand-gated.

## Reward

A video the user chooses to watch for an in-app reward. ow-electron 42.11.4
gives the rewarded flow to a normal slot whose `adstyle` contains
`rewarded-ad;`, in a container of at least 400x300:

```js
const slot = document.createElement('owadview');
slot.setAttribute('cid', 'reward');
slot.setAttribute('adstyle', 'rewarded-ad;');
rewardContainer.appendChild(slot); // 400x300 or larger, visible
```

Events: `video_ad_ready` (`readyAds`, `ad_uid`) once a video is preloaded,
then `player_loaded`. **Nothing plays while the slot just stays visible.**
Playback starts when the slot goes hidden and then visible again: then
`play`, `impression`, and about 41 s later `complete`; a new
`video_ad_ready` follows a few seconds after. There is no reward, close or
skip event: `complete` is the only signal.

### Granting the reward

```js
let ready = false;
let playedSinceComplete = false;

slot.addEventListener('video_ad_ready', () => {
  ready = true;
  rewardContainer.style.display = 'none'; // park it hidden
  watchButton.disabled = false;
});
watchButton.addEventListener('click', () => {
  if (!ready) return;
  ready = false;
  watchButton.disabled = true;
  rewardContainer.style.display = ''; // hidden -> visible starts the video
});
slot.addEventListener('play', () => {
  playedSinceComplete = true;
});
slot.addEventListener('complete', () => {
  if (!playedSinceComplete) return;
  playedSinceComplete = false;
  grantReward(); // once per play
  rewardContainer.style.display = 'none'; // ready for the next preload
});
```

- Grant on `complete` from the rewarded element, and only after a `play`
  from that element since its last `complete`; grant at most once per play.
- `ad_uid` stays the same across cycles of one slot, so it is not a per-ad
  key.
- The grant is client-side only. Overwolf documents no server-side
  verification or postback, and none was observed, so it cannot stop a
  determined cheater.
- A slot smaller than 400x300 gets no event at all. Treat "no
  `video_ad_ready` within N seconds" (the showcase uses 10 s) as "reward
  unavailable".
- Show the slot only on a real user action. Never script hide and show
  cycles to force plays: Overwolf's policy forbids faking impressions.
- Hiding and showing the slot during playback does not restart the video.
- The match is a substring: `rewarded-ads;` also works, `rewarded;` and
  `reward-ad;` do not.

An archived version of Overwolf's documentation describes reward ads as the
`performance` element with a reward unit chosen on Overwolf's side. ow-tauri
passes that through too, unchanged. Which path Overwolf supports, and
whether a server-side check exists, is
[OQ-A1](OPEN-QUESTIONS.md#oq-a1-reward-ads). Live: demand-gated.

## Events

Every event the ad page sends is dispatched on the element under its own
name, in order; the list below is what was observed or documented.

| Event | Formats | Own properties |
|---|---|---|
| `display_ad_loaded` | display, high impact, interstitial | none (twice per fill) |
| `impression` | display, video, reward | `pos`, `cpm`, `bidder`, ... |
| `player_loaded`, `play`, `pause`, `ended`, `complete` | video, reward | `ad_uid` and others |
| `video_ad_ready` | reward | `readyAds`, `ad_uid` |
| `high-impact-ad-loaded`, `high-impact-ad-removed` | high impact | `sourceIsOwAdIframe`, `ad_uid` |
| `performance_ad_loaded`, `shutdown` | interstitial | none |
| `performance_ad_error` | interstitial | a string, spread per character |
| `performance_ad_dismiss`, `performance_ad_clicked`, `performance_ad_video_complete`, `performance_ad_video_skipped` | interstitial (documented) | |
| `house_ad_action` and `house-ad-action` | house | `action` |
| `ad-clicked` and `ad_clicked` | any | `url` (host) |
| `did-attach`, `dom-ready`, `did-finish-load`, `did-fail-load`, `render-process-gone`, `destroyed` | any (lifecycle) | per [CONTRACT.md](CONTRACT.md) B.3.5 |

## What not to expect

- **In-stream ads.** Overwolf documents them only for ow-native's `OwAd`;
  `<owadview>` has no attribute or method for them, on ow-electron or
  ow-tauri ([OQ-A7](OPEN-QUESTIONS.md#oq-a7-in-stream-ads)).
- **A server-verified reward.** None exists, documented or observed.
- **`performance_ad_no_fill`.** It is documented, but a no-fill sends
  `shutdown` only.
- **Live fill of high impact, interstitial or reward** before Overwolf
  qualifies the app
  ([OQ-A10](OPEN-QUESTIONS.md#oq-a10-live-demand-for-demand-gated-formats)).
- **House ads in test mode.**
- **HTML above an ad.** Ads are native webviews and paint above the page.
  Hide the element (or an ancestor) to show a menu or dialog over it; the
  runtime hides the ad with it. CSS transforms and `clip-path` on ancestors
  do not move or clip the ad.
- **Linux overlays.** On Linux the ad guests cannot overlap the page yet, so
  positions, the interstitial overlay and its pass-through do not work
  there.
- **Identical macOS request headers.** WebKit offers no public way to add
  ow-electron's subresource headers ([CONTRACT.md](CONTRACT.md) D.8.3);
  Windows matches.

## Open questions with Overwolf

[OPEN-QUESTIONS.md](OPEN-QUESTIONS.md#ad-formats) lists them: the reward
path and its verification (OQ-A1), valid `unit` values (OQ-A2), whether a
dismissed or clicked interstitial always ends with `shutdown` (OQ-A3), house
ads in test mode (OQ-A4), `owAdTestAd` (OQ-A5), interstitial close-button
colours (OQ-A6), in-stream (OQ-A7), the high-impact re-append (OQ-A9) and
live demand for the gated formats (OQ-A10).
