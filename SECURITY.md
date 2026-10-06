# Security policy

## Supported versions

ow-tauri is in preview. Only the latest commit on `main` receives fixes.

## Reporting a vulnerability

Please report privately through the repository's security advisory form
("Report a vulnerability") rather than a public issue. Include the affected
component (Rust plugin, npm package, guest scripts, example), the version or
commit, and steps to reproduce. We aim to acknowledge reports within 5 working
days.

Do not include real ad configurations, consent strings, email hashes or machine
identifiers in a report; synthetic values are enough.

## Security model in brief

The full model is in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#5-security-model).

- **Window classes get different capabilities.** The hidden main webview has the
  broad host permissions. UI windows can only use the IPC router and the
  `<owadview>` commands. Remote pages (ad guests, the consent window, any window
  that loads a remote URL) get no access to the IPC router at all; ad guests and
  the consent window can call only their own one-command permission set.
- **Remote content is contained.** Ad guests cannot open windows or navigate the
  top frame away from Overwolf hosts; popups go to the system browser through the
  opener plugin.
- **No shell strings.** Paths and URLs from a renderer are opened through the
  opener plugin, never through a shell.
- **Tauri 2.12.0 or newer is required.** It fixes GHSA-w28w-mhc8-qvjv, which
  matters because ow-tauri places remote pages next to app webviews.
- **Privacy defaults.** Extra analytics fields are off by default, the machine
  id strategy defaults to a per-install id, and ow-tauri never scans user data
  for email addresses.

Issues in Overwolf's own services or ad pages should go to Overwolf.
