# API reference

ow-tauri has two APIs: the npm package `ow-tauri`, which app code imports in
its webviews, and the Rust crate `tauri-plugin-overwolf`, which the app's
`src-tauri` crate registers. The reference for both is generated from the
source comments. The pages here say what each area is for and link to the
specification; the generated pages give every member and type.

| Area | Import or crate | Runs in | Page | Specification |
|---|---|---|---|---|
| Main process | `ow-tauri/main` | the hidden main webview (`ow-main`) | [main.md](main.md) | [CONTRACT B.1](../CONTRACT.md#b1-ow-taurimain) |
| Electron subset | `ow-tauri/electron` (bundler alias for `electron`) | `ow-main`, preload scripts, UI windows | [electron.md](electron.md) | [CONTRACT B.2](../CONTRACT.md#b2-ow-taurielectron) |
| Renderer and `<owadview>` | `ow-tauri/renderer` | UI windows (`bw-*`) | [renderer.md](renderer.md) | [CONTRACT B.3](../CONTRACT.md#b3-ow-taurirenderer) |
| Test helpers | `ow-tauri/testing` | unit tests | [testing.md](testing.md) | [CONTRACT B](../CONTRACT.md#b-javascript-api) |
| Typings | `ow-tauri/types` | TypeScript | [electron.md](electron.md#typings) | [CONTRACT B.4](../CONTRACT.md#b4-typings) |
| Rust plugin | `tauri-plugin-overwolf` | the app's native process and `build.rs` | [rust.md](rust.md) | [CONTRACT A](../CONTRACT.md#a-rust-plugin) |
| Signing CLI | `npx ow-tauri sign`, `npx ow-tauri sign-exe` | the app's build | [MIGRATION step 11](../MIGRATION.md#11-sign-the-build) | [CONTRACT G.4](../CONTRACT.md#g4-signing) |

## Build the reference

From the repository root, after `npm ci`:

```shell
npm run docs:api            # both
npm run docs:api -- --ts    # typedoc only
npm run docs:api -- --rust  # rustdoc only
```

[`scripts/build-api-docs.mjs`](../../scripts/build-api-docs.mjs) runs
`npm run docs --workspace ow-tauri` (typedoc) and
`cargo doc --no-deps --all-features --locked -p tauri-plugin-overwolf`
(rustdoc). Warnings fail both, as in CI. It prints where the output went:

| Output | Open |
|---|---|
| typedoc | `packages/ow-tauri/docs-out/index.html` |
| rustdoc | `target/doc/tauri_plugin_overwolf/index.html` (under `$CARGO_TARGET_DIR` when it is set) |

Both folders are generated and git-ignored. Never commit them.

typedoc runs with `treatWarningsAsErrors` and checks that every exported
member is documented and every `{@link}` resolves
(`packages/ow-tauri/typedoc.json`). The crate denies `missing_docs`, so
rustdoc covers every public item; internal modules are hidden from it and
are not a stable API.
