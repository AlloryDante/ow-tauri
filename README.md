# ow-tauri

Overwolf ads, consent and app analytics for Tauri 2 apps.

`tauri-plugin-overwolf` gives a Tauri app what ow-electron gives an Electron
app: the `<owadview>` ad element, Overwolf's consent flow, email hashes, the
anonymous app analytics, the app uid and machine ids, and Overwolf's update
feed on Windows. Overwolf receives the same data it receives from an
ow-electron app, with one intended difference: the host label says `tauri`
where ow-electron says `electron`.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/showcase/layouts-dark.webp">
  <img alt="The ad-showcase example in test mode: the page list on the left, a 160x600 test ad and a 400x600 video test ad in the middle, and the live event timeline on the right." src="docs/images/showcase/layouts-light.webp">
</picture>

<sub>The <a href="examples/ad-showcase">ad-showcase</a> example running on Tauri with Overwolf test ads. Every <code>&lt;owadview&gt;</code> event shows up in the timeline on the right.</sub>

> **Status: 1.0.0-rc.1, a release candidate.** Overwolf has not yet confirmed
> live ads in production or console uploads for apps built on Tauri; see
> [docs/OVERWOLF-ONBOARDING.md](docs/OVERWOLF-ONBOARDING.md). This project is
> not affiliated with or endorsed by Overwolf.

## Packages

| Package | Registry | What it is |
|---|---|---|
| `tauri-plugin-overwolf` | crates.io | the Tauri plugin (Rust) |
| `tauri-plugin-overwolf-api` | npm | the JavaScript API and the `<owadview>` runtime |
| `tauri-plugin-overwolf-cli` | npm | the `ow-tauri` command: `init`, `migrate`, `doctor`, `sign`, `sign-exe` |
| `tauri-plugin-overwolf-unstable` | crates.io | a helper the plugin uses to turn on Tauri's `unstable` feature; you never add it yourself |

## Platforms

| Platform | Status |
|---|---|
| Windows 10 and 11, x64 | supported; ads need WebView2 98.0.1108.44 or newer |
| macOS 14 or newer, Apple Silicon | supported |
| Windows arm64, Intel Macs, macOS before 14 | best effort |
| Linux | builds and runs; ads report `unsupported` |

The `tauri` crate must be 2.12.1 or newer, below 3. See
[docs/COMPATIBILITY.md](docs/COMPATIBILITY.md).

## Quick start

In a Tauri 2 app:

```sh
cd src-tauri
cargo add tauri-plugin-overwolf@1.0.0-rc.1
cargo add tauri-plugin-overwolf@1.0.0-rc.1 --build --no-default-features --features build
cd ..
npm add tauri-plugin-overwolf-api@1.0.0-rc.1
npm add -D tauri-plugin-overwolf-cli@1.0.0-rc.1
npm exec --no -- ow-tauri init --author "Example Studio" --name "Example App"
```

`init` adds the `plugins.overwolf` block (with test ads on), the
`overwolf:default` permission for your first window's webview, the Windows installer
hooks and a `.gitignore` line.

`src-tauri/build.rs`:

```rust
fn main() {
    tauri_plugin_overwolf::build::run().expect("tauri-plugin-overwolf build step failed");
    tauri_build::build();
}
```

`src-tauri/src/lib.rs`:

```rust
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_overwolf::init());

    #[cfg(target_os = "macos")]
    let builder = builder.on_web_content_process_terminate(
        tauri_plugin_overwolf::web_content_process_terminate_hook(),
    );

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

In `src/main.ts`:

```ts
import 'tauri-plugin-overwolf-api/adview';
```

In `index.html`:

```html
<div style="width: 400px; height: 300px">
  <owadview cid="main-mrec" slotsize="400x300"></owadview>
