# Config-merge fixtures

Golden cases for reading `plugins.overwolf` from a Tauri app's merged configuration. The CLI and the
crate must read it the same way. Both readers must produce `expected.json` for every case:

- the CLI (`packages/cli/src/tauri-config.ts`, tested by `config-merge.test.ts`);
- the crate's build step (`tauri_plugin_overwolf::build::run`).

Each folder is a Tauri folder (`tauri.conf.json`, optional `tauri.<target>.conf.json` overlays, `Cargo.toml`, and
any file a case reads) plus:

| File | Content |
| --- | --- |
| `case.json` | `target` (`windows`, `macos`, `linux`, `android` or `ios`): the overlay that applies; optional `tauriConfig`: the `TAURI_CONFIG` environment value; `description` |
| `expected.json` | `config`: the merged document (base, then the target overlay, then `TAURI_CONFIG`, each an RFC 7396 merge patch, before Tauri's defaults); `identity`: `uid`, `cuid`, `name` (`<PN>`), `author` and `version` |

Every expected uid is one of the CONTRACT G.2 test vectors.
