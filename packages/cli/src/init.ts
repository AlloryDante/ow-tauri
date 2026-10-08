/// <reference types="node" />
/**
 * `ow-tauri init [--author <a>] [--name <n>]` (DESIGN §2.3, §2.4,
 * DX-minor-9): sets a fresh Tauri app up for the plugin. It writes
 *
 * - the `plugins.overwolf` block of `tauri.conf.json` (`author`, `name`,
 *   test ads on);
 * - `overwolf:default` in a capability selected by `webviews`
 *   (`capabilities/default.json`);
 * - the NSIS hooks of the Windows overlay `tauri.windows.conf.json`;
 * - `/gen/overwolf` in the Tauri folder's `.gitignore`.
 *
 * Every step is idempotent: a second run changes nothing. An existing
 * `author` or `name` is never changed (that would change the uid).
 *
 * @packageDocumentation
 */

import { existsSync, readdirSync } from 'node:fs';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

import { formatJson, isObject, parseJsonObject, writeAtomic, type JsonObject } from './json.js';
import type { Logger } from './sign.js';
import { BASE_FILE, child, overlayFile, overwolfBlock } from './tauri-config.js';

/** The hooks file `build::run` writes, relative to the Tauri folder. */
export const INSTALLER_HOOKS = './gen/overwolf/installer-hooks.nsh';
/** The `.gitignore` line for the build step's output. */
export const GITIGNORE_LINE = '/gen/overwolf';
/** The permission set every ad-hosting webview needs. */
export const DEFAULT_PERMISSION = 'overwolf:default';

/** `ow-tauri init` options. */
export interface InitOptions {
  /** The Tauri folder. */
  readonly tauriDir: string;
  /** `plugins.overwolf.author` (the uid input). */
  readonly author?: string | undefined;
  /** `plugins.overwolf.name`; default `productName`. */
  readonly name?: string | undefined;
  /** Messages. */
  readonly log: Logger;
}

/** What {@link init} changed, by file. */
export interface InitResult {
  /** Files created or updated. */
  readonly changed: readonly string[];
}

async function readText(path: string): Promise<string | undefined> {
  try {
    return await readFile(path, 'utf8');
  } catch {
    return undefined;
  }
}

function nonEmpty(value: unknown): string | undefined {
  return typeof value === 'string' && value !== '' ? value : undefined;
}

/** Sets one identity key, refusing to change an existing different value. */
function identityKey(
  block: JsonObject,
  key: 'author' | 'name',
  wanted: string | undefined,
  fallback: string | undefined,
): void {
  const current = nonEmpty(block[key]);
  if (current !== undefined) {
    if (wanted !== undefined && wanted !== current) {
      throw new Error(
        `[OW] plugins.overwolf.${key} is already "${current}"; changing it changes the app uid, so edit tauri.conf.json by hand if you mean it`,
      );
    }
    return;
  }
  const value = wanted ?? fallback;
  if (value === undefined || value === '') {
    throw new Error(
      key === 'author'
        ? '[OW] pass --author "<your studio>" (an input of the app uid; for an ow-electron app use ow-tauri migrate instead)'
        : '[OW] pass --name "<app name>" or set productName in tauri.conf.json',
    );
  }
  block[key] = value;
}

async function configStep(options: InitOptions): Promise<boolean> {
  const path = join(options.tauriDir, BASE_FILE);
  const text = await readFile(path, 'utf8');
  const config = parseJsonObject(text, path);
  const plugins = isObject(config['plugins']) ? { ...config['plugins'] } : {};
  const block: JsonObject = { ...overwolfBlock(config) };
  identityKey(block, 'author', options.author, undefined);
  identityKey(block, 'name', options.name, nonEmpty(config['productName']));
  if (block['ads'] === undefined) block['ads'] = { testAd: true };
  plugins['overwolf'] = block;
  const next = formatJson({ ...config, plugins }, text);
  if (next === text) return false;
  await writeAtomic(path, next);
  return true;
}

/** The capabilities of one file: a capability, an array of them, or `{ capabilities: [...] }`. */
export function capabilitiesOf(document: unknown): JsonObject[] {
  if (Array.isArray(document)) return document.filter(isObject);
  if (!isObject(document)) return [];
  const list = document['capabilities'];
  if (Array.isArray(list)) return list.filter(isObject);
  return [document];
}

/**
 * Whether a capability grants a permission (as a string or `{ identifier }`).
 *
 * @param capability - the capability
 * @param permission - the permission identifier
 * @returns whether it is listed
 */
export function grants(capability: JsonObject, permission: string): boolean {
  const permissions = capability['permissions'];
  return (
    Array.isArray(permissions) &&
    permissions.some((p) => p === permission || (isObject(p) && p['identifier'] === permission))
  );
}

/**
 * Parses JSON text, `undefined` for missing or invalid text.
 *
 * @param text - the text, if any
 * @returns the value
 */
