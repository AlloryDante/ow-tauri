# tauri-plugin-overwolf-api

The JavaScript API of
[`tauri-plugin-overwolf`](https://github.com/AlloryDante/ow-tauri), for the
pages of a Tauri 2 app that uses the plugin: Overwolf ads (`<owadview>`),
consent, email hashes, the analytics switches, identity and the Windows
updater.

The Rust plugin produces everything Overwolf receives. This package calls
its commands over Tauri's IPC and runs the `<owadview>` element in your page.
It is ESM only, runs in your app's webviews, and depends only on
`@tauri-apps/api`.

This project is not affiliated with or endorsed by Overwolf.

## Install

Not on npm yet. Build the package from a clone of the repository and
install the tarball (`npm pack` prints its file name):

```sh
git clone https://github.com/AlloryDante/ow-tauri ../ow-tauri
cd ../ow-tauri
npm ci
npm pack -w tauri-plugin-overwolf-api
cd -
npm add ../ow-tauri/tauri-plugin-overwolf-api-0.1.0.tgz
```

The package needs the Rust plugin in your app. Add and register it as the
[getting started guide](https://github.com/AlloryDante/ow-tauri/blob/main/docs/GETTING-STARTED.md)
shows. Then grant `overwolf:default` to the webviews that call it, never to
the windows. An ad is a child webview inside your window, so a capability
that names the window would also cover the ad. The opt-in sets are
`overwolf:machine-id`, `overwolf:email-hashes`, `overwolf:analytics` and
`overwolf:updater`
([permissions](https://github.com/AlloryDante/ow-tauri/blob/main/docs/api/permissions.md)).

## Entry points

| Import | What it is |
|---|---|
| `tauri-plugin-overwolf-api` | `getInfo()`, `getMachineIds()`, consent, email hashes, analytics switches, `setWindowName()`, `OverwolfError` |
| `tauri-plugin-overwolf-api/adview` | the `<owadview>` element (a side-effect import) |
| `tauri-plugin-overwolf-api/updater` | `check()` and `Update`, in the shape of `@tauri-apps/plugin-updater` |
| `tauri-plugin-overwolf-api/jsx` | types only: `<owadview>` as a React JSX element |
| `tauri-plugin-overwolf-api/testing` | `mockOverwolf()`, a fake plugin for unit tests |

## Show an ad

Import the element once in each page that shows ads, then put it in a sized
container:

```ts
import 'tauri-plugin-overwolf-api/adview';
```

```html
<div style="width: 400px; height: 300px">
  <owadview cid="main-mrec" slotsize="400x300"></owadview>
</div>
```

Each element gets a native ad webview over its box while it is in the
document. Size the container, not the element. The ad always paints above
your page, so a menu that must cover an ad has to hide the element.

Ad events (`display_ad_loaded`, `impression`, ...) are plain DOM events on
the element. The element, its attributes, members and events are the same as
in ow-electron, so you can keep an ow-electron app's ad HTML and add the
import
([migration guide](https://github.com/AlloryDante/ow-tauri/blob/main/docs/MIGRATION.md#4-keep-the-ad-html)).

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/AlloryDante/ow-tauri/main/docs/images/showcase/sizes-dark.webp">
  <img alt="The Sizes page of the ad showcase example: Overwolf test ads in 160x600, 400x600, 400x300 and 300x250 containers, each an owadview element with its load state above it." src="https://raw.githubusercontent.com/AlloryDante/ow-tauri/main/docs/images/showcase/sizes-light.webp">
</picture>

## Call the plugin

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

The `app.overwolf` calls of an ow-electron main process map to these
functions as the
[migration guide](https://github.com/AlloryDante/ow-tauri/blob/main/docs/MIGRATION.md#6-move-the-appoverwolf-calls)
lists.

## Check for updates

```ts
import { check } from 'tauri-plugin-overwolf-api/updater';

const update = await check();
if (update) await update.downloadAndInstall();
```

The updater is Windows only. It needs the `overwolf:updater` permission and
the crate's `updater` feature.

## Test your code

`mockOverwolf()` replaces the plugin in unit tests and records the calls:

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
- [Migrating from ow-electron](https://github.com/AlloryDante/ow-tauri/blob/main/docs/MIGRATION.md)
- [Troubleshooting](https://github.com/AlloryDante/ow-tauri/blob/main/docs/TROUBLESHOOTING.md)

## License

MIT or Apache-2.0, at your option.
