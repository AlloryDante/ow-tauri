/// <reference types="node" />
/**
 * `ow-tauri migrate --from <package.json> [--write <tauri.conf.json>]`
 * (DESIGN §2.9, §3.1): the `plugins.overwolf` block that keeps an
 * ow-electron app's uid, `app_name` and Overwolf flags in a Tauri build.
 *
 * The identity is read from the packaged form of `package.json`
 * (`build.extraMetadata` merged in), as ow-electron reads it at run time:
 *
 * - `name` = `productName` if it is a non-empty string, else `name`;
 * - `author` = a non-empty string verbatim, else a non-empty `author.name`,
 *   else `"unknown"` (CONTRACT G.2);
 * - `uid` = `overwolf.uid` when set (trimmed; skipped with a warning when it
 *   is not 1 to 64 ASCII letters or digits, as ow-electron skips it).
 *
 * `author` and `name` are always written, so the uid never depends on a
 * default again (DX-M7). The flags come from `build.overwolf`.
 *
 * @packageDocumentation
 */

import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

import {
  formatJson,
  isObject,
  mergePatch,
  parseJsonObject,
  readJsonObject,
  writeAtomic,
  type JsonObject,
} from './json.js';
import { buildOverwolf, packagedForm } from './package-json.js';
import type { Logger } from './sign.js';
import { computeUid, normalizeUid, overwolfBlock, resolveIdentity } from './tauri-config.js';

/** What {@link migrateBlock} derives from a `package.json`. */
export interface Migration {
  /** The `plugins.overwolf` block. */
  readonly block: JsonObject;
  /** The uid the ow-electron app runs with. */
  readonly uid: string;
  /** Its computed uid (`app_cuid`). */
  readonly cuid: string;
  /** Notes for the developer. */
  readonly warnings: readonly string[];
}

function nonEmpty(value: unknown): string | undefined {
  return typeof value === 'string' && value !== '' ? value : undefined;
}

/**
 * The author input of the uid formula (CONTRACT G.2).
 *
 * @param author - `package.json` `author`
 * @returns the author string, `"unknown"` when there is none
 */
export function electronAuthor(author: unknown): string {
  if (typeof author === 'string') return author === '' ? 'unknown' : author;
  if (isObject(author)) return nonEmpty(author['name']) ?? 'unknown';
  return 'unknown';
}

/**
 * The `plugins.overwolf` block of an ow-electron `package.json`.
 *
 * @param pkg - the parsed `package.json`
 * @returns the block, the uids and warnings
 * @throws when the package has neither `productName` nor `name`
 */
export function migrateBlock(pkg: JsonObject): Migration {
  const packaged = packagedForm(pkg);
  const warnings: string[] = [];
  const name = nonEmpty(packaged['productName']) ?? nonEmpty(packaged['name']);
  if (name === undefined) {
    throw new Error('[OW] the package.json has neither "productName" nor "name"');
  }
  const author = electronAuthor(packaged['author']);
  if (author === 'unknown' && packaged['author'] !== 'unknown') {
    warnings.push(
      'the package.json has no author; writing "unknown", which is what ow-electron hashed into the uid',
    );
  }
  const cuid = computeUid(author, name);
  const block: JsonObject = { author, name };
  let uid = cuid;
  const ow = isObject(packaged['overwolf']) ? packaged['overwolf'] : {};
  const rawUid = ow['uid'];
  if (typeof rawUid === 'string' && rawUid.trim() !== '') {
    const normalized = normalizeUid(rawUid);
    if (normalized === undefined) {
      warnings.push(
        `overwolf.uid "${rawUid}" is not 1 to 64 ASCII letters or digits; ow-electron ignores it, so it is not written`,
      );
    } else {
      block['uid'] = normalized;
      uid = normalized;
    }
  }
  const packages = ow['packages'];
  if (Array.isArray(packages) && packages.length > 0) {
    warnings.push(
      `overwolf.packages (${packages.map(String).join(', ')}): packages are not available on Tauri yet`,
    );
  }
  const owBuild = buildOverwolf(pkg);
  block['ads'] = { disableOptimization: owBuild['disableAdOptimization'] === true };
  block['signing'] = {
    requireSigning: owBuild['requireSigning'] !== false,
    owCertSigning: owBuild['enableOWCertSigning'] === true,
  };
  return { block, uid, cuid, warnings };
}

/** `ow-tauri migrate` options. */
export interface MigrateOptions {
  /** The ow-electron `package.json`. */
  readonly from: string;
  /** The `tauri.conf.json` to merge the block into, if any. */
  readonly write?: string | undefined;
  /** The working directory. */
  readonly cwd: string;
  /** Messages. */
  readonly log: Logger;
  /** Where the block is printed. */
  readonly out: (text: string) => void;
}

/**
 * Prints the block and, with `write`, merges it into `plugins.overwolf` of
 * a `tauri.conf.json` (other keys of the file and of the block are kept).
 *
 * @param options - the options
 * @returns the migration
 */
export async function migrate(options: MigrateOptions): Promise<Migration> {
  const migration = migrateBlock(await readJsonObject(resolve(options.cwd, options.from)));
  for (const warning of migration.warnings) options.log.warn(warning);
  options.out(formatJson({ plugins: { overwolf: migration.block } }));
  options.log.info(`uid ${migration.uid} (computed ${migration.cuid})`);
  if (options.write === undefined) return migration;
  const target = resolve(options.cwd, options.write);
  const text = await readFile(target, 'utf8').catch((error: unknown) => {
    throw new Error(`[OW] cannot read ${target}: ${(error as Error).message}`, { cause: error });
  });
  const config = parseJsonObject(text, target);
  const merged = mergePatch(config, { plugins: { overwolf: migration.block } }) as JsonObject;
  const configured = overwolfBlock(config)['uid'];
  if (migration.block['uid'] === undefined && typeof configured === 'string') {
    const identity = await resolveIdentity(merged, resolve(target, '..'));
    if (identity.uid !== migration.uid) {
      options.log.warn(
        `${target} pins plugins.overwolf.uid "${identity.uid}", but the ow-electron app runs as ${migration.uid}; remove the uid to keep the ow-electron one`,
      );
    }
  }
  await writeAtomic(target, formatJson(merged, text));
  options.log.info(`wrote plugins.overwolf to ${target}`);
  return migration;
}
