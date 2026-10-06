/**
 * Entry of the injected ad guest shim (`crates/tauri-plugin-overwolf/js/adview-host.js`,
 * `docs/CONTRACT.md` D). The build replaces the free identifier below with
 * the token `/*__OW_TAURI_ADVIEW_CONFIG__*\/null`, which Rust replaces with
 * the guest's configuration (D.1).
 *
 * @packageDocumentation
 */
import { installAdviewHost } from './adview-host-core.js';

declare const __OW_TAURI_ADVIEW_CONFIG__: unknown;

installAdviewHost(window, __OW_TAURI_ADVIEW_CONFIG__);
