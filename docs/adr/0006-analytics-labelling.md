# ADR 0006: Send ow-electron's analytics, labelled "tauri" through one setting

- Status: Accepted (amended 2026-10-06)
- Date: 2026-10-06

## Context

Overwolf documents that anonymous app analytics are on by default for
ow-electron apps and that `disableAnonymousAnalytics()` reduces them to a
mandatory minimum. Overwolf uses these reports for app health and usage
dashboards (DAU, window time, installs, uninstalls). A Tauri host sends from
a different runtime: different network stack, different user agent, no
Electron version.

The parity harness recorded exactly what ow-electron 42.11.4 sends: the
Counter and InsertStats requests, their order, query and body encoding,
headers, cookies and user agent (CONTRACT E). ow-electron names itself in the
event names (`electron_app_start`, ...), in `owver` (`42.11.4`, `42_11_4`),
in the version the ad and consent pages receive, and in the user agent
(`Electron/42.11.4`).

The project owner's decision of 2026-10-06: send exactly the ow-electron
stats, and wherever ow-electron says "electron", say "tauri", through a
single setting Overwolf can change.

## Decision

- ow-tauri sends the same requests as ow-electron, in the same order, with
  the same fields and encoding (CONTRACT E.1, E.2), and no extra host fields.
- One setting, `analytics.hostLabel` (default `"tauri"`), with
  `analytics.hostVersion` (default the Tauri crate version), produces every
  self-naming value (CONTRACT section 0, "Host label"):
  - event names `<label>_app_first_launch`, `<label>_app_start`,
    `<label>_app_heartbeat`, `<label>_window_closed`,
    `<label>_owadview_crashed`, and the installer's
    `ow_<label>_app_uninstall`;
  - `owver` / `owVersion` / `oweVersion` = `<label>-<hostVersion>`
    (`tauri-2.12.1`), or `<hostVersion>` alone when the label is `electron`;
  - the user agent token `Tauri/<hostVersion>`, placed where Electron places
    `Electron/<version>`; the engine part of the user agent is never faked.
- Overwolf's own identifiers are never relabelled: URLs (`monsdk/electron`,
  `electron-updates`), the uid formula's `.electron` suffix, the
  `ow-electron` state directory, registry keys, cookie names, and the
  analytics Overwolf's pages send themselves.
- The machine id follows ow-electron's derivation (ADR 0014); the per-install
  id is a non-parity option.
- `ads.owVersionOverride` (unset by default) can give the ad and consent
  pages a numeric version if a lab check shows they need one, without
  changing the analytics label.

## Consequences

- Overwolf can tell Tauri traffic apart by event name and `owver` alone, and
  can switch the label back (`hostLabel: "electron"`, `hostVersion:
  "42.11.4"`) without a code change.
- Dashboards that filter on the `electron_` names or on a numeric `owver`
  will not count Tauri apps until Overwolf maps the new names (OQ-03, OQ-04).
  This is the known cost of honest labelling and is raised with Overwolf.
- The ad and consent pages derive their own analytics versions from
  `owVersion` / `oweVersion`; a lab check confirms they still fill and report
  with `tauri-<version>` (PARITY.md).

## Alternatives considered

- **Pretend to be ow-electron (`owver` = an Electron version, `electron_`
  names).** Misleads Overwolf's dashboards about the host. Rejected; it stays
  possible only as Overwolf's own configuration choice.
- **Keep the `electron_` event names and label only `owver`** (the original
  decision). Contradicts the owner's rule and leaves two places that name the
  host. Superseded.
- **Extra host fields** (`host`, `hostVersion`, `platform`, the original
  opt-in). ow-electron sends none, and InsertStats `Extra` is positional.
  Removed.

## Amendments

- 2026-10-06, the project owner's decision: event names now use the label (`tauri_*`); one
  `hostLabel` setting drives every self-naming value including the user
  agent; the `hostFields` option is removed; the muid default follows
  ow-electron (ADR 0014).
- 2026-10-06, second harness round: two more labelled names were observed,
  `electron_owadview_crashed` (now confirmed, with Kind 400024) and
  `electron_sub_info` (`setExternalPaymentUserId`); both follow the label
  (`<label>_owadview_crashed`, `<label>_sub_info`).
