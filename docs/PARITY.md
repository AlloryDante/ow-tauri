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
6. **Public API results match**: `app.overwolf` members and the packages
   manager return what ow-electron returns, including error messages
   (CONTRACT B.1, H.1).

The one deliberate difference is the **host label**: where ow-electron names
itself (`electron_*` event names, the `42.11.4` version, the `Electron/` UA
token), ow-tauri says `tauri` (CONTRACT section 0,
[ADR 0006](adr/0006-analytics-labelling.md)). It is one setting, so Overwolf
can change it.

Parity is measured against a fixed **baseline**: ow-electron 42.11.4 (the
`latest` dist-tag on 2026-10-06; Electron 42.11.4, Chrome 148.0.7778.280),
observed on macOS 25.5 arm64. A new ow-electron release means a new baseline
run ([Re-running the harness](#re-running-the-harness)).

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
2. **The ow-tauri side: the Tauri lab.** The example app built with ow-tauri,
   run with the same identity and the same scenario, with its traffic
   captured: on Windows through WebView2's net log (`--log-net-log` in the
   browser arguments) and the plugin's own request log; on macOS through a
   lab-only HTTPS proxy with a local certificate authority, inside an
   isolated home directory.

Safety rules for both halves:

- No visible windows. Windows that must fill ads are shown at alpha 0,
  ignore the mouse, are not focusable and stay off the taskbar; a window that
  is never shown never fills, in ow-electron and in ow-tauri. The dock icon
  is hidden.
- No input is ever sent to an ad. Ads are never clicked.
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

## Parity matrix

Status values:

- **Target**: the ow-electron behaviour is settled and CONTRACT specifies the
  same; the Tauri lab diff ([Lab checks](#lab-checks)) confirms it per platform.
- **Gap**: the platform cannot reproduce it with public API; CONTRACT names
  the fallback, and Overwolf is asked whether the gap is acceptable.
- **R2-n**: harness round 2 item `R2-n` ([Harness round 2](#harness-round-2)) still has to observe
  part of it; CONTRACT gives the interim behaviour.
- **Overwolf**: needs an answer only Overwolf can give.
- **Deferred**: outside the current scope (packages).

### Identity and files

| Behaviour | ow-electron (observed) | ow-tauri target | CONTRACT | Status |
|---|---|---|---|---|
| uid rule | `sha1` of `{'author':...,'name':'<productName or name>.electron'}`, verbatim author, `"unknown"` fallback | identical; 13 test vectors | G.2 | Target |
| `overwolf.uid` in `package.json` | used verbatim | identical | G.2 | Target |
| `app_cuid` when `overwolf.uid` is set | not observed | computed uid | G.2 | R2-14 |
| `process.env.OVERWOLF_APP_UID` | set when the main module loads | set before app scripts run | B.1.1 | Target |
| muid, macOS | `guid(sha256(lowercase(IOPlatformUUID)))`, `muidV2 = muid` | identical; 4 test vectors | E.4 | Target |
| muid, Windows and Linux | not observed yet | registry values shared, else derived | E.4 | R2-10 |
| phase percent | MD5 character-code sum of the muid, modulo 100 | identical | E.4 | Target |
| `ow-electron.json` | `firstLaunch`, `cmp` with URL-encoded unified string and seconds `timeStamp`; nothing else in the directory | identical bytes | F.2 | Target |
| `utmParams` absent | `undefined` | `undefined` | F.2 | Target |
| logs | none written; `logger.enabled: false` | none unless `logging.enabled` | F.4 | Target |
| `logsFolderPath` | literal `<userData>/..\ow-electron/<uid>/logs` | identical string | F.4 | Target |
| `__settings__` | fixed URLs and flags | identical constant | B.1.1 | Target |

### Analytics

| Behaviour | ow-electron (observed) | ow-tauri target | CONTRACT | Status |
|---|---|---|---|---|
| launch sequence | first launch, `cmp-eu-only`, start, heartbeat, 400022, 400023, within about 100 ms | identical order, from `main_ready` | E.2 | Target |
| first-show heartbeat | Counter + 400023 when the first window shows | identical | E.2 | Target |
| ad guest Kind | 400025 per guest | identical | E.2 | Target |
| `window_closed` | `name`, constructor `title`, `length` in seconds; never for unshown windows | identical | E.2 | Target |
| window name normalisation | documented; hyphen and length rules not observed | interim rule | E.2 | R2-8 |
| guest crash event | not observed | reference implementation shape | E.2 | R2-5 |
| periodic heartbeat | none within 6 minutes | hourly check, 12 h | E.2 | R2-9 |
| Counter query and `Extra` | key order, URLSearchParams encoding, `os_ver` from `os.release()` | identical | E.1 | Target |
| InsertStats body | `Kind` + positional `Extra`, no event fields | identical | E.1 | Target |
| host request headers | fetch-metadata headers, `priority: u=4, i`, no Origin or Referer, session cookies | identical, cookies from the ads data store | E.1 | Target |
| user agent | Chromium UA with `<PNNS>/<ver>` and `Electron/42.11.4` | same shape, `Tauri/<tv>`; engine never faked | E.1 | Target (label) |
| `owver` | `42.11.4`, `42_11_4` | `tauri-<tv>`, `tauri-<tv with _>` | 0 | Target (label); Overwolf (dashboards) |
| event names | `electron_*` | `tauri_*` | 0 | Target (label); Overwolf (dashboards) |
| `disableAnonymousAnalytics()` | keeps first-launch Counter and both heartbeats | identical | E.3 | Target |
| `setExternalPaymentUserId` | not observed | sends nothing | A.2.2 | R2-3 |
| uninstall event | `ow_electron_app_uninstall` from the NSIS uninstaller (builder) | `ow_tauri_app_uninstall` from Tauri NSIS hooks | I.6 | Target (label) |

### Consent

| Behaviour | ow-electron (observed) | ow-tauri target | CONTRACT | Status |
|---|---|---|---|---|
| startup consent window | hidden 1 x 32 window on `ow-cmp-v2.html` every launch, query in a fixed order, closes itself | identical | D.6.1 | Target |
| `unifiedcmp` | empty, or the stored value URL-encoded once more | identical | D.6.1 | Target |
| skipped when consent not required | not observed | always opened | D.6.1 | R2-7 |
| consent page globals | `cmp.saveConsent`, `cmp.saveUnifiedConsent`, `privacy.*`, native `close` | identical names and storage | D.6.6 | Target |
| consent cookies | written by the page, 365 days, every launch, before ads | written by the page; host fallback only if missing | D.6.3 | Target |
| `isCMPRequired()` | one `cmp-eu-only` request, `true` | identical | D.6.2 | Target |
| `isCMPRequired()` rule and caching | not observed | `true`, one request per launch | D.6.2 | R2-7 |
| settings window | `cmp.html` | `cmp.html`, interim query and size | D.6.4 | R2-6 |
| `openCMPWindow` promise timing | not observed | resolves on close | A.2.2 | R2-6 |

### Ads

| Behaviour | ow-electron (observed) | ow-tauri target | CONTRACT | Status |
|---|---|---|---|---|
| `__overwolf__` | 31 keys in a fixed order, frozen, `consent` and `pageUrl` empty | identical; `owVersion` labelled | D.2 | Target |
| `systemInfo`, macOS | CPU brand, one empty GPU entry, named displays with dpi | identical | D.2 | Target |
| `systemInfo`, Windows and Linux | not observed | interim adapters list | D.2 | R2-10, R2-11 |
| `unit` in test mode | not observed | rewritten to `testAd` (safety guard) | D.2 | R2-13 |
| element shape | `HTMLElement` prototype, open shadow root with `style` and `iframe` | instance members only; same shadow root | B.3.3, B.3.4 | Target; R2-4 rechecks own properties |
| element events | plain `Event`, data as own properties, `display_ad_loaded` twice, sub-frame `did-fail-load` | identical | B.3.5 | Target |
| host to guest messages | not observed | `customTracking` only | D.5 | R2-1 |
| email hashes in the guest | absent from `__overwolf__` | not delivered | A.2.2 | R2-2 |
| ad document request | `Referer: https://www.overwolf.com/<uid>`, `Origin`, full header order, cookies | identical | D.8.2 | Target (Windows, macOS, Linux) |
| subresource `Origin` | forced on every guest request | Windows identical | D.8.3 | Target (Windows); Gap (macOS); Gap until a web extension (Linux) |
| `x-ow-*` on `owads.min.js` | `x-ow-uid`, `x-ow-phase`, `x-ow-window` | Windows identical | D.8.3 | Target (Windows); Gap (macOS, Linux); Overwolf |
| guest web security | off; insecure content allowed | off on Windows and Linux | D.8.1 | Target (Windows, Linux); Gap (macOS) |
| hidden embedder | loads, never fills | identical | B.3.4 | Target |
| visibility thresholds | not observed | element-level model | B.3.4 | R2-12 |
| crash recovery | not observed | 10 reloads, 5 s retry | D.7 | R2-5 |
| test and live | identical shaping, only `testAd` differs | identical | D.7 | Target |

### Packages, updates, signing

| Behaviour | ow-electron (observed) | ow-tauri target | CONTRACT | Status |
|---|---|---|---|---|
| packages on a host without them | no events; `getChannel()` `{}`; `getAvailableChannels()` rejects with a fixed message; `hasPendingUpdates()` empty | identical | H.1 | Target |
| `setChannel()`, sync throw or rejection, package objects | not observed | rejection; `undefined` objects | H.1 | R2-15 |
| GEP, overlay, recorder, utility, CRN | Windows-only packages | not implemented | Appendix P | Deferred |
| update feed | `latest.yml` on Overwolf's feed, Windows only | same client, same feed | I.1 | Target; Overwolf (console accepting Tauri installers) |
| installer registry and uninstall cleanup | builder NSIS templates | Tauri NSIS hooks | I.6 | Target |
| signing | builder flow: `/sign/electron`, `integrity.dll`, `OWE` resource, Authenticode, `/sign/asar` | all but `/sign/asar` | G.4 | Target; Overwolf (integrity target) |

## Lab checks

These checks run in the Tauri lab before a release, per platform. Each
compares the Tauri capture with the baseline capture of the same scenario.

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
6. **macOS fill.** Test ads and at most 10 live loads fill comparably to the
   ow-electron live baseline (7 loads, real demand), despite the macOS gap.
7. **Inferences.** Each [INF] item in CONTRACT is confirmed or corrected,
   including `author: { name: "" }` mapping to `"unknown"`.

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

node analyze.mjs captures/<run-id>           # report.md and report.json per run
```

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

**For Overwolf.** The harness answers "what does ow-electron do" without any
access to ow-electron's internals, so Overwolf can run it as is, compare its
captures with ours, and correct any [OBS] fact that is wrong. Overwolf can
also answer the "Overwolf" items in the [parity matrix](#parity-matrix) directly; each is listed in
[OPEN-QUESTIONS.md](OPEN-QUESTIONS.md).

## Harness round 2

Items still to observe. All runs use ow-electron `latest`, invisible windows
and test ads, and never click.

| Id | What to observe | For |
|---|---|---|
| R2-1 | messages the host passes to the guest's `__overwolf__.onmessage`, around embedder hide, minimize, restore and resize, `customTracking` changes, `setUserEmailHashes`, `disableAdsFPD` and consent changes | OQ-13, OQ-11 |
| R2-2 | `__overwolf__` keys and messages after `setUserEmailHashes`; the key casing `generateUserEmailHashes` returns | OQ-10, OQ-11 |
| R2-3 | the request `setExternalPaymentUserId` sends (endpoint, name, fields), with a dummy user id | OQ-12 |
| R2-4 | the element's own properties after attach; `pageurl` attribute versus the guest's `pageUrl` | OQ-32 |
| R2-5 | guest crash in test mode, three times with spacing: reload delay, retry cap, crash Counter and Kind 400024; one blocked `adview.html` | OQ-28, OQ-04 |
| R2-6 | `openAdPrivacySettingsWindow()` and `openCMPWindow()` opened off-screen and hidden: URL and query, size, modal, promise timing, writes on close | OQ-07, OQ-26 |
| R2-7 | `isCMPRequired()` called three times plus a restart: request count and caching; a modified `cmp-eu-only` body served locally in an isolated home: the `params` rule, and whether the startup consent window is skipped | OQ-06, OQ-07 |
| R2-8 | window `name` normalisation (hyphens, more than 20 characters, spaces); `window_closed` on `hide()` without close, and for a window visible less than 1 s | OQ-04 |
| R2-9 | a 13-hour hidden session: periodic heartbeat cadence | OQ-04 |
| R2-10 | Windows and Linux: machine-id derivation; Windows `MUID` / `MUIDV2` registry values before and after a first run; `systemInfo.gpus` | OQ-02, OQ-19 |
| R2-11 | Windows `systemInfo` shape (GPU names, display names, dpi) | OQ-19 |
| R2-12 | viewability: element `display: none`, scrolled out, window minimized: the guest's `visibilityState` and messages | OQ-27 |
| R2-13 | the `unit` attribute in test mode: the guest's `unit` value | CONTRACT D.2 |
| R2-14 | `overwolf.uid` set to a value other than the formula's: `app_cuid` in the Counter `Extra` | CONTRACT G.2 |
| R2-15 | `typeof app.overwolf.packages.gep`; the `setChannel` message; synchronous throw or rejection (called without `await` inside `try`) | OQ-36, OQ-37 |
