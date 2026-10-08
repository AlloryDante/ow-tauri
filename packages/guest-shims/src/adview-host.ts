/**
 * Entry of the injected ad guest shim (`crates/tauri-plugin-overwolf/js/adview-host.js`,
 * `docs/CONTRACT.md` D). The build replaces the free identifier below with
 * the token `/*__OW_TAURI_ADVIEW_CONFIG__*\/null`, which Rust replaces with
 * the guest's configuration (D.1).
 *
 * Page dialogs are silenced in every frame the script runs in; the shim
 * itself installs only in the main frame of the ad page.
 *
 * @packageDocumentation
 */
import { installAdviewHost } from './adview-host-core.js';
import { silenceDialogs } from './dialogs.js';

declare const __OW_TAURI_ADVIEW_CONFIG__: unknown;

silenceDialogs(window);
installAdviewHost(window, __OW_TAURI_ADVIEW_CONFIG__);
