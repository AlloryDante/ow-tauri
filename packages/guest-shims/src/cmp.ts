/**
 * Entry of the injected consent page shim (`crates/tauri-plugin-overwolf/js/cmp.js`,
 * `docs/CONTRACT.md` D.6.6). The build replaces the free identifier below
 * with the token `/*__OW_TAURI_CMP_CONFIG__*\/null`, which Rust replaces with
 * `{ adOptimization }` (D.1).
 *
 * @packageDocumentation
 */
import { installCmp } from './cmp-core.js';

declare const __OW_TAURI_CMP_CONFIG__: unknown;

installCmp(window, __OW_TAURI_CMP_CONFIG__);
