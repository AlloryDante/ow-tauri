# ADR 0007: Share ow-electron's per-app state directory and uid

- Status: Accepted
- Date: 2026-10-06

## Context

The app uid keys Overwolf's developer console, the app's ad configuration
and analytics. Overwolf documents that it is derived from the app name and
author and that both must stay constant across versions. ow-electron keeps
per-app state under `<appData>/ow-electron/<uid>/` (observed on machines
running ow-electron apps): `ow-electron.json` with `firstLaunch`, the consent
strings under `cmp`, and the installer's `utmParams` (which the official
sample reads).

An app that moves from ow-electron to Tauri must not become a new app to
Overwolf, re-send first-launch events, or lose the user's consent.

## Decision

- The uid formula keeps the `.electron` suffix (CONTRACT G.2); a
  console-assigned `overwolf.uid` or a configured uid wins.
- ow-tauri reads and writes the shared keys of the same `ow-electron.json`
  (`firstLaunch`, `cmp.*`), reads `utmParams`, preserves every other key, and
  writes atomically.
- ow-tauri's own state goes into a sibling `ow-tauri.json`; its log into
  `logs/ow-tauri.log`.
- `app.getPath('userData')` resolves to Electron's location
  (`<appData>/<productName>`), so app-level prefs files are found too.

## Consequences

- Migration keeps the uid, consent, first-launch state and UTM data.
- ow-tauri and ow-electron builds of the same app can alternate on one machine.
  Concurrent runs can race on `ow-electron.json`; writes are atomic, so the
  worst case is a lost consent update, and Overwolf apps are normally
  single-instance.
- Package channel choices do not migrate (ow-electron keeps them in its own
  browser storage).

## Alternatives considered

- **Own state directory with a one-time import.** Simpler ownership, but
  switching back and forth loses consent and re-sends first launch. Rejected.
- **Drop the `.electron` suffix.** Produces a different uid: a new app for
  Overwolf. Rejected.
- **Write ow-tauri keys into `ow-electron.json`.** Risks confusing ow-electron.
  Rejected in favour of the sibling file.
