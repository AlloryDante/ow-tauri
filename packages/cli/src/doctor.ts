/// <reference types="node" />
/**
 * `ow-tauri doctor` (DESIGN §4.15): read-only checks of an app's plugin
 * setup. It never writes a file.
 *
 * - the resolved identity (uid, cuid, `<PN>`, version) and whether the uid
 *   is pinned;
 * - the capability lint of §2.4 (`windows` selectors reach ad guests; remote
 *   URLs must never cover Overwolf origins);
 * - `get_webview_window` / `webview_windows` / `WebviewWindow` in
 *   `src-tauri/src`, which stop seeing a window that hosts an ad (§2.10);
 * - the macOS terminate hook;
 * - the `tauri` crate and `@tauri-apps/api` minor versions;
 * - the WebView2 floor, the test-ad state, and two updaters at once.
 *
 * @packageDocumentation
 */

import { existsSync, readdirSync, statSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { join, relative } from 'node:path';

import { capabilitiesOf, DEFAULT_PERMISSION, grants, parseLoose } from './init.js';
import { isObject } from './json.js';
import {
  UNPINNED_MESSAGE,
  child,
  overwolfBlock,
  projectDirOf,
  resolveIdentity,
  type LoadedConfig,
} from './tauri-config.js';

/** The WebView2 Runtime the ads need on Windows (W0c ruling 5). */
export const WEBVIEW2_MINIMUM = '98.0.1108.44';

/** One finding. */
export interface Finding {
  /** `error` fails the command; `warn` needs a look; `info` and `ok` are notes. */
  readonly level: 'ok' | 'info' | 'warn' | 'error';
  /** The text. */
  readonly message: string;
}

/** Uses that stop working while a window hosts an ad (DESIGN §2.10). */
const WEBVIEW_WINDOW_USES = /\b(get_webview_window|webview_windows|WebviewWindow)\b/;

async function readText(path: string): Promise<string | undefined> {
  try {
    return await readFile(path, 'utf8');
  } catch {
    return undefined;
  }
}

function rustFiles(dir: string): string[] {
  if (!existsSync(dir)) return [];
  const out: string[] = [];
  for (const name of readdirSync(dir).sort()) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) out.push(...rustFiles(path));
    else if (name.endsWith('.rs')) out.push(path);
  }
  return out;
}

/**
 * Whether a capability remote URL pattern covers Overwolf origins or every
 * https origin (an ad guest could then call the app's commands).
 *
 * @param url - a `remote.urls` entry
 * @returns whether it is refused
 */
export function coversOverwolf(url: string): boolean {
  const lower = url.toLowerCase();
  return lower.includes('overwolf.com') || /^https?:\/\/\*(?:[/:]|$)/.test(lower) || lower === '*';
}

async function capabilityFindings(tauriDir: string): Promise<Finding[]> {
  const dir = join(tauriDir, 'capabilities');
  const findings: Finding[] = [];
  const files = existsSync(dir)
    ? readdirSync(dir)
        .filter((name) => name.endsWith('.json'))
        .sort()
    : [];
  let granted = false;
  for (const name of files) {
    const capabilities = capabilitiesOf(parseLoose(await readText(join(dir, name))));
    for (const capability of capabilities) {
      const id = typeof capability['identifier'] === 'string' ? capability['identifier'] : name;
      if (grants(capability, DEFAULT_PERMISSION)) granted = true;
      if (capability['windows'] !== undefined) {
        findings.push({
          level: 'warn',
          message: `capability "${id}" (capabilities/${name}) selects "windows": it also reaches the ad guests inside those windows; select "webviews" instead`,
        });
      }
      const urls = child(capability, 'remote')['urls'];
      for (const url of Array.isArray(urls) ? urls : []) {
        if (typeof url === 'string' && coversOverwolf(url)) {
          findings.push({
            level: 'error',
            message: `capability "${id}" (capabilities/${name}) allows the remote URL "${url}", which covers Overwolf ad pages; remove it`,
          });
        }
      }
    }
  }
  if (!granted) {
    findings.push({
      level: 'warn',
      message: `no capability grants "${DEFAULT_PERMISSION}" (run ow-tauri init, or add it to the capability of the webview that shows ads)`,
    });
  }
  return findings;
}

async function sourceFindings(tauriDir: string): Promise<Finding[]> {
  const findings: Finding[] = [];
  let hook = false;
  for (const file of rustFiles(join(tauriDir, 'src'))) {
    const text = (await readText(file)) ?? '';
    if (text.includes('on_web_content_process_terminate')) hook = true;
    text.split(/\r?\n/).forEach((line, index) => {
      const use = WEBVIEW_WINDOW_USES.exec(line);
      if (use) {
        findings.push({
          level: 'warn',
          message: `${relative(tauriDir, file)}:${String(index + 1)}: ${use[1] ?? ''} does not see a window while it hosts an ad; use get_window / get_webview and Window / Webview (DESIGN §2.10)`,
        });
      }
    });
  }
  findings.push(
    hook
      ? { level: 'ok', message: 'the macOS web content terminate hook is wired' }
      : {
          level: 'info',
          message:
            'macOS: wire builder.on_web_content_process_terminate(tauri_plugin_overwolf::web_content_process_terminate_hook()) so crashed ad guests recover',
        },
  );
  return findings;
}

