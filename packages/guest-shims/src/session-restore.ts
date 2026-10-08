/**
 * Entry of the recreate prelude (`crates/tauri-plugin-overwolf/js/session-restore.js`).
 *
 * On macOS the plugin recreates an ad guest's webview where the page asked
 * for a reload (memory), which would start the page with an empty
 * `sessionStorage`. Before closing the old webview the plugin reads the top
 * frame's `sessionStorage`; this prelude, added to the new webview before the
 * guest shim, puts it back before any page script runs, so the reload keeps
 * the page's session data as an in-place reload does. The plugin removes the
 * prelude natively after the first load (one-shot), so later reloads never
 * re-apply a stale snapshot.
 *
 * The build replaces the free identifier below with the token
 * `/*__OW_TAURI_SESSION_SNAPSHOT__*\/null`, which Rust replaces with the
 * snapshot: a JSON string literal of a `{ key: value }` object.
 *
 * @packageDocumentation
 */
import { ADVIEW_ORIGIN } from './adview-host-core.js';
import { restoreSessionStorage } from './session.js';

declare const __OW_TAURI_SESSION_SNAPSHOT__: unknown;

restoreSessionStorage(window, ADVIEW_ORIGIN, __OW_TAURI_SESSION_SNAPSHOT__);
