# Open questions

ow-tauri replicates ow-electron. This file lists every question that came up
while specifying that, with its status, the answer and where the answer comes
from. Ids are stable; numbers are not an order.

Most questions are now settled: Overwolf's documentation answered some, black-box
observation of ow-electron 42.11.4 with the parity harness (rounds 1 and 2)
answered more ([PARITY.md](PARITY.md)), and the project owner decided the
rest. What is still open is either a question only Overwolf can answer, or a
detail that a harness item still has to observe: the 13-hour run (R2-9), the
Windows and Linux runs (R2-10, R2-11), or round 3 (R3-n).

Status:

| Status | Meaning |
|---|---|
| **Answered** | settled by Overwolf's documentation or by observing ow-electron; CONTRACT follows the answer |
| **Decided** | no source pins it down; the owner or the lead decided, following "copy ow-electron" wherever it is observable |
| **Open: Overwolf** | only Overwolf can answer; CONTRACT has a documented interim behaviour |
| **Open: pending harness** | observable; harness item `R2-n` or `R3-n` ([PARITY.md](PARITY.md#harness-rounds)) will settle it; CONTRACT has an interim behaviour |
| **Deferred** | belongs to the package runtime, which the scope cut defers ([ADR 0004](adr/0004-packages-backend-selection.md)) |

Sources are written as: *observed* (the parity harness against ow-electron
42.11.4, rounds 1 and 2, [PARITY.md](PARITY.md)), a `dev.overwolf.com` URL, *typings* (the
published ow-electron 42.11.4 typings), *builder* (the published
`@overwolf/app-builder-lib` 26.9.3 and `@overwolf/ow-cli` 0.1.10 JavaScript),
or *owner decision* (the project owner's round-2 decisions of 2026-10-06).

Impact: **High** blocks parity or Overwolf adoption; **Medium** changes data
Overwolf receives or a user-visible behaviour; **Low** affects edge cases.

## Index

| Id | Topic | Question | Impact | Status | CONTRACT |
|---|---|---|---|---|---|
| [OQ-01](#oq-01-uid-rule-for-a-string-author-and-the-42112-change) | Identity | uid rule for a string `author` | High | Answered | G.2 |
| [OQ-02](#oq-02-muid-derivation-muidv2-and-phase-bucketing-for-a-tauri-host) | Identity | muid derivation, `muidV2`, phases | High | Answered (macOS); open: pending harness (R2-10: Windows, Linux) | E.4 |
| [OQ-03](#oq-03-host-labelling-owver-owversion-extra-fields) | Analytics | host labelling | Medium | Decided; open: Overwolf (dashboards) | 0, E.1 |
| [OQ-04](#oq-04-analytics-event-catalogue) | Analytics | event catalogue | High | Answered; open: pending harness (R2-9 12 h heartbeat, R3-4 crash threshold) | E.2 |
| [OQ-12](#oq-12-setexternalpaymentuserid-report) | Analytics | `setExternalPaymentUserId` report | Medium | Answered; open: pending harness (R3-3, with analytics disabled) | A.2.2, E.2 |
| [OQ-14](#oq-14-windows-ad-optimisation-helper) | Analytics | Windows ad-optimisation helper | Medium | Decided; open: Overwolf; pending harness (R3-7, R3-8) | G.1, A.2.2 |
| [OQ-05](#oq-05-request-shaping-for-the-ad-page) | Ads | request shaping | High | Answered; open: Overwolf (macOS gap) | D.8 |
| [OQ-10](#oq-10-email-hash-encoding) | Ads | email hash encoding | Medium | Answered; open: pending harness (R3-6, gmail rule) | A.2.2 |
| [OQ-11](#oq-11-email-hashes-in-the-ad-guest) | Ads | email hashes in the guest | Medium | Answered; open: pending harness (R3-3, after `disableAdsFPD`) | A.2.2, D.5 |
| [OQ-13](#oq-13-host-to-guest-messages) | Ads | host-to-guest messages | Low to Medium | Answered; open: pending harness (R3-1, minimize) | D.5 |
| [OQ-17](#oq-17-click-and-navigation-rules) | Ads | click and navigation rules | Low | Decided | D.7 |
| [OQ-19](#oq-19-systeminfo-contents) | Ads | `systemInfo` contents | Low to Medium | Answered (macOS); open: pending harness (R2-10, R2-11: Windows, Linux) | D.2 |
| [OQ-20](#oq-20-live-ads-from-a-tauri-host) | Ads | live ads approval | High | Decided (labs); open: Overwolf (production) | ADR 0005 |
| [OQ-27](#oq-27-viewability) | Ads | viewability model | Medium | Answered; open: pending harness (R3-1, minimize) | B.3.4 |
| [OQ-28](#oq-28-crash-and-load-error-recovery) | Ads | guest recovery | Low | Answered; open: pending harness (R3-4, report threshold) | D.7, E.2 |
| [OQ-29](#oq-29-performance-ads) | Ads | performance ad geometry | Medium | Decided | B.3.4 |
| [OQ-30](#oq-30-linux) | Ads | Linux | Low | Decided | D.8.3 |
| [OQ-32](#oq-32-element-extensions-pageurl-setpageurl-sendcommand) | Ads | `pageUrl`, `setPageUrl`, `sendCommand` | Medium | Answered (present); open: pending harness (R3-2, behaviour) | B.3.2, B.3.3, D.2 |
| [OQ-35](#oq-35-dom-event-shape) | Ads | DOM event shape | Low | Answered | B.3.5 |
| [OQ-06](#oq-06-iscmprequired-source) | Consent | `isCMPRequired` source | High | Answered; open: Overwolf (the rule that gives `false`) | D.6.2 |
| [OQ-07](#oq-07-consent-pages-and-the-first-layer) | Consent | consent pages, first layer, `cmpURL` | High | Answered; open: Overwolf (skip when not required); pending harness (R3-5) | D.6.1, D.6.4 |
| [OQ-08](#oq-08-consent-cookies) | Consent | consent cookies | High | Answered | D.6.3 |
| [OQ-26](#oq-26-opencmpwindow-promise-timing) | Consent | `openCMPWindow` promise timing | Low | Answered | A.2.2 |
| [OQ-38](#oq-38-default-consent-written-by-the-first-settings-window-call) | Consent | default consent written by the first settings-window call | Medium | Answered (copied); open: Overwolf (intended?) | D.6.4 |
| [OQ-21](#oq-21-a-host-agnostic-package-runtime) | Packages | host-agnostic package runtime | High | Deferred; open: Overwolf | H, Appendix P |
| [OQ-33](#oq-33-overlay-rendering-in-a-webview-host) | Packages | overlay rendering in a WebView host | High | Deferred; open: Overwolf | Appendix P |
| [OQ-09](#oq-09-signing-and-integrity-for-tauri-builds) | Packages | signing for Tauri builds | High | Decided; open: Overwolf (integrity target) | G.4 |
| [OQ-22](#oq-22-dev-mode) | Packages | dev mode | Medium | Decided; use deferred | A.1 |
| [OQ-23](#oq-23-failure-reasons) | Packages | failure reasons | Low | Answered | H.1 |
| [OQ-34](#oq-34-implicit-utility-package) | Packages | implicit `utility` package | Medium | Decided; deferred | Appendix P.6 |
| [OQ-36](#oq-36-package-object-lifetime) | Packages | package object lifetime | Medium | Answered (no runtime); rest deferred | H.1, Appendix P.6 |
| [OQ-37](#oq-37-throw-or-reject-for-unlisted-package-names) | Packages | throw or reject for unlisted names | Low | Answered; no-name case pending harness (R3-9) | H.1 |
| [OQ-15](#oq-15-gep-payload-details) | Packages | GEP payload details | Medium | Deferred | Appendix P |
| [OQ-16](#oq-16-overlay-game-launched-default) | Packages | overlay `game-launched` default | Low | Deferred | Appendix P.4 |
| [OQ-25](#oq-25-crn) | Packages | CRN | Low | Deferred | Appendix P |
| [OQ-18](#oq-18-updates-and-the-console) | Distribution | updates and the console | High | Answered (feed); open: Overwolf (Tauri installers) | I.1 |
| [OQ-24](#oq-24-installer-and-utm-parameters) | Distribution | installer and UTM parameters | Medium | Answered | F.2 |
| [OQ-31](#oq-31-version-delta) | Distribution | version delta since 42.7.1 | Medium | Answered | [PARITY.md](PARITY.md) |

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
- **Status.** Answered on macOS. Windows and Linux: **open, pending harness**
  (R2-10, which needs a Windows and a Linux host).
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
- **Open.** R2-10: the Windows source and whether `MUIDV2` is a separate
  persisted id; the Linux source.

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

### OQ-04: analytics event catalogue

- **Question.** Which events, Kinds, fields, cadence and mandatory subset does
  a host send?
- **Status.** Answered. **Open, pending harness** for two details.
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
- **Source.** Observed (rounds 1 and 2);
  https://dev.overwolf.com/ow-electron/getting-started/onboarding-resources/ow-electron-technical-overview#app-usage-analytics
  (mandatory minimum);
  https://dev.overwolf.com/ow-electron/guides/product-guidelines/app-screen-behavior/window-names
  (window names; its 20-character limit is not applied by ow-electron
  42.11.4); builder (uninstall event, CONTRACT I.6).
- **Open.** R2-9 (periodic heartbeat cadence; a 13-hour run is in progress);
  R3-4 (a crash 2 s after a recovery is not reported; the threshold is between
  3 and 20 s, interim 10 s).

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
  uses the app UA. ow-tauri does all of it on Windows; on macOS only the
  document request can be shaped with public API; on Linux subresource
  shaping waits for a web-process extension. CONTRACT D.8 and
  [ADR 0013](adr/0013-request-shaping-per-os.md).
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
- **Answer.** As an `eHashes` host message (`{ sha1, md5, sha256 }`) to every
  existing ad guest, after `setUserEmailHashes()` and also after
  `generateUserEmailHashes()`; never as a `__overwolf__` key; not resent when a
  guest reloads; no request and no file (CONTRACT D.5). ow-tauri never scans
  user data for email addresses.
- **Source.** Observed;
  https://dev.overwolf.com/ow-electron/monetization/advertising/user-identity.
- **Open.** R3-3: what happens to hash calls after `disableAdsFPD()`.
  Interim: ignored.

### OQ-13: host-to-guest messages

- **Question.** Which messages does the host pass to the ad page's
  `__overwolf__.onmessage` handlers, with which payloads?
- **Status.** Answered. Minimize and restore **pending harness** (R3-1).
- **Answer.** Exactly four types (CONTRACT D.5): `consent` (twice per consent
  save: the TCF string, then the URL-encoded unified string; not resent after
  a reload), `customTracking` (on change, and again after every later reload),
  `eHashes` (OQ-11) and `window-hidden` (on hide; nothing on show). On each
  load the host also mutes the guest and signals its visibility and focus;
  ow-tauri reproduces those inside the shim, not as messages. ow-tauri adds
  `ad-clicked` (OQ-17). `ads.legacyHostMessages` is removed.
- **Source.** Observed.
- **R3-1 (minimize).** ow-electron sends `window-minimized` and
  `window-hidden` when the minimize ends, and the guest document turns
  `hidden` after both; a running performance ad then dismisses itself and
  shuts down. ow-tauri sends the same in the same order (CONTRACT D.5); with
  the hidden-page timer alignment (CONTRACT B.3.4) its performance ad
  dismisses itself and shuts down as in ow-electron [OBS: lab
  perf-minimize].
- **Open.** A restore with a live guest (not observed). Interim: nothing sent
  beyond the visibility.

### OQ-17: click and navigation rules

- **Question.** What gesture window should a host allow between a user gesture
  in the ad and a top-level navigation that it opens externally?
- **Status.** Decided (Low: copy the current behaviour).
- **Answer.** 1500 ms (`ads.gestureWindowMs`), popups always to the system
  browser, one open per gesture. It cannot be measured without clicking an
  ad, which Overwolf's ad policy forbids.
- **Source.** Lead decision;
  https://dev.overwolf.com/ow-electron/monetization/advertising/overview
  (ad policy).

### OQ-19: `systemInfo` contents

- **Question.** Which `systemInfo` fields does the ad page get, and in which
  format?
- **Status.** Answered on macOS. Windows and Linux GPU and display details
  **pending harness** (R2-10, R2-11, which need a Windows and a Linux host).
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
- **Status.** Answered. Minimize **pending harness** (R3-1).
- **Answer.** A guest whose embedder window was never shown loads and never
  fills; a shown window fills even at opacity 0. ow-electron signals the guest
  `hidden` for `display: none`, for an element scrolled out of the viewport and
  for a hidden window; resizing signals nothing; the window's position on the
  screen plays no part. After `hidden` the ad page stops and asks the host to
  reload it (3 to 5 s), then waits until it is visible again. ow-tauri keeps
  its element-level model (window shown, intersection, hidden ancestors) and
  passes the result to the guest the same way (CONTRACT B.3.4, D.5).
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
- **Status.** Decided.
- **Answer.** The guest covers the embedder window's content area;
  `adstyle` is passed through; one per window; the element closes after
  `shutdown`; the documented 1000 x 600 minimum window is not enforced
  (CONTRACT B.3.4).
- **Source.**
  https://dev.overwolf.com/ow-electron/monetization/advertising/unique-ad-sizes/interstitial-ads;
  lead decision.

### OQ-30: Linux

- **Question.** Are ads supported on Linux hosts?
- **Status.** Decided.
- **Answer.** Yes, on WebKitGTK; only ad services are supported on macOS and
  Linux. Request shaping on Linux covers the document request until a
  web-process extension exists (CONTRACT D.8.3).
- **Source.** https://dev.overwolf.com/ow-electron/guides/dev-tools/non-windows-dev;
  lead decision.

### OQ-32: element extensions `pageUrl`, `setPageUrl`, `sendCommand`

- **Question.** Does `<owadview>` support `pageUrl`, `setPageUrl()` or
  `sendCommand()`?
- **Status.** Answered (they exist). Their behaviour is **pending harness**
  (R3-2).
- **Answer.** Yes. Round 1 inspected the element only at creation, when it is
  a plain `HTMLElement`; round 2 showed that after attach ow-electron upgrades
  it (an `OwAdViewElement` prototype with Electron's `<webview>` methods plus
  `setPageUrl` and `sendCommand`, and own properties including `pageUrl`), and
  that the `pageurl` attribute becomes the guest's `__overwolf__.pageUrl`.
  ow-tauri defines `pageUrl`, `setPageUrl` and `sendCommand` on the element and
  reads `pageurl` (CONTRACT B.3.2, B.3.3, D.2); it does not provide the generic
  `<webview>` methods ([PARITY.md](PARITY.md#deviations)).
- **Source.** Observed; absent from the typings and the documentation.
- **Open.** R3-2: what `setPageUrl` and `sendCommand` do. Interim:
  `setPageUrl` sets `pageurl` for the next guest load; `sendCommand` does
  nothing.

### OQ-35: DOM event shape

- **Question.** Are `<owadview>` events plain `Event`s or `CustomEvent`s, and
  where is their data?
- **Status.** Answered.
- **Answer.** Plain, non-bubbling, non-cancelable `Event`s with the data as
  own properties and `detail` `null`; `display_ad_loaded` fires twice per
  fill; `did-fail-load` also fires for sub-frames (CONTRACT B.3.5).
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
  and opens a new startup consent window. ow-tauri does all of this.
- **Source.** Observed; typings ("will never throw an exception - the default
  value is true");
  https://dev.overwolf.com/ow-electron/reference/ads/consent-management-platform.
- **Open with Overwolf.** Which response, if any, makes it `false`, and is the
  startup consent window then skipped (OQ-07)?

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
  `https://content.overwolf.com/monsdk/electron/` (lead decision).
- **Source.** Observed;
  https://dev.overwolf.com/ow-electron/reference/ads/consent-management-platform
  (an app with its own installer shows the first layer; `openCMPWindow` is
  deprecated); typings (`cmpURL`).
- **Open with Overwolf.** Is the startup window skipped when consent is not
  required? (Not observable: OQ-06.)
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
  owner's rule of replicating ow-electron.
- **Source.** Observed.
- **Open with Overwolf.** Intended, or a defect? If a defect, ow-tauri drops
  the window when Overwolf fixes it in ow-electron.

## Packages

The scope cut defers the package runtime: `app.overwolf.packages` behaves as
ow-electron behaves where packages are unavailable (CONTRACT H).

### OQ-21: a host-agnostic package runtime

- **Question.** Will Overwolf ship GEP, overlay, recorder, utility and CRN in a
  form a non-Electron host can load?
- **Status.** Deferred (owner decision). **Open: Overwolf.**
- **Answer for now.** No package loads; no events; the observed results of
  ow-electron on an unsupported host (CONTRACT H.1). The proposed interface is
  kept as a deferred design (CONTRACT Appendix P).
- **Source.** Owner decision; observed;
  https://dev.overwolf.com/ow-electron/guides/dev-tools/non-windows-dev.

### OQ-33: overlay rendering in a webview host

- **Question.** How would Overwolf's overlay runtime render a Tauri app's
  overlay windows (capture of an ordinary window, a composition API, or not at
  all)? WebView2 has no off-screen rendering mode.
- **Status.** Deferred. **Open: Overwolf.**

### OQ-09: signing and integrity for Tauri builds

- **Question.** What should Overwolf signing cover for a Tauri app (no asar,
  no Node entry file), and how would a runtime verify it?
- **Status.** Decided (what can be done now). **Open: Overwolf** (integrity
  target).
- **Answer.** `ow-tauri sign` reproduces the published builder's flow:
  `/sign/electron` for the console-assigned uid and `_metadata.json`,
  `integrity.dll`, the `OWEINTEGRITY/OWE` resource, Authenticode with the
  developer's or Overwolf's certificate, and the builder's gating. `/sign/asar`
  is not done and nothing is faked (CONTRACT G.4,
  [ADR 0016](adr/0016-signing-approach.md)). Ads and analytics never depend on
  signing.
- **Source.** Builder;
  https://dev.overwolf.com/ow-electron/guides/dev-tools/app-signing;
  https://dev.overwolf.com/ow-electron/developers-console/releases-management/app-keys.
- **Open with Overwolf.** Does `/sign/electron` accept a non-Electron
  manifest, and is `electronVersion` required? What are `fileHashes` used for?
  What integrity target should a Tauri build have? Does `integrity.dll` have a
  host-agnostic interface?

### OQ-22: dev mode

- **Question.** How are the dev-mode credentials handled for a Tauri host?
- **Status.** Decided; use deferred.
- **Answer.** Read with the documented precedence (`OW_CLI_EMAIL` with
  `OW_CLI_API_KEY`, else `OW_DEV_KEY`) in debug builds only, never sent
  anywhere by ow-tauri, kept for a future package runtime (CONTRACT A.1).
- **Source.** https://dev.overwolf.com/ow-electron/guides/dev-tools/dev-mode.

### OQ-23: failure reasons

- **Question.** What does ow-electron emit for packages on hosts where they do
  not run?
- **Status.** Answered.
- **Answer.** Nothing: no `loading`, `ready` or `failed-to-initialize`. The
  `failed-to-initialize` listener signature is `(event, packageName)`
  (CONTRACT H.1).
- **Source.** Observed (macOS, `gep` and `overlay` listed);
  https://dev.overwolf.com/ow-electron/reference/Overwolf-electron-APIs/packages/interfaces/OverwolfPackageManager#onfailed-to-initialize.

### OQ-34: implicit `utility` package

- **Question.** Should a host load `utility` whenever other packages are
  listed?
- **Status.** Decided; deferred with the package runtime.
- **Answer.** Yes, when a runtime exists: the documentation's `getChannel()`
  example lists `utility` next to the app's packages, and the builder adds it
  whenever any package is listed. The earlier build warning is removed
  (CONTRACT Appendix P.6).
- **Source.** https://dev.overwolf.com/ow-electron/guides/dev-tools/package-channels;
  builder.

### OQ-36: package object lifetime

- **Question.** When does `app.overwolf.packages.<name>` exist?
- **Status.** Answered for hosts without packages; the lifetime with a
  runtime is deferred.
- **Answer.** `undefined` for every package while no package runtime exists,
  as in ow-electron (CONTRACT H.1). The proposed lifetime with a runtime is in
  CONTRACT Appendix P.6.
- **Source.** Observed.

### OQ-37: throw or reject for unlisted package names

- **Question.** Do `setChannel` and `getAvailableChannels` throw synchronously
  or reject?
- **Status.** Answered. The no-name case is **pending harness** (R3-9).
- **Answer.** Both reject asynchronously, with
  `setChannel - package '<name>' is not registered in this app` and
  `getAvailableChannels - package '<name>' is not registered in this app`,
  even for a listed name on a host without packages. `getChannel()` resolves
  `{}`; `hasPendingUpdates()` returns its object synchronously; `relaunch()`
  returns `undefined` (CONTRACT H.1).
- **Source.** Observed;
  https://dev.overwolf.com/ow-electron/guides/dev-tools/package-channels#error-cases.
- **Open.** R3-9: `getAvailableChannels()` with no names. Interim: resolves
  `{}`.

### OQ-15: GEP payload details

- **Question.** What is the fourth `game-detected` argument, and are
  `new-info-update` / `new-game-event` values raw strings or parsed JSON?
- **Status.** Deferred.

### OQ-16: overlay `game-launched` default

- **Question.** What happens if no listener calls `inject()` or `dismiss()`?
- **Status.** Deferred. The deferred design proposes dismiss (CONTRACT
  Appendix P.4).

### OQ-25: CRN

- **Question.** Where do CRN notifications come from, and what does a host need
  to provide?
- **Status.** Deferred.

## Distribution

### OQ-18: updates and the console

- **Question.** Where do updates come from, and will the console serve Tauri
  installers?
- **Status.** Answered for the feed. **Open: Overwolf** for Tauri
  installers.
- **Answer.** The console serves an electron-updater generic feed per app at
  `https://electron-updates.overwolf.com/electron-updates/electron/<app id>`,
  Windows only (`latest.yml` with `IsAdminRightsRequired` and
  `blockMapSize`; `latest-mac.yml` and `latest-linux.yml` return 404). The
  owner decided to use Overwolf's feed; macOS and Linux use a self-hosted
  feed of the same shape (CONTRACT I.1).
- **Source.**
  https://dev.overwolf.com/ow-electron/developers-console/releases-management/release-management#setting-up-electron-auto-updates;
  observed (the public feed of the official sample's app id); owner decision.
- **Open with Overwolf.** Will the console accept a Tauri NSIS `setup.exe`
  through `ow electron upload` and serve it? Testing it needs an upload to a
  test channel, which needs the app owner's explicit approval.

### OQ-24: installer and UTM parameters

- **Question.** How does UTM data reach an app?
- **Status.** Answered.
- **Answer.** `utmParams` is read from `ow-electron.json`, where Overwolf's
  installer writes it; when absent, `app.overwolf.utmParams` is `undefined`
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
