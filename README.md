# ow-tauri

Move an Overwolf app from ow-electron to Tauri 2 and keep Overwolf's ads
system exactly as ow-electron runs it: `<owadview>` ads, the consent flow,
email hashes and the anonymous app analytics, with the same requests, in the
same order, with the same identifiers. The only intended difference is the
host label (`tauri` where ow-electron says `electron`).

ow-tauri is vendor-neutral and MIT-licensed. It is written so that Overwolf can
review it, re-run its parity checks and adopt it.

> **Status: preview, ads system first.** The contract and architecture are
> specified; the implementation is in progress. Nothing here is endorsed by
> Overwolf yet. Gaming packages (GEP, overlay, recorder, utility, CRN) are
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

## Repository layout

```
crates/tauri-plugin-overwolf/   Rust plugin: identity, state file, ads host, request
                                shaping, consent, analytics, packages manager,
                                updater, IPC router
packages/ow-tauri/              npm package "ow-tauri": ./main, ./electron, ./renderer,
                                and the "ow-tauri sign" CLI
examples/packages-sample/       Overwolf's official ow-electron sample, ported
tools/parity-harness/           runs ow-electron and ow-tauri side by side and
                                compares what they send
docs/                           architecture, contract, parity, ADRs, open questions,
                                port map
```

## Quick start

> Placeholder: the steps below become runnable as the implementation lands.
> The full guide will live in `docs/MIGRATION.md`.

```sh
# 1. Add the Rust plugin to your src-tauri crate
cargo add tauri-plugin-overwolf

# 2. Add the JS runtime
npm install ow-tauri

# 3. Alias electron in your bundler (webpack example) and in tsconfig paths
#    resolve: { alias: { electron: 'ow-tauri/electron' } }

# 4. Keep package.json "overwolf" and "build.overwolf" blocks as they are

# 5. Run with test ads (without it, ads are live, as in ow-electron)
OW_TAURI_TEST_AD=1 npm run tauri dev
```

On Windows `cmd`, use `set OW_TAURI_TEST_AD=1` first. To pass `--test-ad` on
the command line instead, put it in a package script (`"start-ad": "tauri dev
-- -- --test-ad"`): typed directly after `npm run tauri dev`, npm consumes one
`--` and the switch would reach Cargo instead of the app.

## Documentation

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): components, process model, data flows, security
- [docs/CONTRACT.md](docs/CONTRACT.md): commands, events, JS API, IPC, guest shim, consent, request shaping, analytics, state file, manifest, signing, packages, updater; the package runtime design in Appendix P
- [docs/PARITY.md](docs/PARITY.md): what parity with ow-electron means, how the harness proves it, and how to re-run it
- [docs/adr/](docs/adr/): architecture decision records
- [docs/PORT-MAP.md](docs/PORT-MAP.md): every file of the upstream sample and where it goes
- [docs/OPEN-QUESTIONS.md](docs/OPEN-QUESTIONS.md): what is answered, what was decided, and what Overwolf still needs to confirm
- [SECURITY.md](SECURITY.md): threat model and reporting

Planned, tracked in [CONTRIBUTING.md](CONTRIBUTING.md#documentation-checklist):
`docs/MIGRATION.md` (step-by-step guide and full mapping tables) and
`docs/api/` (reference per area). A guide for package runtime authors waits
for the packages work (CONTRACT Appendix P).

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
