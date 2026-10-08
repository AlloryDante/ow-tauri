# tauri-plugin-overwolf-cli

The `ow-tauri` command line of `tauri-plugin-overwolf`: sets
a Tauri 2 app up for Overwolf ads, migrates an ow-electron app's identity, checks the setup, and signs
the app with Overwolf's signing service.

```sh
npm add -D tauri-plugin-overwolf-cli
```

Always run it from the local install, `npm exec --no -- ow-tauri <command>` or a `package.json`
script. Never use `npx ow-tauri`: without a local install `npx` would download whatever package is
named `ow-tauri`.

## Commands

| Command                                                         | What it does                                                                                                                                                                             |
| --------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `init [--author <a>] [--name <n>]`                              | Adds `plugins.overwolf` (author, name, test ads), `overwolf:default` in a `webviews` capability, the NSIS hooks of `tauri.windows.conf.json` and `/gen/overwolf` to `.gitignore`. Safe to run again. |
| `migrate --from <package.json> [--write <tauri.conf.json>]`     | Prints (and merges) the `plugins.overwolf` block that keeps an ow-electron app's uid: `author`, `name`, `uid` when pinned, and the ad and signing flags.                                 |
| `doctor`                                                        | Read-only: the resolved uid, cuid, name and version; capability lint; Rust uses that miss windows hosting ads; tauri and `@tauri-apps/api` versions; test ads.                           |
| `sign [--main <file>] [--out <dir>] [--write-uid] [--dry-run]`  | Overwolf signing after the frontend build. Writes `signed/` in the project folder.                                                                                                      |
| `sign-exe <file> [--app-exe <name>] [--fallback "<cmd %1>"]`    | For `bundle.windows.signCommand`: Overwolf certificate signing of the app exe.                                                                                                           |

`init`, `migrate`, `doctor` and `sign` read `tauri.conf.json` the way the Tauri build does: the base
file, then the target's `tauri.<platform>.conf.json` overlay, then `TAURI_CONFIG`. They accept
`--tauri-dir <dir>`, `--platform win32|darwin|linux` and repeated `--config <json|file>` (merged like
`tauri build --config`).

## Signing

Signing is off until `plugins.overwolf.signing.enabled` is `true`. It needs `OW_CLI_EMAIL`,
`OW_CLI_API_KEY` and `OW_BUILD_KEY` in the environment, and refuses to continue when the uid Overwolf
signed differs from the one `plugins.overwolf` resolves to (`--write-uid` pins the signed uid instead).

## License

MIT OR Apache-2.0.
