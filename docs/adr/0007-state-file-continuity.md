# ADR 0007: Share ow-electron's per-app state directory, file encoding and uid

- Status: Accepted (amended 2026-10-06)
- Date: 2026-10-06

## Context

The app uid keys Overwolf's developer console, the app's ad configuration
and analytics. Overwolf documents that it is derived from the app name and
author and that both must stay constant across versions. ow-electron keeps
per-app state under `<appData>/ow-electron/<uid>/ow-electron.json`: the
first-launch flag, the consent strings under `cmp`, and the installer's
`utmParams` (which the official sample reads).

The parity harness showed the exact encoding ow-electron uses
(CONTRACT F.2):
`{"firstLaunch":true,"cmp":{"cmpString":"...","timeStamp":<unix seconds>,"unifiedConsentString":"cmp%3D...%26ac%3D..."}}`.
`timeStamp` is in seconds and refreshed on every launch; the unified string is
stored URL-encoded; `firstLaunch: true` means "the first launch was already
reported"; it is the only file ow-electron writes in that directory, and it
writes no logs.

An app that moves from ow-electron to Tauri must not become a new app to
Overwolf, re-send first-launch events, or lose the user's consent, and the
two builds must be able to read each other's file.

## Decision

- The uid rule is ow-electron's (CONTRACT G.2): the formula keeps the
  `.electron` suffix, the name is `productName` else `name`, a missing author
  becomes `"unknown"`; a console-assigned `overwolf.uid` or a configured uid
  wins.
- ow-tauri reads and writes the shared keys of the same `ow-electron.json`
  with **byte-identical encoding**: compact JSON, the same key order, seconds
  for `cmp.timeStamp`, the URL-encoded `cmp.unifiedConsentString`. It reads
  `utmParams` (absent means `undefined`), preserves every other key, and
  writes atomically.
- ow-tauri's own state goes into a sibling `ow-tauri.json`. Its log,
  `logs/ow-tauri.log`, exists only with `logging.enabled` (off by default),
  because ow-electron writes none.
- `app.getPath('userData')` resolves to `<appData>/<productName or name>`,
  the directory ow-electron uses, so app-level prefs files are found too.

## Consequences

- Migration keeps the uid, consent, first-launch state and UTM data; the
  startup consent window reuses the stored consent (ADR 0015).
- ow-tauri and ow-electron builds of the same app can alternate on one
  machine and read each other's file. Concurrent runs can race on
  `ow-electron.json`; writes are atomic, so the worst case is a lost consent
  update, and Overwolf apps are normally single-instance.
- Browser-profile data (cookies, web storage) does not migrate between
  engines; the consent page rewrites the consent cookies on the first
  ow-tauri launch.

## Alternatives considered

- **Own state directory with a one-time import.** Simpler ownership, but
  switching back and forth loses consent and re-sends first launch. Rejected.
- **Drop the `.electron` suffix.** Produces a different uid: a new app for
  Overwolf. Rejected.
- **Write ow-tauri keys into `ow-electron.json`.** Risks confusing ow-electron.
  Rejected in favour of the sibling file.
- **Store the unified consent string decoded and the timestamp in
  milliseconds** (the original draft). Readable, but not what ow-electron
  writes, so the two builds would disagree. Rejected.

## Amendments

- 2026-10-06, parity revision: exact encoding (`timeStamp` in seconds,
  URL-encoded `unifiedConsentString`, `firstLaunch` meaning) as observed;
  `build.productName` no longer used for the name; `"unknown"` author
  fallback; logging off by default.
