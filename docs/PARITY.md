# Parity with ow-electron

ow-tauri's rule is: **replicate how ow-electron does it.** This document
defines what parity means, how it is proven, where it stands per behaviour,
and how anyone, Overwolf included, can re-run the proof.

Related: the wire-level specification is [CONTRACT.md](CONTRACT.md); the
questions behind each decision, with their sources, are in
[OPEN-QUESTIONS.md](OPEN-QUESTIONS.md).

## What parity means

ow-tauri is at parity with ow-electron when Overwolf's services and pages
cannot tell the difference between an app running on ow-tauri and the same
app running on ow-electron, except where ow-tauri says it is a Tauri host.

Concretely, for the same `package.json` on the same machine:

1. **Identity is identical**: uid, computed uid, machine id (`muid`,
   `muidV2`) and phase percent (CONTRACT G.2, E.4).
2. **Requests are identical**: every request the host makes (analytics, the
   consent feature flag) and every request the ad guest's document makes has
   the same URL, query (keys, order, encoding), body, header set and order,
   and cookies (CONTRACT E.1, D.8).
3. **Page inputs are identical**: the ad page sees the same
   `window.__overwolf__` (keys, order, types, values), user agent shape,
   `document.referrer` and cookies; the consent page sees the same URL, query
   and globals (CONTRACT D.2, D.6).
4. **Files are identical**: `ow-electron.json` has the same bytes after the
   same sequence of launches (CONTRACT F.2).
5. **Timing and order match** where they are observable: launch analytics
   order, consent cookies before the first ad document, one consent window
   per launch (CONTRACT E.2, D.6).
6. **Public API results match**: each function of the plugin's API returns
   what the same `app.overwolf` call returns in ow-electron, including error
   messages (CONTRACT A.2.2, B.1).

