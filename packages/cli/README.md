# tauri-plugin-overwolf-cli

The `ow-tauri` command of
[`tauri-plugin-overwolf`](https://github.com/AlloryDante/ow-tauri). It
sets a Tauri 2 app up for Overwolf ads, keeps an ow-electron app's uid,
checks the setup, and signs the app with Overwolf's signing service.

This project is not affiliated with or endorsed by Overwolf.

Not on npm yet. Build the package from a clone of the repository and install
the tarball (`npm pack` prints its file name):

```sh
git clone https://github.com/AlloryDante/ow-tauri ../ow-tauri
cd ../ow-tauri
npm ci
npm pack -w tauri-plugin-overwolf-cli
cd -
npm add -D ../ow-tauri/tauri-plugin-overwolf-cli-0.1.0.tgz
```

Always run it from the local install: `npm exec --no -- ow-tauri <command>`,
or a `package.json` script. Never use `npx ow-tauri`: without a local install,
`npx` may download a different package from the registry.

## Commands

### `init [--author <a>] [--name <n>] [--tauri-dir <dir>]`

Sets a fresh Tauri app up for the plugin:

- the `plugins.overwolf` block of `tauri.conf.json`: `author`, `name` and
  test ads on;
- `overwolf:default` in `capabilities/default.json`, selected by `webviews`
  (your first window's label, else `main`);
- the NSIS installer hooks in the Windows overlay `tauri.windows.conf.json`;
- `/gen/overwolf` in the Tauri folder's `.gitignore`.

Safe to run again. It never changes an existing `author` or `name`, because
that would change the uid.

### `migrate --from <package.json> [--write <tauri.conf.json>]`

Prints the `plugins.overwolf` block that keeps an ow-electron app's uid:
`author`, `name`, `uid` when the `package.json` pins one, and the
ad-optimisation and signing flags. With `--write` it merges the block into
the given file. See
[migration](https://github.com/AlloryDante/ow-tauri/blob/main/docs/MIGRATION.md#2-keep-the-uid-with-ow-tauri-migrate).

### `doctor`

Read-only checks: the uid the app will use, the capabilities, Rust code that
uses `get_webview_window` or `WebviewWindow`, the macOS terminate hook, the
`tauri` and `@tauri-apps/api` minor versions, two updaters at once, the
updater's publisher settings, the installer hooks, the signing output and
test ads. It exits with 1 when a check fails.

### `sign [--main <file>] [--out <dir>] [--write-uid] [--dry-run]`

Overwolf signing, after the frontend build. It is off until
`plugins.overwolf.signing.enabled` is `true`. It reads `OW_CLI_EMAIL`,
`OW_CLI_API_KEY` and `OW_BUILD_KEY` from the environment (and
`OW_CLI_API_URL`, default `https://console-be.overwolf.com`), and writes
`signed/` in the project folder: `package.json`, `_metadata.json`,
`integrity.dll`, `owe.json` and `sign-result.json`.

When the uid Overwolf signed differs from the app's uid, it stops:

```text
[OW] the console signed uid <signed> but plugins.overwolf resolves to <resolved>; set plugins.overwolf.uid to "<signed>" (or run ow-tauri sign --write-uid)
```

`--write-uid` pins the signed uid in `tauri.conf.json` instead.

### `sign-exe <file> [--app-exe <name.exe>] [--signed-dir <dir>] [--fallback "<cmd %1>"]`

For `bundle.windows.signCommand`:

```json
"bundle": { "windows": { "signCommand": "npm exec --no -- ow-tauri sign-exe %1" } }
```

When Overwolf certificate signing is on (`signing.owCertSigning`), Overwolf
signs the app's exe. Every other file, and every file when Overwolf signing
is off, goes to the `--fallback` command (your own signing), or stays
unsigned when there is none.

## Configuration options

`doctor` and `sign` read `tauri.conf.json` the way the Tauri build does: the
base file, then the target's `tauri.<platform>.conf.json` overlay, then
`TAURI_CONFIG`. They accept:

| Option | Meaning |
|---|---|
| `--tauri-dir <dir>` | the folder of `tauri.conf.json` (default: `.` or `./src-tauri`) |
| `--config <json\|file>` | extra configuration, merged like `tauri build --config`; repeatable, replaces `TAURI_CONFIG` |
| `--platform <os>` | the build target: `win32`, `darwin` or `linux` (default: this OS) |

Of these three options, `init` accepts only `--tauri-dir`. `migrate` takes
none of them, only `--from` and `--write`.

## License

MIT or Apache-2.0, at your option.
