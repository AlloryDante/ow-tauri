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

## Threat model

The design is in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#5-security-model).
This section says what we defend against, what we do not, and what an app
built on ow-tauri must do itself.

### Assets

- The host APIs that only the main webview may use: windows, files in the
  app's scope, consent and identity state, the updater, package runtimes.
- The user's consent, email hashes (in memory only), the muid and the uid.
- The integrity of the installed app (updates).

### Actors and what they can do

| Actor | Runs in | Can | Cannot |
|---|---|---|---|
| App code (main-process code) | `ow-main` | everything `overwolf:main` grants | anything outside the plugin's commands; no Node, no shell |
| App UI and any script injected into it (XSS, a compromised dependency) | a `bw-*` webview | call **every** `ipcMain` handler on every channel, with any arguments (`ipc_invoke` is reachable from any script in the page, whatever the preload exposes); mount ads | call `overwolf:main` commands; observe other webviews' traffic; run app code or reach IPC from a remote document (on Windows top-level navigations away from the app origin are cancelled; on macOS and Linux a script can still load a remote page into the window, which then has no IPC) |
| A remote page shown in a `BrowserWindow` | a `bwr-*` webview (or, on macOS and Linux, a `bw-*` webview a script navigated away; embedded frames anywhere) | nothing on the host; open new windows only through the app's `setWindowOpenHandler` | any IPC or command |
| Overwolf's ad page **and every third-party ad script in it** | an `owad-*` webview | call `adview_event` with any name and data, at the per-guest rate limits; open a few `http(s)` URLs per minute in the system browser after a reported gesture | reach the IPC router, other slots, other webviews, files or OS APIs; open other schemes; exceed the limits |
| Overwolf's consent pages | `ow-cmp-startup` (hidden, every launch), `ow-cmp-default` (hidden, first settings-window call), `ow-cmp` (the settings window) | call `cmp_event`: save a validated consent string, toggle ad optimisation, close the window | anything else; calls from a page outside Overwolf's consent path are refused |
| A network attacker | between the app and Overwolf's or the developer's servers | nothing beyond TLS failures | tamper with updates undetected (hash plus publisher signature, failing closed) |

### Mitigations

- **Per-webview capabilities matched by webview label**, not window label, so
  child webviews do not inherit a window's permissions; defence in depth with a
  class check in every command.
- **Per-webview IPC channels.** Rust never uses Tauri events, so no webview
  can listen to another's messages.
- **No app scripts in remote documents.** Initialization scripts are guarded
  by origin, the UI capability is local-only, and a window that loads a
  remote URL gets a fresh webview without scripts. Top-level navigations away
  from the app origin are cancelled and opened in the system browser (on
  macOS and Linux for link clicks and form submissions, which the runtime
  intercepts, because those engines report frame and top-level navigations
  alike).
- **Guest limits.** One scoped command per guest, validated names and sizes,
  token-bucket rate limits, one external open per gesture and a per-minute
  cap (ADR 0011).
- **Shell.** URLs are parsed and limited to `http`, `https` and `mailto`;
  `shell.openPath` is limited to the file scope and refuses executables and
  launchers unless the app opts in.
- **Updates.** SHA-512 from the feed plus a publisher signature: Authenticode
  on Windows when the app is signed, the team id on macOS, a minisign
  signature on Linux (required) and optionally elsewhere.
- **Tauri 2.12.1.** Includes the fix for GHSA-w28w-mhc8-qvjv (patched in
  2.11.6 and 2.12.0): Tauri's channel-data fetch command skipped the ACL, so
  queued channel payloads and large invoke responses could be fetched by
  other webviews. ow-tauri moves all host traffic over channels and places
  remote webviews next to app webviews, so it requires the fix.
- **Privacy.** ow-tauri sends the analytics ow-electron sends and nothing
  more (no extra host fields). The muid is derived from the machine id
  exactly as ow-electron derives it, so it is stable per machine and shared
  with the ow-electron build of the same app; it is hashed and never logged,
  and `analytics.muidStrategy: "per-install"` is a non-parity option for apps
  that want a random per-install id. Logging is off by default, email
  addresses are hashed in memory and never stored, and ow-tauri never scans
  user data for email addresses.

### What the app must do

1. **Validate every `ipcMain` handler's arguments** as untrusted input. Any
   script in any UI window can call any handler (ADR 0002).
2. **Ship a strict CSP** for the main and UI webviews (baseline in
   ARCHITECTURE section 5.5) and set `app.security.freezePrototype: true`.
3. **Declare your own Tauri commands** with
   `tauri_build::AppManifest::commands` and grant them with your own
   capability; otherwise every local webview may call them.
4. **Sign your Windows build**, so the updater can require the same publisher
   on updates.
5. Keep `shell.openPathAllowExecutables` off unless you must open executables,
   and then only for paths your own code chose.

### Out of scope

- Vulnerabilities in Overwolf's ad and consent pages or services: report them
  to Overwolf.
- A compromised user account or machine.
- Native package runtimes, which are not part of ow-tauri.
