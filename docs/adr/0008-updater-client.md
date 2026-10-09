# ADR 0008: Ship an electron-updater compatible update client

- Status: Accepted (amended 2026-10-08)
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
- verifies size and SHA-512 of the download over HTTPS, then the publisher
  signature, failing closed: on Windows the installer must be Authenticode
  signed by the same publisher as the running executable (when that is
  signed); on macOS the bundle must pass code-signature validation with the
  running app's team id; with a configured minisign public key, a detached
  `.sig` must verify (required on Linux);
- installs per OS (NSIS or MSI on Windows, `.app` zip on macOS, AppImage on
  Linux);
- exposes an electron-updater-shaped `autoUpdater` in `ow-tauri/main`.

## Consequences

- Apps keep their updater code with a one-line import change.
- The SHA-512 in the feed only proves the file matches the feed; if the feed
  or CDN were compromised, both would be replaced together. The publisher
  check is what stops a compromised feed from becoming code execution. This
  matches electron-updater, which verifies the Windows signature against the
  publisher when the app is signed, and Squirrel.Mac, which checks the code
  signature. Linux AppImages have no OS signature, so ow-tauri requires a
  detached minisign signature there.
- Apps that are not signed on Windows get only the hash check, with a
  warning; signing the app is the fix.
- Whether Overwolf's console accepts and serves Tauri installers is open
  (OQ-18). Until then the client works with any generic feed the developer
  hosts.

## Alternatives considered

- **`tauri-plugin-updater`.** Needs Overwolf to publish a second manifest
  format and signatures. Kept as an option for apps that host their own
  updates; not the default.
- **No updater.** Leaves apps without a supported update path. Rejected.

## Amendments

- 2026-10-06, contract review: the publisher check is mandatory when the app
  is signed (it was optional), a detached signature is supported and required
  on Linux, and the trust-model statement was corrected.
- 2026-10-06, parity revision: Overwolf's feed serves Windows setup files
  only (`latest-mac.yml` and `latest-linux.yml` return 404) and its entries
  carry `blockMapSize` and `IsAdminRightsRequired`; macOS and Linux builds use
  a self-hosted feed with the same YAML shape. The Tauri NSIS installer gets
  hooks that do the install and uninstall work of Overwolf's installer
  (CONTRACT I.1, I.6). Whether the console accepts a Tauri setup upload stays
  open (OQ-18).
- 2026-10-08, Tauri-native pivot ([ADR 0017](0017-tauri-native-pivot.md)): the client is Windows-only in 1.0; other OSes return `unsupported`. It has the shape of `tauri-plugin-updater`: `check()` → `Update`, then `download`, `install` and `downloadAndInstall`. There is no `autoUpdater`. Release builds must configure `updater.publisherNames` (Authenticode) or `updater.pubkey` (minisign), and there is no default publisher. NSIS `.exe` only; MSI is `unsupported` (CONTRACT I).
