/**
 * Entry point of the injected bootstrap (`crates/tauri-plugin-overwolf/js/bootstrap.js`).
 *
 * The plugin injects the built file as an initialization script into
 * `ow-main` and every `bw-*` webview, wrapped in its app-origin guard
 * (`docs/CONTRACT.md` A.2.3.1). It installs `globalThis.__OW_TAURI_RUNTIME__`
 * and the `process` shim and subscribes the host-message channel before any
 * page script runs.
 *
 * @packageDocumentation
 */
import { installRuntime } from './install.js';

installRuntime();
