/**
 * Window entry: waits for the preload API, asks the main process for host
 * and identity, then starts the app.
 *
 * @packageDocumentation
 */
import { startApp } from './app.js';

async function boot(): Promise<void> {
  const root = document.getElementById('app');
  if (!root) return;
  const api = window.showcase;
  if (!api) {
    root.textContent =
      'window.showcase is missing: open this page in the showcase app (npm run start:electron or start:tauri).';
    return;
  }
  try {
    startApp(root, api, await api.info());
  } catch (error) {
    root.textContent = `Start failed: ${String(error)}`;
  }
}

void boot();