/**
 * The version of the `tauri` crate: from a `Cargo.lock` of the Tauri or
 * project folder, else the `Cargo.toml` requirement.
 *
 * @param tauriDir - the Tauri folder
 * @returns the version text, if found
 */
async function tauriCrateVersion(tauriDir: string): Promise<string | undefined> {
  for (const dir of [tauriDir, projectDirOf(tauriDir)]) {
    const lock = await readText(join(dir, 'Cargo.lock'));
    const match = lock && /\[\[package\]\]\s*\nname = "tauri"\s*\nversion = "([^"]+)"/.exec(lock);
    if (match?.[1]) return match[1];
  }
  const toml = (await readText(join(tauriDir, 'Cargo.toml'))) ?? '';
  const dep = /^tauri\s*=\s*(?:"([^"]+)"|\{[^}]*version\s*=\s*"([^"]+)")/m.exec(toml);
  return dep?.[1] ?? dep?.[2];
}

async function apiVersion(projectDir: string): Promise<string | undefined> {
  const installed = parseLoose(
    await readText(join(projectDir, 'node_modules', '@tauri-apps', 'api', 'package.json')),
  );
  if (isObject(installed) && typeof installed['version'] === 'string') return installed['version'];
  const pkg = parseLoose(await readText(join(projectDir, 'package.json')));
  if (!isObject(pkg)) return undefined;
  for (const key of ['dependencies', 'devDependencies']) {
    const spec = child(pkg, key)['@tauri-apps/api'];
    if (typeof spec === 'string') return spec;
  }
  return undefined;
}

/**
 * `major.minor` of a version or requirement (`^2.12.1` → `2.12`).
 *
 * @param version - the text
 * @returns `major.minor`, if it has one
 */
export function minorOf(version: string): string | undefined {
  const match = /(\d+)\.(\d+)/.exec(version);
  return match ? `${match[1] ?? ''}.${match[2] ?? ''}` : undefined;
}

async function versionFindings(tauriDir: string): Promise<Finding[]> {
  const crate = await tauriCrateVersion(tauriDir);
  const api = await apiVersion(projectDirOf(tauriDir));
  if (crate === undefined || api === undefined) {
    return [
      {
        level: 'info',
        message: `could not compare the tauri crate (${crate ?? 'not found'}) with @tauri-apps/api (${api ?? 'not found'})`,
      },
    ];
  }
  const a = minorOf(crate);
  const b = minorOf(api);
  return [
    a !== undefined && a === b
      ? { level: 'ok', message: `tauri ${crate} and @tauri-apps/api ${api} share the minor ${a}` }
      : {
          level: 'warn',
          message: `tauri ${crate} and @tauri-apps/api ${api} differ in the minor version; keep them on the same 2.x minor`,
        },
  ];
}

async function updaterFindings(tauriDir: string): Promise<Finding[]> {
  const toml = (await readText(join(tauriDir, 'Cargo.toml'))) ?? '';
  const official = /^tauri-plugin-updater\s*=/m.test(toml);
  const ours = /^tauri-plugin-overwolf\s*=\s*\{[^}]*features\s*=\s*\[[^\]]*"updater"/m.test(toml);
  return official && ours
    ? [
        {
          level: 'warn',
          message:
            'both tauri-plugin-updater and the overwolf "updater" feature are on: two updaters would race at exit on Windows; register only one there (docs/INTEROP.md)',
        },
      ]
    : [];
}

/**
 * Runs every check.
 *
 * @param loaded - the merged configuration
 * @returns the findings, in report order
 */
export async function doctor(loaded: LoadedConfig): Promise<Finding[]> {
  const { config, tauriDir } = loaded;
  const findings: Finding[] = [];
  try {
    const identity = await resolveIdentity(config, tauriDir);
    findings.push(
      {
        level: 'info',
        message: `uid ${identity.uid} (${identity.uidConfigured ? 'plugins.overwolf.uid' : 'computed from author and name'})`,
      },
      { level: 'info', message: `cuid ${identity.cuid}` },
      { level: 'info', message: `name "${identity.name}" (from ${identity.nameSource})` },
      { level: 'info', message: `author "${identity.author}"` },
      { level: 'info', message: `version ${identity.version ?? '(not set)'}` },
      identity.pinned
        ? { level: 'ok', message: 'the uid is pinned for release builds' }
        : { level: 'warn', message: UNPINNED_MESSAGE },
      ...identity.warnings.map((message): Finding => ({ level: 'warn', message })),
    );
  } catch (error) {
    findings.push({ level: 'error', message: (error as Error).message });
  }
  const ads = child(overwolfBlock(config), 'ads');
  if (ads['testAd'] === true) {
    findings.push({
      level: 'info',
      message: 'test ads are on (plugins.overwolf.ads.testAd); remove it before shipping',
    });
  }
  findings.push(
    ...(await capabilityFindings(tauriDir)),
    ...(await sourceFindings(tauriDir)),
    ...(await versionFindings(tauriDir)),
    ...(await updaterFindings(tauriDir)),
    {
      level: 'info',
      message: `Windows: ads need the WebView2 Runtime ${WEBVIEW2_MINIMUM} or newer`,
    },
  );
  return findings;
}

/**
 * Formats findings, one per line.
 *
 * @param findings - the findings
 * @returns the report text
 */
export function formatFindings(findings: readonly Finding[]): string {
  return findings.map((f) => `${f.level.padEnd(5)} ${f.message}\n`).join('');
}
