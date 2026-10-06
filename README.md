# ow-tauri

Move an Overwolf app from ow-electron to Tauri 2 and keep everything Overwolf
gives you: `<owadview>` ads, consent, anonymous app analytics, and the
`app.overwolf.packages` API (GEP, overlay, recorder, utility, CRN).

ow-tauri is vendor-neutral and MIT-licensed. It is written so that Overwolf can
adopt it, review it and ship native package runtimes against a documented
interface.

> **Status: preview.** The contract and architecture are specified; the
> implementation is in progress. Nothing here is endorsed by Overwolf yet.
> Live ads in a Tauri host need Overwolf's approval for your app; until then,
> run with test ads (`--test-ad`). See [docs/OPEN-QUESTIONS.md](docs/OPEN-QUESTIONS.md).

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
   each element is backed by a native child webview that loads Overwolf's ad page.
   See [ADR 0003](docs/adr/0003-owadview-native-child-webviews.md).

## What you get

| Area | ow-electron | ow-tauri |
|---|---|---|
| `<owadview>` ads, events, high-impact, performance ads | built in | native child webview per element |
| Consent (CMP) windows and storage | built in | same windows, same state file |
| Email hashes, `disableAdsFPD`, `disableAdsOptimization` | built in | implemented |
| Anonymous app analytics | built in | implemented, labelled as a Tauri host |
| App uid, muid, phase percent | built in | same uid formula; muid strategy is an option |
| `app.overwolf.packages` manager, channels, events | built in | implemented over a pluggable package runtime |
| GEP, overlay, recorder, utility, CRN packages | native Windows packages | native runtime interface for Overwolf to implement; simulated backends for development |
| electron-updater feed on Overwolf's CDN | electron-updater | built-in compatible update client |

The full member-by-member status is in [docs/CONTRACT.md](docs/CONTRACT.md).

## Repository layout

```
crates/tauri-plugin-overwolf/   Rust plugin: identity, state file, ads host, consent,
                                analytics, packages manager + runtime interface,
                                updater, IPC router
packages/ow-tauri/              npm package "ow-tauri": ./main, ./electron, ./renderer
examples/packages-sample/       Overwolf's official ow-electron sample, ported
docs/                           architecture, contract, ADRs, open questions, port map
```

## Quick start

> Placeholder: the steps below become runnable as the implementation lands.
> The full guide will live in `docs/MIGRATION.md`.

```sh
# 1. Add the Rust plugin to your src-tauri crate
cargo add tauri-plugin-overwolf

# 2. Add the JS runtime
npm install ow-tauri

# 3. Alias electron in your bundler (webpack example)
#    resolve: { alias: { electron: 'ow-tauri/electron' } }

# 4. Keep package.json "overwolf" and "build.overwolf" blocks as they are

# 5. Run with test ads
npm run tauri dev -- -- --test-ad
```

## Documentation

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): components, process model, data flows, security
- [docs/CONTRACT.md](docs/CONTRACT.md): commands, events, JS API, IPC, guest shim, analytics, state file, manifest, package runtime, updater
- [docs/adr/](docs/adr/): architecture decision records
- [docs/PORT-MAP.md](docs/PORT-MAP.md): every file of the upstream sample and where it goes
- [docs/OPEN-QUESTIONS.md](docs/OPEN-QUESTIONS.md): what we need Overwolf to confirm

## Requirements

- Rust 1.90+ (edition 2024); developed on 1.98
- Node.js 22.12+
- Tauri 2.12.1+ with the `unstable` feature (child webviews)
- Windows 10+, macOS 12+, or Linux with WebKitGTK 4.1

## Contributing and security

See [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md).

## License

MIT, see [LICENSE](LICENSE). The upstream sample in `examples/packages-sample`
keeps its own MIT notice (Copyright Overwolf Ltd.).

Overwolf and ow-electron are trademarks of Overwolf Ltd. This project is not
affiliated with or endorsed by Overwolf.
