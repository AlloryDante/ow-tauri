# tauri-plugin-overwolf-cli

The `ow-tauri` command of
[`tauri-plugin-overwolf`](https://github.com/AlloryDante/ow-tauri), for
developers of a Tauri 2 app that uses the plugin. It sets the app up for
Overwolf ads, keeps an ow-electron app's uid, checks the setup, and signs the
app with Overwolf's signing service.

This project is not affiliated with or endorsed by Overwolf.

## Install

Not on npm yet. Build the package from a clone of the repository and
install the tarball as a dev dependency (`npm pack` prints its file name):

```sh
git clone https://github.com/AlloryDante/ow-tauri ../ow-tauri
cd ../ow-tauri
npm ci
npm pack -w tauri-plugin-overwolf-cli
cd -
npm add -D ../ow-tauri/tauri-plugin-overwolf-cli-0.1.0.tgz
```

Run it from the local install, with `npm exec --no -- ow-tauri <command>` or
from a `package.json` script. Do not use `npx ow-tauri`: without a local
install, `npx` may download a different package from the registry.

The CLI needs Node.js 22.12 or newer.

| Command | Use it to |
|---|---|
| [`init`](#init) | set a fresh Tauri app up for the plugin |
| [`migrate`](#migrate) | keep an ow-electron app's uid in the Tauri app |
| [`doctor`](#doctor) | check the setup before a build |
| [`sign`](#sign) | sign the app with Overwolf's signing service |
| [`sign-exe`](#sign-exe) | sign the Windows exe from `bundle.windows.signCommand` |

## init

```sh
npm exec --no -- ow-tauri init --author "Example Studio" --name "Example App"
```

Options: `--author <a>`, `--name <n>` and `--tauri-dir <dir>`. `--name`
defaults to the app's `productName`. `init` writes:

- the `plugins.overwolf` block of `tauri.conf.json`, with `author`, `name`
  and test ads on;
- `overwolf:default` in `capabilities/default.json`, selected by `webviews`
  (your first window's label, else `main`);
- the NSIS installer hooks in the Windows overlay `tauri.windows.conf.json`;
- `/gen/overwolf` in the Tauri folder's `.gitignore`.

You can run it again. It never changes an existing `author` or `name`,
because that would change the uid.

`init` does not touch `Cargo.toml`, `build.rs` or your Rust code. Add the
crate, its build step and the plugin as the
[getting started guide](https://github.com/AlloryDante/ow-tauri/blob/main/docs/GETTING-STARTED.md)
shows.

## migrate

```sh
npm exec --no -- ow-tauri migrate --from ../my-electron-app/package.json --write src-tauri/tauri.conf.json
```

Prints the `plugins.overwolf` block that keeps an ow-electron app's uid:
`author`, `name`, `uid` when the `package.json` pins one, and the
ad-optimisation and signing flags. With `--write <file>` it merges the block
into that file. The steps around it are in the
[migration guide](https://github.com/AlloryDante/ow-tauri/blob/main/docs/MIGRATION.md#2-keep-the-uid-with-ow-tauri-migrate).

## doctor

```sh
npm exec --no -- ow-tauri doctor
```

Read-only checks: the uid the app will use, the capabilities, Rust code that
uses `get_webview_window`, `webview_windows` or `WebviewWindow`, the macOS
terminate hook, the `tauri` and `@tauri-apps/api` minor versions, the
WebView2 minimum, two updaters at once, the updater's publisher settings, the
installer hooks, the signing output and test ads. It exits with 1 when a check
fails.

## sign

```sh
npm exec --no -- ow-tauri sign --dry-run
```

Options: `--main <file>`, `--out <dir>`, `--write-uid` and `--dry-run`.

Run `sign` after the frontend build. It is off until
`plugins.overwolf.signing.enabled` is `true`, or `OW_REQUIRE_SIGNING` is set
to a value other than empty, `0` or `false`. It reads `OW_CLI_EMAIL`,
`OW_CLI_API_KEY` and `OW_BUILD_KEY` from the environment (and
`OW_CLI_API_URL`, default `https://console-be.overwolf.com`), and writes
`signed/` in the project folder: `package.json`, `_metadata.json`,
`integrity.dll`, `owe.json` and `sign-result.json`.

When the uid Overwolf signed differs from the app's uid, it stops:

```text
[OW] the console signed uid <signed> but plugins.overwolf resolves to <resolved>; set plugins.overwolf.uid to "<signed>" (or run ow-tauri sign --write-uid)
```

`--write-uid` pins the signed uid in `tauri.conf.json` instead. `--dry-run`
prints the signing request and sends nothing. The `signing` settings are in
[configuration](https://github.com/AlloryDante/ow-tauri/blob/main/docs/CONFIG.md#signing),
and the build steps around them in the
[production checklist](https://github.com/AlloryDante/ow-tauri/blob/main/docs/PRODUCTION-CHECKLIST.md#overwolf-signing-optional).

## sign-exe

Tauri calls it for each file it signs. Set it as the sign command in
`tauri.conf.json`:

```json
"bundle": { "windows": { "signCommand": "npm exec --no -- ow-tauri sign-exe %1" } }
```

Options: `--app-exe <name.exe>`, `--signed-dir <dir>` and
`--fallback "<cmd %1>"`.

When the last `ow-tauri sign` run recorded that the app asked for Overwolf
certificate signing (`signing.owCertSigning` or `OW_ENABLE_CERT_SIGNING`) and
the signing service enabled it, Overwolf signs the app's exe. Every other
file, and every file when Overwolf signing is off, goes to the `--fallback`
command (your own signing), or stays unsigned when there is none.

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
none of them, only `--from` and `--write`. `sign-exe` takes none of them.

## Documentation

- [Getting started](https://github.com/AlloryDante/ow-tauri/blob/main/docs/GETTING-STARTED.md)
- [Migrating from ow-electron](https://github.com/AlloryDante/ow-tauri/blob/main/docs/MIGRATION.md)
- [Configuration](https://github.com/AlloryDante/ow-tauri/blob/main/docs/CONFIG.md)
- [Production checklist](https://github.com/AlloryDante/ow-tauri/blob/main/docs/PRODUCTION-CHECKLIST.md)
- [Troubleshooting](https://github.com/AlloryDante/ow-tauri/blob/main/docs/TROUBLESHOOTING.md)

## License

MIT or Apache-2.0, at your option.
