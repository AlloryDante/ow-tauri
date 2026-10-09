# tauri-plugin-overwolf-api

The JavaScript API of
[`tauri-plugin-overwolf`](https://crates.io/crates/tauri-plugin-overwolf):
Overwolf ads (`<owadview>`), consent, email hashes, the analytics switches,
identity and the Windows updater, for Tauri 2 apps.

It is a thin layer over Tauri's IPC. The Rust plugin produces everything
Overwolf receives; this package calls its commands and runs the `<owadview>`
element in your page. It is ESM only, has no Node dependencies, and runs in
your app's webviews.

This project is not affiliated with or endorsed by Overwolf.

## Install

```sh
npm add tauri-plugin-overwolf-api@1.0.0-rc.1
```

Register the Rust plugin in your app
([getting started](https://github.com/AlloryDante/ow-tauri/blob/main/docs/GETTING-STARTED.md))
and grant `overwolf:default` to the **webviews** that call it, never to the
windows: an ad is a child webview inside your window, and a capability that
names the window would also cover the ad. The opt-in sets are
`overwolf:machine-id`, `overwolf:email-hashes`, `overwolf:analytics` and
`overwolf:updater`
([permissions](https://github.com/AlloryDante/ow-tauri/blob/main/docs/api/permissions.md)).

## Entry points

| Import | What it is |
|---|---|
| `tauri-plugin-overwolf-api` | `getInfo()`, consent, email hashes, analytics switches, window name, errors |
| `tauri-plugin-overwolf-api/adview` | the `<owadview>` element (a side-effect import) |
| `tauri-plugin-overwolf-api/updater` | `check()` and `Update`, in the shape of `@tauri-apps/plugin-updater` |
| `tauri-plugin-overwolf-api/jsx` | types only: `<owadview>` as a React JSX element |
| `tauri-plugin-overwolf-api/testing` | `mockOverwolf()`, a fake plugin for unit tests |

## Ads

```ts
import 'tauri-plugin-overwolf-api/adview';
```

```html
<div style="width: 400px; height: 300px">
  <owadview cid="main-mrec" slotsize="400x300"></owadview>
</div>
```

Each element gets a native ad webview over its box while it is in the
document. Ad events (`display_ad_loaded`, `impression`, ...) are dispatched
on the element as plain DOM events, as in ow-electron. Size the container,
not the element. The ad always paints above your page.

## Calls

```ts
import { getInfo, isCMPRequired, openAdPrivacySettingsWindow } from 'tauri-plugin-overwolf-api';

const info = await getInfo();
console.log(info.uid, info.testAd);
if (await isCMPRequired()) await openAdPrivacySettingsWindow();
```

A failed call rejects with an `OverwolfError` whose `code` is one of
`unsupported`, `invalid-argument`, `not-found`, `forbidden` (including a
command the webview's capability does not allow), `io`, `network`,
`verification`, `backend`, `config` or `tauri`.

## Updater

```ts
import { check } from 'tauri-plugin-overwolf-api/updater';

const update = await check();
if (update) await update.downloadAndInstall();
```

Windows only. Needs the `overwolf:updater` permission and the crate's
`updater` feature.

## Testing

```ts
import { mockOverwolf } from 'tauri-plugin-overwolf-api/testing';

const overwolf = mockOverwolf({ info: { testAd: true } });
// ... run your code, then:
overwolf.callsOf('set_window_name');
overwolf.restore();
```

## Documentation

- [JavaScript API](https://github.com/AlloryDante/ow-tauri/blob/main/docs/api/js.md)
- [`<owadview>` reference](https://github.com/AlloryDante/ow-tauri/blob/main/docs/api/owadview.md)
- [Ad formats](https://github.com/AlloryDante/ow-tauri/blob/main/docs/AD-FORMATS.md)
- [Test helpers](https://github.com/AlloryDante/ow-tauri/blob/main/docs/api/testing.md)
- [Troubleshooting](https://github.com/AlloryDante/ow-tauri/blob/main/docs/TROUBLESHOOTING.md)

## License

MIT or Apache-2.0, at your option.
