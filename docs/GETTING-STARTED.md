# Getting started

This guide takes a new Tauri 2 app to its first Overwolf ad, in test mode. It
takes about fifteen minutes, most of it the first Rust build.

The result is the app in
[examples/quickstart-vanilla](../examples/quickstart-vanilla). CI builds that
app on Windows, macOS and Linux, so every file below is known to compile. A
React version of the same app is in
[examples/quickstart-react](../examples/quickstart-react).

<!-- image: docs/images/quickstart/window -->

## Contents

- [Before you start](#before-you-start)
- [1. Create the app](#1-create-the-app)
- [2. Add the Rust plugin](#2-add-the-rust-plugin)
- [3. Configure the plugin](#3-configure-the-plugin)
- [4. Grant the permission](#4-grant-the-permission)
- [5. Add the JavaScript API and an ad](#5-add-the-javascript-api-and-an-ad)
- [6. Run with test ads](#6-run-with-test-ads)
- [7. Check that test ads fill](#7-check-that-test-ads-fill)
- [What happens on the first launch](#what-happens-on-the-first-launch)
- [The React version](#the-react-version)
- [Living with Tauri's `unstable` feature](#living-with-tauris-unstable-feature)
- [Next steps](#next-steps)

## Before you start

You need:

- the [Tauri 2 prerequisites](https://tauri.app/start/prerequisites/) for
  your OS (Rust, and on Windows the WebView2 Runtime and the MSVC build
  tools);
- Node.js 22.12 or newer;
- Windows 10/11 or macOS 14 or newer to see ads. On Linux the app builds and
  runs, but ads report `unsupported` (see
  [COMPATIBILITY.md](COMPATIBILITY.md)).

Not on crates.io or npm yet. Until the first release (1.0.0-rc.1, planned),
the crate comes from GitHub and the npm packages are built from a clone.
Clone the repository into the folder that will hold your app, build the two
npm packages, and note the commit:

```sh
git clone https://github.com/AlloryDante/ow-tauri
cd ow-tauri
npm ci
npm pack -w tauri-plugin-overwolf-api -w tauri-plugin-overwolf-cli
git rev-parse HEAD
cd ..
```

`npm pack` builds both packages and writes
`tauri-plugin-overwolf-api-<version>.tgz` and
`tauri-plugin-overwolf-cli-<version>.tgz` into `ow-tauri/`. The version is
`0.1.0` today; the commands below use that name. `git rev-parse HEAD` prints
the commit. Step 2 pins the crate to it, so the crate and the npm packages
come from the same code.

## 1. Create the app

In the same folder as `ow-tauri/`:

```sh
npm create tauri-app@latest my-game-app -- --template vanilla-ts --manager npm
cd my-game-app
npm install
```

Any template works. This guide uses `vanilla-ts` because it has the fewest
files.

## 2. Add the Rust plugin

In `src-tauri/Cargo.toml`, with `<commit>` replaced by the commit from
[Before you start](#before-you-start):

```toml
[dependencies]
tauri-plugin-overwolf = { git = "https://github.com/AlloryDante/ow-tauri", rev = "<commit>" }

[build-dependencies]
tauri-plugin-overwolf = { git = "https://github.com/AlloryDante/ow-tauri", rev = "<commit>", default-features = false, features = ["build"] }
```

The first entry adds the plugin. The second adds it again as a build
dependency with only its `build` feature: that is the build step below.
Without `rev`, Cargo takes the newest commit on `main` and records it in
`Cargo.lock`; `cargo update -p tauri-plugin-overwolf` moves it forward. Do
not use `cargo add --git` for the plugin: it also writes the current version
requirement (`version = "0.1.0"`), which stops matching once the repository
moves to `1.0.0-rc.1`.

Then add the log plugin. It is optional; it shows the plugin's messages.

```sh
cd src-tauri
cargo add tauri-plugin-log@2
cd ..
```

The plugin needs `tauri` 2.12.1 or newer. Its default `ads` feature turns on
Tauri's `unstable` feature, on Windows and macOS only, for the whole app (see
[Living with Tauri's `unstable` feature](#living-with-tauris-unstable-feature)).

`src-tauri/build.rs`:

```rust
fn main() {
    // Reads tauri.conf.json exactly as tauri-build does (platform overlay + TAURI_CONFIG merges), validates
    // plugins.overwolf and the capability files, and on Windows targets writes
    // gen/overwolf/installer-hooks.nsh (install record + uninstall analytics) and, when signing is enabled,
    // the OWEINTEGRITY resource.
    tauri_plugin_overwolf::build::run().expect("tauri-plugin-overwolf build step failed");
    tauri_build::build();
}
```

`src-tauri/src/lib.rs`:

```rust
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        // Register the log plugin first so the overwolf plugin's setup messages reach it.
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_overwolf::init());

    // macOS only: lets the plugin recover crashed ad guests and report them.
    #[cfg(target_os = "macos")]
    let builder = builder.on_web_content_process_terminate(
        tauri_plugin_overwolf::web_content_process_terminate_hook(),
    );

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

The example app also registers `tauri-plugin-single-instance` and
`tauri-plugin-window-state`. [INTEROP.md](INTEROP.md) explains the order and
the filter they need.

Add this line to `src-tauri/.gitignore`. The build step writes the folder on
every build:

```gitignore
/gen/overwolf
```

## 3. Configure the plugin

Add `plugins.overwolf` to `src-tauri/tauri.conf.json`. The quickstart's whole
file:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "My Game App",
  "version": "1.0.0",
  "identifier": "com.example.mygameapp",
  "build": {
    "beforeDevCommand": "npm run dev",
    "devUrl": "http://localhost:1420",
    "beforeBuildCommand": "npm run build",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [{ "label": "main", "title": "My Game App", "width": 1200, "height": 800 }],
    "security": {
      "csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src ipc: http://ipc.localhost; frame-ancestors 'self'"
    }
  },
  "bundle": { "active": true },
  "plugins": {
    "overwolf": {
      "author": "Example Studio",
      "name": "My Game App",
      "ads": { "testAd": true }
    }
  }
}
```

`author` and `name` are the inputs of the app uid, the id Overwolf knows
your app by. The plugin computes it with ow-electron's formula. Once the
Overwolf console assigns your app a uid, add it as `"uid"` (see
[OVERWOLF-ONBOARDING.md](OVERWOLF-ONBOARDING.md)). A release build fails until
`uid`, or both `author` and `name`, are set, so a later rename of
`productName` can never change the uid by accident.

`"ads": { "testAd": true }` turns on Overwolf's test ads. Remove it before you
ship.

Every key is in [CONFIG.md](CONFIG.md).

For Windows, add `src-tauri/tauri.windows.conf.json`:

```json
{
  "bundle": {
    "targets": ["nsis"],
    "windows": { "nsis": { "installerHooks": "./gen/overwolf/installer-hooks.nsh" } }
  }
}
```

The build step writes those hooks. They do the work Overwolf's installer does
for an ow-electron app: the install record, and on uninstall the uninstall
analytics and the state folder removal. Overwolf distribution uses NSIS only
([PRODUCTION-CHECKLIST.md](PRODUCTION-CHECKLIST.md#installers)).

## 4. Grant the permission

Replace `src-tauri/capabilities/default.json`:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Main webview: core APIs and Overwolf ads, consent and identity",
  "webviews": ["main"],
  "permissions": ["core:default", "overwolf:default"]
}
```

Select the webview with `"webviews"`, never with `"windows"`. An ad lives in
a child webview inside your window, so a capability that names the window
also reaches the ad page. The build step warns about every `windows`
selector while the `ads` feature is on.

`overwolf:default` covers ads, consent and the read-only identity. The
opt-in sets (machine ids, email hashes, analytics settings, updater) are in
[api/permissions.md](api/permissions.md).

## 5. Add the JavaScript API and an ad

Install the two tarballs you built in [Before you start](#before-you-start),
from `my-game-app/`:

```sh
npm add ../ow-tauri/tauri-plugin-overwolf-api-0.1.0.tgz
npm add -D ../ow-tauri/tauri-plugin-overwolf-cli-0.1.0.tgz
```

npm records them as `file:` dependencies in `package.json`. To update, pull
the clone, run `npm pack` again and repeat these two lines.

`src/main.ts`:

```ts
import 'tauri-plugin-overwolf-api/adview'; // installs the <owadview> runtime in this page
import { getInfo, isCMPRequired, openAdPrivacySettingsWindow } from 'tauri-plugin-overwolf-api';

window.addEventListener('DOMContentLoaded', async () => {
  const info = await getInfo();
  console.log(`uid ${info.uid}, test ads ${info.testAd}`);

  const ad = document.querySelector('owadview')!;
  ad.addEventListener('display_ad_loaded', () => console.log('ad loaded'));
  ad.addEventListener('impression', () => console.log('impression'));

  const privacy = document.querySelector<HTMLButtonElement>('#privacy')!;
  privacy.hidden = !(await isCMPRequired());
  privacy.addEventListener('click', () => void openAdPrivacySettingsWindow());
});
```

In `index.html`, put the ad inside a sized box:

```html
<div class="ad-container" style="width: 400px; height: 300px">
  <owadview cid="main-mrec" slotsize="400x300"></owadview>
</div>
<button id="privacy" hidden>Ad privacy settings</button>
```

`<owadview>` is the same element ow-electron apps use. The element fills its
box; the plugin places a native ad webview over it. `cid` names the ad
container and `slotsize` asks for an inventory size. The privacy button is
required in regions where consent applies: `isCMPRequired()` says when to
show it. The element reference is [api/owadview.md](api/owadview.md).

Then check the setup:

```sh
npm exec --no -- ow-tauri doctor
```

It prints the resolved uid and checks the capabilities, the Rust code and the
installer settings. Always run the CLI from the local install like this, or
from a `package.json` script. Never use `npx ow-tauri`: without a local
install, `npx` downloads whatever package is called `ow-tauri`.

## 6. Run with test ads

```sh
npm run tauri dev
```

Test ads are on when any of these is true:

| Switch | Where |
|---|---|
| `"ads": { "testAd": true }` | `plugins.overwolf` (this guide) |
| `OW_TAURI_TEST_AD=1` | environment of the app process |
| `--test-ad` | an argument of the app binary |
| `Builder::new().test_ad(true)` | Rust |

To use the environment variable instead of the config key:

```sh
# macOS, Linux
OW_TAURI_TEST_AD=1 npm run tauri dev
```

```powershell
# Windows PowerShell
$env:OW_TAURI_TEST_AD = "1"; npm run tauri dev
```

Without any of them the app requests live ads. Live ads need Overwolf's
approval of your app first ([OVERWOLF-ONBOARDING.md](OVERWOLF-ONBOARDING.md)).

## 7. Check that test ads fill

Open the devtools of the window (right-click, Inspect, in a debug build).
Within a few seconds the console shows:

```text
uid <your uid>, test ads true
ad loaded
impression
```

`display_ad_loaded` often arrives twice per fill; ow-electron does the same.
If nothing loads, see [TROUBLESHOOTING.md](TROUBLESHOOTING.md#no-ad-fills).

## What happens on the first launch

When Tauri reports the app ready (`RunEvent::Ready`), the plugin starts what
ow-electron starts when its app is ready:

1. It asks Overwolf whether this user needs consent, and opens Overwolf's
   consent page in a hidden window (label `ow-cmp-startup`). This happens on
   every launch, as in ow-electron.
2. It sends the launch analytics: the first-launch counter (first launch
   only), the app start and a heartbeat. The data is the same as
   ow-electron's; the host label says `tauri` where ow-electron says
   `electron`.
3. It writes its state file, `ow-electron/<uid>/ow-electron.json` in the app
   data folder (`%APPDATA%` on Windows, `~/Library/Application Support` on
   macOS, `~/.config` on Linux). ow-electron uses the same file with the
   same content, so an app that moves from ow-electron keeps its consent and
   first-launch state.

An `<owadview>` that mounts during the consent round waits for it, up to
3 seconds, then the ad page loads. The plugin writes nothing to disk before
`RunEvent::Ready`.

## The React version

[examples/quickstart-react](../examples/quickstart-react) is the same app in
React 19. Two differences matter:

- Import the runtime once, before React renders, in `src/main.tsx`:
  `import 'tauri-plugin-overwolf-api/adview';`.
- React does not map the element's underscore event names to `on*` props.
  Attach listeners with a `ref` and `addEventListener`.

```tsx
import { useEffect, useRef } from 'react';
import type {} from 'tauri-plugin-overwolf-api/jsx'; // types only: <owadview> in JSX
import type { OwAdViewElement } from 'tauri-plugin-overwolf-api/adview';

export function Banner({ onLoaded }: { onLoaded: () => void }) {
  const ref = useRef<OwAdViewElement>(null);
  useEffect(() => {
    const ad = ref.current;
    if (!ad) return undefined;
    ad.addEventListener('display_ad_loaded', onLoaded);
    return () => ad.removeEventListener('display_ad_loaded', onLoaded);
  }, [onLoaded]);
  return (
    <div style={{ width: 400, height: 300 }}>
      <owadview ref={ref} cid="main-mrec" slotsize="400x300" />
    </div>
  );
}
```

StrictMode mounts, unmounts and mounts again in development. The runtime
handles that: one ad loads.

## Living with Tauri's `unstable` feature

The ads need child webviews, which Tauri ships behind its `unstable` feature.
The plugin's `ads` feature turns it on for the app on Windows and macOS. Two
things change for your Rust code:

- While a window shows an ad it has two webviews. Tauri's `WebviewWindow`
  helpers then stop seeing it: `app.get_webview_window("main")` returns
  `None`, `app.webview_windows()` leaves it out, and a command argument typed
  `WebviewWindow` fails with "current webview is not a WebviewWindow". Use
  `app.get_window(label)` and `app.get_webview(label)`, and `Window` or
  `Webview` command arguments. `ow-tauri doctor` lists every use of the old
  helpers in `src-tauri/src`.
- On macOS, Tauri's child-webview mode breaks some keyboard input (arrow
  keys insert characters, keys do nothing until the first click). The plugin
  repairs it for every webview of the app. An app with its own fix turns the
  repair off with `Builder::new().macos_key_fix(false)`
  ([TROUBLESHOOTING.md](TROUBLESHOOTING.md#keyboard-input-on-macos)).

## Next steps

- [CONFIG.md](CONFIG.md): every `plugins.overwolf` key.
- [api/js.md](api/js.md) and [api/rust.md](api/rust.md): the full API.
- [AD-FORMATS.md](AD-FORMATS.md): high-impact, performance and other formats.
- [OVERWOLF-ONBOARDING.md](OVERWOLF-ONBOARDING.md): from test ads to live ads.
- [PRODUCTION-CHECKLIST.md](PRODUCTION-CHECKLIST.md): before the first
  release.
- [MIGRATION.md](MIGRATION.md): if the app comes from ow-electron.
