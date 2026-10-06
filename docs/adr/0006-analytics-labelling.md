# ADR 0006: Label analytics honestly as a Tauri host

- Status: Accepted
- Date: 2026-10-06

## Context

Overwolf documents that anonymous app analytics are on by default for
ow-electron apps and that `disableAnonymousAnalytics()` reduces them to a
mandatory minimum. Overwolf uses these reports for app health and usage. A
Tauri host sends from a different runtime: different network stack, different
user agent, no Electron version.

Sending reports that claim to come from ow-electron would mislead Overwolf's
dashboards. Sending a different format would break their pipeline.

## Decision

- Same event names (with the `electron_` prefix), same endpoints and field
  order as the reference implementation (CONTRACT E), so existing pipelines
  parse them.
- `owver` is `tauri-<tauri version>` (and the guest's `owVersion` the same),
  never an Electron version.
- Extra host fields (`host`, `hostVersion`, `platform`) are **off** by
  default; `analytics.hostFields: true` appends them. InsertStats `Extra` is
  positional, so appending fields only happens when Overwolf agrees.
- The muid strategy is an option: `per-install` (random, default) or
  `machine-id` (available once Overwolf specifies the derivation).
- Every window class the user sees is tracked, like ow-electron tracks every
  `BrowserWindow`.

## Consequences

- Overwolf can tell Tauri traffic apart by `owver` alone, with no parser
  change.
- With `per-install`, the same machine running the ow-electron and Tauri builds
  of an app counts as two machines, and phase buckets differ (OQ-02).
- Reports Overwolf has not specified for hosts other than ow-electron (the
  `setExternalPaymentUserId` report, the Windows optimisation-helper events)
  are not sent.

## Alternatives considered

- **Pretend to be ow-electron (`owver` = an Electron version).** Dishonest
  and breaks Overwolf's ability to reason about hosts. Rejected.
- **New `tauri_*` event names.** Breaks the pipeline. Rejected.
- **Host fields on by default.** Changes the positional InsertStats format
  without Overwolf's agreement. Rejected.
