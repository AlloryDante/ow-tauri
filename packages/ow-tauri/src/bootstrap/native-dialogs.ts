/**
 * Entry of the injected dialog guard (`crates/tauri-plugin-overwolf/js/native-dialogs.js`,
 * `docs/CONTRACT.md` B.2.6). The plugin registers it as its own plugin
 * script, which Tauri runs before `tauri-plugin-dialog`'s.
 *
 * @packageDocumentation
 */
import { installNativeDialogGuard } from './native-dialogs-core.js';

installNativeDialogGuard(globalThis);
