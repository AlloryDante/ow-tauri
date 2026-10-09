# JavaScript API

`tauri-plugin-overwolf-api` is the browser-side API of the plugin. It is
ESM-only, has no Node dependencies, and runs in your app's webviews.

```sh
npm add tauri-plugin-overwolf-api@1.0.0-rc.1
```

| Import | Contents |
|---|---|
| `tauri-plugin-overwolf-api` | identity, consent, email hashes, analytics switches, window name, errors |
| `tauri-plugin-overwolf-api/adview` | the `<owadview>` runtime (side-effect import), [owadview.md](owadview.md) |
| `tauri-plugin-overwolf-api/updater` | the Windows updater: `check()`, `Update` |
| `tauri-plugin-overwolf-api/testing` | `mockOverwolf()` for unit tests, [testing.md](testing.md) |
| `tauri-plugin-overwolf-api/jsx` | types only: `<owadview>` in React JSX |

Every function calls one plugin command. The command runs only in a webview
whose capability grants its permission ([permissions.md](permissions.md)) and
that shows a local app page. Every function rejects with an
[`OverwolfError`](#errors), except `isCMPRequired()`, which never rejects.

The generated reference (typedoc) lists every type and member:
`npm run docs` at the repository root writes it to
`packages/api/docs-out/`.

## Contents

- [Identity](#identity)
- [Consent](#consent)
- [Email hashes](#email-hashes)
- [Analytics switches](#analytics-switches)
- [Window name](#window-name)
- [Updater](#updater)
- [Errors](#errors)
- [Types](#types)
- [JSX types](#jsx-types)

## Identity

### `getInfo(): Promise<OverwolfInfo>`

The app's Overwolf identity and the plugin's state. Permission:
`overwolf:default`.

```ts
import { getInfo } from 'tauri-plugin-overwolf-api';

const { uid, testAd, adsSupported } = await getInfo();
```

| Field | Type | Value |
|---|---|---|
| `uid` | `string` | the app uid: `plugins.overwolf.uid`, else ow-electron's formula |
| `appCuid` | `string` | the computed uid, even when `uid` is pinned |
| `phasePercent` | `number` | this machine's rollout bucket, 0 to 99 |
| `utmParams` | `Record<string, string> \| null` | the UTM parameters recorded at install |
| `testAd` | `boolean` | test ads are on |
| `adsSupported` | `boolean` | `<owadview>` can show ads here (`false` on Linux, without the `ads` feature, and below the WebView2 minimum) |
| `name` | `string` | the app name Overwolf sees (`plugins.overwolf.name`, else `productName`) |
| `version` | `string` | the app version (`tauri.conf.json` `version`) |
| `host` | `HostInfo` | `{ label, version, owVersion }`: the host label (default `"tauri"`), its version (default the Tauri version), and the runtime version the ad pages receive (`"<label>-<version>"`) |

### `getMachineIds(): Promise<MachineIds>`

The machine ids every Overwolf app on this machine shares. Permission:
**`overwolf:machine-id`** (opt-in); without it the call rejects with
`forbidden`.

| Field | Value |
|---|---|
| `muid` | ow-electron's `app.overwolf.muid`: `muidV2` when present, else the first-generation id |
| `muidV2` | the second-generation machine id |

These ids identify the machine across apps. Ask for them only when you need
them.

## Consent

### `isCMPRequired(): Promise<boolean>`

Whether the user must be asked for ad consent. It never rejects: when the
answer is unknown (no network, a timeout) it resolves `true`, as ow-electron
does. Show an "Ad privacy settings" entry when it is `true`. Permission:
`overwolf:default`.

### `openAdPrivacySettingsWindow(options?: CMPWindowOptions): Promise<void>`

Opens Overwolf's ad privacy settings window. Resolves when the window
exists. Permission: `overwolf:default`.

```ts
await openAdPrivacySettingsWindow({ tab: 'vendors', modal: true });
```

| Option | Type | Meaning |
|---|---|---|
| `tab` | `'purposes' \| 'features' \| 'vendors'` | the tab to open |
| `modal` | `boolean` | owned by the parent window and kept above it |
| `parent` | `string` | the Tauri label of the parent window; with `modal` and no `parent`, the caller's window |
| `center` | `boolean` | centre the window |
| `backgroundColor` | `string` | window background (CSS colour) |
| `preLoaderSpinnerColor` | `string` | colour of the loading spinner |
| `width`, `height` | `number` | size in logical pixels (default 800 x 800) |
| `x`, `y` | `number` | position in logical pixels |
| `cmpURL` | `string` | another consent page; its origin must be in `consent.allowedCmpOrigins` |
| `language` | `string` | the page language, for example `"de"` |

Rejects with `invalid-argument` for a bad option or a `cmpURL` whose origin
is not allowed, and `not-found` for an unknown `parent`.

### `openCMPWindow(options?: CMPWindowOptions): Promise<void>`

Deprecated ow-electron name of `openAdPrivacySettingsWindow`. Opens the same
window.

## Email hashes

All three need **`overwolf:email-hashes`** (opt-in). The hashes are personal
data: they are sent to Overwolf's ad pages and stored as `eHashes` in
`ow-electron.json`, as ow-electron does.

### `generateUserEmailHashes(email: string): Promise<EmailHashes>`

Hashes the address in the plugin (the same bytes ow-electron produces),
sends the hashes to the ads and stores them. The address itself is never
stored or logged. Returns `{ sha1?, md5?, sha256? }`, lower-case hex by
default (`emailHashes.encoding` in [CONFIG.md](../CONFIG.md#emailhashes)).

### `setUserEmailHashes(hashes?: EmailHashes): Promise<void>`

Sends and stores hashes your app computed. Empty hashes are ignored. Calls
after `disableAdsFPD()` are ignored with one warning in the log. Without an
argument the call does nothing.

### `clearUserEmailHashes(): Promise<void>`

Forgets the hashes and removes `eHashes` from `ow-electron.json`. Rejects
with `io` or `backend` when the file cannot be written.

## Analytics switches

| Function | Permission | Effect |
|---|---|---|
| `disableAnonymousAnalytics()` | `overwolf:default` | from now on, only the mandatory analytics are sent |
| `disableAdsOptimization()` | `overwolf:default` | turns ad optimisation off for this launch |
| `disableAdsFPD()` | `overwolf:default` | no first-party data reaches the ads for this launch |
| `setAnonymousAnalyticsPreference(enabled: boolean)` | `overwolf:analytics` | stores the user's choice in `ow-tauri.json`; it applies from the next launch's first requests |
| `setAnalyticsUserEnabled(enabled: boolean)` | `overwolf:analytics` | the app-level user switch; rejects with `unsupported` unless `analytics.userSwitch` is `true` |
| `setExternalPaymentUserId(options)` | `overwolf:analytics` | sends one `sub_info` event with the user's id at a payment provider |

All return `Promise<void>`.

**Timing matters for `disableAnonymousAnalytics()`.** In ow-electron an app
can call it before the app is ready, so even the launch requests are
reduced. Your page always runs after the launch requests were sent, so a call
from JavaScript affects only what follows, and the plugin logs one warning:

```text
too late for this launch's burst; use plugins.overwolf.analytics.disableAnonymous, the Builder, or setAnonymousAnalyticsPreference(false)
```

To honour a user's opt-out from the next launch on:

```ts
await setAnonymousAnalyticsPreference(false); // next launches
await disableAnonymousAnalytics(); // the rest of this launch
```

To turn it off for every user, use `analytics.disableAnonymous` in
[CONFIG.md](../CONFIG.md#analytics) or `Builder::disable_anonymous_analytics()`
in Rust.

`setExternalPaymentUserId` takes `{ providerName?, userId, paymentId? }`.
`userId` is required (a non-empty string or a number); otherwise it rejects
with `invalid-argument` and the message `providerName and userId are
mandatory`. The keys are sent in the order you wrote them, as ow-electron
sends them. It resolves after Overwolf answered.

## Window name

### `setWindowName(name: string): Promise<void>`

Names the caller's own window for Overwolf: the `name` of the
`window_closed` analytics event and the `windowName` the ad pages receive.
Permission: `overwolf:default`.

You rarely need it. By default the plugin derives the name from the page URL
the way ow-electron does (`index.html` gives `index`). ow-electron has no
such call, so a name you set here is a difference from ow-electron. `name`
is 1 to 128 printable ASCII characters; otherwise the call rejects with
`invalid-argument`.

## Updater

```ts
import { check } from 'tauri-plugin-overwolf-api/updater';

const update = await check();
if (update) {
  let downloaded = 0;
  await update.downloadAndInstall((event) => {
    if (event.event === 'Started') console.log(`size ${event.data.contentLength ?? '?'}`);
    if (event.event === 'Progress') downloaded += event.data.chunkLength;
    if (event.event === 'Finished') console.log(`downloaded ${downloaded}`);
  });
}
```

Windows only, with the crate's `updater` feature. Permission:
**`overwolf:updater`** (opt-in). Elsewhere `check()` rejects with
`unsupported`; use `@tauri-apps/plugin-updater` on macOS and Linux. Setup:
[CONFIG.md](../CONFIG.md#updater).

### `check(options?: CheckOptions): Promise<Update | null>`

Reads Overwolf's update feed for the app. Resolves `null` when the app is up
to date.

| Option | Meaning |
|---|---|
| `channel` | a feed channel other than `latest` |
| `allowDowngrade` | accept an older version |
| `allowPrerelease` | accept pre-release versions |
| `timeout` | request timeout, in ms |

`channel` and `allowDowngrade` never cause a downgrade unless
`updater.allowJsDowngrade` is `true`. A page cannot change the feed URL, the
publisher check or the installer arguments.

### `Update`

| Member | Meaning |
|---|---|
| `version` | the version on the feed |
| `currentVersion` | the running version |
| `date?`, `body?` | release date and notes from the feed |
| `raw` | the feed entry as published |
| `download(onEvent?)` | downloads the installer and checks its hash and signer |
| `install()` | checks the installer again, starts it and exits the app |
| `downloadAndInstall(onEvent?)` | both |
| `close()` | releases the plugin resource (an install does it too) |

`onEvent` receives `DownloadEvent`s: `{ event: 'Started', data: {
contentLength? } }`, `{ event: 'Progress', data: { chunkLength } }`, then
`{ event: 'Finished' }`. A failed hash or signer check rejects with
`verification`.

## Errors

### `OverwolfError`

```ts
import { OverwolfError, getMachineIds } from 'tauri-plugin-overwolf-api';

try {
  await getMachineIds();
} catch (error) {
  if (error instanceof OverwolfError && error.code === 'forbidden') {
    // the capability does not grant overwolf:machine-id
  }
}
```

| Member | Meaning |
|---|---|
| `code` | one of the codes below; stable across releases |
| `message` | one English sentence |
| `data` | code-specific details, for example `{ status }` for `network` |

| Code | When |
|---|---|
| `unsupported` | not available on this platform, build or configuration |
| `invalid-argument` | an argument failed validation |
| `not-found` | the window, element or update does not exist |
| `forbidden` | the caller may not make this call: a missing permission, a plugin webview, or a page that is not a local app page |
| `io` | a file or window operation failed |
| `network` | an HTTP request failed |
| `verification` | an update failed its signature or hash check |
| `backend` | another failure inside the plugin |
| `config` | the plugin configuration is invalid |
| `tauri` | Tauri rejected the call before the plugin ran it; the original text is in `data.raw` |

`instanceof OverwolfError` also works for an error created by another bundled
copy of the package. `ERROR_CODES` lists the codes; `isOverwolfErrorWire(value)`
tells whether a value has the plugin's `{ code, message, data? }` shape.

## Types

Exported from `tauri-plugin-overwolf-api`: `OverwolfInfo`, `HostInfo`,
`MachineIds`, `CMPTab`, `CMPWindowOptions`, `EmailHashes`,
`ExternalPaymentUserIdOptions`, `OverwolfErrorCode`, `OverwolfErrorWire`,
`OverwolfErrorOptions`.

From `tauri-plugin-overwolf-api/adview`: `OwAdViewElement`, `AdviewApi`,
`AdviewAttributes`, `AdviewGeometry`, `AdviewRect`, `AdviewEventMessage`.

From `tauri-plugin-overwolf-api/updater`: `CheckOptions`, `DownloadEvent`,
`Update`.

## JSX types

```ts
import type {} from 'tauri-plugin-overwolf-api/jsx';
```

Declares `<owadview>` as an intrinsic element of React's `JSX` namespace
(React 18 and 19). It also exports `OwAdViewAttributes` and `OwAdViewProps`
for other frameworks. See [owadview.md](owadview.md#frameworks).