The one deliberate difference is the **host label**: where ow-electron names
itself (`electron_*` event names, the `42.11.4` version, the `Electron/` UA
token), ow-tauri says `tauri` (CONTRACT section 0,
[ADR 0006](adr/0006-analytics-labelling.md)). It is one setting, so Overwolf
can change it. Every other known difference is listed under
[Deviations](#deviations), with its reason.

Parity is measured against a fixed **baseline**: ow-electron 42.11.4 (the
`latest` dist-tag on 2026-10-06; Electron 42.11.4, Chrome 148.0.7778.280),
observed on macOS arm64 and, through the Windows lab on CI, on Windows
Server 2025. A new ow-electron release means a new baseline run
([Re-running the harness](#re-running-the-harness)).

## How parity is proven

Two halves, diffed field by field:

1. **The ow-electron side: the parity harness** (`tools/parity-harness`). A
   small ow-electron app plus scripts that observe `@overwolf/ow-electron` as
   a black box through Electron's public APIs: Chromium's net log, the
   DevTools protocol, `session.webRequest`, cookie change events, the files
   on disk, and the values `app.overwolf` and the guests expose. It never
   modifies ow-electron. Facts it establishes are tagged [OBS] ("matches
   ow-electron (observed)") in CONTRACT. Its README lists every capture file
   and option.
2. **The ow-tauri side: the Tauri edition of the harness**
   (`tools/parity-harness/tauri-app`, `node run.mjs --host tauri`). The same
   harness app on ow-tauri: a minimal Tauri app whose Rust driver
   (`tauri-app/src-tauri/src/driver.rs`) runs each scenario through the
   plugin's Rust API (`app.overwolf()`) and, in the ad window's page, its
   JavaScript API, step by step as the ow-electron app does.
   The plugin's `lab` Cargo feature (off by default,
   never shipped) writes a trace in the ow-electron capture shapes: host
   requests as sent, guest requests (on Windows through WebView2's
   `Network.requestWillBeSent` events), cookies, the state file, element
   events, host-to-guest messages, and the native state of each guest
   (`transparent`, `zorder` and `passthrough` records, with `-native`
   variants read back from the platform). Each run uses the same identity
   and scenario as its ow-electron baseline, inside an isolated home
   directory.
3. **The diff: `parity-diff.mjs`.** It compares an ow-electron capture with
   an ow-tauri capture of the same scenario, after normalising volatile
   values, and classes every difference: `intended:host-label` (the
   labelling rule), `intended:os-gap` (a documented platform gap),
   `intended:optimised` ([Optimised, same outcome](#optimised-same-outcome)),
   `intended:deviation` ([Deviations](#deviations)), `variance` (it differs
   between two ow-electron runs too: ad content, HTTP cache, playback speed),
   `not-mirrored` (a harness step the Tauri edition cannot run), or `BUG`
   (anything else). A run passes when no `BUG` remains. It refuses two
   captures whose scenario definition, layouts or mode differ, since every
   difference of definition would read as a `BUG`. Ad-format runs are
   also compared on per-element event names, order, counts and payload keys,
   DOM state and removal timings, the ad library's options on the wire, guest
   bounds, z-order, pass-through and mute timelines
   ([harness README](../tools/parity-harness/README.md)).

Safety rules for both halves:

- No visible windows. Windows that must fill ads are shown at alpha 0,
  inside the screen bounds (live demand does not fill an off-screen window),
  ignore the mouse, are not focusable and stay off the taskbar; a window that
  is never shown never fills, in ow-electron and in ow-tauri. The dock icon
  is hidden. A window monitor kills a run the moment any window becomes
  visible, and a front-app monitor fails a run whose app ever became
  frontmost. Nothing captures the screen. The Windows lab runs only on a CI
  runner, whose desktop is nobody's screen.
- No input is ever sent to an ad. Ads are never clicked. A pass-through
  probe may send one click to the app's own control in test mode, and only
  when the native hit test there names the app's webview.
- Test ads by default. Live loads only when a run opts in, at most 10 per
  run, each one logged.
- Each run uses an isolated home directory, so the real profile, consent and
  cookies are never touched. Offline probes (the uid matrix) cannot reach the
  network.
- Raw machine identifiers are never printed; the machine-id probe reports
  only which derivation matched.

### Scenarios

| Scenario | What it establishes |
|---|---|
| Test ads, window shown at opacity 0, the seven documented slot sizes, 90 s, close then quit | launch analytics and order, ad document and subresource shaping, `__overwolf__`, element events, consent window and cookies, `window_closed` |
| Same, window never shown | hidden embedders never fill; no `window_closed`, no second heartbeat |
| Live ads, at most 10 loads, never clicked | the wire shape is the same as test mode |
| First launch, then second launch on one profile | `firstLaunch`, consent reuse, `timeStamp` refresh, cookies rewritten, `unifiedcmp` encoding |
| 360 s session, quit with the window open | no periodic heartbeat within 6 minutes; `window_closed` at quit |
| With `session.webRequest` hooks installed | the app cannot remove the shaping |
| `overwolf.packages: ["gep", "overlay"]` | the packages manager on a host without packages |
| `disableAnonymousAnalytics()` at module load | the mandatory analytics set |
| uid matrix (40 `package.json` variants, offline) | the uid rule, checked against `ow client calc-electron-uid` |
| machine-id probe and stand-in platform id | the muid derivation and phase percent |
| `messages` (two test slots, timed actions) | host-to-guest messages, guest-to-host channels, visibility signals for `display: none`, scrolling and window hide, email hashes, `setExternalPaymentUserId` |
| `crash` | guest crash recovery, crash analytics, the quick re-crash case |
| `block` | main-frame load failures and the retry interval |
| `cmp` | the consent settings window and the default-consent window, pinned invisible |
| `cmp-required`, with a local feature-flag stand-in | `isCMPRequired()` caching, response variants, a hung request |
| `windows`, `windows-urls` (offline) | window analytics names and `window_closed` triggers |
| `offscreen` | test and live fill with the window off-screen or on-screen at opacity 0 |
| `packages`, `introspect` | the package manager surface; the element's members after attach |
| `--overwolf-uid` | `app_id` and `app_cuid` with a console-assigned uid |
| `long` (13 hours, hidden) | the periodic heartbeat cadence (R2-9) |
| `sizes`, `tower-plus` | the seven standard containers and a recommended layout: events, the ad library's options, refresh |
| `high-impact`, `high-impact-small-zone`, `high-impact-only` | the high-impact zone, its events and the container growth |
| `perf`, `perf-sample`, `perf-unit`, `perf-small`, `perf-twice`, `perf-remove`, `perf-with-standard`, `perf-minimize` | the interstitial (performance) element: DOM shape, input, events, error and no-fill paths, one per window, removal, stacking over a standard slot, minimize |
| `reward`, `reward-two-slots`, `reward-visibility-probe`, `reward-play-probe`, `reward-optin` | the rewarded flow: preload, opt-in through hide then show (1 frame, 50 ms, 500 ms, 2 s), play, `complete` |
| `house`, `adstyle-probe`, `send-command-probe`, `owadtestad` | house-ad configuration, which `adstyle` values switch the ad library, `sendCommand` / `setPageUrl` delivery, `localStorage.owAdTestAd` |
| `lab-layers`, `audio`, `standard-remove` | the [ad-format lab checks](#ad-format-lab-checks): transparency, z-order, pass-through, blur, mute, removal |

## Parity matrix

Status values:

- **Target**: the ow-electron behaviour is settled and CONTRACT specifies the
  same; the Tauri lab diff ([Lab checks](#lab-checks)) confirms it per platform.
- **Gap**: the platform cannot reproduce it with public API; CONTRACT names
  the fallback, and Overwolf is asked whether the gap is acceptable.
- **R2-n / R3-n**: a harness item ([Harness rounds](#harness-rounds)) still
  has to observe part of it; CONTRACT gives the interim behaviour.
- **Deviation**: ow-tauri differs on purpose ([Deviations](#deviations)).
- **Optimised**: ow-tauri gets the same outcome another way
  ([Optimised, same outcome](#optimised-same-outcome)).
- **Overwolf**: needs an answer only Overwolf can give.
- **Deferred**: outside the current scope (packages).

### Identity and files

| Behaviour | ow-electron (observed) | ow-tauri target | CONTRACT | Status |
|---|---|---|---|---|
| uid rule | `sha1` of `{'author':...,'name':'<productName or name>.electron'}`, verbatim author, `"unknown"` fallback | identical; 13 test vectors | G.2 | Target |
| `overwolf.uid` in `package.json` | used verbatim | identical | G.2 | Target |
| `app_cuid` when `overwolf.uid` is set | override as `app_id`, computed uid as `app_cuid` | identical | G.2 | Target |
| muid, macOS | `guid(sha256(lowercase(IOPlatformUUID)))`, `muidV2 = muid` | identical; 4 test vectors | E.4 | Target |
| muid, Windows | `MUID` (machine) in analytics and guests; `MUIDV2` a random v4 per install, also `app.overwolf.muid` | registry values shared, else derived and written | E.4 | Target (Windows lab) |
| muid, Linux | not observed | derived from `/etc/machine-id` | E.4 | R2-10 |
| phase percent | MD5 character-code sum of the muid, modulo 100 | identical | E.4 | Target |
| `ow-electron.json` | `firstLaunch`, `cmp` with URL-encoded unified string and seconds `timeStamp`, then `eHashes` once the app set email hashes; nothing else in the directory | identical bytes; ow-tauri's own options in `ow-tauri.json` beside it | F.2, F.3 | Target; Optimised (`ow-tauri.json`) |
| `utmParams` absent | `undefined` | `undefined` | F.2 | Target |
| logs | none written; `logger.enabled: false` | no log files; the plugin logs through the `log` crate | F.1 | Target |

### Analytics

| Behaviour | ow-electron (observed) | ow-tauri target | CONTRACT | Status |
|---|---|---|---|---|
| launch sequence | first launch, `cmp-eu-only`, start, heartbeat, 400022, 400023, within about 100 ms | identical order, from `main_ready` | E.2 | Target |
| first-show heartbeat | Counter + 400023 when the first window shows, before the 400025 of a guest attached after the show | identical | E.2 | Target |
| ad guest Kind | 400025 per guest | identical | E.2 | Target |
| `window_closed` | one per visible period (`hide()`, close, quit), nothing under 1 s; constructor `title`; `length` rounded down; never for unshown windows or the consent settings window | identical | E.2 | Target |
| window analytics name | last path segment of the URL at first show, `.html` dropped, sanitised, not truncated; the `name` option is ignored | identical | E.2 | Target |
| guest crash event | Counter `sessionTS`, `reason`; Kind 400024 with `reason` first | identical | E.1, E.2 | Target |
| crash report suppression | a crash 2 s after a recovery is not reported; threshold unknown | no report under 10 s | E.2 | R3-4 |
| periodic heartbeat | in a 13-hour hidden session: nothing after the launch burst for 12 h, then one Counter (`hasVisibleWindow: false`) and one 400023, nothing in the next hour | an hourly check that sends 12 h after the last heartbeat | E.2 | Target (R2-9); a 12 h timer against an hourly check, and a reset by the first-show heartbeat, not settled |
| Counter query and `Extra` | key order, URLSearchParams encoding, `os_ver` from `os.release()` | identical | E.1 | Target |
| InsertStats body | `Kind` + positional `Extra`, no event fields | identical | E.1 | Target |
| host request headers | fetch-metadata headers, `priority: u=4, i`, no `accept`, no Origin or Referer, `content-length` first on InsertStats, no cookies sent or stored | identical (its own HTTP client) | E.1 | Target |
| HTTP cache | a URL Chromium's cache holds is revalidated (`if-none-match`, 304) | no HTTP cache: the plain request, same server outcome | E.1, E.2 | Optimised |
| user agent | Chromium UA with `<PNNS>/<ver>` and `Electron/42.11.4` | same shape, `Tauri/<tv>`; on WKWebView Safari's `Version/` and `Safari/` tokens are added where Electron keeps Chromium's; engine never faked | E.1 | Target (label) |
| `owver` | `42.11.4`, `42_11_4` | `tauri-<tv>`, `tauri-<tv with _>` | 0 | Target (label); Overwolf (dashboards) |
| event names | `electron_*` | `tauri_*` | 0 | Target (label); Overwolf (dashboards) |
| `disableAnonymousAnalytics()` | keeps first-launch Counter and both heartbeats | identical | E.3 | Target |
| `setExternalPaymentUserId` | Counter `electron_sub_info`, options appended, `providerName` defaulted; resolves after the response | identical, `tauri_sub_info` | A.2.2, E.2 | Target (label); R3-3 (with analytics disabled) |
| uninstall event | `ow_electron_app_uninstall` from the NSIS uninstaller (builder) | `ow_tauri_app_uninstall` from Tauri NSIS hooks | I.6 | Target (label) |

### Consent

| Behaviour | ow-electron (observed) | ow-tauri target | CONTRACT | Status |
|---|---|---|---|---|
| startup consent window | hidden 1 x 32 window on `ow-cmp-v2.html` every launch, created when the `cmp-eu-only` request completes, query in a fixed order, closes itself | identical | D.6.1 | Target |
| `unifiedcmp` | empty, or the stored value URL-encoded once more | identical | D.6.1 | Target |
| skipped when consent not required | cannot be observed (every response gave `true`) | always opened | D.6.1 | Overwolf |
| consent page globals | `cmp.saveConsent`, `cmp.saveUnifiedConsent`, `privacy.*`, native `close` | identical names and storage | D.6.6 | Target |
| consent cookies | written by the page, 365 days, every launch, before ads | written by the page; host fallback only if missing | D.6.3 | Target |
| `isCMPRequired()` | one `cmp-eu-only` request per launch, not persisted, no timeout; resolves at the startup page's load; a `{}` body disables the cache | identical | D.6.2 | Target |
| `isCMPRequired()` rule | `true` for every response served | `true` | D.6.2 | Overwolf |
| settings window | title `CMP`, 800 x 800, not modal, preloader then `cmp.html` with a fixed query; closing writes nothing | identical, verified in the lab | D.6.4 | Target; R3-5 (`firstRun`, `cmpRequired` on later launches) |
| settings window promise | resolves on creation; a second call focuses | identical | A.2.2 | Target |
| default consent on the first settings call | a hidden `ow-cmp-v2.html` window overwrites the stored consent with a new default | identical | D.6.4 | Target; Overwolf (OQ-38) |

### Ads

| Behaviour | ow-electron (observed) | ow-tauri target | CONTRACT | Status |
|---|---|---|---|---|
| `__overwolf__` | 31 keys in a fixed order, frozen; `consent` and `consentFull` = the stored unified consent string as it was at launch (empty on a first launch, not updated in the session); `pageUrl` from the `pageurl` attribute | identical; `owVersion` labelled | D.2 | Target |
| `systemInfo`, macOS | CPU brand, one empty GPU entry, named displays with dpi | identical | D.2 | Target |
| `systemInfo`, Windows | one GPU entry per DXGI adapter with only `driverVersion`; displays by their friendly names | identical | D.2 | Target (Windows lab) |
| `unit` | passed through, in test mode too; a performance ad's `forceAdUnit` | identical | D.2 | Target |
| `pageurl` attribute | becomes the guest's `pageUrl` | identical | B.3.2, D.2 | Target |
| element shape | a plain `HTMLElement` until attach; then `OwAdViewElement` prototype with Electron's `<webview>` methods plus `setPageUrl` and `sendCommand`, own attribute-backed properties, an open shadow root with `style` and `iframe` | own attribute-backed accessors in ow-electron's order; `setPageUrl`, `sendCommand`, `setAudioMuted`, `reload` on an inserted prototype; the shadow root is attempted (engines refuse it on `owadview`); no generic `<webview>` methods | B.3.3, B.3.4 | Target (owadview members); Gap (shadow root); Deviation (webview methods) |
| `setPageUrl()`, `sendCommand()` | forwarded to the ad page as private messages; no visible effect | identical; `setPageUrl` also sets `pageurl` for the next load | B.3.3, D.5 | Target |
| element events | plain `Event`, data copied as `Object.assign` copies it (a string payload spreads per character), `display_ad_loaded` twice, sub-frame `did-fail-load`; Electron's standard `<webview>` events also forwarded | identical for the ad events; `render-process-gone`, navigation and console events | B.3.5 | Target; Deviation (`did-frame-*`, `media-*`) |
| element removed or moved after attach | dead: the guest detaches and the element never attaches again; a plain `destroyed` only when it is back in the document by then (a move) | identical | B.3.4, B.3.5 | Target |
| host to guest messages | `consent` (twice), `customTracking` (resent per reload), `eHashes`, `window-minimized` (minimize), `window-hidden` (also on minimize, after `window-minimized`, except on Windows; on every platform the guest turns hidden before `window-minimized`), `sendCommand`, `setPageUrl` | identical | D.5 | Target |
| email hashes | `{sha1, md5, sha256}` lower-case hex, trimmed and lower-cased input; sent as `eHashes`, never in `__overwolf__` | identical | A.2.2, D.5 | Target; R3-6 (gmail rule) |
| ad document request | `Referer: https://www.overwolf.com/<uid>`, `Origin`, full header order, cookies | identical | D.8.2 | Target (Windows, macOS) |
| subresource `Origin` | forced on every guest request | Windows identical | D.8.3 | Target (Windows); Gap (macOS) |
| `x-ow-*` on `owads.min.js` | `x-ow-uid`, `x-ow-phase`, `x-ow-window` | Windows identical | D.8.3 | Target (Windows); Gap (macOS); Overwolf |
| guest web security | off; insecure content allowed | off on Windows | D.8.1 | Target (Windows); Gap (macOS) |
| hidden embedder | loads, never fills | identical | B.3.4 | Target |
| visibility | `hidden` for `display: none`, scrolled out, window hidden or minimized; screen position ignored; the page reloads itself after `hidden`; guests hidden before their window closes | identical signals and `visibilitychange` events (one in the old state on hide, three on show; the engine's own events are stopped); the hidden main frame's timeouts of 1 s or more aligned to 1 s wake-ups as in Chromium (shorter timers and the ad frames run on time); 0.5 intersection ratio; a reload asked for while hidden is held until 2.5 s after `hidden` or until visible again; Windows: guests hidden natively while minimized | B.3.4, D.5 | Partial (sub-100 ms hides read with more jitter on Windows) |
| crash recovery | immediate reload, no cap, `render-process-gone` | identical | D.7 | Target |
| load errors | main frame reloaded every 5000 ms, no cap, no analytics | identical | D.7 | Target |
| test and live | identical shaping, only `testAd` differs | identical | D.7 | Target |
| live fill on macOS | fill impressions with the window on-screen at opacity 0; none off-screen; no `display_ad_loaded` in live runs | measured: 2 fill impressions per load on both hosts (300x250, a lab live run against the harness's ow-electron live run) | D.8.3 | Target (lab check 6) |
| external opens | one gesture, one open | one gesture, one open; at most 20 per guest per minute (Overwolf's QA step clicks one ad five times) | D.7 | Target |

### Ad formats

The developer view of each format is [AD-FORMATS.md](AD-FORMATS.md).
Results are from the macOS ad-format lab (24 scenarios, test mode) and the
Windows lab (27 scenarios), both against ow-electron 42.11.4.

| Behaviour | ow-electron (observed) | ow-tauri target | CONTRACT | Status |
|---|---|---|---|---|
| standard display, 7 sizes | all seven fill in test mode (970x90 included); `display_ad_loaded` twice per fill, refresh about every 30 s | identical events and ad library options | B.3.2, B.3.5 | Target |
| standard video (400x300, 400x600) | `player_loaded`, `play`, `impression`, `complete`; guests start muted | identical; mute timeline equal (L5) | B.3.5, D.7 | Target |
| house ads | configuration request per uid; `house_ad_action` / `house-ad-action` with `{ action }` | identical request; both spellings dispatched | B.3.5 | Target; not served in test mode on either host (OQ-A4) |
| high impact | `adstyle="high-impact-ad;"` passed to the ad library; `high-impact-ad-loaded`, then `high-impact-ad-removed` about 15 s later; the guest follows the zone's growth | identical | B.3.2, B.3.4 | Target |
| interstitial DOM | no shadow root, inline `pointer-events: none;`, one fixed full-viewport `div`; `auto` at `performance_ad_loaded` | identical, character for character | B.3.4 | Target |
| interstitial input | the page under it takes input until `performance_ad_loaded` | native pass-through until that event (Windows window region, macOS `hitTest:`) | B.3.4 | Target |
| interstitial stacking | above every other ad (`z-index: 999999`) | the newest performance guest raised to the top after every mount | B.3.4 | Target |
| interstitial end | `shutdown`, then the element leaves the document (+185 ms); no-fill sends `shutdown` alone; under 500 x 500 `performance_ad_error` (a string), then `shutdown` | identical (removal +1 ms, in the next task) | B.3.4, B.3.5 | Target |
| second interstitial | removed in the same task, no guest, no event | identical | B.3.2 | Target |
| interstitial after a minimize | macOS: dismisses itself (`performance_ad_dismiss`), then `shutdown`, in every run; Windows: the dismiss in some runs only | the same order of messages and the same dismiss (macOS 4 of 4 runs) | B.3.4, D.5 | Target (Windows variance on both hosts) |
| reward | `adstyle="rewarded-ad;"` on a slot of at least 400 x 300: `video_ad_ready`, `player_loaded`, play after hide then show, `impression`, `complete` | identical for hides of 1 frame, 50 ms, 500 ms and 2 s (L7) | B.3.2, B.3.4 | Target |
| transparency | a slot with no ad shows the app's container; an interstitial's dim shows the app | guests transparent from creation (`ads.transparentGuests`) | B.3.4 | Optimised (L1, L1-W) |
| `localStorage.owAdTestAd` in the guest | the same `testAd` result | identical in test mode | D.7 | Target (live not compared) |
| in-stream | no `<owadview>` API | none | none | Not supported (OQ-A7) |

### Packages, updates, signing

| Behaviour | ow-electron (observed) | ow-tauri target | CONTRACT | Status |
|---|---|---|---|---|
| packages manager on a host without packages | no events; `getChannel()` `{}`; `getAvailableChannels()` rejects with a fixed message; `hasPendingUpdates()` empty; async rejections with fixed messages; package objects `undefined` | no packages API | H.1 | Deferred |
| GEP, overlay, recorder, utility, CRN | Windows-only packages | not implemented | H | Deferred |
| update feed | `latest.yml` on Overwolf's feed, Windows only | same client, same feed | I.1 | Target; Overwolf (console accepting Tauri installers) |
| installer registry and uninstall cleanup | builder NSIS templates | Tauri NSIS hooks | I.6 | Target |
| signing | builder flow: `/sign/electron`, `integrity.dll`, `OWE` resource, Authenticode, `/sign/asar` | all but `/sign/asar` | G.3 | Target; Overwolf (integrity target) |

## Deviations

Known differences from ow-electron, each deliberate:

| Difference | Why | CONTRACT |
|---|---|---|
| host label (`tauri_*`, `tauri-<tv>`, `Tauri/<tv>`) | the project's labelling rule; one setting reverts it | 0 |
| macOS: no subresource `Origin`, no `x-ow-*` headers, web security on | no public WebKit API (OQ-05) | D.8.3 |
| no generic Electron `<webview>` methods on `<owadview>`; no `did-frame-*` / `media-*` events | undocumented for `<owadview>`, no platform equivalent, and some would give app code control of remote content | B.3.3, B.3.5 |
| the host sends `ad-clicked` to the guest and the element | popups and gesture navigations open in the system browser (OQ-17) | D.5, D.7 |
| first ad navigation waits for the startup consent window as ow-electron's does, but at most 3 s from the mount | a consent page that never closes cannot hold the ads back | D.6.5 |
| crash-report threshold of 10 s | ow-electron's threshold lies between 3 and 20 s and is not measured exactly (harness item R3-4) | E.2 |
| `cmp-eu-only` has a client timeout, `consent.euOnlyTimeoutMs` (60 s); a timeout counts as a failed request, so `isCMPRequired()` resolves `true` and the startup consent window still opens [OBS: ow-electron sets none; with the server hung for 45 s its call resolved after 45.8 s] | a hung request cannot hold `isCMPRequired()` forever | D.6.2 |
| consent cookies written by the host if the page could not | only when both cookies are missing (`consent.hostCookieFallback`) | D.6.3 |
| an `ow-electron.json` holding `[]` or a shared key of the wrong type is reset once, like an unparseable file: one extra `app_first_launch`, the consent saved again, then normal launches [OBS: ow-electron keeps such a file, sends `app_first_launch` on every launch for `[]` and fails every consent save for wrong types] | a state file that can never be repaired would report a first launch forever or lose every consent | F.2 |
| no default application menu: Electron gives framed windows on Windows and Linux its File / Edit / View / Window / Help menu bar unless the app removes it (`Menu.setApplicationMenu(null)`, `removeMenu()`), and on Windows that bar takes 26 px from the content area [OBS: Windows lab, content `{8, 57, 984, 695}` against ow-tauri's `{8, 31, 984, 721}` in a 1000 x 760 frame]; a Tauri window has only the menu the app builds | Tauri menus are built in Rust; the frame is the same on both hosts. The Windows lab's harness app removes Electron's menu so that the two content areas compare | none |
| ow-tauri options (`analytics.userSwitch`, `analytics.muidStrategy: per-install`, a numeric `ads.maxRecoveries`, `ads.transparentGuests: false`) | off by default; documented as non-parity | A.1 |

In test mode a non-empty `unit` passes through unchanged, as in ow-electron
([ADR 0005](adr/0005-ads-test-live-parity.md), amendment of 2026-10-07).

### Known platform gaps

Where the platform cannot reproduce ow-electron with public API. Each is in
CONTRACT with its fallback, and `parity-diff.mjs` classes it
`intended:os-gap`.

| Gap | Platform | CONTRACT |
|---|---|---|
| no subresource `Origin`, no `x-ow-*` headers, web security on in guests | macOS (WebKit) | D.8.3 |
| no ad guests: ads report `unsupported` | Linux | 0 |
| after a cross-origin redirect, the later hops carry `Origin: null` (WebView2 lets a host change a request once, not per hop) | Windows | D.8.3 |
| `<owadview>` gets no shadow root (engines refuse `attachShadow` on it) | all | B.3.4 |
| Electron's `<webview>` events `did-frame-*`, `media-*` and the like | all | B.3.5 |
| guests mounted together report 400025 over their creation time (WebView2 creates them one after another on the main thread); the requests themselves leave on a thread of their own, as soon as they are queued [OBS: Windows lab `sizes`, seven guests: each batch of 400025 starts 4 to 19 ms after the batch's last guest is created; before, two of the first four waited about 460 ms more] | Windows | E.2 |
| an ad guest's content process keeps the memory of replaced ad pages until the system is short of memory, where ow-electron's guest renderer does not grow across reloads: the footprint grows by about 50 to 90 MB per guest reload and stays [OBS: macOS packages-sample idle runs, `tower-right`, test ads: 30 min with the ad library's own reload at about 20 min, guests 122 to 313 MB and 79 to 219 MB against ow-electron's 79 to 97 MB and 71 to 74 MB; 10 min with a reload every minute, the larger guest 118 to 574 MB against ow-electron's guests staying between 60 and 104 MB. The growth is WebKit's own heap (`WebKit malloc`, 285 of 325 MB), not the plugin: the app process (about 49 MB) and the plugin's queues stay flat, and turning off the back/forward cache or clearing the memory cache at each reload changed nothing. WebKit's low-memory signal, sent on memory pressure, freed it: within 20 s the larger guest went from 328 to 202 MB and the other from 107 to 49 MB]. These runs reloaded in place. With `ads.recreateOnReload` (the default), a reload builds a fresh `WKWebView` instead, and the growth stopped in the forced-reload run of [ADR 0024](adr/0024-recreate-on-reload.md); a reload inside the recreate rate guard, or with `ads.recreateOnReload: false`, still reloads in place | macOS (WebKit) | D.8.1 |
| `SameSite=None` cookies read back as no policy; `document.cookie` order follows the WebKit store | macOS | D.6.3 |

## Optimised, same outcome

ow-tauri reaches the same observable outcome another way. `parity-diff.mjs`
classes these `intended:optimised`.

| ow-electron | ow-tauri | Why the outcome is the same |
|---|---|---|
| the ad guest is an in-DOM `<webview>` driven by `GUEST_VIEW_*` IPC | a native child webview per `<owadview>`, driven by the plugin's own commands and host messages ([ADR 0003](adr/0003-owadview-native-child-webviews.md)) | the page sees the same `__overwolf__`, messages and visibility; the element gets the same events |
| the guest is part of the page, so it has no background of its own | guests are created transparent (`ads.transparentGuests`, default `true`); macOS clears the `WKWebView` background without Tauri's `macos-private-api` | an empty slot shows the app's container and an interstitial's dim shows the app [OBS L1, L1-W] |
| the overlay's `z-index` puts the interstitial above other ads | the newest performance guest is raised natively after every guest mount | the interstitial is the top ad [OBS L2] |
| `pointer-events: none` on the overlay until the ad loads | the native guest passes input through until `performance_ad_loaded` | the app under an empty interstitial takes clicks [OBS L3, L3-W] |
| Electron's `GUEST_INSTANCE_FOCUS_CHANGE` | the shim's `setEmbedderFocus` (D.3) | `hasWindowFocus()` and `document.hasFocus()` read the same |
| Chromium's HTTP cache revalidates a cached analytics URL (304) | no HTTP cache for host requests | the same request reaches the same server |
| one state file, `ow-electron.json` | `ow-electron.json` shared byte for byte, plus `ow-tauri.json` for ow-tauri's own options (F.3) | Overwolf's file is identical |

## Lab checks

These checks run in the Tauri lab before a release, per platform. Each
compares the Tauri capture with the baseline capture of the same scenario.
On Windows the [Windows lab](#windows-lab) runs checks 1, 4 and 8 on every
push that touches the plugin.

The results on this page come from these runs, all recorded from
2026-10-06 to 2026-10-08, before the plugin's Tauri-native rewrite
([ADR 0017](adr/0017-tauri-native-pivot.md)):

- macOS, 2026-10-07: 0 `BUG` in the first and second launch, consent,
  `messages`, quit and live runs;
- macOS, the ad-format lab: 24 scenarios
  ([Ad-format lab checks](#ad-format-lab-checks));
- Windows, the lab run on commit `a333efb` (2026-10-08), and the request
  shaping read on commit `caa7065` ([Windows lab](#windows-lab)).

[Re-running the harness](#re-running-the-harness) gives the commands to
repeat any of them on the current code.

1. **Field-by-field diff** of: Counter and InsertStats URLs, queries and
   bodies; host request header set and order; cookies on host requests; the
   ad document's request headers; `__overwolf__` as JSON; the startup consent
   window's URL; the consent cookies and their attributes; the bytes of
   `ow-electron.json` after a first and a second launch. Expected differences
   are only the host-label values.
2. **Labelled versions still work.** With `owVersion` / `oweVersion` =
   `tauri-<tv>`, the ad page fills and sends its own analytics, and the
   consent page completes. If not, set `ads.owVersionOverride` and record it
   (CONTRACT section 0).
3. **Consent cookies.** After the startup consent window closes, the ads
   data store holds `euconsent-v2` and `acconsent` with the observed
   attributes, written by the page. Record whether the host fallback
   (`consent.hostCookieFallback`) was needed.
4. **Windows header changes reach the wire.** WebView2 sends the changed
   `Origin` and `Referer` on the document and the forced `Origin` on
   subresources; `x-ow-*` headers are present on `owads.min.js`.
5. **macOS document headers.** WebKit keeps the custom `Referer` and `Origin`
   on the first navigation, and the guest's `document.referrer` is
   `https://www.overwolf.com/<uid>`.
6. **macOS fill.** Test ads fill, and at most 10 live loads give fill
   impressions comparably to ow-electron's live runs, despite the macOS gap.
   The metric is fill impressions (`scalibur-impression` with `pos: fill`,
   `adViewable`), not `display_ad_loaded`: ow-electron's live runs on macOS
   showed fill impressions but no `display_ad_loaded`. The lab window must be
   on-screen at alpha 0: off-screen, ow-electron got no live fill at all,
   while test ads filled.
7. **Inferences.** Each [INF] item in CONTRACT is confirmed or corrected,
   including `author: { name: "" }` mapping to `"unknown"`, and the guest
   visibility override (the ad page logs `<owadview> is not visible.
   waiting...` while ow-tauri reports it hidden, and reloads itself as in
   ow-electron).
8. **Host messages.** The guest receives `consent` twice per consent save,
   `customTracking` after every reload once it has changed, `eHashes` after
   both email-hash calls, and `window-hidden` on hide, and nothing else.

### Ad-format lab checks

Test mode, both hosts, the scenarios of the [scenario table](#scenarios).
Nothing is ever sent into an ad.

| Id | Check | Pass when | macOS (ad-format lab) | Windows (CI) |
|---|---|---|---|---|
| L1 | transparency: an unfilled slot over a red container; an interstitial's dim | the app's colour shows where the guest has no content | pass: probes see the container under the guest; native routing matches the DOM | L1-W pass: the composed window shows the red container under a ready reward slot; every guest's WebView2 background is transparent |
| L2 | z-order: interstitial up, then a standard slot remounted | the performance guest stays on top | pass (`zorder` record) | pass: the performance guest's container is the top child window before load, after load and after the remount |
| L3 | pass-through while the interstitial loads | the app's control takes a click while loading; after `performance_ad_loaded` the ad would | pass on routing: `pointer-events` `none` then `auto`, native hit test on the app while loading; the click itself is not mirrored (WebKit ignores synthesised events in the invisible window) | L3-W pass: empty window region and one click delivered to the app while loading; after load the region is gone and the click is refused |
| L4 | `background-blur` | same visual outcome | pass | pass (diff only) |
| L5 | mute timeline per guest | equal to ow-electron's `isAudioMuted()` | pass | pass (native `IsMuted`) |
| L6 | payload spread, `destroyed`, removal timings | as ow-electron | pass | pass |
| L7 | reward opt-in after hides of 1 frame, 50 ms, 500 ms, 2 s | plays exactly when ow-electron plays | pass, all four | pass; a 50 ms hide can differ run to run (variance) |
| L8 | minimize and restore under an interstitial | the same messages in the same order | pass (macOS: hidden, `window-minimized`, `window-hidden`, then `performance_ad_dismiss` in 4 of 4 runs, 0 `BUG`); Windows: `performance_ad_dismiss` varies on both hosts | pass: hidden, then `window-minimized`, no `window-hidden` |
| L9 | removal of a standard slot | as ow-electron | pass | pass |
| L10 | the ad library's options on the wire, per format | equal on both hosts | pass, every scenario | pass |
| L11 | `localStorage.owAdTestAd` in the guest origin | the same `testAd` result | pass in test mode; the live pair was not run | pass in test mode |
| L12 | a playing rewarded slot hidden (`display: none`) for 2 s, then shown | the guest keeps playing to `complete`, no reload, as in ow-electron | pass, 3 of 3 runs, with the same guest visibility sequence as ow-electron's run (run with a lab overlay; the harness has no scenario for it) | not run |

The macOS ad-format lab ran 24 scenarios with no window ever visible and the
app never frontmost: 0 `BUG` in 23; the 24th, `perf-minimize`, was the
dismiss above. Its cause was WebKit's own `visibilitychange` in the guest;
with it stopped, `perf-minimize` diffs with 0 `BUG` on macOS (4 of 4 runs
dismissed, as ow-electron's 4 of 4). 9 live loads in total (under the cap of 10), never
clicked: standard and Tower Plus filled on ow-tauri; reward, interstitial
and high impact did not fill on ow-tauri, as expected for an unqualified
uid (OQ-A10).

### Windows lab

`.github/workflows/windows-lab.yml` runs on pushes to `main` that touch the
plugin, `ow-tauri` or the harness, and on demand. It builds the Tauri
harness app with the `lab` feature, then four shards run every scenario on
ow-electron and on ow-tauri one after the other on the same runner (Windows
Server 2025, display 1920 x 1080), diff the pair and evaluate the Windows
checks. Test ads only, with the harness's neutral identity.

Last full run on `a333efb`: 27 scenarios (the base runs `A`, `cmp` and
`messages`, and every ad-format scenario), 0 `BUG`, and the checks L1-W,
L2, L3-W and L5 true, as were the window frame (G1) and content area (G2)
checks. Request shaping, last read on `caa7065`, matched on the
wire: the ad library request
carries the same `x-ow-uid`, `x-ow-phase` and `x-ow-window` headers and
`Referer` as ow-electron's, the ad document the same `Referer` and `Origin`,
and subresources the forced `Origin` (176 of 178; the 2 others are later
hops of a cross-origin redirect, a [known gap](#known-platform-gaps)).

## Re-running the harness

Anyone can reproduce the baseline. The harness uses only public packages:
`@overwolf/ow-electron` and `@overwolf/ow-cli` on their `latest` dist-tag.

```sh
cd tools/parity-harness
npm install --workspaces=false
npm view @overwolf/ow-electron dist-tags     # confirm the baseline version

# Run the scenarios listed above (each writes captures/<run-id>/):
node run.mjs --mode test --present transparent --duration 90
node run.mjs --mode test --layout 300x250 --duration 60
node run.mjs --mode live --live-ok --max-live-loads 10 --present transparent --duration 90
node run.mjs --home profile:p1 --present transparent --duration 40 --run-id first
node run.mjs --home profile:p1 --present transparent --duration 40 --run-id second
node run.mjs --present transparent --duration 360 --quit-style quit
node run.mjs --webrequest --present transparent
node run.mjs --packages gep,overlay
node run.mjs --disable-analytics --present transparent
node uid-matrix.mjs
node muid-probe.mjs --experiment             # macOS

# Behaviour scenarios (each prints its run id):
node run.mjs --scenario messages --window-monitor
node run.mjs --scenario crash --window-monitor
node run.mjs --scenario block --window-monitor
node run.mjs --scenario cmp --window-monitor
node run.mjs --scenario cmp-required --features empty-object
node run.mjs --scenario windows --window-monitor
node run.mjs --scenario windows-urls --window-monitor
node run.mjs --scenario offscreen --window-monitor
node run.mjs --scenario packages
node run.mjs --scenario introspect
nohup taskpolicy -b node run.mjs --scenario long --allow-long --caffeinate --no-cdp &

# Ad formats and the ad-format lab checks (test ads):
node run.mjs --scenario sizes --window-monitor
node run.mjs --scenario perf-sample --window-monitor
node run.mjs --scenario reward-optin --window-monitor
node run.mjs --scenario lab-layers --window-monitor
# ... every scenario of the table above; the full list is in the harness README

node analyze.mjs captures/<run-id>           # report.md and report.json per run
node lib/adformat-report.mjs captures/<run-id>   # per-element ad-format facts

# The same scenario on ow-tauri (builds tools/parity-harness/tauri-app once,
# with the plugin's lab feature), then the diff:
taskpolicy -b node run.mjs --host tauri --scenario sizes --run-id T-sizes
node parity-diff.mjs captures/<ow-electron run> captures/T-sizes   # exit 1 while a BUG remains
```

The Windows lab is the `windows-lab.yml` workflow (`workflow_dispatch` with
a scenario list, or a push to `main`); its captures and diffs are the
`windows-lab-captures-<shard>` artifacts.

Captures are git-ignored and stay on the machine that made them; they
contain cookies and identifiers. The harness runs as a neutral example app
by default; to observe a registered app's real ad setup, put its identity in
the git-ignored `local.identity.json` ([harness README](../tools/parity-harness/README.md)).

When a new `latest` ow-electron is published:

1. Re-run every scenario above on the new version.
2. Diff each `report.json` against the previous baseline.
3. Update CONTRACT for every change, with [OBS], and update the baseline
   version at the top of CONTRACT and in [What parity means](#what-parity-means).
4. Re-run the [lab checks](#lab-checks).

For Overwolf: the harness answers "what does ow-electron do" without any
access to ow-electron's internals, so Overwolf can run it as is, compare its
captures with ours, and correct any [OBS] fact that is wrong. Overwolf can
also answer the "Overwolf" items in the [parity matrix](#parity-matrix) directly; each is listed in
[OPEN-QUESTIONS.md](OPEN-QUESTIONS.md).

## Harness rounds

The harness observations were made in three rounds against the same
baseline. The first set the baseline above. Items numbered `R2-n` come from
the second round and `R3-n` from the third.

### Second round (R2 items)

The second round ran on the same baseline (ow-electron 42.11.4, macOS), with every
window pinned invisible before it existed on screen (a window-server monitor
confirmed none became visible), no input sent to any page, and two live loads
in total. Its results are folded into CONTRACT, tagged [OBS]:

| Item | Result | CONTRACT |
|---|---|---|
| R2-1, R2-12 | host-to-guest messages (`consent`, `customTracking`, `eHashes`, `window-hidden`), guest visibility and focus signals, the page-driven reload after `hidden`, the events forwarded to the element | B.3.4, B.3.5, D.3, D.5 |
| R2-2 | email hash keys, order, normalisation; delivery as `eHashes` | A.2.2, D.5 |
| R2-3 | `setExternalPaymentUserId` sends `electron_sub_info` | A.2.2, E.2 |
| R2-4, R2-13 | the element after attach (`pageUrl`, `setPageUrl`, `sendCommand`); `pageurl` reaches the guest; `unit` is not rewritten in test mode | B.3.2, B.3.3, D.2 |
| R2-5 | crash recovery and analytics; load-error retries | D.7, E.1, E.2 |
| R2-6 | the settings window, its promise, the default-consent window | A.2.2, D.6.4 |
| R2-7 | `cmp-eu-only` caching, timing, response variants; startup window after the response | D.6.1, D.6.2 |
| R2-8 | window analytics names and `window_closed` triggers | E.2 |
| R2-14 | `app_cuid` with a console-assigned uid | G.2 |
| R2-15 | the package manager's async rejections and `undefined` package objects | H.1 |
| live fill | the first round's live run had fill impressions but no `display_ad_loaded`; no live fill off-screen | lab check 6 |
| R2-9 | the 13-hour hidden session (`R2-long-20261006-2023`): one heartbeat Counter and one 400023 at 12 h, nothing else; no `window_closed` at quit for a window never shown | E.2 |
| R2-10, R2-11 (Windows) | `MUID` / `MUIDV2` and `app.overwolf.muid`; `systemInfo` GPUs and display names (Windows lab) | D.2, E.4 |

Still open from the second round:

| Id | What to observe | For |
|---|---|---|
| R2-10 | Linux: machine-id derivation; Windows: whether `MachineGuid` is the `MUID` source | OQ-02 |

R2-11, the Linux `systemInfo` shape, does not apply: Linux builds have no
ad guests (OQ-19).

### Third round (R3 items)

Gaps the second round left, plus the ad formats. Same safety rules. Settled:

| Id | Result | CONTRACT |
|---|---|---|
| R3-1 | minimize: the guest document turns hidden, then `window-minimized` and `window-hidden` when the minimize ends (on Windows `window-minimized` only); a running interstitial dismisses itself (on Windows in some runs), then shuts down. A restore with a live guest was not observed | B.3.4, D.5 |
| R3-2 | `setPageUrl(url)` and `sendCommand(...)` are forwarded to the ad page as private messages; no visible effect | B.3.3, D.5 |
| ad formats | every documented format in test mode: events, DOM, removal rules, the ad library's options, the reward opt-in ([Ad formats](#ad-formats)) | B.3, D.5, D.7 |

Still open:

| Id | What to observe | For |
|---|---|---|
| R3-3 | `setExternalPaymentUserId` and `setUserEmailHashes` after `disableAnonymousAnalytics()` and after `disableAdsFPD()` | OQ-11, OQ-12 |
| R3-4 | the crash-report threshold: crashes 3, 5, 10 and 15 s after a recovery | OQ-28 |
| R3-5 | the settings window's `firstRun` and `cmpRequired` on a second launch of one profile | OQ-07 |
| R3-6 | `generateUserEmailHashes` for `gmail.com` addresses with `.` and `+suffix` | OQ-10 |
| R3-7 | `disableAdsOptimization()` and a guest mounted after the call | OQ-14 |
| R3-8 | `privacy.enableAdOptimization(true)`, then `getIsAdOptimizationEnabled()` | OQ-14 |
| R3-9 | `getAvailableChannels()` with no names | OQ-37 |
