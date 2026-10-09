# Compatibility

## Versions

| tauri-plugin-overwolf | `tauri` crate | `@tauri-apps/api` | Rust | Node.js (CLI, build) |
|---|---|---|---|---|
| 1.0.x | 2.12.1 or newer, below 3 | the same minor as your `tauri` crate | 1.90 or newer | 22.12 or newer |

The `tauri` requirement is a caret range (`^2.12.1`), so new Tauri 2 minors
work without a plugin release. CI builds against 2.12, and a weekly job
builds against the newest Tauri 2.x and the newest stable Rust. A Tauri 3
release will need a new plugin major.

Keep the `tauri` crate and `@tauri-apps/api` on the same minor version;
`ow-tauri doctor` compares them. `tauri-plugin-overwolf-api` and
`tauri-plugin-overwolf-cli` have the same version as the crate; use the same
version of all three.

The plugin reproduces the data of ow-electron 42.11.4, with two differences:
the analytics host label is `tauri`, and on macOS ad subresource requests do
not carry ow-electron's `Origin` and `x-ow-*` headers
([CONTRACT D.8.3](CONTRACT.md#d83-per-platform),
[OQ-05](OPEN-QUESTIONS.md#oq-05-request-shaping-for-the-ad-page)).

## Platforms

| Platform | Status | Ads |
|---|---|---|
| Windows 10 22H2 and 11, x64 | supported | yes, with the WebView2 Runtime 98.0.1108.44 or newer |
| macOS 14 or newer, Apple Silicon | supported | yes |
| Windows on arm64 | best effort | yes |
| macOS on Intel, macOS before 14 | best effort | yes |
| Linux | builds and runs | no: ads report `unsupported` |
| Android, iOS | builds | no: every command answers `unsupported` |

"Best effort" means the plugin builds there and bugs are fixed when they can
be, but CI does not test it.

Overwolf's update feed serves Windows installers only, so the plugin's
updater runs on Windows only ([INTEROP.md](INTEROP.md#tauri-plugin-updater)).

## WebView2

On Windows the ads need the WebView2 Runtime 98.0.1108.44 or newer. Below
that, `getInfo().adsSupported` is `false` and mounting an ad rejects with
`unsupported`. The Evergreen runtime updates itself. Tauri's NSIS installer
installs WebView2 when it is missing (the default `webviewInstallMode`). An app that ships a fixed-version runtime must
keep it at or above the minimum.

## Tauri's `unstable` feature

Ads are child webviews, which Tauri ships behind its `unstable` feature. The
plugin's default `ads` feature turns it on for Windows and macOS builds,
through the helper crate `tauri-plugin-overwolf-unstable`. Linux and mobile
builds keep Tauri stable.

`unstable` changes how Tauri's `WebviewWindow` helpers see a window that
shows an ad ([TROUBLESHOOTING.md](TROUBLESHOOTING.md#get_webview_window-returns-none)).
On macOS it also breaks some keyboard input, which the plugin repairs until
Tauri fixes it ([TROUBLESHOOTING.md](TROUBLESHOOTING.md#keyboard-input-on-macos)).

Without ads (`default-features = false, features = ["plugin"]`) the plugin
does not turn `unstable` on.

## Tauri security releases

When a Tauri security release needs a change in the plugin, a plugin release
follows within 72 hours ([SECURITY.md](../SECURITY.md)). Until then, you can
update `tauri` yourself within the caret range.
