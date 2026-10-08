/// <reference types="node" />
/**
 * The app's Tauri configuration as the build sees it (DESIGN §3.1, "one
 * parser"): the same three layers `tauri-build` merges, in the same order.
 *
 * 1. `tauri.conf.json` of the Tauri folder;
 * 2. the platform overlay `tauri.<windows|macos|linux|android|ios>.conf.json`
 *    of the build target, merged as an RFC 7396 merge patch;
 * 3. the `TAURI_CONFIG` environment variable (JSON), merged the same way.
 *    The Tauri CLI sets it from its `--config` values (merged in order into
 *    one patch starting from `{}`, replacing an inherited `TAURI_CONFIG`);
 *    `ow-tauri --config` does the same. A `null` in a `--config` value only
 *    removes the key from that combined patch, as in the Tauri CLI.
 *
 * Only `tauri.conf.json` files are read: a project whose configuration is
 * `tauri.conf.json5` or `Tauri.toml` gets an error instead of a guess.
 *
 * `fixtures/config-merge` holds golden cases shared with the crate's
 * `build::run`, so the CLI and the build always agree on the app identity.
 *
 * @packageDocumentation
 */

import { createHash } from 'node:crypto';
import { existsSync, statSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';

import { isObject, mergePatch, parseJsonObject, readJsonObject, type JsonObject } from './json.js';

/** A Tauri build target (`tauri_utils::platform::Target`). */
export type TauriTarget = 'windows' | 'macos' | 'linux' | 'android' | 'ios';

/** The base configuration file. */
export const BASE_FILE = 'tauri.conf.json';

/** Configuration formats `ow-tauri` does not read. */
const OTHER_FORMATS = ['tauri.conf.json5', 'Tauri.toml'];

/**
 * The overlay file of a target.
 *
 * @param target - the build target
 * @returns its file name
 */
export function overlayFile(target: TauriTarget): string {
  return `tauri.${target}.conf.json`;
}

/**
 * The build target of a Node platform or a Rust target triple, as
 * `Target::from_triple` maps it (`darwin` → macOS, `windows`/`win32` →
 * Windows, Android, iOS, everything else Linux).
 *
 * @param platform - `process.platform`, a target name or a triple
 * @returns the target
 */
export function targetOf(platform: string): TauriTarget {
  const p = platform.toLowerCase();
  if (p === 'macos' || p.includes('darwin')) return 'macos';
  if (p === 'win32' || p.includes('windows')) return 'windows';
  if (p.includes('android')) return 'android';
  if (p === 'ios' || p.includes('-ios')) return 'ios';
  return 'linux';
}

/**
 * The Tauri folder: `explicit` when given, else `cwd` when it holds
 * `tauri.conf.json`, else `cwd/src-tauri`.
 *
 * @param cwd - the working directory
 * @param explicit - `--tauri-dir`
 * @returns the folder
 * @throws when no `tauri.conf.json` is found, or the project uses another format
 */
export function findTauriDir(cwd: string, explicit?: string): string {
  const candidates =
    explicit === undefined ? [cwd, join(cwd, 'src-tauri')] : [resolve(cwd, explicit)];
  for (const dir of candidates) {
    if (existsSync(join(dir, BASE_FILE))) return dir;
    const other = OTHER_FORMATS.find((name) => existsSync(join(dir, name)));
    if (other !== undefined) {
      throw new Error(
        `[OW] ${join(dir, other)}: ow-tauri reads tauri.conf.json only; convert the configuration to JSON`,
      );
    }
  }
  throw new Error(
    `[OW] no ${BASE_FILE} in ${candidates.join(' or ')}; run ow-tauri in the app folder or pass --tauri-dir`,
  );
}

/** Options of {@link loadTauriConfig}. */
export interface LoadOptions {
  /** The Tauri folder (holds `tauri.conf.json`). */
  readonly tauriDir: string;
  /** The build target whose overlay applies. */
  readonly target: TauriTarget;
  /** The environment (`TAURI_CONFIG`). */
  readonly env: Readonly<Record<string, string | undefined>>;
  /**
   * `--config` values: JSON text (starting with `{`) or a JSON file path
   * relative to {@link LoadOptions.cwd}. When any is given they replace
   * `TAURI_CONFIG`, as the Tauri CLI does.
   */
  readonly configs?: readonly string[] | undefined;
  /** The folder `--config` file paths are relative to. */
  readonly cwd: string;
}

/** The merged configuration. */
export interface LoadedConfig {
  /** The merged document (before Tauri's own defaults). */
  readonly config: JsonObject;
  /** The Tauri folder. */
  readonly tauriDir: string;
  /** The configuration files read, base first. */
  readonly files: readonly string[];
  /** The patch of layer 3, if any. */
  readonly extra: JsonObject | undefined;
}

async function readExtra(options: LoadOptions): Promise<JsonObject | undefined> {
  const configs = options.configs ?? [];
  if (configs.length > 0) {
    let merged: unknown = {};
    for (const value of configs) {
      const text = value.trimStart();
      const patch = text.startsWith('{')
        ? parseJsonObject(text, '--config')
        : await readJsonObject(resolve(options.cwd, value));
      merged = mergePatch(merged, patch);
    }
    return merged as JsonObject;
  }
  const env = options.env['TAURI_CONFIG'];
  return env === undefined ? undefined : parseJsonObject(env, 'TAURI_CONFIG');
}

/**
 * Reads the merged configuration (layers 1 to 3).
 *
 * @param options - folder, target, environment and `--config` values
 * @returns the configuration and the files it came from
 */
export async function loadTauriConfig(options: LoadOptions): Promise<LoadedConfig> {
  const base = join(options.tauriDir, BASE_FILE);
  let config: unknown = await readJsonObject(base);
  const files = [base];
  const overlay = join(options.tauriDir, overlayFile(options.target));
  if (existsSync(overlay)) {
    config = mergePatch(config, await readJsonObject(overlay));
    files.push(overlay);
  } else {
    for (const name of [`tauri.${options.target}.conf.json5`, `Tauri.${options.target}.toml`]) {
      if (existsSync(join(options.tauriDir, name))) {
        throw new Error(
          `[OW] ${join(options.tauriDir, name)}: ow-tauri reads JSON overlays only; convert it to ${overlayFile(options.target)}`,
        );
      }
    }
  }
  const extra = await readExtra(options);
  if (extra !== undefined) config = mergePatch(config, extra);
  return { config: config as JsonObject, tauriDir: options.tauriDir, files, extra };
}

/**
 * `plugins.overwolf` of a configuration, or an empty object.
 *
 * @param config - the merged configuration
 * @returns the plugin block
 */
export function overwolfBlock(config: JsonObject): JsonObject {
  const plugins = config['plugins'];
  const block = isObject(plugins) ? plugins['overwolf'] : undefined;
  return isObject(block) ? block : {};
}

/**
 * A nested object of a JSON object, or an empty object.
 *
 * @param object - the parent
 * @param key - the key
 * @returns the child object
 */
export function child(object: JsonObject, key: string): JsonObject {
  const value = object[key];
  return isObject(value) ? value : {};
}

/**
 * The uid formula of ow-electron (CONTRACT G.2 rule 3), including the
 * `.electron` suffix: `sha1("{'author':'<author>','name':'<name>.electron'}")`
 * with each byte `b` written as `chr(97 + (b & 15)) + chr(97 + (b >> 4))`.
 * Nothing is trimmed or escaped.
 *
 * @param author - the author string (`"unknown"` when there is none)
 * @param name - the app name (`<PN>`)
 * @returns the 40-character uid
 */
export function computeUid(author: string, name: string): string {
  const digest = createHash('sha1')
    .update(`{'author':'${author}','name':'${name}.electron'}`, 'utf8')
    .digest();
  let uid = '';
  for (const byte of digest) {
    uid += String.fromCharCode(97 + (byte & 15), 97 + (byte >> 4));
  }
  return uid;
}

/** The uid rule of `plugins.overwolf.uid`: 1 to 64 ASCII letters or digits after trimming. */
const UID_PATTERN = /^[A-Za-z0-9]{1,64}$/;

/**
 * A configured uid after trimming, or `undefined` when it breaks the rule.
 *
 * @param value - the configured value
 * @returns the uid, or `undefined`
 */
export function normalizeUid(value: string): string | undefined {
  const trimmed = value.trim();
  return UID_PATTERN.test(trimmed) ? trimmed : undefined;
}

/** The app identity the plugin derives from the configuration (DESIGN §3.1). */
export interface Identity {
  /** The uid: `plugins.overwolf.uid`, else the computed uid. */
  readonly uid: string;
  /** The computed uid (always the formula). */
  readonly cuid: string;
  /** `<PN>`: `plugins.overwolf.name`, else `productName`, else the Cargo package name. */
  readonly name: string;
  /** The uid author input (`"unknown"` when unset). */
  readonly author: string;
  /** The app version (`version`, a JSON file's `version`, or Cargo's), if any. */
  readonly version: string | undefined;
  /** Whether `plugins.overwolf.uid` sets the uid. */
  readonly uidConfigured: boolean;
  /** Whether the uid cannot change through defaults: `uid`, or both `author` and `name`, set. */
  readonly pinned: boolean;
  /** Where `<PN>` came from. */
  readonly nameSource: 'plugins.overwolf.name' | 'productName' | 'Cargo.toml';
  /** Notes for the developer (missing inputs that fell back to defaults). */
  readonly warnings: readonly string[];
}

function nonEmpty(value: unknown): string | undefined {
  return typeof value === 'string' && value !== '' ? value : undefined;
}

/** The Cargo `[package]` name and version of a `Cargo.toml` text. */
export function cargoPackage(toml: string): { name?: string; version?: string } {
  let section = '';
  const out: { name?: string; version?: string } = {};
  for (const raw of toml.split(/\r?\n/)) {
    const line = raw.replace(/\s+#.*$/, '').trim();
    const header = /^\[\[?\s*([^\]]+?)\s*\]\]?$/.exec(line);
    if (header) {
      section = header[1] ?? '';
      continue;
    }
    if (section !== 'package') continue;
    const kv = /^(name|version)\s*=\s*["']([^"']*)["']$/.exec(line);
    if (kv?.[1] === 'name') out.name = kv[2] ?? '';
    if (kv?.[1] === 'version') out.version = kv[2] ?? '';
  }
  return out;
}

async function readCargo(tauriDir: string): Promise<{ name?: string; version?: string }> {
  try {
    return cargoPackage(await readFile(join(tauriDir, 'Cargo.toml'), 'utf8'));
  } catch {
    return {};
  }
}

/**
 * The app version: `version` as written, or, when it names an existing file
 * (relative to the Tauri folder, where Tauri reads it), that JSON file's
 * `version`; else the Cargo package version.
 *
 * @param config - the merged configuration
 * @param tauriDir - the Tauri folder
 * @param cargoVersion - the Cargo package version
 * @returns the version, if any
 */
async function resolveVersion(
  config: JsonObject,
  tauriDir: string,
  cargoVersion: string | undefined,
): Promise<string | undefined> {
  const version = nonEmpty(config['version']);
  if (version === undefined) return cargoVersion;
  const path = resolve(tauriDir, version);
  if (existsSync(path) && statSync(path).isFile()) {
    const file = await readJsonObject(path);
    const value = file['version'];
    if (typeof value !== 'string') {
      throw new Error(
        `[OW] ${path}: "version" must be a string (tauri.conf.json > version names this file)`,
      );
    }
    return value;
  }
  return version;
}

/**
 * The app identity of a merged configuration.
 *
 * @param config - the merged configuration
 * @param tauriDir - the Tauri folder (version files, `Cargo.toml`)
 * @returns the identity
 * @throws when `plugins.overwolf.uid` breaks the uid rule or no app name is found
 */
export async function resolveIdentity(config: JsonObject, tauriDir: string): Promise<Identity> {
  const ow = overwolfBlock(config);
  const warnings: string[] = [];
  const cargo = await readCargo(tauriDir);
  const owName = nonEmpty(ow['name']);
  const productName = nonEmpty(config['productName']);
  let name: string;
  let nameSource: Identity['nameSource'];
  if (owName !== undefined) {
    name = owName;
    nameSource = 'plugins.overwolf.name';
  } else if (productName !== undefined) {
    name = productName;
    nameSource = 'productName';
  } else if (cargo.name) {
    name = cargo.name;
    nameSource = 'Cargo.toml';
    warnings.push(
      `neither plugins.overwolf.name nor productName is set; the app name falls back to the Cargo package name "${name}"`,
    );
  } else {
    throw new Error('[OW] set plugins.overwolf.name or productName in tauri.conf.json');
  }
  const owAuthor = nonEmpty(ow['author']);
  const author = owAuthor ?? 'unknown';
  if (owAuthor === undefined) {
    warnings.push(
      'plugins.overwolf.author is not set; the uid uses "unknown" (debug builds only, a release build fails)',
    );
  }
  const cuid = computeUid(author, name);
  let uid = cuid;
  let uidConfigured = false;
  const rawUid = ow['uid'];
  if (rawUid !== undefined && rawUid !== null) {
    const normalized = typeof rawUid === 'string' ? normalizeUid(rawUid) : undefined;
    if (normalized === undefined) {
      throw new Error('[OW] plugins.overwolf.uid: must be 1 to 64 ASCII letters or digits');
    }
    uid = normalized;
    uidConfigured = true;
  }
  return {
    uid,
    cuid,
    name,
    author,
    version: await resolveVersion(config, tauriDir, nonEmpty(cargo.version)),
    uidConfigured,
    pinned: uidConfigured || (owAuthor !== undefined && owName !== undefined),
    nameSource,
    warnings,
  };
}

/** The release rule of an unpinned uid (DESIGN §3.2, DX-M7). */
export const UNPINNED_MESSAGE =
  'plugins.overwolf: set "uid", or both "author" and "name", before a release build (the uid must not depend on defaults)';

/**
 * The project folder of a Tauri folder: its parent (where `package.json`
 * and the frontend live in the standard layout).
 *
 * @param tauriDir - the Tauri folder
 * @returns the project folder
 */
export function projectDirOf(tauriDir: string): string {
  return dirname(tauriDir);
}
