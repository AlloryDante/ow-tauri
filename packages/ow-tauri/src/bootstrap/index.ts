/**
 * Entry point of the injected bootstrap (`crates/tauri-plugin-overwolf/js/bootstrap.js`).
 *
 * The plugin injects the built file as an initialization script into
 * `ow-main` and every `bw-*` webview, wrapped in its app-origin guard
 * (`docs/CONTRACT.md` A.2.3.1). It installs `globalThis.__OW_TAURI_RUNTIME__`
 * and the `process` shim and subscribes the host-message channel before any
 * page script runs. In UI windows it also starts the `<owadview>` runtime
 * (B.3), so ads work even when the app never imports `ow-tauri/renderer`,
 * and the window behaviour of B.3.6 and A.2.3.1 (`app-region` dragging,
 * external links on macOS and Linux).
 *
 * @packageDocumentation
 */
import { adviewRuntimeOf } from '../renderer/owadview.js';
import { browserChromeEnvironment, installWindowChrome } from '../renderer/window-chrome.js';
import { installRuntime } from './install.js';
import { platformFromUserAgent } from './process-shim.js';

const kernel = installRuntime();
if (kernel.context === 'ui') {
  adviewRuntimeOf(kernel);
  const platform = kernel.state.get('platform');
  installWindowChrome(
    kernel,
    browserChromeEnvironment(
      typeof platform === 'string'
        ? platform
        : platformFromUserAgent(typeof navigator === 'object' ? navigator.userAgent : ''),
    ),
  );
}
