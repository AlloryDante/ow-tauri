# `tauri-plugin-overwolf`

The Rust half: a Tauri 2 plugin that creates the hidden main webview, hosts
ad guests and consent windows, sends analytics, keeps the per-app state file,
runs the updater, and exposes the commands the npm package calls.

Generated reference: `target/doc/tauri_plugin_overwolf/index.html`
([how to build it](README.md#build-the-reference)).

| Item | Use | Specification |
|---|---|---|
| `Builder` | registers the plugin: `Builder::new().manifest_json(embedded_manifest!()).build()`; overrides for the uid, test ads, host label and others | [CONTRACT A.5](../CONTRACT.md#a5-rust-api) |
| `embedded_manifest!()` | the `package.json` that `build::embed_manifest` embedded | [CONTRACT G.3](../CONTRACT.md#g3-build-helper) |
| `OverwolfExt`, `Overwolf` | `app.overwolf()` on any `Manager`: `uid`, `muid`, the analytics and ads switches, `emit_second_instance`, `report_web_content_terminated`, `updater` | [CONTRACT A.5](../CONTRACT.md#a5-rust-api) |
| `build` | for the app's `build.rs`: `embed_manifest`, `write_nsis_installer_hooks` | [CONTRACT G.3](../CONTRACT.md#g3-build-helper), [I.6](../CONTRACT.md#i6-installer-parity-tauri-nsis-hooks) |
| `config` | the `plugins.overwolf` configuration | [CONTRACT A.1](../CONTRACT.md#a1-configuration) |
| `manifest`, `identity` | manifest parsing and the uid and machine-id rules | [CONTRACT G](../CONTRACT.md#g-manifest), [E.4](../CONTRACT.md#e4-machine-id-muid-muidv2-phasepercent) |
| `Error`, `ErrorCode`, `Result` | command errors | [CONTRACT A.4](../CONTRACT.md#a4-errors) |
| `PackagesBackend`, `packages` | the package backend switch | [CONTRACT H](../CONTRACT.md#h-packages) |
| `fs_scope`, `paths`, `shell` | the file scope and path rules behind `files` and `shell.openPath` | [CONTRACT A.2.3](../CONTRACT.md#a23-main-webview-windows-screen-shell-dialogs-files-overwolfmain) |

Cargo features:

| Feature | Default | Use |
|---|---|---|
| `plugin` | on | the plugin itself; without it only the manifest parser, identity functions and `build` helpers remain, for `[build-dependencies]` |
| `embed-resource` | off | links the `OWEINTEGRITY/OWE` resource of a signed Windows release build; enable it on the build-dependency |
| `devtools` | off | `webContents.openDevTools()` in release builds |
| `test-util` | off | hooks for tests on Tauri's mock runtime; not a stable API |
| `lab` | off | this repository's parity lab; never enable it in an app you ship |

The commands and permission sets are listed in
[CONTRACT A.2](../CONTRACT.md#a2-commands); the capabilities the plugin adds
at runtime in [ARCHITECTURE section 5.2](../ARCHITECTURE.md#52-capabilities).
