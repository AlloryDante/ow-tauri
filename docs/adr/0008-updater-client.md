# ADR 0008: Ship an electron-updater compatible update client

- Status: Accepted
- Date: 2026-10-06

## Context

Overwolf's console hosts app updates as an electron-updater generic feed per
app (`https://electron-updates.overwolf.com/electron-updates/electron/<app id>`,
with console channels). The official sample uses `electron-updater` against it.
`tauri-plugin-updater` reads a different manifest (`latest.json`) and requires
minisign signatures, so it cannot consume that feed.

## Decision

The plugin includes a small update client that:

- reads the generic feed's `<channel>.yml` / `-mac.yml` / `-linux.yml`;
- verifies size and SHA-512 of the download over HTTPS, with an optional
  Windows Authenticode publisher check;
- installs per OS (NSIS or MSI on Windows, `.app` zip on macOS, AppImage on
  Linux);
- exposes an electron-updater-shaped `autoUpdater` in `ow-tauri/main`.

## Consequences

- Apps keep their updater code with a one-line import change.
- Integrity relies on TLS to Overwolf's CDN plus the feed's SHA-512, the same
  trust model electron-updater uses for generic feeds; there is no detached
  signature.
- Whether Overwolf's console accepts and serves Tauri installers is open
  (OQ-18). Until then the client works with any generic feed the developer
  hosts.

## Alternatives considered

- **`tauri-plugin-updater`.** Needs Overwolf to publish a second manifest
  format and signatures. Kept as an option for apps that host their own
  updates; not the default.
- **No updater.** Leaves apps without a supported update path. Rejected.
