# Open questions for Overwolf

ow-tauri follows ow-electron wherever Overwolf's public documentation, the
published typings, the official sample, the published builder code or
observable behaviour pin the behaviour down. The questions below are the
places where they do not. Each lists why it matters and what ow-tauri does
until Overwolf answers. Every interim behaviour is configurable, so an answer
becomes a default change, not a redesign.

Impact: **High** blocks parity or Overwolf adoption; **Medium** changes data
Overwolf receives or a user-visible behaviour; **Low** affects edge cases.

Questions are grouped by topic below; numbers are stable ids, not an order.
Items marked "needs Overwolf confirmation" describe ow-tauri options that are
off by default until Overwolf confirms the behaviour.

## Index

| Id | Topic | Question | Impact | Interim (CONTRACT) |
|---|---|---|---|---|
| [OQ-01](#oq-01-uid-rule-for-a-string-author-and-the-42112-change) | Identity | uid rule for a string `author` | High | string used verbatim (G.2) |
| [OQ-02](#oq-02-muid-derivation-muidv2-and-phase-bucketing-for-a-tauri-host) | Identity | muid derivation, `muidV2`, phases | High | per-install muid (E.4) |
| [OQ-03](#oq-03-host-labelling-owver-owversion-extra-fields) | Analytics | host labelling | Medium | `tauri-<version>`, no extra fields (E.1) |
| [OQ-04](#oq-04-analytics-event-catalogue) | Analytics | event catalogue | High | E.2 |
| [OQ-12](#oq-12-setexternalpaymentuserid-report) | Analytics | `setExternalPaymentUserId` report | Medium | validated, nothing sent (A.2.2) |
| [OQ-14](#oq-14-windows-ad-optimisation-helper) | Analytics | Windows ad-optimisation helper | Medium | not shipped (G.1) |
| [OQ-05](#oq-05-request-shaping-for-the-ad-page) | Ads | request shaping | High | none; opt-in hook (A.1) |
| [OQ-10](#oq-10-email-hash-encoding) | Ads | email hash encoding | Medium | hex (A.2.2) |
| [OQ-11](#oq-11-email-hashes-in-the-ad-guest) | Ads | email hashes in the guest | Medium | off (D.2, D.5) |
| [OQ-13](#oq-13-host-to-guest-messages) | Ads | host-to-guest messages | Low to Medium | D.5 set only |
| [OQ-17](#oq-17-click-and-navigation-rules) | Ads | click and navigation rules | Low | 1500 ms gesture window (D.7) |
| [OQ-19](#oq-19-systeminfo-contents) | Ads | `systemInfo` contents | Low to Medium | no hardware models (D.2) |
| [OQ-20](#oq-20-live-ads-from-a-tauri-host) | Ads | live ads approval | High | test ads in every lab |
| [OQ-27](#oq-27-viewability) | Ads | viewability model | Medium | B.3.4 |
| [OQ-28](#oq-28-crash-and-load-error-recovery) | Ads | guest recovery | Low | D.7 |
| [OQ-29](#oq-29-performance-ads) | Ads | performance ad geometry | Medium | full embedder viewport, one per document (B.3.4) |
| [OQ-30](#oq-30-linux) | Ads | Linux | Low | ads on WebKitGTK |
| [OQ-32](#oq-32-element-extensions-pageurl-setpageurl-sendcommand) | Ads | `pageUrl`, `setPageUrl`, `sendCommand` | Medium | ow-tauri option, off (B.3) |
| [OQ-35](#oq-35-dom-event-shape) | Ads | DOM event shape | Low | `CustomEvent` with `detail` (B.3.5) |
| [OQ-06](#oq-06-iscmprequired-source) | Consent | `isCMPRequired` source | High | `true` (A.2.2) |
| [OQ-07](#oq-07-consent-pages-and-the-first-layer) | Consent | consent pages, first layer, `cmpURL` | High | D.6 |
| [OQ-08](#oq-08-consent-cookies) | Consent | consent cookies | High | written by the shim (D.6) |
| [OQ-26](#oq-26-opencmpwindow-promise-timing) | Consent | `openCMPWindow` promise timing | Low | resolves on close (A.2.2) |
| [OQ-21](#oq-21-a-host-agnostic-package-runtime) | Packages | host-agnostic package runtime | High | simulated / `unsupported-host` (H.1) |
| [OQ-33](#oq-33-overlay-rendering-in-a-webview-host) | Packages | overlay rendering in a WebView host | High | ordinary windows (B.1.4) |
| [OQ-09](#oq-09-signing-and-integrity-for-tauri-builds) | Packages | signing for Tauri builds | High | build warning (G.1) |
| [OQ-22](#oq-22-dev-mode) | Packages | dev mode | Medium | passed to the runtime (H.3) |
| [OQ-23](#oq-23-failure-reasons) | Packages | failure reasons | Low | `unsupported-host` (H.1) |
| [OQ-34](#oq-34-implicit-utility-package) | Packages | implicit `utility` package | Medium | list used as written (G.1) |
| [OQ-36](#oq-36-package-object-lifetime) | Packages | package object lifetime | Medium | defined from `loading` (B.1.3) |
| [OQ-37](#oq-37-throw-or-reject-for-unlisted-package-names) | Packages | throw or reject for unlisted names | Low | reject (B.1.3) |
| [OQ-15](#oq-15-gep-payload-details) | Packages | GEP payload details | Medium | raw strings (B.1.4) |
| [OQ-16](#oq-16-overlay-game-launched-default) | Packages | overlay `game-launched` default | Low | dismiss (H.5) |
| [OQ-25](#oq-25-crn) | Packages | CRN | Low | simulated only |
| [OQ-18](#oq-18-updates-and-the-console) | Distribution | updates and the console | High | any generic feed (I) |
| [OQ-24](#oq-24-installer-and-utm-parameters) | Distribution | installer and UTM parameters | Medium | read when present (F.2) |
| [OQ-31](#oq-31-version-delta) | Distribution | version delta since 42.7.1 | Medium | no change assumed |

## Identity and analytics

### OQ-01: uid rule for a string `author`, and the 42.11.2 change

- **Question.** How is the uid computed when `package.json` `author` is a plain
  string (the official sample uses `"Overwolf Ltd."`)? Is an npm-style
  `"Name <email> (url)"` string parsed? What did 42.11.2's "Fixed the UID
  calculation on Linux and macOS" change?
- **Impact.** High: a different uid makes a ported app a new app for the
  console, ads and analytics.
- **Interim.** The string is used verbatim; `author.name` for objects
  (CONTRACT G.2). Apps can pin the uid with `plugins.overwolf.uid`. We
  recommend checking with `ow client calc-electron-uid`.

### OQ-02: muid derivation, `muidV2`, and phase bucketing for a Tauri host

- **Question.** May a Tauri host use a per-install random machine id, or must
  it derive the muid from the OS machine identifier like ow-electron? If the
  latter, what is the exact derivation, and what is `muidV2` on Windows? Is the
  phase percent computed from the muid the value Overwolf expects for phased
  rollouts?
- **Impact.** High: user counting, phased rollouts and consent keying.
- **Interim.** `analytics.muidStrategy: "per-install"` (random UUID, stored in
  `ow-tauri.json`); `muidV2 = muid`; phase percent from the muid as in the
  reference implementation. `"machine-id"` exists as an option and stays
  disabled until Overwolf specifies it.

### OQ-03: host labelling (`owver`, `owVersion`, extra fields)

- **Question.** How should a non-Electron host identify itself in analytics
  `owver` and in the ad page's `owVersion`? May analytics carry extra
  `host`, `hostVersion`, `platform` fields (the InsertStats `Extra` field is
  positional)?
- **Impact.** Medium: dashboards and any version-dependent logic in the ad page.
- **Interim.** `owver` and `owVersion` = `tauri-<tauri version>`; extra fields
  off (`analytics.hostFields: false`). See ADR 0006.

### OQ-04: analytics event catalogue

- **Question.** Please confirm the event names, Kinds, fields, cadence and
  mandatory subset that a host must send (CONTRACT E.2), and whether the 42.11.x
  releases changed them.
- **Impact.** High: "same data" depends on it.
- **Interim.** CONTRACT E as implemented by the reference implementation.

### OQ-12: `setExternalPaymentUserId` report

- **Question.** What does `setExternalPaymentUserId` send, to which endpoint,
  with which fields?
- **Impact.** Medium: subscription attribution.
- **Interim.** Validates input and rejects exactly as typed
  (`'ow-electron is not ready yet!'` before ready); sends nothing.

### OQ-14: Windows ad-optimisation helper

- **Question.** The builder downloads `owutility.dll` unless
  `build.overwolf.disableAdOptimization` is set. What does a host do with it,
  and is there a documented interface a non-Electron host can use?
- **Impact.** Medium: ad performance on Windows; related analytics events.
- **Interim.** Not shipped and not used. `disableAdOptimization` only sets the
  guest's `settings.disableOptimization` default. Related events are not sent.

## Ads

### OQ-05: request shaping for the ad page

- **Question.** Does the ad page or ad server require specific request headers,
  `Origin`, `Referer` or user agent from the host, or a particular web-security
  setting in the guest? We observe that the ad script URL already carries the
  uid, phase and window name as query parameters; are those sufficient?
- **Impact.** High: fill rate and attribution, especially on macOS, where
  WKWebView has no public API to add headers to subresource requests.
- **Interim.** No extra headers; the platform's default user agent and
  security settings. An opt-in hook (`ads.requestShaping`) exists for Windows
  and Linux once a specification is available.

### OQ-11: email hashes in the ad guest

- **Question.** How should `setUserEmailHashes` reach the ad page (a field in
  `__overwolf__`, a host message, both), and is the FPD opt-out
  (`disableAdsFPD`) the only switch?
- **Impact.** Medium: first-party data targeting.
- **Interim.** The API is implemented and stores hashes for the session;
  delivery to guests is an ow-tauri option, off by default
  (`ads.exposeEmailHashesToGuest`), that needs Overwolf confirmation of the
  field and message names and payloads (CONTRACT D.2, D.5). ow-tauri never
  scans user data for email addresses.

### OQ-10: email hash encoding

- **Question.** Are `generateUserEmailHashes` outputs lower-case hex or base64?
- **Impact.** Medium: targeting quality and byte-for-byte parity.
- **Interim.** Hex (`emailHashes.encoding`), normalisation per the UID2 rules
  linked from the typings.

### OQ-13: host-to-guest messages

- **Question.** Which host-to-guest messages does the ad page expect from a
  host, with which payloads, and which of them are required?
- **Impact.** Low to Medium.
- **Interim.** Sends `ad-clicked`, `window-minimized`, `window-hidden`,
  `consent` and `customTracking` (CONTRACT D.5); `setPageUrl`, `sendCommand`
  and `eHashes` only as off-by-default options (OQ-32, OQ-11). No others.

### OQ-17: click and navigation rules

- **Question.** What gesture window should a host allow between a user gesture
  in the ad and a top-level navigation that it opens externally?
- **Impact.** Low: click attribution and accidental redirects.
- **Interim.** 1500 ms (`ads.gestureWindowMs`), popups always to the system
  browser.

### OQ-19: `systemInfo` contents

- **Question.** Which `systemInfo` fields does the ad page use (CPU model, GPU,
  display list), and in which format?
- **Impact.** Low to Medium.
- **Interim.** `{ os, arch, cpu: "<arch> (<n> logical cores)", displays[] }`, no
  hardware model strings (privacy by default).

### OQ-20: live ads from a Tauri host

- **Question.** What is the approval path for serving live ads from an app
  that runs on ow-tauri rather than ow-electron?
- **Impact.** High: monetisation.
- **Interim.** Live is technically identical to ow-electron (ADR 0005); every
  ow-tauri lab and CI job runs test ads only.

### OQ-27: viewability

- **Question.** Is element-level visibility (intersection at 50 %, hidden
  ancestors, minimized or hidden windows hide the guest) an acceptable
  viewability model for a host whose ads are native child webviews?
- **Impact.** Medium.
- **Interim.** As described in CONTRACT B.3.4.

### OQ-28: crash and load-error recovery

- **Question.** Should a host reload a crashed or failed ad guest, how often,
  and after which delay?
- **Impact.** Low.
- **Interim.** Up to 10 reloads per element per document, 5 s after a failed
  load (`ads.maxRecoveries`, `ads.loadErrorRetryMs`).

### OQ-29: performance ads

- **Question.** What geometry and input behaviour should a full-page
  performance ad have (for example click-through outside the creative)?
- **Impact.** Medium.
- **Interim.** A `performance` element mounts regardless of its own box; the
  guest covers the embedder webview's full viewport while it exists; one
  performance ad per document (CONTRACT B.3.4). Regular elements get a
  zero-specificity default style that makes them fill their container.

### OQ-32: element extensions `pageUrl`, `setPageUrl`, `sendCommand`

- **Question.** Does `<owadview>` support a `pageUrl` attribute and
  `setPageUrl()` / `sendCommand()` methods? If so, what do they deliver to the
  ad page (message names, payload shapes), and are they part of the
  supported developer API? They are not in the published typings,
  documentation or sample.
- **Impact.** Medium: page-level reporting and video controls.
- **Interim.** Needs Overwolf confirmation. Implemented only as an ow-tauri
  option, `ads.experimentalElementApi`, off by default: without it the
  methods are not defined and `pageurl` is ignored; with it, the host sends
  `setPageUrl` (URL string) and `sendCommand` (`{ command, args }`) messages
  whose payloads are interim (CONTRACT B.3.2, B.3.3, D.5).

### OQ-35: DOM event shape

- **Question.** Are `<owadview>` events plain `Event`s or `CustomEvent`s, and
  if they carry data, where (`detail`, properties on the event)?
- **Impact.** Low: listeners that read event data.
- **Interim.** Non-bubbling, non-cancelable `CustomEvent` with the guest's
  data in `detail` (CONTRACT B.3.5). Listeners written for a plain `Event`
  keep working.

### OQ-30: Linux

- **Question.** Overwolf documents that only ads are supported on Linux. Are
  ads served to Linux hosts in production?
- **Impact.** Low.
- **Interim.** Ads work on Linux with WebKitGTK; no special casing.

## Consent

### OQ-06: `isCMPRequired` source

- **Question.** What decides `isCMPRequired()`, and can a host query the same
  source?
- **Impact.** High: regulated regions must see the consent flow.
- **Interim.** `consent.cmpRequired: "always"` returns `true`, the documented
  default. An app may configure an endpoint URL.

### OQ-07: consent pages and the first layer

- **Question.** Which page should `openCMPWindow` load and which
  `openAdPrivacySettingsWindow`? Overwolf's installer shows the first consent
  layer on Windows; what must an app that is installed by another installer
  (a Tauri installer) show, and when?
- **Impact.** High.
- **Interim.** Both open `cmp/22.3.27/cmp.html` with the query in CONTRACT D.6;
  `cmpURL` overrides. With `consent.gateAdsOnConsent`, the first ad waits for
  consent where it is required.
- **Also.** The typings put no restriction on `CMPWindowOptions.cmpURL`.
  ow-tauri accepts only URLs under
  `https://content.overwolf.com/monsdk/electron/`, as a deliberate security
  choice: the consent window's single command is granted to that scope only,
  so a page elsewhere could not save consent or close itself (ADR 0011). Is
  that scope right, or should other Overwolf consent hosts be allowed?

### OQ-08: consent cookies

- **Question.** Is writing the consent cookies (`euconsent-v2`, `acconsent`) on
  `.overwolf.com` from the ad guest's own document an acceptable way to give
  the ad page the user's consent?
- **Impact.** High.
- **Interim.** Yes, as in the reference implementation, plus a `consent`
  message to running guests.

### OQ-26: `openCMPWindow` promise timing

- **Question.** When does the promise from `openCMPWindow` /
  `openAdPrivacySettingsWindow` resolve: on open or on close?
- **Impact.** Low.
- **Interim.** On close.

## Packages

### OQ-21: a host-agnostic package runtime

- **Question.** Will Overwolf ship GEP, overlay, recorder, utility and CRN in a
  form a non-Electron host can load? We propose the interface in CONTRACT H
  (Rust trait, JSON-RPC sidecar, C ABI) and are happy to adapt it.
- **Impact.** High: all gaming features.
- **Interim.** Simulated backends in debug builds; `failed-to-initialize` with
  `reason: "unsupported-host"` in release builds. Overlay rendering is a
  separate question (OQ-33).

### OQ-33: overlay rendering in a webview host

- **Question.** ow-electron's overlay windows can render off-screen and share
  textures with the game (`useSharedTexture`, `dpiAware`, `enableIsolation`).
  WebView2 has no off-screen rendering mode. How would Overwolf's overlay
  runtime render a Tauri app's overlay windows: by capturing an ordinary
  window it creates through the host (CONTRACT H.2 `create_window` and
  `native_window_handle`), through a composition API, or not at all?
- **Impact.** High for in-game overlays.
- **Interim.** Simulated overlay windows are ordinary transparent,
  always-on-top windows; the off-screen options are ignored (CONTRACT B.1.4).

### OQ-09: signing and integrity for Tauri builds

- **Question.** What should Overwolf signing hash for a Tauri app (there is no
  asar and no Node entry file), and how would a runtime verify it?
- **Impact.** High for packages; none for ads and analytics.
- **Interim.** `build.overwolf.requireSigning` and `enableOWCertSigning` are
  validated and produce a build warning. Nothing is signed or faked.

### OQ-22: dev mode

- **Question.** How should a native runtime verify the dev-mode credentials
  (`OW_DEV_KEY`, or `OW_CLI_EMAIL` with `OW_CLI_API_KEY`) for a Tauri host?
- **Impact.** Medium (development of gaming features).
- **Interim.** Credentials are passed to the runtime in `initialize` and
  never used by ow-tauri itself.

### OQ-23: failure reasons

- **Question.** What does ow-electron emit for packages on platforms where they
  do not run: `failed-to-initialize` with which `reason`, or no events?
- **Impact.** Low: UI text.
- **Interim.** `loading`, then `failed-to-initialize` with
  `{ reason: "unsupported-host", version }`.

### OQ-34: implicit `utility` package

- **Question.** The builder adds `utility` to the package list whenever any
  package is listed. Does the runtime rely on `utility` being loaded whenever
  other packages are, and should a host do the same when it loads packages
  at runtime?
- **Impact.** Medium: an app that lists `gep` only would get no `utility`
  events on ow-tauri.
- **Interim.** The list is used as written; the build helper warns when
  packages are listed without `utility` (CONTRACT G.1).

### OQ-36: package object lifetime

- **Question.** When does `app.overwolf.packages.<name>` exist: before the
  package's `ready`, only after it, and after `failed-to-initialize`? What do
  its members do before `ready`, after a crash, and after a hot update
  (`updated`)?
- **Impact.** Medium: startup code such as the sample's `UtilityService`
  reads the object before `ready`.
- **Interim.** Defined from `loading` until the app quits; async members
  reject `not-ready` until `ready` and after a failure or crash; sync members
  return empty values; the object emits its own `ready(version)` before the
  manager's `ready` (CONTRACT B.1.3).

### OQ-37: throw or reject for unlisted package names

- **Question.** The typings say `setChannel` and `getAvailableChannels`
  "throw" for a package not listed in `overwolf.packages`, but both return
  promises. Is that a synchronous throw or a rejected promise?
- **Impact.** Low: code that does not `await` inside `try`.
- **Interim.** Both reject with `invalid-argument` (CONTRACT A.2.4, B.1.3).

### OQ-15: GEP payload details

- **Question.** What is the fourth `game-detected` argument (`gameInfo`)? Are
  `new-info-update` and `new-game-event` `value`s raw strings or parsed JSON?
- **Impact.** Medium for app logic.
- **Interim.** Simulated GEP passes an overlay-style `GameInfo` and raw
  strings.

### OQ-16: overlay `game-launched` default

- **Question.** What happens if no listener calls `inject()` or `dismiss()`?
- **Impact.** Low.
- **Interim.** Dismissed after all listeners settle (10 s at most).

### OQ-25: CRN

- **Question.** Where do CRN notifications come from, and what does a host need
  to provide (windows, endpoints)?
- **Impact.** Low.
- **Interim.** Simulated only, from scenarios.

## Distribution

### OQ-18: updates and the console

- **Question.** Will the console accept Tauri installers (NSIS, MSI, macOS zip)
  through `ow electron upload` and serve them in the electron-updater generic
  feed? Or should Tauri apps use a different feed?
- **Impact.** High for distribution.
- **Interim.** The built-in client reads any generic feed (CONTRACT I).

### OQ-24: installer and UTM parameters

- **Question.** Overwolf's installer writes `utmParams` for ow-electron apps.
  How does UTM data reach an app installed with a Tauri installer?
- **Impact.** Medium for attribution.
- **Interim.** Read from `ow-electron.json` when present; otherwise `null`.

### OQ-31: version delta

- **Question.** The reference implementation's analytics and ad behaviour was
  built while ow-electron 42.7.1 was current; the current release is 42.11.4.
  Did anything in between change what a host must send?
- **Impact.** Medium.
- **Interim.** No changes assumed; revisit with each ow-electron release.