export function parseLoose(text: string | undefined): unknown {
  if (text === undefined) return undefined;
  try {
    return JSON.parse(text.replace(/^\uFEFF/, '')) as unknown;
  } catch {
    return undefined;
  }
}

function windowLabel(config: JsonObject): string {
  const windows = child(config, 'app')['windows'];
  const first: unknown = Array.isArray(windows) ? windows[0] : undefined;
  return (isObject(first) ? nonEmpty(first['label']) : undefined) ?? 'main';
}

async function capabilityStep(options: InitOptions): Promise<boolean> {
  const dir = join(options.tauriDir, 'capabilities');
  const files = existsSync(dir)
    ? readdirSync(dir)
        .filter((name) => name.endsWith('.json'))
        .sort()
    : [];
  for (const name of files) {
    const path = join(dir, name);
    if (capabilitiesOf(parseLoose(await readText(path))).some((c) => grants(c, DEFAULT_PERMISSION)))
      return false;
  }
  const path = join(dir, 'default.json');
  const text = await readText(path);
  if (text === undefined) {
    const config = parseJsonObject(
      await readFile(join(options.tauriDir, BASE_FILE), 'utf8'),
      BASE_FILE,
    );
    await mkdir(dir, { recursive: true });
    await writeFile(
      path,
      formatJson({
        $schema: '../gen/schemas/desktop-schema.json',
        identifier: 'default',
        description: 'Main webview: core APIs and Overwolf ads, consent and identity',
        webviews: [windowLabel(config)],
        permissions: ['core:default', DEFAULT_PERMISSION],
      }),
    );
    return true;
  }
  const capability = parseJsonObject(text, path);
  const permissions: unknown[] = Array.isArray(capability['permissions'])
    ? capability['permissions']
    : [];
  capability['permissions'] = [...permissions, DEFAULT_PERMISSION];
  if (capability['windows'] !== undefined) {
    options.log.warn(
      `${path} selects "windows": a window's capability also reaches the ad guests inside it; select "webviews" instead (DESIGN §2.4)`,
    );
  }
  await writeAtomic(path, formatJson(capability, text));
  return true;
}

async function overlayStep(options: InitOptions): Promise<boolean> {
  const path = join(options.tauriDir, overlayFile('windows'));
  const text = await readText(path);
  if (text === undefined) {
    await writeFile(
      path,
      formatJson({
        bundle: { targets: ['nsis'], windows: { nsis: { installerHooks: INSTALLER_HOOKS } } },
      }),
    );
    return true;
  }
  const overlay = parseJsonObject(text, path);
  const bundle = child(overlay, 'bundle');
  const windows = child(bundle, 'windows');
  const nsis = child(windows, 'nsis');
  const hooks = nsis['installerHooks'];
  if (hooks === INSTALLER_HOOKS) return false;
  if (hooks !== undefined) {
    options.log.warn(
      `${path} has its own NSIS hooks: !include gen/overwolf/overwolf-hooks.nsh there and call OW_TAURI_HOOK_POSTINSTALL / OW_TAURI_HOOK_POSTUNINSTALL from its NSIS_HOOK_* macros`,
    );
    return false;
  }
  const next = {
    ...overlay,
    bundle: {
      ...bundle,
      windows: { ...windows, nsis: { ...nsis, installerHooks: INSTALLER_HOOKS } },
    },
  };
  await writeAtomic(path, formatJson(next, text));
  return true;
}

/** Lines of a `.gitignore` that already cover `gen/overwolf`. */
const COVERING = new Set(['gen', 'gen/', 'gen/overwolf', 'gen/overwolf/', 'gen/*', '**/gen']);

async function gitignoreStep(options: InitOptions): Promise<boolean> {
  const path = join(options.tauriDir, '.gitignore');
  const text = (await readText(path)) ?? '';
  const covered = text
    .split(/\r?\n/)
    .map((line) => line.trim().replace(/^\//, ''))
    .some((line) => COVERING.has(line));
  if (covered) return false;
  const separator = text === '' || text.endsWith('\n') ? '' : '\n';
  await writeFile(path, `${text}${separator}${GITIGNORE_LINE}\n`);
  return true;
}

/**
 * Runs the steps in order and reports each.
 *
 * @param options - the options
 * @returns the files changed
 */
export async function init(options: InitOptions): Promise<InitResult> {
  const steps: [string, (o: InitOptions) => Promise<boolean>][] = [
    [BASE_FILE, configStep],
    [join('capabilities', 'default.json'), capabilityStep],
    [overlayFile('windows'), overlayStep],
    ['.gitignore', gitignoreStep],
  ];
  const changed: string[] = [];
  for (const [file, step] of steps) {
    if (await step(options)) {
      changed.push(file);
      options.log.info(`updated ${file}`);
    } else {
      options.log.info(`${file} is up to date`);
    }
  }
  if (changed.includes(BASE_FILE)) {
    options.log.info('test ads are on (plugins.overwolf.ads.testAd); remove it before shipping');
  }
  return { changed };
}
