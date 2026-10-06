# Open questions for Overwolf

ow-tauri follows ow-electron wherever Overwolf's public documentation, the
published typings, the official sample, the published builder code or
observable behaviour pin the behaviour down. The questions below are the
places where they do not. Each lists why it matters and what ow-tauri does
until Overwolf answers. Every interim behaviour is configurable, so an answer
becomes a default change, not a redesign.

Impact: **High** blocks parity or Overwolf adoption; **Medium** changes data
Overwolf receives or a user-visible behaviour; **Low** affects edge cases.

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
  delivery to guests is off by default (`ads.exposeEmailHashesToGuest`). ow-tauri
  never scans user data for email addresses.

### OQ-10: email hash encoding

- **Question.** Are `generateUserEmailHashes` outputs lower-case hex or base64?
- **Impact.** Medium: targeting quality and byte-for-byte parity.
- **Interim.** Hex (`emailHashes.encoding`), normalisation per the UID2 rules
  linked from the typings.

### OQ-13: host-to-guest messages beyond the reference set

- **Question.** Which host messages does the ad page expect a host to send
  (for example installed games or hardware summaries), with which payloads,
  and are any of them required?
- **Impact.** Low to Medium.
- **Interim.** Sends `ad-clicked`, `window-minimized`, `window-hidden`,
  `consent`, `customTracking`, `setPageUrl`, `sendCommand` (CONTRACT D.5).

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
- **Interim.** The guest covers the embedder webview while the creative is
  showing; one performance ad per document.

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
  `reason: "unsupported-host"` in release builds.

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
