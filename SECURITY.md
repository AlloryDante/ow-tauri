# Security policy

## Supported versions

| Version | Supported |
|---|---|
| Latest 1.x minor (and, before 1.0, the latest release candidate) | Yes |
| Older minors and release candidates | No: upgrade to the latest |
| `main` | Fixes land here first |

Nothing is released yet: until the first release (1.0.0-rc.1, planned), the
supported version is the latest commit on `main`. After it, fixes ship as a
patch release of the latest minor, for all four packages
together: `tauri-plugin-overwolf`, `tauri-plugin-overwolf-unstable`,
`tauri-plugin-overwolf-api` and `tauri-plugin-overwolf-cli`.

### Tauri security releases

ow-tauri requires `tauri` 2.12.1 or newer within 2.x. Tauri may publish a
security advisory that affects the plugin, its ad guests or its consent
windows. When it does, we publish a release that requires the fixed Tauri
version **within 72 hours** of the advisory. Until then, update `tauri`
in your own lockfile: the caret range already accepts the fix.

## Reporting a vulnerability

Report privately through GitHub's private vulnerability reporting: the
**Security** tab of the repository, then **Report a vulnerability**. Do not
put details in a public issue. If the **Report a vulnerability** button is
missing, open an issue titled "Security contact request" with no details;
a maintainer replies there with a private way to send the report. Include:

- the affected package and version, or the commit;
- the OS and the webview version (WebView2 or macOS);
- steps to reproduce and what an attacker gains.

We acknowledge reports within 5 working days. We tell you our assessment
and the planned fix date, and we credit you in the advisory unless you ask
us not to.

Use synthetic values in reports. Do not send real consent strings, email
hashes, machine ids, uids or app credentials.

Vulnerabilities in Overwolf's own ad pages, consent pages or services are
out of scope here. Report those to Overwolf.

## Threat model

The threat model, the permission sets and what the plugin writes to disk are
in [docs/SECURITY.md](docs/SECURITY.md).
