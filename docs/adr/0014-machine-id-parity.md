# ADR 0014: Derive the machine id exactly as ow-electron does

- Status: Accepted
- Date: 2026-10-06

## Context

`muid`, `muidV2` and `phasePercent` identify the machine to Overwolf: the
console counts unique users and installs by it, staged package rollouts
bucket by it, and the ad and consent pages receive it [DOC] [OBS]. The
original draft used a random per-install id by default, because the
derivation was unknown.

The parity harness determined the macOS derivation by black-box
observation, verified with four stand-in platform ids (CONTRACT E.4):
`muid` is the first 32 hex characters of
`sha256(lowercase(IOPlatformUUID))` formatted as a GUID, `muidV2` equals it,
and `phasePercent` is the character-code sum of the muid's MD5 hex, modulo
100. Overwolf's published NSIS uninstaller reads `MUID` from
`HKCU\Software\OverwolfElectron` and `MUIDV2` from
`HKCU\Software\OverwolfPersist` on Windows [BUILDER].

The owner's decision (Q02): the same derivation as ow-electron.

## Decision

- `analytics.muidStrategy` defaults to `machine-id`.
- macOS: the observed formula over `IOPlatformUUID`, read through IOKit
  directly, never by running `ioreg` from `PATH`.
- Windows: read `MUID` / `MUIDV2` from the two registry keys and use them as
  they are, so ow-tauri and every ow-electron app on the machine share the
  same ids; when absent, derive `muid` with the macOS formula from
  `MachineGuid`, set `muidV2 = muid`, and write both values so Overwolf's
  uninstall event works for Tauri installs.
- Linux: the macOS formula over `/etc/machine-id`, else
  `/var/lib/dbus/machine-id`.
- The Windows and Linux derivations are inferences until harness round 2
  (R2-10) observes them; the test vectors cover macOS.
- `per-install` stays as a documented non-parity option.

## Consequences

- A user who moves from the ow-electron build to the Tauri build is the same
  user, the same install and the same rollout bucket for Overwolf.
- ow-tauri reads a hardware-derived identifier, as ow-electron does; it is
  hashed before use and never logged. Overwolf's privacy policy covers it.
- If R2-10 shows a different Windows or Linux source, only the fallback
  derivation changes; the registry sharing on Windows stays.

## Alternatives considered

- **Per-install random id (the original default).** Private by default, but
  every migrating user becomes a new machine and lands in a different
  rollout bucket. Kept only as an option.
- **Run `ioreg` like ow-electron does.** Same value, but depends on `PATH`
  and spawns a process. Rejected in favour of IOKit.
