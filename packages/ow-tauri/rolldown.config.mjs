// Builds the injected per-webview runtime (CONTRACT section B, "One runtime
// per webview") as one self-contained IIFE for the plugin to embed.
// Output: crates/tauri-plugin-overwolf/js/bootstrap.js (override with
// OW_TAURI_INJECTED_OUT).
import { defineConfig } from 'rolldown';

export default defineConfig({
  input: 'src/bootstrap/index.ts',
  platform: 'browser',
  output: {
    file: process.env.OW_TAURI_INJECTED_OUT ?? '../../crates/tauri-plugin-overwolf/js/bootstrap.js',
    format: 'iife',
    minify: true,
    sourcemap: false,
  },
});
