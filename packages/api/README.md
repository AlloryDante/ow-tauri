# tauri-plugin-overwolf-api

The JavaScript API of `tauri-plugin-overwolf`: Overwolf
ads (`<owadview>`), consent (CMP), analytics and identity calls, and the Overwolf-hosted updater for
a Tauri 2 app.

It is a thin layer over Tauri's IPC. Everything Overwolf receives is produced by the Rust plugin;
this package only calls its commands and runs the `<owadview>` element in your page.

## Install

```sh
npm install tauri-plugin-overwolf-api
```

Register the Rust plugin (`tauri-plugin-overwolf`) in your app and grant `overwolf:default` to the
windows that call it. The opt-in permission sets are `overwolf:machine-id`,
`overwolf:email-hashes`, `overwolf:analytics` and `overwolf:updater`.

## Entry points

| Import                              | What it is                                                 |
| ----------------------------------- | ---------------------------------------------------------- |
| `tauri-plugin-overwolf-api`         | `getInfo()`, consent, analytics, email hashes, window name |
| `tauri-plugin-overwolf-api/adview`  | the `<owadview>` element (a side-effect import)            |
| `tauri-plugin-overwolf-api/updater` | `check()`, in the shape of `@tauri-apps/plugin-updater`    |
| `tauri-plugin-overwolf-api/jsx`     | types only: `<owadview>` as a React JSX element            |
| `tauri-plugin-overwolf-api/testing` | `mockOverwolf()`, a fake plugin for unit tests             |

The package is ESM only and has no Node dependencies; it runs in the app's webviews.

## Ads

```ts
import 'tauri-plugin-overwolf-api/adview';
```

```html
<div style="width: 400px; height: 300px">
  <owadview cid="main-mrec" slotsize="400x300"></owadview>
</div>
```

The element mounts an ad guest over its box while it is in the document and visible, keeps the
guest's geometry in step, and dispatches the ad events (`display_ad_loaded`, `impression`, ...) on
the element as plain DOM events. Events that arrive before the mount completes are delivered after
it; events of a guest that was destroyed or replaced are dropped. One runtime serves the page even
when several bundles include this package.

## Calls

```ts
import { getInfo, isCMPRequired, openAdPrivacySettingsWindow } from 'tauri-plugin-overwolf-api';

const info = await getInfo();
if (await isCMPRequired()) await openAdPrivacySettingsWindow();
```

Failed calls reject with an `OverwolfError` whose `code` is one of `unsupported`,
`invalid-argument`, `not-found`, `forbidden` (also a command the window's capability does not
allow), `io`, `network`, `verification`, `backend`, `config` or `tauri`.

## Updater

```ts
import { check } from 'tauri-plugin-overwolf-api/updater';

const update = await check();
if (update) await update.downloadAndInstall();
```

Windows only; needs the `overwolf:updater` permission and the crate's `updater` feature.

## Testing

```ts
import { mockOverwolf, settle } from 'tauri-plugin-overwolf-api/testing';

const overwolf = mockOverwolf({ info: { testAd: true } });
// ... exercise app code, then:
overwolf.callsOf('set_window_name');
overwolf.restore();
```

## License

MIT OR Apache-2.0.
