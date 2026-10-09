# API reference

The plugin has two halves. The npm package `tauri-plugin-overwolf-api` runs in
your app's webviews. The crate `tauri-plugin-overwolf` runs in your app's
native process and build script.

| Page | Covers |
|---|---|
| [js.md](js.md) | `tauri-plugin-overwolf-api`: identity, consent, email hashes, the analytics switches, the window name, `./updater`, errors and types |
| [owadview.md](owadview.md) | the `<owadview>` element (`tauri-plugin-overwolf-api/adview`): attributes, members, events, lifecycle and layout |
| [rust.md](rust.md) | `tauri-plugin-overwolf`: `Builder`, `OverwolfExt`, the macOS terminate hook, the updater, the build step, errors and types |
| [permissions.md](permissions.md) | the permission sets and the caller check |
| [testing.md](testing.md) | `tauri-plugin-overwolf-api/testing`, the fake plugin for unit tests |

The command-line tool is described in
[packages/cli/README.md](../../packages/cli/README.md), and every
`plugins.overwolf` key in [CONFIG.md](../CONFIG.md).

## Generated reference

typedoc and rustdoc build the full reference from the source comments. From
the repository root, after `npm ci`:

```shell
npm run docs:api            # both
npm run docs:api -- --ts    # typedoc only
npm run docs:api -- --rust  # rustdoc only
```

| Output | Open |
|---|---|
| typedoc | `packages/api/docs-out/index.html` |
| rustdoc | `target/doc/tauri_plugin_overwolf/index.html` (under `$CARGO_TARGET_DIR` when it is set) |

Warnings fail both, as in CI. Both folders are generated and ignored by git.
