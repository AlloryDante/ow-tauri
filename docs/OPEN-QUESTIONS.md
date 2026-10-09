# Open questions

ow-tauri is Overwolf's ow-electron SDK adapted to Tauri: the app is a plain
Tauri app, and Overwolf receives what ow-electron sends, except the host
label and the platform gaps that [CONTRACT](CONTRACT.md) names (for example
the macOS request headers, [OQ-05](#oq-05-request-shaping-for-the-ad-page)). This
file lists every question that came up while specifying that, with its
status, the answer and where the answer comes from. Ids are stable; numbers
are not an order.

The questions were first written for a design that emulated Electron. The
Tauri-native rewrite ([ADR 0017](adr/0017-tauri-native-pivot.md)) closed the
ones about the Electron API and took packages out of 1.0. It also raised four
new questions for Overwolf (OQ-40 to OQ-43).

Most questions are settled: Overwolf's documentation answered some, black-box
observation of ow-electron 42.11.4 with the parity harness (three rounds of
observations, the 13-hour run and the Windows lab) answered more
([PARITY.md](PARITY.md)), and the project maintainers decided the rest. What
is still open is either a question only Overwolf can answer, or a detail
that a harness item still has to observe: Linux (R2-10) or the third round
(R3-n).
The ad-format questions for Overwolf are under [Ad formats](#ad-formats)
(OQ-A1 to OQ-A10).

Status:

| Status | Meaning |
|---|---|
| **Answered** | settled by Overwolf's documentation or by observing ow-electron; CONTRACT follows the answer |
| **Decided** | no source pins it down; the project maintainers decided, following "copy ow-electron" wherever it is observable |
| **Open: Overwolf** | only Overwolf can answer; CONTRACT has a documented interim behaviour |
| **Open: pending harness** | observable; harness item `R2-n` or `R3-n` ([PARITY.md](PARITY.md#harness-rounds)) will settle it; CONTRACT has an interim behaviour |
| **Out of scope (1.0)** | belongs to the package runtime; 1.0 has no `packages` API ([ADR 0004](adr/0004-packages-backend-selection.md)) |
| **Closed** | no longer applies to the Tauri-native plugin |

Sources are written as: *observed* (the parity harness against ow-electron
42.11.4, [PARITY.md](PARITY.md)), a `dev.overwolf.com` URL, *typings* (the
published ow-electron 42.11.4 typings), *builder* (the published
`@overwolf/app-builder-lib` 26.9.3 and `@overwolf/ow-cli` 0.1.10 JavaScript),
*owner decision* (the project owner's decisions of 2026-10-06), or
*maintainer decision* (a decision of the project maintainers).

Impact: **High** blocks parity or Overwolf adoption; **Medium** changes data
Overwolf receives or a user-visible behaviour; **Low** affects edge cases.

## Index

| Id | Topic | Question | Impact | Status | CONTRACT |
|---|---|---|---|---|---|
| [OQ-01](#oq-01-uid-rule-for-a-string-author-and-the-42112-change) | Identity | uid rule for a string `author` | High | Answered | G.2 |
| [OQ-02](#oq-02-muid-derivation-muidv2-and-phase-bucketing-for-a-tauri-host) | Identity | muid derivation, `muidV2`, phases | High | Answered (macOS, Windows); open: pending harness (R2-10: Linux) | E.4 |
| [OQ-03](#oq-03-host-labelling-owver-owversion-extra-fields) | Analytics | host labelling | Medium | Decided; open: Overwolf (dashboards) | 0, E.1 |
| [OQ-04](#oq-04-analytics-event-catalogue) | Analytics | event catalogue | High | Answered; open: pending harness (R3-4 crash threshold) | E.2 |
| [OQ-12](#oq-12-setexternalpaymentuserid-report) | Analytics | `setExternalPaymentUserId` report | Medium | Answered; open: pending harness (R3-3, with analytics disabled) | A.2.2, E.2 |
| [OQ-14](#oq-14-windows-ad-optimisation-helper) | Analytics | Windows ad-optimisation helper | Medium | Decided; open: Overwolf; pending harness (R3-7, R3-8) | G.1, A.2.2 |
| [OQ-05](#oq-05-request-shaping-for-the-ad-page) | Ads | request shaping | High | Answered; open: Overwolf (macOS gap) | D.8 |
| [OQ-10](#oq-10-email-hash-encoding) | Ads | email hash encoding | Medium | Answered; open: pending harness (R3-6, gmail rule) | A.2.2 |
| [OQ-11](#oq-11-email-hashes-in-the-ad-guest) | Ads | email hashes in the guest | Medium | Answered; open: pending harness (R3-3, after `disableAdsFPD`) | A.2.2, D.5 |
| [OQ-13](#oq-13-host-to-guest-messages) | Ads | host-to-guest messages | Low to Medium | Answered (R3-1 minimize included); open: a restore with a live guest | D.5 |
| [OQ-17](#oq-17-click-and-navigation-rules) | Ads | click and navigation rules | Low | Decided | D.7 |
| [OQ-19](#oq-19-systeminfo-contents) | Ads | `systemInfo` contents | Low to Medium | Answered (macOS, Windows; Linux has no ad guests) | D.2 |
| [OQ-20](#oq-20-live-ads-from-a-tauri-host) | Ads | live ads approval | High | Decided (labs); open: Overwolf (production) | ADR 0005 |
| [OQ-27](#oq-27-viewability) | Ads | viewability model | Medium | Answered | B.3.4 |
| [OQ-28](#oq-28-crash-and-load-error-recovery) | Ads | guest recovery | Low | Answered; open: pending harness (R3-4, report threshold) | D.7, E.2 |
| [OQ-29](#oq-29-performance-ads) | Ads | performance ad geometry, input and removal | Medium | Answered | B.3.2, B.3.4 |
| [OQ-30](#oq-30-linux) | Ads | Linux | Low | Decided (no ads on Linux) | 0 |
| [OQ-32](#oq-32-element-extensions-pageurl-setpageurl-sendcommand) | Ads | `pageUrl`, `setPageUrl`, `sendCommand` | Medium | Answered | B.3.2, B.3.3, D.2, D.5 |
| [OQ-35](#oq-35-dom-event-shape) | Ads | DOM event shape | Low | Answered | B.3.5 |
| [OQ-06](#oq-06-iscmprequired-source) | Consent | `isCMPRequired` source | High | Answered (`no-cmp` gives `false`); open: Overwolf (other values) | D.6.2 |
| [OQ-07](#oq-07-consent-pages-and-the-first-layer) | Consent | consent pages, first layer, `cmpURL` | High | Answered; open: Overwolf (skip when not required); pending harness (R3-5) | D.6.1, D.6.4 |
| [OQ-08](#oq-08-consent-cookies) | Consent | consent cookies | High | Answered | D.6.3 |
| [OQ-26](#oq-26-opencmpwindow-promise-timing) | Consent | `openCMPWindow` promise timing | Low | Answered | A.2.2 |
| [OQ-38](#oq-38-default-consent-written-by-the-first-settings-window-call) | Consent | default consent written by the first settings-window call | Medium | Answered (copied); open: Overwolf (intended?) | D.6.4 |
| [OQ-21](#oq-21-a-host-agnostic-package-runtime) | Packages | host-agnostic package runtime | High | Out of scope (1.0); open: Overwolf | H |
| [OQ-33](#oq-33-overlay-rendering-in-a-webview-host) | Packages | overlay rendering in a WebView host | High | Out of scope (1.0); open: Overwolf | H |
| [OQ-09](#oq-09-signing-and-integrity-for-tauri-builds) | Distribution | signing for Tauri builds | High | Decided; open: Overwolf (manifest, `fileHashes`, integrity target) | G.3 |
| [OQ-22](#oq-22-dev-mode) | Packages | dev mode | Medium | Closed (no package runtime; the CLI reads its own credentials) | G.3 |
| [OQ-23](#oq-23-failure-reasons) | Packages | failure reasons | Low | Out of scope (1.0) | H |
| [OQ-34](#oq-34-implicit-utility-package) | Packages | implicit `utility` package | Medium | Out of scope (1.0) | H |
| [OQ-36](#oq-36-package-object-lifetime) | Packages | package object lifetime | Medium | Out of scope (1.0) | H |
| [OQ-37](#oq-37-throw-or-reject-for-unlisted-package-names) | Packages | throw or reject for unlisted names | Low | Out of scope (1.0) | H |
| [OQ-15](#oq-15-gep-payload-details) | Packages | GEP payload details | Medium | Out of scope (1.0) | H |
| [OQ-16](#oq-16-overlay-game-launched-default) | Packages | overlay `game-launched` default | Low | Out of scope (1.0) | H |
| [OQ-25](#oq-25-crn) | Packages | CRN | Low | Out of scope (1.0) | H |
| [OQ-18](#oq-18-updates-and-the-console) | Distribution | updates and the console | High | Answered (feed; Windows NSIS only); open: Overwolf (Tauri installers) | I.1 |
| [OQ-24](#oq-24-installer-and-utm-parameters) | Distribution | installer and UTM parameters | Medium | Answered | F.2 |
| [OQ-31](#oq-31-version-delta) | Distribution | version delta since 42.7.1 | Medium | Answered | [PARITY.md](PARITY.md) |
| [OQ-39](#oq-39-javascript-dialogs) | Guest pages | `alert()`, `confirm()`, `prompt()` | Low | Closed (app webviews are the app's; guests are silenced) | D.3 |
| [OQ-40](#oq-40-a-corrupt-ow-electronjson) | State | a corrupt `ow-electron.json` | Medium | Decided (reset; `[]` and wrong types are a listed deviation) | F.2 |
| [OQ-41](#oq-41-the-install-record-of-a-per-machine-install) | Distribution | install record of a per-machine install (HKLM) | Medium | Open: Overwolf | I.6 |
| [OQ-42](#oq-42-a-uid-override-and-attribution) | Identity | a `uid` override and attribution | Medium | Open: Overwolf | G.2 |
| [OQ-43](#oq-43-installer-signing-expectations) | Distribution | installer signing expectations | Medium | Open: Overwolf | G.3, I.3 |
| [OQ-A1](#oq-a1-reward-ads) | Ad formats | reward ads: element, opt-in, grant signal, verification | High | Open: Overwolf | B.3.2, [AD-FORMATS.md](AD-FORMATS.md#reward) |
| [OQ-A2](#oq-a2-unit) | Ad formats | valid `unit` values; ignored on standard slots | Medium | Open: Overwolf | B.3.2, D.2 |
| [OQ-A3](#oq-a3-does-every-performance-ad-end-with-shutdown) | Ad formats | does every performance ad end with `shutdown` | Medium | Answered (no fill, error); open: Overwolf (dismiss, click) | B.3.4 |
| [OQ-A4](#oq-a4-house-ads-in-test-mode) | Ad formats | house ads in test mode | Low | Open: Overwolf | D.7 |
| [OQ-A5](#oq-a5-owadtestad) | Ad formats | which storage `owAdTestAd` needs | Low | Answered (test mode); open: Overwolf | D.7 |
| [OQ-A6](#oq-a6-interstitial-close-button-colours) | Ad formats | interstitial close-button colours | Low | Open: Overwolf | B.3.2 |
| [OQ-A7](#oq-a7-in-stream-ads) | Ad formats | in-stream ads | Low | Open: Overwolf | none |
| [OQ-A8](#oq-a8-970x90-in-test-mode) | Ad formats | 970x90 in test mode | Low | Answered | B.3.2 |
| [OQ-A9](#oq-a9-re-appending-a-removed-element) | Ad formats | re-appending a removed element (high-impact handler) | Medium | Answered (copied); open: Overwolf (intended?) | B.3.4 |
| [OQ-A10](#oq-a10-live-demand-for-demand-gated-formats) | Ad formats | live demand for high impact, interstitial and reward | High | Open: Overwolf | ADR 0005 |

## Identity and analytics

### OQ-01: uid rule for a string `author`, and the 42.11.2 change

- **Question.** How is the uid computed when `author` is a plain string or an
  npm-style `"Name <email> (url)"` string? Which name field is used?
- **Status.** Answered.
- **Answer.** `sha1("{'author':'<author>','name':'<name>.electron'}")`, each
  digest byte written as `'a' + (b & 15)` then `'a' + (b >> 4)`. `<name>` is
  the top-level `productName`, else `name`; `build.productName` is ignored. A
  string author is used verbatim (no npm parsing, nothing trimmed); an object
  author gives `author.name`; a missing, empty or non-string author gives
  `"unknown"`. 13 test vectors in CONTRACT G.2. The 42.11.4 runtime is the
  baseline; the 42.11.2 fix concerned Linux and macOS and is included in it.
- **Source.** Observed (40 `package.json` variants), cross-checked with the
  builder's `ow client calc-electron-uid`;
  https://dev.overwolf.com/ow-electron/getting-started/onboarding-resources/first-app#unique-app-id
  ("The productName (defaults to name if missing)"; the author is the name
  field under `author`);
  https://dev.overwolf.com/ow-electron/getting-started/changelog/ow-changelog
  (42.11.2 uid fix).
- **Remaining edge.** `author: { name: "" }` was not observed; it is treated
  as `"unknown"` (an inference, kept in the lab checks).

### OQ-02: muid derivation, `muidV2`, and phase bucketing for a Tauri host

- **Question.** How does ow-electron derive the machine id, what is `muidV2`,
  and how is the phase percent computed?
- **Status.** Answered on macOS and Windows. Linux: **open, pending
  harness** (R2-10, which needs a Linux host).
- **Answer.** Owner decision: same derivation as ow-electron; the
  per-install id becomes a non-parity option. macOS:
  `guid(sha256(lowercase(IOPlatformUUID)))`, `muidV2 = muid`; phase percent =
  the sum of the character codes of the MD5 hex of the muid without `-`,
  modulo 100. Windows: read and share the `MUID` / `MUIDV2` registry values,
  else derive from `MachineGuid` and write them (derivation inferred). Linux:
  derive from `/etc/machine-id` (inferred). Test vectors in CONTRACT E.4.
- **Source.** Observed (macOS, with stand-in platform ids); builder (the NSIS
  uninstaller reads `HKCU\Software\OverwolfElectron` `MUID` and
  `HKCU\Software\OverwolfPersist` `MUIDV2`); owner decision;
  https://dev.overwolf.com/ow-electron/developers-console/performance-statistics
  (users and installs are machine-keyed).
- **Windows (R2-10, Windows lab).** `MUIDV2` is a separate random v4 per
  install, and `app.overwolf.muid` answers it; analytics and guests keep the
  machine-derived `MUID` (CONTRACT E.4). Still unknown: whether
  `MachineGuid` is ow-electron's `MUID` source.
- **Open.** The Linux source (R2-10).

### OQ-03: host labelling (`owver`, `owVersion`, extra fields)

- **Question.** How should a non-Electron host identify itself in analytics
  and to the ad and consent pages? May it add host fields?
- **Status.** Decided. **Open: Overwolf** for the dashboard impact.
- **Answer.** Owner decision: wherever ow-electron says "electron", say
  "tauri", through one setting (`analytics.hostLabel`, default `tauri`, plus
  `analytics.hostVersion`). `owver`, `owVersion` and `oweVersion` become
  `tauri-<tauri version>`; ow-electron sends `42.11.4` / `42_11_4`. No extra
  host fields: ow-electron sends none. CONTRACT section 0 and
  [ADR 0006](adr/0006-analytics-labelling.md).
- **Source.** Owner decision; observed values.
- **Open with Overwolf.** Do console dashboards (DAU, window time, installs,
  uninstalls) or the ad and consent pages depend on the `electron_` names or a
  numeric version? `ads.owVersionOverride` exists if the pages do.

### OQ-42: a `uid` override and attribution

- **Question.** `plugins.overwolf.uid` lets an app use any uid, including the
  console-assigned one, which the formula cannot produce. Nothing stops an
  app from naming another app's uid. Is that a concern for attribution?
- **Status.** **Open: Overwolf.**
- **Answer for now.** ow-electron uses a `package.json` `overwolf.uid`
  verbatim, whoever wrote it. ow-tauri likewise sends whatever uid the
  config names. Binding a uid to its owner is up to Overwolf's
  console and signing (OQ-09). CONTRACT G.2.
- **Open with Overwolf.** Does Overwolf check that ads, analytics or updates
  for a uid come from that uid's signed app?

### OQ-04: analytics event catalogue

- **Question.** Which events, Kinds, fields, cadence and mandatory subset does
  a host send?
- **Status.** Answered. **Open, pending harness** for one detail.
- **Answer.** CONTRACT E.2: first launch (Counter + 400022), the
  `cmp-eu-only` request, start, launch heartbeat (Counter + 400023), first-show
  heartbeat (once per run), 400025 per ad guest, `window_closed`, the guest
  crash (Counter `sessionTS`, `reason`; Kind 400024 with the reason first) and
  `sub_info` (OQ-12), in the observed order and with the observed fields;
  InsertStats carries no event fields; the mandatory set under
  `disableAnonymousAnalytics()` is the first-launch Counter and both
  heartbeats. `window_closed` is sent at the end of each visible period
  (`hide()`, close, quit; nothing under 1 s), with the name taken from the
  last path segment of the URL loaded at first show (the `name` option is
  ignored, `.html` dropped, not truncated) and the constructor title. Names
  use the host label (OQ-03).
- **Source.** Observed (the first and second harness rounds);
  https://dev.overwolf.com/ow-electron/getting-started/onboarding-resources/ow-electron-technical-overview#app-usage-analytics
  (mandatory minimum);
  https://dev.overwolf.com/ow-electron/guides/product-guidelines/app-screen-behavior/window-names
  (window names; its 20-character limit is not applied by ow-electron
  42.11.4); builder (uninstall event, CONTRACT I.6).
- **R2-9 (answered).** A 13-hour session whose window was never shown sent
  nothing after the launch burst until 12 hours later, then one heartbeat
  Counter (`hasVisibleWindow: false`) and one 400023, and nothing in the
  hour after (CONTRACT E.2 #9). ow-tauri's hourly check with a 12-hour
  threshold matches it. Not settled by that run: a 12-hour timer against an
  hourly check (at most an hour apart), and whether the first-show heartbeat
  restarts the 12 hours.
- **Open.** R3-4 (a crash 2 s after a recovery is not reported; the
  threshold is between 3 and 20 s, interim 10 s).

### OQ-12: `setExternalPaymentUserId` report

- **Question.** What does `setExternalPaymentUserId` send, to which endpoint,
  with which fields?
- **Status.** Answered. **Open, pending harness** (R3-3) for the case with
  analytics disabled.
- **Answer.** One Counter, `electron_sub_info` (`<label>_sub_info` in
  ow-tauri), no InsertStats. `Extra` is the usual six fields, then the
  options as the app passed them, then `providerName: "tebex"` when the
  options had none. The promise resolves after the HTTP response; a missing
  `userId` rejects asynchronously with `providerName and userId are
  mandatory`; before ready it rejects with `ow-electron is not ready yet!`
  (CONTRACT A.2.2, E.2 #10).
- **Source.** Observed; typings;
  https://dev.overwolf.com/ow-electron/getting-started/changelog/ow-changelog
  (42.7.1: the options object is forwarded as sent).
- **Open.** R3-3: whether it is sent after `disableAnonymousAnalytics()`.
  Interim: sent.

### OQ-14: Windows ad-optimisation helper

- **Question.** The builder downloads `owutility.dll` unless
  `build.overwolf.disableAdOptimization` is set. Is there an interface a
  non-Electron host can use?
- **Status.** Decided. **Open: Overwolf.** Two details pending harness
  (R3-7, R3-8).
- **Answer.** Not shipped and not used: an undocumented native call cannot be
  replicated, and shipping a DLL that nothing loads adds nothing.
  `disableAdOptimization` drives `settings.disableOptimization` in the guest.
  `disableAdsOptimization()` and `disableAdsFPD()` send guests nothing and
  leave a running guest's `settings` unchanged; they set the host's
  `__settings__.adsOptimization` to `{ anonymous: true, disable: true }`
  (observed). The consent page's `getIsAdOptimizationEnabled()` was answered
  `false` (observed).
- **Source.** Builder; typings; observed.
- **Open with Overwolf.** The helper's host interface.
- **Open.** R3-7: whether a guest mounted after `disableAdsOptimization()`
  gets `disableOptimization: true` (interim: yes). R3-8: whether
  `enableAdOptimization(true)` changes the consent page's answer.

## Ads

### OQ-05: request shaping for the ad page

- **Question.** Which headers, `Origin`, `Referer` and user agent does the ad
  page need from the host, and with which web-security setting?
- **Status.** Answered. **Open: Overwolf** for the macOS gap.
- **Answer.** Owner decision: replicate exactly. ow-electron sends the
  document with `Referer: https://www.overwolf.com/<uid>` and
  `Origin: https://www.overwolf.com`, forces that `Origin` on every guest
  subresource (third-party frames included), adds `x-ow-uid`, `x-ow-phase`
  and `x-ow-window` to `owads.min.js`, runs guests with web security off, and
  uses the platform webview's default user agent with ow-electron's tokens
  (CONTRACT E.1). ow-tauri does all of it on Windows. On macOS only the
  document request can be shaped with public API. Linux has no ads in 1.0.
  CONTRACT D.8 and [ADR 0013](adr/0013-request-shaping-per-os.md).
- **Source.** Observed (test and live runs, and with app request hooks
  installed); owner decision.
- **Open with Overwolf.** Is the macOS gap (no subresource `Origin`, no
  `x-ow-*` headers, web security on) acceptable for fill and attribution? The
  uid, phase and window name still reach the server in the `owads.min.js`
  query string.

### OQ-10: email hash encoding

- **Question.** Are `generateUserEmailHashes` outputs lower-case hex or
  base64, and how are the keys spelled?
- **Status.** Answered. The gmail rule is **pending harness** (R3-6).
- **Answer.** Lower-case hex; lower-case keys in the order `sha1`, `md5`,
  `sha256`. The input is trimmed and lower-cased first; an input that is not
  an email address is hashed all the same. Vectors for
  `test.email@overwolf.com` in CONTRACT A.2.2.
- **Source.** Observed;
  https://dev.overwolf.com/ow-electron/monetization/advertising/user-identity
  (the page's example hashes match; the page spells the keys in upper case);
  typings (`EmailHashes`).
- **Open.** R3-6: whether ow-electron applies the UID2 gmail rule (remove `.`
  and `+suffix`). Interim: applied, as the typings' link describes.

### OQ-11: email hashes in the ad guest

- **Question.** How does `setUserEmailHashes` reach the ad page, and is
  `disableAdsFPD()` the only switch?
- **Status.** Answered. One detail **pending harness** (R3-3).
- **Answer.** As one `eHashes` host message to every existing ad guest,
  after `setUserEmailHashes(value)` and also after
  `generateUserEmailHashes()`. The data is the value as given, or `{}` when
  it is `undefined` or falsy; `generateUserEmailHashes()` sends
  `{ sha1, md5, sha256 }`. Never as a `__overwolf__` key; not resent when a
  guest reloads; no request. The value is also stored as `eHashes` in
  `ow-electron.json`, and `undefined` removes it (CONTRACT A.2.2, D.5, F.2).
  ow-tauri never scans user data for email addresses.
- **Source.** Observed;
  https://dev.overwolf.com/ow-electron/monetization/advertising/user-identity.
- **Open.** R3-3: what happens to hash calls after `disableAdsFPD()`.
  Interim: a value is ignored; `undefined` and `clearUserEmailHashes()`
  still remove the stored hashes and send `{}`.

### OQ-13: host-to-guest messages

- **Question.** Which messages does the host pass to the ad page's
  `__overwolf__.onmessage` handlers, with which payloads?
- **Status.** Answered, minimize included (R3-1). A restore with a live
  guest was not observed.
- **Answer.** These types (CONTRACT D.5): `consent` (twice per consent
  save: the TCF string, then the URL-encoded unified string; not resent after
  a reload), `customTracking` (on change, and again after every later reload),
  `eHashes` (OQ-11), `window-hidden` (on hide; nothing on show),
  `window-minimized` (on minimize, below), and `sendCommand` / `setPageUrl`
  when the app calls those element methods (OQ-32). On each
  load the host also mutes the guest and signals its visibility and focus;
  ow-tauri reproduces those inside the shim, not as messages. ow-tauri adds
  `ad-clicked` (OQ-17). `ads.legacyHostMessages` is removed.
- **Source.** Observed.
- **R3-1 (minimize).** ow-electron turns the guest document `hidden`,
  then sends `window-minimized` and `window-hidden` when the minimize ends;
  a running performance ad then dismisses itself and shuts down. ow-tauri
  sends the same in the same order (CONTRACT D.5) and stops WebKit's own
  `visibilitychange`, one event more than ow-electron's guest sees; its
  performance ad then dismisses itself and shuts down as in ow-electron
  [OBS: macOS lab perf-minimize, 4 of 4 runs on each host].
- **Open.** A restore with a live guest (not observed). Interim: nothing sent
  beyond the visibility.

### OQ-17: click and navigation rules

- **Question.** What gesture window should a host allow between a user gesture
  in the ad and a top-level navigation that it opens externally?
- **Status.** Decided.
- **Answer.** Popups and top-level navigations away from Overwolf's ad page
  always open in the system browser, and only after the operating system
  reports a user action on that ad: WebView2's user activation, or recent
  native input over the ad, on Windows; a native mouse-down or key press on
  the ad on macOS. One action allows one open within
  `guestLimits.activationWindowMs` (5000 ms), with at most 20 opens per
  minute per ad and 20 per app (CONTRACT D.7,
  [ADR 0020](adr/0020-native-gesture-authority.md)). ow-electron's own
  window cannot be measured without clicking an ad, which Overwolf's ad
  policy forbids.
- **Source.** Maintainer decision;
  https://dev.overwolf.com/ow-electron/monetization/advertising/overview
  (ad policy).

### OQ-19: `systemInfo` contents

- **Question.** Which `systemInfo` fields does the ad page get, and in which
  format?
- **Status.** Answered on macOS and Windows (Windows lab: one GPU entry per
  DXGI adapter with only `driverVersion`, display `name` = the monitor
  friendly name; CONTRACT D.2). Linux builds have no ad guests, so no
  Linux shape applies.
- **Answer.** `{ gpus: [{ name, model, driverVersion, vendor }], cpu: <brand string>, displays: [{ name, isMain, position, resolution, dpi }] }`,
  no `os`, `arch` or `scaleFactor` (CONTRACT D.2). This replaces the earlier
  privacy-reduced shape; Overwolf's privacy policy covers this data.
- **Source.** Observed (macOS); Overwolf's privacy policy ("device type,
  operating system, graphics card").

### OQ-20: live ads from a Tauri host

- **Question.** What is the approval path for serving live ads from an app
  that runs on ow-tauri?
- **Status.** Decided for labs. **Open: Overwolf** for production.
- **Answer.** Owner decision: labs may load live ads (at most 10 loads per
  run, every load logged, never clicked, windows invisible). The host itself
  follows ow-electron: live unless `--test-ad`
  ([ADR 0005](adr/0005-ads-test-live-parity.md)).
- **Source.** Owner decision;
  https://dev.overwolf.com/ow-electron/monetization/advertising/overview
  (Overwolf enables ads for an app after its QA passes).
- **Open with Overwolf.** Production enablement and QA for an app whose host
  is ow-tauri.

### OQ-27: viewability

- **Question.** What makes a guest "visible" to the ad page?
- **Status.** Answered.
- **Answer.** A guest whose embedder window was never shown loads and never
  fills; a shown window fills even at opacity 0. ow-electron signals the guest
  `hidden` for `display: none`, for an element scrolled out of the viewport and
  for a hidden window; resizing signals nothing; the window's position on the
  screen plays no part. After `hidden` the ad page stops and asks the host to
  reload it (3 to 5 s), then waits until it is visible again. ow-tauri keeps
  its element-level model (window shown, intersection, hidden ancestors) and
  passes the result to the guest the same way (CONTRACT B.3.4, D.5). The
  intersection line is measured: ow-electron reports a guest visible from
  half of it in view (49 % is hidden) on both axes, and ow-tauri matches it
  (harness `inview-probe`, `inview-fine`).
- **Source.** Observed;
  https://dev.overwolf.com/ow-electron/monetization/advertising/overview
  (containers stay visible; `display: none` pauses ads).
- **R3-1.** A minimized embedder's guests are hidden until the restore, in
  both hosts (CONTRACT B.3.4).

### OQ-28: crash and load-error recovery

- **Question.** How often, and after which delay, does the host reload a
  crashed or failed ad guest?
- **Status.** Answered. The crash-report threshold is **pending harness**
  (R3-4).
- **Answer.** A crashed guest is reloaded at once, with no cap; the element
  gets `render-process-gone`. A failed main-frame load is reloaded every
  5000 ms, with no cap, no backoff and no analytics; sub-frame failures reload
  nothing (CONTRACT D.7). `ads.maxRecoveries` now defaults to no cap. Guest
  crashes never reach the app's crash hooks (documented).
- **Source.** Observed;
  https://dev.overwolf.com/ow-electron/getting-started/changelog/ow-changelog.
- **Open.** R3-4 (OQ-04).

### OQ-29: performance ads

- **Question.** What geometry and input behaviour should a full-page
  performance ad have?
- **Status.** Answered (the third harness round and the ad-format lab).
- **Answer.** The guest covers the embedder window's content area and stays
  above every other ad guest; `adstyle` and `unit` are passed through. The
  element gets no shadow root, the inline style `pointer-events: none;` and
  one fixed full-viewport overlay `div`; input reaches the app under it until
  the first `performance_ad_loaded`, then the ad takes it. One per window: a
  second element is removed in the same task with no guest and no event. On
  `shutdown` the element is removed from the document in the next task and
  hears nothing more. The host enforces no minimum window size; the ad page
  answers a window under 500 x 500 with `performance_ad_error` and
  `shutdown`, and a no-fill with `shutdown` alone (CONTRACT B.3.2, B.3.4,
  [AD-FORMATS.md](AD-FORMATS.md#interstitial-performance-ads)).
- **Source.** Observed (third harness round; lab perf, perf-sample, perf-small,
  perf-twice, perf-remove, perf-with-standard, lab-layers);
  https://dev.overwolf.com/ow-electron/monetization/advertising/unique-ad-sizes/interstitial-ads.

### OQ-30: Linux

- **Question.** Are ads supported on Linux hosts?
- **Status.** Decided.
- **Answer.** No. Linux builds compile and run, with consent and
  analytics, but they create no ad guests: `<owadview>` reports
  `unsupported` and `getInfo().adsSupported` is `false` (CONTRACT section 0,
  "Platforms"). Overwolf documents ad services only for macOS and Linux
  outside Windows.
- **Source.** https://dev.overwolf.com/ow-electron/guides/dev-tools/non-windows-dev;
  maintainer decision.

### OQ-32: element extensions `pageUrl`, `setPageUrl`, `sendCommand`

- **Question.** Does `<owadview>` support `pageUrl`, `setPageUrl()` or
  `sendCommand()`?
- **Status.** Answered.
- **Answer.** Yes. The first harness round inspected the element only at
  creation, when it is a plain `HTMLElement`; the second showed that after attach ow-electron upgrades
  it (an `OwAdViewElement` prototype with Electron's `<webview>` methods plus
  `setPageUrl` and `sendCommand`, and own properties including `pageUrl`), and
  that the `pageurl` attribute becomes the guest's `__overwolf__.pageUrl`.
  ow-tauri defines `pageUrl`, `setPageUrl` and `sendCommand` on the element and
  reads `pageurl` (CONTRACT B.3.2, B.3.3, D.2); it does not provide the generic
  `<webview>` methods ([PARITY.md](PARITY.md#deviations)).
- **R3-2 (answered).** ow-electron forwards both to the running ad page as
  private messages, `{ type: 'sendCommand', data: [...args] }` and
  `{ type: 'setPageUrl', data: [url] }`; the test ad page showed no visible
  effect. ow-tauri forwards them the same way, and `setPageUrl` also sets
  `pageurl` for the next guest load (CONTRACT B.3.3, D.5).
- **Source.** Observed (second harness round, and the third round's
  `send-command-probe`); absent
  from the typings and the documentation.

### OQ-35: DOM event shape

- **Question.** Are `<owadview>` events plain `Event`s or `CustomEvent`s, and
  where is their data?
- **Status.** Answered.
- **Answer.** Plain, non-bubbling, non-cancelable `Event`s with the data
  copied as `Object.assign` copies it (an object's fields, an array's
  indexes, a string's characters) and `detail` `null`; `display_ad_loaded`
  fires twice per fill; `did-fail-load` also fires for sub-frames; an
  element moved after attach gets a plain `destroyed` (CONTRACT B.3.5).
- **Source.** Observed.

## Consent

### OQ-06: `isCMPRequired` source

- **Question.** What decides `isCMPRequired()`?
- **Status.** Answered. **Open: Overwolf** for the rule that gives `false`.
- **Answer.** ow-electron sends one
  `GET https://features.overwolf.com/experiments/cmp-eu-only` per launch, at
  startup, with no client timeout, and does not persist the answer. Every call
  awaits it and the load of the startup consent page that opens after it
  (CONTRACT D.6.1, D.6.2). Every response served resolved `true`: empty and
  non-empty `params`, `enabled: false`, HTTP errors, invalid JSON, a dropped
  connection. A `{}` body disables the cache: each call then requests again
  and opens a new startup consent window. ow-tauri does all of this, except
  that it stops waiting after `consent.euOnlyTimeoutMs` (60 s) and treats
  that as a failed request (a listed deviation in
  [PARITY](PARITY.md#deviations)).
- **Source.** Observed; typings ("will never throw an exception - the default
  value is true");
  https://dev.overwolf.com/ow-electron/reference/ads/consent-management-platform.
- **Answered (Windows lab, a US runner).** `{"params":["no-cmp"]}` makes it
  `false`; the startup window is not skipped but loads
  `ow-cmp-v2.html?clear=true`, which clears the stored consent (CONTRACT
  D.6.2). ow-tauri does the same.
- **Open with Overwolf.** Any other response that gives `false`.

### OQ-07: consent pages and the first layer

- **Question.** Which pages does ow-electron load for consent, when, and with
  which inputs? What must an app installed by its own installer show?
- **Status.** Answered. **Open: Overwolf** for the skip rule; one detail
  **pending harness** (R3-5).
- **Answer.** On every launch, once the `cmp-eu-only` request completes,
  ow-electron opens a hidden 1 x 32 window on `ow-cmp-v2.html` with
  `unifiedcmp, muid, uid, muidv2, oweVersion, appVersion`; on first launch the
  page generates a default Full consent; the page closes itself (CONTRACT
  D.6.1, [ADR 0015](adr/0015-startup-consent-window.md)). The settings window
  (`openAdPrivacySettingsWindow`, and the deprecated `openCMPWindow`) is a
  window titled `CMP`, 800 x 800, centred, not modal, showing a preloader and
  then `cmp.html` with
  `uid, appName, tabName, lang, firstRun, cmpRequired, muid, muidv2, oweVersion, appVersion`;
  closing it writes nothing; its first call of a launch also writes a fresh
  default consent (OQ-38) (CONTRACT D.6.4). `cmpURL` accepts any `https:` URL,
  and the consent globals are granted only under
  `https://content.overwolf.com/monsdk/electron/` (maintainer decision).
- **Source.** Observed;
  https://dev.overwolf.com/ow-electron/reference/ads/consent-management-platform
  (an app with its own installer shows the first layer; `openCMPWindow` is
  deprecated); typings (`cmpURL`).
- **Answered (OQ-06).** When consent is not required the startup window
  is not skipped: it loads `ow-cmp-v2.html?clear=true`.
- **Open.** R3-5: `firstRun` and `cmpRequired` in the settings query on a
  later launch (observed `true` on fresh profiles).

### OQ-08: consent cookies

- **Question.** How does the ad page get the user's consent?
- **Status.** Answered.
- **Answer.** Through `euconsent-v2` and `acconsent` cookies on
  `.overwolf.com` (path `/`, `Secure`, `SameSite=None`, 365 days), written by
  the startup consent page itself on every launch before the first ad
  document. The host writes no consent cookies, except as a fallback when a
  platform blocks the page's write (CONTRACT D.6.3). `consent` and
  `consentFull` in the guest are always `""`.
- **Source.** Observed.

### OQ-26: `openCMPWindow` promise timing

- **Question.** Does the promise from `openCMPWindow` /
  `openAdPrivacySettingsWindow` resolve on open or on close?
- **Status.** Answered.
- **Answer.** On open: it resolves once the window has been created (54 to
  308 ms after the call). A second call while the window is open focuses it
  and resolves at once (CONTRACT A.2.2, D.6.4).
- **Source.** Observed; typings (`Promise<void>`, no timing).

### OQ-38: default consent written by the first settings-window call

- **Question.** Is it intended that the first `openAdPrivacySettingsWindow()`
  or `openCMPWindow()` call of a launch replaces the user's stored consent
  with a new default?
- **Status.** Answered (copied). **Open: Overwolf.**
- **Answer.** ow-electron's first call also opens a hidden window on
  `ow-cmp-v2.html?unifiedcmp=&firstRun=true`; that page generates a new
  default consent string and saves it, overwriting the stored consent and
  both consent cookies. ow-tauri copies it (CONTRACT D.6.4), following the
  project's rule of replicating ow-electron.
- **Source.** Observed.
- **Open with Overwolf.** Intended, or a defect? If a defect, ow-tauri drops
  the window when Overwolf fixes it in ow-electron.

## Packages

ow-tauri 1.0 has no `packages` API: no GEP, overlay, recorder, utility or CRN
([ADR 0004](adr/0004-packages-backend-selection.md), CONTRACT H). The
questions below stay open for a future package runtime. Their answers
describe ow-electron as observed; ow-tauri implements none of it.

### OQ-21: a host-agnostic package runtime

- **Question.** Will Overwolf ship GEP, overlay, recorder, utility and CRN in a
  form a non-Electron host can load?
- **Status.** Out of scope (1.0). **Open: Overwolf.**
- **Answer for now.** No packages API in 1.0. A runtime would need a form
  of each package that a non-Electron host can load.
- **Source.** Owner decision; observed;
  https://dev.overwolf.com/ow-electron/guides/dev-tools/non-windows-dev.

### OQ-33: overlay rendering in a webview host

- **Question.** How would Overwolf's overlay runtime render a Tauri app's
  overlay windows (capture of an ordinary window, a composition API, or not at
  all)? WebView2 has no off-screen rendering mode.
- **Status.** Out of scope (1.0). **Open: Overwolf.**

### OQ-22: dev mode

- **Question.** How are the dev-mode credentials handled for a Tauri host?
- **Status.** Closed for 1.0.
- **Answer.** Dev mode unlocks packages, which 1.0 does not have. The plugin
  reads no credentials at run time. Only the `ow-tauri` CLI reads
  `OW_CLI_EMAIL` and `OW_CLI_API_KEY`, for signing (CONTRACT G.3).
- **Source.** https://dev.overwolf.com/ow-electron/guides/dev-tools/dev-mode.

### OQ-23: failure reasons

- **Question.** What does ow-electron emit for packages on hosts where they do
  not run?
- **Status.** Out of scope (1.0); answered for ow-electron.
- **Answer.** Nothing: no `loading`, `ready` or `failed-to-initialize`. The
  `failed-to-initialize` listener signature is `(event, packageName)`.
- **Source.** Observed (macOS, `gep` and `overlay` listed);
  https://dev.overwolf.com/ow-electron/reference/Overwolf-electron-APIs/packages/interfaces/OverwolfPackageManager#onfailed-to-initialize.

### OQ-34: implicit `utility` package

- **Question.** Should a host load `utility` whenever other packages are
  listed?
- **Status.** Out of scope (1.0).
- **Answer.** Yes, when a runtime exists: the documentation's `getChannel()`
  example lists `utility` next to the app's packages, and the builder adds it
  whenever any package is listed.
- **Source.** https://dev.overwolf.com/ow-electron/guides/dev-tools/package-channels;
  builder.

### OQ-36: package object lifetime

- **Question.** When does `app.overwolf.packages.<name>` exist?
- **Status.** Out of scope (1.0).
- **Answer.** Observed: `undefined` for every package on an ow-electron host
  without packages.
- **Source.** Observed.

### OQ-37: throw or reject for unlisted package names

- **Question.** Do `setChannel` and `getAvailableChannels` throw synchronously
  or reject?
- **Status.** Out of scope (1.0).
- **Answer.** Both reject asynchronously, with
  `setChannel - package '<name>' is not registered in this app` and
  `getAvailableChannels - package '<name>' is not registered in this app`,
  even for a listed name on a host without packages. `getChannel()` resolves
  `{}`; `hasPendingUpdates()` returns its object synchronously; `relaunch()`
  returns `undefined`.
- **Source.** Observed;
  https://dev.overwolf.com/ow-electron/guides/dev-tools/package-channels#error-cases.

### OQ-15: GEP payload details

- **Question.** What is the fourth `game-detected` argument, and are
  `new-info-update` / `new-game-event` values raw strings or parsed JSON?
- **Status.** Out of scope (1.0).

### OQ-16: overlay `game-launched` default

- **Question.** What happens if no listener calls `inject()` or `dismiss()`?
- **Status.** Out of scope (1.0).

### OQ-25: CRN

- **Question.** Where do CRN notifications come from, and what does a host need
  to provide?
- **Status.** Out of scope (1.0).

## Distribution

### OQ-09: signing and integrity for Tauri builds

- **Question.** What should Overwolf signing cover for a Tauri app (no asar,
  no Node entry file, no `package.json`), and how would a runtime verify it?
- **Status.** Decided (what can be done now). **Open: Overwolf** (manifest,
  `fileHashes`, integrity target).
- **Answer.** `ow-tauri sign` runs the published builder's flow:
  - `/sign/electron` for the console-assigned uid, with a `packageJson` body
    built from the merged Tauri config (`name`, `productName`, `version`,
    `author`, `overwolf.uid` when set, `main`);
  - `fileHashes` keyed by `--main`, else `plugins.overwolf.signing.entry`,
    else an error;
  - `_metadata.json`, `integrity.dll` and the `OWEINTEGRITY/OWE` resource,
    which the build step links into a Windows release build;
  - Authenticode with the developer's or Overwolf's certificate
    (`ow-tauri sign-exe`).

  The CLI checks that the signed uid equals the uid the app computes at run
  time; `--write-uid` pins it in `tauri.conf.json`. `/sign/asar` is not done
  and nothing is faked (CONTRACT G.3,
  [ADR 0016](adr/0016-signing-approach.md)). Ads and analytics never depend
  on signing.
- **Source.** Builder;
  https://dev.overwolf.com/ow-electron/guides/dev-tools/app-signing;
  https://dev.overwolf.com/ow-electron/developers-console/releases-management/app-keys.
- **Open with Overwolf.** Does `/sign/electron` accept a manifest built from
  Tauri config, and is `electronVersion` required? What are `fileHashes` used
  for, and which file should they cover in a Tauri app? What integrity target
  should a Tauri build have? Does `integrity.dll` have a host-agnostic
  interface?

### OQ-18: updates and the console

- **Question.** Where do updates come from, and will the console serve Tauri
  installers?
- **Status.** Answered for the feed. **Open: Overwolf** for Tauri
  installers.
- **Answer.** The console serves an electron-updater generic feed per app at
  `https://electron-updates.overwolf.com/electron-updates/electron/<app id>`,
  Windows only (`latest.yml` with `IsAdminRightsRequired` and
  `blockMapSize`; `latest-mac.yml` and `latest-linux.yml` return 404).
  ow-tauri's update client reads that feed and is Windows-only in 1.0. It
  installs NSIS `setup.exe` files only; MSI is unsupported. macOS and Linux
  apps use their own update path, such as `tauri-plugin-updater`
  (CONTRACT I.1).
- **Source.**
  https://dev.overwolf.com/ow-electron/developers-console/releases-management/release-management#setting-up-electron-auto-updates;
  observed (the public feed of the official sample's app id); owner decision.
- **Open with Overwolf.** Will the console accept a Tauri NSIS `setup.exe`
  through `ow electron upload` and serve it? Testing it needs an upload to a
  test channel, which needs the app owner's explicit approval.

### OQ-41: the install record of a per-machine install

- **Question.** Where should the install record live when a Tauri NSIS
  installer installs for all users?
- **Status.** **Open: Overwolf.**
- **Answer for now.** The installer hooks write `InstallLocation`, `version`
  and `ShortcutName` under `Software\OverwolfElectron\<uid>` in the install
  context, as Overwolf's builder does: HKCU for a per-user install (Tauri's
  default), HKLM for `bundle.windows.nsis.installMode: "perMachine"`, and
  either for `"both"` (CONTRACT I.6). The state folder and the machine ids stay per user.
- **Open with Overwolf.** Does anything at Overwolf read the record from
  HKLM, or should a per-machine install also write it under HKCU?

### OQ-43: installer signing expectations

- **Question.** What does Overwolf expect of the installer a Tauri app
  uploads: signed by the developer, by Overwolf, or either?
- **Status.** **Open: Overwolf.**
- **Answer for now.** ow-tauri does not assume an Overwolf certificate. A
  release build with the update client must name its own publisher
  (`updater.publisherNames`) or a minisign key (`updater.pubkey`), and the
  client fails closed on a mismatch (CONTRACT I.3). When the signing
  service enables Overwolf certificate signing for the app,
  `ow-tauri sign-exe` sends the app executable to it, as Overwolf's builder
  does (CONTRACT G.3).
- **Open with Overwolf.** Will installers served from the console be
  re-signed by Overwolf? If so, which certificate subject should apps put in
  `publisherNames`?

### OQ-24: installer and UTM parameters

- **Question.** How does UTM data reach an app?
- **Status.** Answered.
- **Answer.** `utmParams` is read from `ow-electron.json`, where Overwolf's
  installer writes it; when absent, `getInfo()` reports no `utmParams`
  (CONTRACT F.2). An app installed by a Tauri installer has none, like an
  ow-electron app with its own installer.
- **Source.** Observed; typings ("Overwolf installer provided UTM params").

### OQ-31: version delta

- **Question.** Did anything between ow-electron 42.7.1 and the current
  release change what a host must send?
- **Status.** Answered.
- **Answer.** The parity baseline is ow-electron 42.11.4, observed directly;
  the changelog lists only the 42.11.2 uid fix and Electron updates in
  between. The harness is re-run on every new `latest` release
  ([PARITY.md](PARITY.md#re-running-the-harness)).
- **Source.** Observed;
  https://dev.overwolf.com/ow-electron/getting-started/changelog/ow-changelog.

## Guest pages

### OQ-39: JavaScript dialogs

- **Question.** How should `alert()`, `confirm()` and `prompt()` behave?
- **Status.** Closed.
- **Answer.** ow-tauri adds no Electron API, so dialogs in the app's own
  webviews behave as Tauri and the platform webview make them behave. In ad
  guests and consent windows, the guest shim silences them: `alert()`
  returns at once, `confirm()` returns `false` and `prompt()` returns
  `null`, so an ad can never block the app (CONTRACT D.3).
- **Source.** Maintainer decision.

## State

### OQ-40: a corrupt `ow-electron.json`

- **Question.** What should the host do when `ow-electron.json` cannot be
  used?
- **Status.** Decided.
- **Answer.** Observed ow-electron behaviour:
  - Garbage, a truncated file, an empty file, `null` or a missing file:
    reset silently. `app_first_launch` is sent again, the consent page saves
    again, and the next launch is normal.
  - `[]`: never repaired. Consent is never stored, and `app_first_launch` is
    sent on every launch.
  - Wrong types (for example `cmp: 42`): never repaired. Every consent save
    fails, and no `app_first_launch` is sent.

  ow-tauri resets on a parse failure as ow-electron does: with no backup
  copy, and with the same requests. It resets `[]` and
  wrong-typed keys the same way. That sends one extra `app_first_launch`,
  then the next launches are normal. This is an intended deviation, listed
  in [PARITY](PARITY.md#deviations). One warning is logged. CONTRACT F.2.
- **Source.** Observed (`corrupt-state` scenarios); maintainer decision for
  `[]` and wrong types.

## Ad formats
## Ad formats

Questions from the ad-format work (the third harness round and the
ad-format lab). The
developer guide is [AD-FORMATS.md](AD-FORMATS.md). Sources:
https://dev.overwolf.com/ow-electron/monetization/advertising/overview and
the pages under it, the archived Performance and Reward ads page
(https://web.archive.org/web/20260317033924/https://dev.overwolf.com/ow-electron/monetization/advertising/unique-ad-sizes/performance-ads/),
the official sample, and observation.

### OQ-A1: reward ads

- **Question.** Is `adstyle="rewarded-ad;"` the supported way to ask for a
  reward ad? Is the `performance` element with a reward unit, from the
  archived documentation, still valid? Is "hide the slot, then show it" the
  intended opt-in, or is there a play command? Is `complete` the grant
  signal, and is there a server-side verification or postback? Can a demo
  app get a reward demo campaign?
- **Status.** **Open: Overwolf.**
- **Observed.** ow-electron 42.11.4 gives the rewarded flow to a slot of at
  least 400 x 300 whose `adstyle` contains `rewarded-ad;` (a substring
  match; `rewarded-ads;` works, `rewarded;` and `reward-ad;` do not). The
  page sends `video_ad_ready`, then `player_loaded`; nothing plays until
  the slot goes hidden and then visible; then `play`, `impression` and,
  about 41 s later, `complete`, then a new `video_ad_ready`. No reward,
  close or skip event exists, and `ad_uid` stays the same across cycles.
  A smaller slot gets no event at all.
- **Interim.** ow-tauri passes both paths through unchanged. The guide's
  grant rule: grant once on `complete` after a `play` from the same element,
  client-side only ([AD-FORMATS.md](AD-FORMATS.md#reward)).

### OQ-A2: `unit`

- **Question.** Which `unit` values are valid per app? On a standard slot it
  is ignored; is that intended?
- **Status.** **Open: Overwolf.**
- **Observed.** On a performance element `unit` becomes the ad library's
  `forceAdUnit`, verbatim, in test mode too; an unknown unit means no fill.
  A standard slot ignores it. ow-tauri passes it through unchanged (the
  earlier test-mode rewrite to `testAd` is removed; CONTRACT D.2).

### OQ-A3: does every performance ad end with `shutdown`

- **Question.** Does `performance_ad_dismiss` or `performance_ad_clicked`
  always end with `shutdown`?
- **Status.** Answered for no fill and for errors: both end with `shutdown`
  (no fill sends `shutdown` alone; a window under 500 x 500 sends
  `performance_ad_error`, then `shutdown`). **Open: Overwolf** for dismiss
  and click, which need a user's click and were never sent in the lab.
- **Interim.** No host watchdog: an overlay that never shuts down blocks
  the window the same way in both hosts.

### OQ-A4: house ads in test mode

- **Question.** Can house ads be served in test mode?
- **Status.** **Open: Overwolf.**
- **Observed.** In test mode the page requests the house-ad configuration
  for the uid (200), but no house creative was served in 120 s for an app
  with none set up in the Dev Console.

### OQ-A5: `owAdTestAd`

- **Question.** The documentation's `localStorage.owAdTestAd = true` switch
  is written for the app window's console. Which storage does it need under
  ow-electron, where the ad page runs in a guest of its own?
- **Status.** Answered for the observable part: set in the guest origin
  (`https://www.overwolf.com`), it gives the same result on both hosts in
  test mode [OBS lab L11]; live mode was not compared. **Open: Overwolf**
  for the intended use. ow-tauri never touches the guest's `localStorage`
  (CONTRACT D.7).

### OQ-A6: interstitial close-button colours

- **Question.** The ad library has `closeButtonColor` and
  `closeButtonHoverColor` for interstitials. Can an ow-electron app set
  them (for example through `adstyle`)?
- **Status.** **Open: Overwolf.** The documented `adstyle` keys are the
  background colour and blur; in every lab run the close-button colours
  stayed at their defaults (`rgb(182, 182, 182)`, hover
  `rgb(255,255,255)`) [OBS].

### OQ-A7: in-stream ads

- **Question.** How do ow-electron apps request in-stream ads, if at all?
- **Status.** **Open: Overwolf.** In-stream ads are documented only for
  ow-native's `OwAd`; `<owadview>` has no matching attribute or method in the
  typings or the documentation. ow-tauri adds nothing and claims no
  in-stream support.

### OQ-A8: 970x90 in test mode

- **Question.** Does the 970x90 container fill in test mode?
- **Status.** Answered: it fills, in ow-electron and in ow-tauri [OBS: lab
  sizes].

### OQ-A9: re-appending a removed element

- **Question.** The documented high-impact handler removes the small
  container and appends it again. In ow-electron 42.11.4 that kills the slot:
  an element removed after attach never attaches again. Is that intended,
  or a bug ow-tauri should not copy?
- **Status.** Answered (ow-tauri copies it: an element removed or moved
  after attach is dead; CONTRACT B.3.4). **Open: Overwolf** (intended?).
  Overwolf's own guidance is not to recycle containers, and the guide tells
  apps to hide with `display: none` instead
  ([AD-FORMATS.md](AD-FORMATS.md#high-impact)).

### OQ-A10: live demand for demand-gated formats

- **Question.** Would Overwolf qualify a demo app (or attach a demo deal) for
  high impact, interstitial and reward ads, so they can be shown live?
- **Status.** **Open: Overwolf.** These formats come from direct deals after
  DevRel qualifies the app (documentation); an unqualified uid gets no live
  fill in either host [OBS]. Test mode shows all three.