</div>
```

Then `npm run tauri dev`. The full walkthrough, with the React version, is
[docs/GETTING-STARTED.md](docs/GETTING-STARTED.md).

Two rules to know from the start:

- **Grant permissions to webviews, not windows.** An ad is a child webview
  inside your window; a capability that names the window would also cover
  the ad. Use `"webviews": ["main"]`
  ([docs/api/permissions.md](docs/api/permissions.md)).
- **Run the CLI with `npm exec --no -- ow-tauri`**, not `npx ow-tauri`, so
  it always runs the copy in your project.

## Coming from ow-electron

Your `<owadview>` HTML stays. `ow-tauri migrate` writes the
`plugins.overwolf` block that keeps your app's uid, so Overwolf and your
users see the same app, with the same consent and first-launch state. Your
main-process code moves to Rust and Tauri plugins. See
[docs/MIGRATION.md](docs/MIGRATION.md).

Overwolf packages (game events, overlay, recorder) are not available on
Tauri.

## Documentation

| Page | For |
|---|---|
| [GETTING-STARTED](docs/GETTING-STARTED.md) | adding the plugin to an app, step by step |
| [CONFIG](docs/CONFIG.md) | every `plugins.overwolf` key |
| [api/](docs/api/README.md) | the JavaScript API, `<owadview>`, the Rust API, permissions and test helpers |
| [AD-FORMATS](docs/AD-FORMATS.md) | display, video, high-impact, interstitial and reward ads |
| [MIGRATION](docs/MIGRATION.md) | moving an ow-electron app |
| [INTEROP](docs/INTEROP.md) | single-instance, window-state, updater and other official plugins |
| [TROUBLESHOOTING](docs/TROUBLESHOOTING.md) | no fill, macOS input, errors and their fixes |
| [PRODUCTION-CHECKLIST](docs/PRODUCTION-CHECKLIST.md) | before a release |
| [OVERWOLF-ONBOARDING](docs/OVERWOLF-ONBOARDING.md) | from test ads to live ads |
| [COMPATIBILITY](docs/COMPATIBILITY.md) | Tauri, Rust, Node and OS versions |
| [SECURITY](docs/SECURITY.md) | the threat model and what your app must do |
| [PARITY](docs/PARITY.md) | how the data is compared with ow-electron |

## See it working

The [ad-showcase](examples/ad-showcase) example puts every ad format on its own page. It runs the same
renderer on ow-electron and on Tauri, so the two can be compared side by side. The pictures below are
Overwolf test ads (the TEST badge in the top bar); the app id is a placeholder and shown masked.

<table>
<tr>
<td width="50%" valign="top">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/showcase/sizes-dark.webp">
  <img alt="The Sizes page with 160x600, 336x280, 400x300 video and 300x250 test ads." src="docs/images/showcase/sizes-light.webp">
</picture>

<b>Sizes.</b> All seven Overwolf ad sizes, each its own <code>&lt;owadview&gt;</code>. A slot loads once half of it is in view.

</td>
<td width="50%" valign="top">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/showcase/high-impact-dark.webp">
  <img alt="The High impact page with a takeover test ad filling the left zone." src="docs/images/showcase/high-impact-light.webp">
</picture>

<b>High impact.</b> A <code>high-impact-ad;</code> slot takes over its zone; the zone's other ads hide and come back when it is removed.

</td>
</tr>
<tr>
<td width="50%" valign="top">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/showcase/interstitial-dark.webp">
  <img alt="The Interstitial page with a full-window test ad over a dimmed page." src="docs/images/showcase/interstitial-light.webp">
</picture>

<b>Interstitial.</b> A <code>performance</code> ad covers the window. Input passes through while it loads; after it loads it stays until the user closes it.

</td>
<td width="50%" valign="top">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/showcase/reward-playing-dark.webp">
  <img alt="The Reward page while the rewarded video test ad plays." src="docs/images/showcase/reward-playing-light.webp">
</picture>

<b>Reward: playing.</b> A <code>rewarded-ad;</code> video slot stays hidden until the player presses Watch, then plays.

</td>
</tr>
<tr>
<td width="50%" valign="top">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/showcase/reward-granted-dark.webp">
  <img alt="The Reward page after the grant: 100 coins and every step checked." src="docs/images/showcase/reward-granted-light.webp">
</picture>

<b>Reward: granted.</b> Coins are granted once, on <code>complete</code> after a <code>play</code>. The grant happens in the app; Overwolf documents no server-side check.

</td>
<td width="50%" valign="top">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/showcase/controls-dark.webp">
  <img alt="The Controls page with a playing video test ad and its control buttons." src="docs/images/showcase/controls-light.webp">
</picture>

<b>Controls.</b> Mute, hide, scroll out of view, hide or minimize the window. Every action is a row in the timeline.

</td>
</tr>
</table>

How to run it, in test and live mode: [examples/ad-showcase](examples/ad-showcase).

## Examples

| Example | Shows |
|---|---|
| [quickstart-vanilla](examples/quickstart-vanilla) | the GETTING-STARTED app |
| [quickstart-react](examples/quickstart-react) | the same in React 19 |
| [ad-showcase](examples/ad-showcase) | every ad format and size |
| [packages-sample](examples/packages-sample) | Overwolf's ow-electron sample, moved to Tauri |

## Repository layout

| Path | Contents |
|---|---|
| `crates/tauri-plugin-overwolf` | the plugin |
| `crates/tauri-plugin-overwolf-unstable` | the `unstable` helper crate |
| `packages/api`, `packages/cli` | the npm packages |
| `packages/guest-shims` | scripts the plugin injects into its own ad and consent webviews |
| `examples/` | the example apps |
| `tools/parity-harness` | the lab that compares the plugin's traffic with ow-electron's |
| `docs/` | the documentation |

## Contributing and security

See [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md).

## License

The crates and npm packages are licensed under MIT or Apache-2.0, at your
option. `examples/packages-sample` keeps the MIT notice of its upstream
(Copyright Overwolf Ltd.).

Overwolf and ow-electron are trademarks of Overwolf Ltd.
