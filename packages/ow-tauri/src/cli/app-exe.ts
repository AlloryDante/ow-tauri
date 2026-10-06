/// <reference types="node" />
/**
 * The file name Tauri gives the app exe, for `ow-tauri sign-exe`
 * (`docs/CONTRACT.md` G.4 step d): only that file goes to Overwolf's
 * certificate service.
 *
 * Tauri names the main binary `mainBinaryName` when `tauri.conf.json` (or
 * `tauri.windows.conf.json`) sets it, and otherwise keeps Cargo's binary
 * name (`default-run`, the `[[bin]]` name or the package name). The
 * `productName` of `tauri.conf.json` and of the signed `package.json` are
 * also accepted, as older Tauri releases renamed the binary after it.
 *
 * @packageDocumentation
 */

import { readFile } from 'node:fs/promises';
import { join } from 'node:path';

import { isObject } from './package-json.js';

async function readText(path: string): Promise<string | null> {
  try {
    return await readFile(path, 'utf8');
  } catch {
    return null;
  }
}

async function readJsonObject(path: string): Promise<Record<string, unknown> | null> {
  const text = await readText(path);
  if (text === null) return null;
  try {
    const parsed: unknown = JSON.parse(text);
    return isObject(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

function str(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() !== '' ? value.trim() : undefined;
}

/**
 * The binary name Cargo builds for the package in `Cargo.toml` text:
 * `package.default-run`, else the first `[[bin]]` name, else
 * `package.name`. A small line reader, enough for the keys it needs.
 *
 * @param toml - the `Cargo.toml` text
 * @returns the binary name, or `undefined`
 */
export function cargoBinaryName(toml: string): string | undefined {
  let section = '';
  let packageName: string | undefined;
  let defaultRun: string | undefined;
  let firstBin: string | undefined;
  let binName: string | undefined;
  for (const raw of toml.split(/\r?\n/)) {
    const line = raw.replace(/\s+#.*$/, '').trim();
    const header = /^\[\[?\s*([^\]]+?)\s*\]\]?$/.exec(line);
    if (header) {
      if (section === '[[bin]]' && firstBin === undefined) firstBin = binName;
      section = line.startsWith('[[') ? `[[${header[1] ?? ''}]]` : (header[1] ?? '');
      binName = undefined;
      continue;
    }
    const kv = /^([A-Za-z0-9_-]+)\s*=\s*["']([^"']*)["']$/.exec(line);
    if (!kv) continue;
    const [, key, value] = kv;
    if (section === 'package' && key === 'name') packageName = value;
    if (section === 'package' && key === 'default-run') defaultRun = value;
    if (section === '[[bin]]' && key === 'name') binName = value;
  }
  if (section === '[[bin]]' && firstBin === undefined) firstBin = binName;
  return str(defaultRun) ?? str(firstBin) ?? str(packageName);
}

/**
 * The app exe names a Tauri project in `dir` builds: `<mainBinaryName>.exe`
 * when set (Tauri renames the binary to it), else Cargo's binary name and
 * the `productName`, each with `.exe`. Empty when `dir` has no
 * `tauri.conf.json`.
 *
 * @param dir - a folder that may hold `tauri.conf.json` and `Cargo.toml`
 * @returns the candidate file names
 */
export async function tauriAppExeNames(dir: string): Promise<string[]> {
  const conf = await readJsonObject(join(dir, 'tauri.conf.json'));
  if (!conf) return [];
  const windows = (await readJsonObject(join(dir, 'tauri.windows.conf.json'))) ?? {};
  const pick = (key: string): string | undefined => str(windows[key]) ?? str(conf[key]);
  const main = pick('mainBinaryName');
  if (main) return [`${main}.exe`];
  const names: string[] = [];
  const cargo = await readText(join(dir, 'Cargo.toml'));
  const binary = cargo === null ? undefined : cargoBinaryName(cargo);
  if (binary) names.push(`${binary}.exe`);
  const product = pick('productName');
  if (product) names.push(`${product}.exe`);
  return names;
}

/**
 * {@link tauriAppExeNames} of the first of `dirs` (and its `src-tauri`
 * folder) that holds a `tauri.conf.json`.
 *
 * @param dirs - folders to look in, in order
 * @returns the candidate file names
 */
export async function findAppExeNames(dirs: readonly string[]): Promise<string[]> {
  for (const dir of dirs) {
    for (const candidate of [dir, join(dir, 'src-tauri')]) {
      const names = await tauriAppExeNames(candidate);
      if (names.length > 0) return names;
    }
  }
  return [];
}
