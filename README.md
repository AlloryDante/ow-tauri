# ow-tauri

Move an Overwolf app from ow-electron to Tauri 2 and keep Overwolf's ads
system exactly as ow-electron runs it: `<owadview>` ads, the consent flow,
email hashes and the anonymous app analytics, with the same requests, in the
same order, with the same identifiers. The only intended difference is the
host label (`tauri` where ow-electron says `electron`).

ow-tauri is vendor-neutral and MIT-licensed. It is written so that Overwolf can
review it, re-run its parity checks and adopt it.

> **Status: preview, ads system first.** The plugin, the npm package and both
> examples are implemented and checked against ow-electron 42.11.4 in a lab
> on macOS and Windows ([docs/PARITY.md](docs/PARITY.md#lab-checks)). Neither
> the crate nor the npm package is published yet, and nothing here is
> endorsed by Overwolf. Gaming packages (GEP, overlay, recorder, utility, CRN) are
> **deferred**: `app.overwolf.packages` keeps its shape and reports packages
> as unavailable, as ow-electron does where they are not available. As in
> ow-electron, ads are live unless you start with `--test-ad` (or
> `OW_TAURI_TEST_AD=1`); serving live ads from a Tauri host in production
> needs Overwolf's approval for your app (OQ-20). See
> [docs/PARITY.md](docs/PARITY.md) and
> [docs/OPEN-QUESTIONS.md](docs/OPEN-QUESTIONS.md).

## Why

An ow-electron app is an Electron app plus Overwolf's runtime. Tauri apps are
smaller and use the system webview, but they have no Node main process, no
`<webview>` tag and no Overwolf runtime. ow-tauri fills those three gaps:

1. **Your main-process code keeps running.** It runs in a hidden, privileged
   "main" webview. A bundler alias `electron -> ow-tauri/electron` gives it
   `app`, `BrowserWindow`, `ipcMain` and friends, and `app.overwolf` keeps its
   shape. See [ADR 0001](docs/adr/0001-hidden-main-webview.md) and
   [ADR 0002](docs/adr/0002-electron-subset-alias.md).
2. **Your renderer keeps its preload API.** Preload scripts run as Tauri
   initialization scripts; `ipcRenderer.invoke` reaches `ipcMain.handle` with the
   same channel strings.
3. **Your ads keep working.** `document.createElement('owadview')` still works;
   each element is backed by a native child webview that loads Overwolf's ad
   page with the request headers ow-electron sends, after the same hidden
   consent window has run. See
   [ADR 0003](docs/adr/0003-owadview-native-child-webviews.md),
   [ADR 0013](docs/adr/0013-request-shaping-per-os.md) and
   [ADR 0015](docs/adr/0015-startup-consent-window.md).

## What you get

| Area | ow-electron | ow-tauri |
|---|---|---|
| `<owadview>` ads, events, high-impact, performance ads | built in | native child webview per element; same plain DOM events and `__overwolf__` data |
| Ad request headers (`Referer`, `Origin`, `x-ow-*`) | built in | same on Windows; macOS sends them on the ad document only (documented gap); Linux the same |
| Startup consent window, consent cookies, settings window | built in | same hidden window on every launch, same cookies, same state file encoding |
| Email hashes, `disableAdsFPD`, `disableAdsOptimization` | built in | implemented |
| Anonymous app analytics | built in | the same requests in the same order, labelled `tauri` through `analytics.hostLabel` |
| App uid, muid, muidV2, phase percent | built in | same uid rule; same machine-id derivation |
| `app.overwolf.packages` | built in | same shape; reports packages as unavailable (no events, observed results) |
| GEP, overlay, recorder, utility, CRN packages | Windows packages | deferred; the runtime interface is a design appendix |
| Signing (`requireSigning`, `enableOWCertSigning`) | ow-electron-builder | `ow-tauri sign`: the same flow, except the asar step (Tauri has no asar) |
| Updates from Overwolf's feed | electron-updater | built-in compatible update client; NSIS hooks for Overwolf's install and uninstall work |

The full member-by-member status is in [docs/CONTRACT.md](docs/CONTRACT.md);
how each row is checked against ow-electron is in
[docs/PARITY.md](docs/PARITY.md).

### Per platform

| Feature | Windows (WebView2) | macOS (WKWebView) | Linux (WebKitGTK) |
|---|---|---|---|
| Main process, windows, IPC, dialogs, screen, shell, files | yes | yes | yes |
| `<owadview>` ads, test ads, consent | yes, lab-checked | yes, lab-checked; ad request headers on the ad document only | ads load; guests cannot overlap the page yet |
| Ad request headers (`Referer`, `Origin`, `x-ow-*`) | every request | ad document only | ad document only |
| Anonymous analytics, email hashes, uid, muid | yes | yes | yes |
| Updates from Overwolf's feed | yes | self-hosted feed (Overwolf serves Windows setups only) | self-hosted feed, signed with `updater.pubkey` |
| NSIS installer with Overwolf's install and uninstall steps | yes | n/a | n/a |
| GEP, overlay, recorder, utility, CRN | deferred | deferred | deferred |

The gaps are listed in
[docs/PARITY.md](docs/PARITY.md#known-platform-gaps).

### Ad formats

| Format | How the app asks for it | Test mode | Status |
|---|---|---|---|
| Standard display (7 sizes) | `<owadview>` in a sized container | fills | same events as ow-electron |
| Standard video | the same, 400x300 or 400x600 | plays a test video | same events, same mute timeline |
| House ads | Dev Console set-up, no code | not served (on either host) | same configuration request |
| High impact | `adstyle="high-impact-ad;"` | fills | same events |
| Interstitial (performance) | `<owadview performance>` on `<body>` | fills | same DOM, input pass-through and end sequence (Windows, macOS) |
| Reward | `adstyle="rewarded-ad;"`, at least 400x300 | plays a test video | same events, also across hides and shows |
| In-stream | no `<owadview>` API | none | not supported on either host |

How to use each format, and what to expect, is in
[docs/AD-FORMATS.md](docs/AD-FORMATS.md). The
[ad showcase](examples/ad-showcase/README.md) shows every format from one
code base on both hosts.

## Repository layout

```
crates/tauri-plugin-overwolf/   Rust plugin: identity, state file, ads host, request
                                shaping, consent, analytics, packages manager,
                                updater, IPC router
packages/ow-tauri/              npm package "ow-tauri": ./main, ./electron, ./renderer,
                                ./testing, typings and the "ow-tauri sign" CLI
examples/packages-sample/       Overwolf's official ow-electron sample, ported
examples/ad-showcase/           every ad format, built for ow-electron and ow-tauri
                                from the same sources
tools/parity-harness/           runs ow-electron and ow-tauri side by side and
                                compares what they send
docs/                           architecture, contract, parity, migration, ad formats,
                                API reference index, ADRs, open questions, port map
```

## Quick start

The step-by-step guide is [docs/MIGRATION.md](docs/MIGRATION.md). In short,
for an existing ow-electron app (ow-tauri is not published yet, so both
halves come from a clone of this repository):

```sh
# 1. JS runtime and Tauri CLI; remove @overwolf/ow-electron, -builder, electron-updater
npm install --save-dev <clone>/ow-tauri-0.1.0.tgz @tauri-apps/cli@2.12.1 @tauri-apps/api@2.12.1
#    (the tarball comes from `npm pack --workspace ow-tauri` in the clone)

# 2. Rust plugin in src-tauri/Cargo.toml, as a git or path dependency
#    tauri = { version = "2.12.1", features = ["unstable"] }
#    tauri-plugin-overwolf = { path = "<clone>/crates/tauri-plugin-overwolf" }

# 3. Alias electron in your bundler and in tsconfig paths
#    resolve: { alias: { electron: 'ow-tauri/electron' } }

# 4. Keep package.json, including its "overwolf" and "build.overwolf" blocks

# 5. Build your bundles, then run with test ads (without them, ads are live):
#    package.json scripts: "start-ad": "tauri dev -- -- --test-ad"
npm run build && npm run start-ad
```

The [packages sample](examples/packages-sample/README.md) is a complete,
ported app to copy from: its `src-tauri/` folder, webpack configs and
`tsconfig.json` are the templates the guide uses.

## Documentation

- [docs/MIGRATION.md](docs/MIGRATION.md): moving an ow-electron app to ow-tauri, step by step, with the API mapping tables
- [docs/AD-FORMATS.md](docs/AD-FORMATS.md): every ad format, how to show it and what to expect
- [docs/api/](docs/api/README.md): the API reference per area, and how to build it (`npm run docs:api`)
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): components, process model, data flows, security
- [docs/CONTRACT.md](docs/CONTRACT.md): commands, events, JS API, IPC, guest shim, consent, request shaping, analytics, state file, manifest, signing, packages, updater; the package runtime design in Appendix P
- [docs/PARITY.md](docs/PARITY.md): what parity with ow-electron means, how the harness proves it, and how to re-run it
- [docs/adr/](docs/adr/): architecture decision records
- [docs/PORT-MAP.md](docs/PORT-MAP.md): every file of the upstream sample and where it goes
- [docs/OPEN-QUESTIONS.md](docs/OPEN-QUESTIONS.md): what is answered, what was decided, and what Overwolf still needs to confirm
- [SECURITY.md](SECURITY.md): threat model and reporting

- [CHANGELOG.md](CHANGELOG.md): what changed, release by release

A guide for package runtime authors waits for the packages work (CONTRACT
Appendix P).

## Requirements

- Rust 1.90+ (edition 2024); developed on 1.98
- Node.js 22.12+
- Tauri 2.12.1 or a newer 2.x; the plugin enables Tauri's `unstable` feature
  (child webviews)
- Windows 10+ (WebView2), macOS 12+, or Linux with WebKitGTK 4.1

## Contributing and security

See [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md).

## License

MIT, see [LICENSE](LICENSE). The upstream sample in `examples/packages-sample`
keeps its own MIT notice (Copyright Overwolf Ltd.).

Overwolf and ow-electron are trademarks of Overwolf Ltd. This project is not
affiliated with or endorsed by Overwolf.
