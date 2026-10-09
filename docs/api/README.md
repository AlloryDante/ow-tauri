# API reference

The plugin has two halves. The npm package `tauri-plugin-overwolf-api` runs in
your app's webviews. The crate `tauri-plugin-overwolf` runs in your app's
native process and build script. Pick the page for what you are calling.

| You are working with | Read |
|---|---|
| The functions of `tauri-plugin-overwolf-api`: identity, consent, email hashes, the analytics switches, the window name, `./updater`, errors and types | [js.md](js.md) |
| The `<owadview>` element (`tauri-plugin-overwolf-api/adview`): attributes, members, events, lifecycle and layout | [owadview.md](owadview.md) |
| The Rust crate `tauri-plugin-overwolf`: `Builder`, `OverwolfExt`, the macOS terminate hook, the updater, the build step, errors and types | [rust.md](rust.md) |
| Capabilities: which permission set to grant, and why a command was refused (the caller check) | [permissions.md](permissions.md) |
| Unit tests of your app code: `tauri-plugin-overwolf-api/testing`, a fake plugin | [testing.md](testing.md) |

The `ow-tauri` command is described in
[packages/cli/README.md](../../packages/cli/README.md), and every
`plugins.overwolf` key in [CONFIG.md](../CONFIG.md). The list of every doc is
in [docs/README.md](../README.md).

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
