/**
 * The `process` shim (`docs/CONTRACT.md` section B.2.5): a frozen object with
 * Node's `platform`, `arch`, `argv`, `env` and `versions`, so code that reads
 * the global `process` without importing it keeps working in a webview.
 *
 * @packageDocumentation
 */
import { PACKAGE_VERSION } from '../shared/protocol.js';
import type { Kernel } from './kernel.js';

/** The `process` subset ow-tauri provides. */
export interface ProcessShim {
  /** `win32`, `darwin` or `linux`. */
  readonly platform: string;
  /** `x64`, `arm64`, ... */
  readonly arch: string;
  /** The app's process arguments. */
  readonly argv: readonly string[];
  /** Only `OVERWOLF_APP_UID`, defined once the main webview is ready. */
  readonly env: Readonly<Record<string, string | undefined>>;
  /** `owTauri`, `tauri`, and `chrome` on WebView2; never `electron`. */
  readonly versions: Readonly<Record<string, string>>;
}

function userAgent(): string {
  return typeof navigator === 'object' ? navigator.userAgent : '';
}

/**
 * Node's `process.platform` derived from the user agent.
 *
 * @param ua - a user-agent string
 * @returns `win32`, `darwin` or `linux`
 */
export function platformFromUserAgent(ua: string): string {
  if (/Windows/i.test(ua)) return 'win32';
  if (/Mac OS X|Macintosh/i.test(ua)) return 'darwin';
  return 'linux';
}

/**
 * Node's `process.arch` derived from the user agent.
 *
 * @param ua - a user-agent string
 * @returns `arm64` when the agent says so, else `x64`
 */
export function archFromUserAgent(ua: string): string {
  return /arm64|aarch64/i.test(ua) ? 'arm64' : 'x64';
}

/**
 * Builds the shim from the snapshot (`platform`, `arch`, `switches.argv`,
 * `versions`) with user-agent fallbacks.
 *
 * @param kernel - the kernel
 * @returns the frozen shim
 */
export function createProcessShim(kernel: Kernel): ProcessShim {
  const ua = userAgent();
  const platform = stringAt(kernel, 'platform') ?? platformFromUserAgent(ua);
  const versions: Record<string, string> = {
    owTauri: stringAt(kernel, 'versions.owTauri') ?? PACKAGE_VERSION,
  };
  const tauri = stringAt(kernel, 'versions.tauri');
  if (tauri !== undefined) versions['tauri'] = tauri;
  const chrome = /(?:Chrome|Edg)\/([\d.]+)/.exec(ua)?.[1];
  if (platform === 'win32' && chrome !== undefined) versions['chrome'] = chrome;
  const argv = kernel.state.get('switches.argv');
  const env = Object.freeze(
    Object.defineProperty({}, 'OVERWOLF_APP_UID', {
      get: () => (kernel.isReady ? stringAt(kernel, 'identity.uid') : undefined),
      enumerable: true,
    }) as Record<string, string | undefined>,
  );
  return Object.freeze({
    platform,
    arch: stringAt(kernel, 'arch') ?? archFromUserAgent(ua),
    argv: Object.freeze(
      Array.isArray(argv) ? argv.filter((a): a is string => typeof a === 'string') : [],
    ),
    env,
    versions: Object.freeze(versions),
  });
}

function stringAt(kernel: Kernel, path: string): string | undefined {
  const value = kernel.state.get(path);
  return typeof value === 'string' ? value : undefined;
}
