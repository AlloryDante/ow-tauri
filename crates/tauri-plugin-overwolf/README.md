# tauri-plugin-overwolf

The Rust half of [ow-tauri](../../README.md): a Tauri 2 plugin that gives an
app ported from ow-electron the same Overwolf runtime services it had before.

- app identity (uid, muid, phase percent) and the per-app state file
- `<owadview>` ads hosted in native child webviews, with the guest page shim
- consent (CMP) windows and storage, email hashes, ad-optimisation switches
- anonymous app analytics
- the `app.overwolf.packages` manager with a pluggable package runtime
- an electron-updater-compatible update client
- the IPC router behind the `ow-tauri/electron` subset

The public surface is specified in [docs/CONTRACT.md](../../docs/CONTRACT.md)
and the design in [docs/ARCHITECTURE.md](../../docs/ARCHITECTURE.md).

## Status

| Area | Contract | State |
|---|---|---|
| Configuration, environment and switches | A.1 | done |
| Main webview: bootstrap, lifecycle, IPC routing | A.2.1, C | done |
| Windows, screen, shell, dialogs, global shortcuts, scoped files | A.2.3 | done |
| UI window commands (`overwolf:renderer`) | A.2.5 | done, without `adview_*` |
| Host messages and errors | A.3, A.4 | done |
| Rust API (`Builder`, `OverwolfExt`) | A.5 | identity, flags, quit, second instance |
| Main webview liveness, soft restart, crash limit, quit sequence | A.6 | done |
| State files and log | F | done |
| Manifest, uid, `build::embed_manifest` | G | done |
| Ads, consent, analytics, updater | A.2.2, A.2.6 to A.2.8, D, E, I | stubs, later milestones |
| Packages | A.2.4, H | reported as unavailable; no simulated backends |

## Usage

```toml
# src-tauri/Cargo.toml
[dependencies]
tauri-plugin-overwolf = "0.1"

[build-dependencies]
tauri-plugin-overwolf = { version = "0.1", default-features = false }
```

```rust
// src-tauri/build.rs
fn main() {
    tauri_plugin_overwolf::build::embed_manifest("../package.json")
        .expect("package.json overwolf manifest");
    tauri_build::build();
}
```

```rust
// src-tauri/src/main.rs
tauri::Builder::default()
    .plugin(
        tauri_plugin_overwolf::Builder::new()
            .manifest_json(tauri_plugin_overwolf::embedded_manifest!())
            .build(),
    )
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
```

The plugin creates the hidden main webview `ow-main` and grants it
`overwolf:main` through a runtime capability. Grant `overwolf:renderer` to
the `BrowserWindow` webviews in a capability file, by webview label only:

```json
{
  "identifier": "ow-tauri-renderer",
  "local": true,
  "webviews": ["bw-*"],
  "permissions": ["overwolf:renderer"]
}
```

It registers `tauri-plugin-opener`, `tauri-plugin-dialog` and
`tauri-plugin-global-shortcut` itself unless the app already did. No webview
is granted their permissions; the plugin calls them from Rust.

### Overwolf signing (Windows release builds)

Run `npx ow-tauri sign` before `tauri build` with `OW_CLI_EMAIL`,
`OW_CLI_API_KEY` and `OW_BUILD_KEY` set. It writes `ow-tauri-signed/` next
to `package.json`; a release build of `embed_manifest` takes the signed uid
from it and, with the `embed-resource` feature on the build-dependency,
links the `OWEINTEGRITY/OWE` resource into the Windows exe:

```toml
[build-dependencies]
tauri-plugin-overwolf = { version = "0.1", default-features = false, features = ["embed-resource"] }
```

```json
{
  "bundle": {
    "resources": {
      "../ow-tauri-signed/integrity.dll": "integrity.dll",
      "../ow-tauri-signed/_metadata.json": "_metadata.json"
    },
    "windows": {
      "signCommand": {
        "cmd": "npx.cmd",
        "args": ["ow-tauri", "sign-exe", "%1", "--fallback", "signtool sign /fd sha256 /a %1"]
      }
    }
  }
}
```

`sign-exe` sends the app exe to Overwolf's certificate service when the app
is eligible and asks for it (`enableOWCertSigning`), and runs `--fallback`
(your own signing) for every other binary. Use the object form: Tauri
splits a string `signCommand` on spaces. `ow-tauri sign --dry-run` prints
the request without sending it.

## Tests

- `cargo test -p tauri-plugin-overwolf`: unit and property tests of the pure
  modules (router ordering and back-pressure, quit sequence, scopes, uid).
- `cargo test -p ow-tauri-acl-tests`: every command from every webview class
  on Tauri's mock runtime, with the ACL compiled from `permissions/`, plus
  IPC routing through real commands (`tests/acl-app`).

## Requirements

- Rust 1.90 or newer (edition 2024)
- tauri 2.12.1 or newer with the `unstable` feature (child webviews,
  see [ADR 0003](../../docs/adr/0003-owadview-native-child-webviews.md))

## License

MIT, see [LICENSE](../../LICENSE).
