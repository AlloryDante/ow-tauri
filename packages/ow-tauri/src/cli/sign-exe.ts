/// <reference types="node" />
/**
 * `ow-tauri sign-exe <file>` (`docs/CONTRACT.md` G.4 step d): the command
 * for Tauri's `bundle.windows.signCommand`.
 *
 * Tauri calls it for every binary it signs. When the last `ow-tauri sign`
 * recorded that the signing service enabled Overwolf certificate signing
 * and the app asked for it (`enableOWCertSigning` or
 * `OW_ENABLE_CERT_SIGNING`), the **app exe only** is posted to
 * `/sign/electron-certificate` and replaced with the signed copy from the
 * returned ZIP (kept as is on `isAlreadySigned`), as Overwolf's builder
 * does. Every other file, and every file when Overwolf signing is off, goes
 * to the `--fallback` command (the developer's own signing), or is left
 * unsigned when there is none.
 *
 * The app exe is the file Tauri names after `mainBinaryName` (else Cargo's
 * binary name or `productName`; see `app-exe.ts`), found from
 * `tauri.conf.json` next to the working folder, the names `ow-tauri sign`
 * recorded, and the signed `package.json`. `--app-exe` overrides them.
 *
 * @packageDocumentation
 */

import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { basename, dirname, join, resolve, sep } from 'node:path';

import { findAppExeNames } from './app-exe.js';

import { download, postFile } from './http.js';
import { isObject } from './package-json.js';
import type { Logger, SignResult } from './sign.js';
import { RESULT_FILE, SIGNED_DIR, apiUrl, credentials, writeAtomic } from './sign.js';
import { readZipEntries } from './zip.js';

/** `ow-tauri sign-exe` options. */
export interface SignExeOptions {
  /** The binary Tauri asks to sign. */
  readonly file: string;
  /**
   * The app exe's file name. Default: the names Tauri may give it, from
   * `tauri.conf.json` (`mainBinaryName`, else Cargo's binary name and
   * `productName`), `sign-result.json` and the signed `package.json`.
   */
  readonly appExe?: string | undefined;
  /** The `ow-tauri sign` output folder; default `./ow-tauri-signed`, else `../ow-tauri-signed`. */
  readonly signedDir?: string | undefined;
  /** A command for every other file; `%1` is replaced by the file path. */
  readonly fallback?: string | undefined;
  /** The working directory. */
  readonly cwd: string;
  /** The environment. */
  readonly env: Record<string, string | undefined>;
  /** Messages. */
  readonly log: Logger;
}

/** What {@link signExe} did. */
export type SignExeOutcome = 'overwolf' | 'already-signed' | 'fallback' | 'skipped';

/**
 * Picks the signed exe out of the certificate service's ZIP: the entry
 * named like the file, else the only `.exe`.
 *
 * @param zip - the archive
 * @param exeName - the file name that was sent
 * @returns the signed bytes
 */
export function extractSignedExe(zip: Buffer, exeName: string): Buffer {
  const entries = readZipEntries(zip);
  const exact = entries.get(exeName);
  if (exact) return exact;
  const exes = [...entries.entries()].filter(([name]) => name.toLowerCase().endsWith('.exe'));
  const only = exes.length === 1 ? exes[0] : undefined;
  if (only) return only[1];
  const names = [...entries.keys()].join(', ') || 'none';
  throw new Error(`[OW] signed zip does not contain "${exeName}" (entries: ${names})`);
}

function findSignedDir(options: SignExeOptions): string | null {
  if (options.signedDir !== undefined) return resolve(options.cwd, options.signedDir);
  for (const candidate of [SIGNED_DIR, join('..', SIGNED_DIR)]) {
    const dir = resolve(options.cwd, candidate);
    if (existsSync(join(dir, RESULT_FILE))) return dir;
  }
  return null;
}

async function readJson(path: string): Promise<Record<string, unknown> | null> {
  try {
    const parsed: unknown = JSON.parse(await readFile(path, 'utf8'));
    return isObject(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

/**
 * Splits a command line into words: spaces separate, double quotes group.
 *
 * @param line - the command line
 * @returns the words
 */
export function splitCommand(line: string): string[] {
  const words: string[] = [];
  let current = '';
  let quoted = false;
  let started = false;
  for (const ch of line) {
    if (ch === '"') {
      quoted = !quoted;
      started = true;
    } else if (/\s/.test(ch) && !quoted) {
      if (started) words.push(current);
      current = '';
      started = false;
    } else {
      current += ch;
      started = true;
    }
  }
  if (started) words.push(current);
  return words;
}

/**
 * The default app exe names: from `tauri.conf.json` next to the working
 * folder or the signed output, then the names `ow-tauri sign` recorded,
 * then the signed `package.json`'s `productName` / `name`.
 *
 * @param cwd - the working folder
 * @param signedDir - the `ow-tauri sign` output, if found
 * @param sign - its `sign-result.json`
 * @param signedPkg - its signed `package.json`
 * @returns the names, without duplicates
 */
async function defaultAppExeNames(
  cwd: string,
  signedDir: string | null,
  sign: Partial<SignResult> | null,
  signedPkg: Record<string, unknown> | null,
): Promise<string[]> {
  const dirs = [cwd, ...(signedDir ? [dirname(signedDir)] : [])];
  const names = [...(await findAppExeNames(dirs))];
  for (const recorded of Array.isArray(sign?.appExeNames) ? sign.appExeNames : []) {
    if (typeof recorded === 'string') names.push(recorded);
  }
  for (const key of ['productName', 'name']) {
    const value = signedPkg?.[key];
    if (typeof value === 'string' && value !== '') names.push(`${value}.exe`);
  }
  return [...new Set(names)];
}

/**
 * Whether Tauri asks to sign a binary that sits where the main binary
 * does: an `.exe` outside the installer `bundle` folders.
 *
 * @param file - the absolute path
 * @returns whether to warn when it is not matched
 */
function looksLikeMainBinary(file: string): boolean {
  return file.toLowerCase().endsWith('.exe') && !file.split(sep).includes('bundle');
}

function runFallback(command: string, file: string): Promise<void> {
  const words = splitCommand(command).map((w) => w.replaceAll('%1', file));
  const [program, ...args] = words;
  if (program === undefined) return Promise.reject(new Error('the --fallback command is empty'));
  return new Promise((resolvePromise, reject) => {
    const child = spawn(program, args, { stdio: 'inherit', windowsHide: true });
    child.on('error', reject);
    child.on('exit', (code) => {
      if (code === 0) resolvePromise();
      else reject(new Error(`the --fallback command exited with ${String(code)}`));
    });
  });
}

/**
 * Signs one binary for Tauri's `signCommand`.
 *
 * @param options - the options
 * @returns what was done
 */
export async function signExe(options: SignExeOptions): Promise<SignExeOutcome> {
  const { log, env } = options;
  const file = resolve(options.cwd, options.file);
  const signedDir = findSignedDir(options);
  const result = signedDir ? await readJson(join(signedDir, RESULT_FILE)) : null;
  const signedPkg = signedDir ? await readJson(join(signedDir, 'package.json')) : null;
  const sign = result as Partial<SignResult> | null;
  const owActive = sign?.isOwCertificateEnabled === true && sign.enableOWCertSigning === true;
  const appExeNames =
    options.appExe !== undefined
      ? [options.appExe]
      : await defaultAppExeNames(options.cwd, signedDir, sign, signedPkg);
  const name = basename(file).toLowerCase();
  const isAppExe = appExeNames.some((n) => n.toLowerCase() === name);
  if (owActive && isAppExe) {
    return signWithOverwolfCertificate(file, env, log);
  }
  if (owActive && looksLikeMainBinary(file)) {
    log.warn(
      `${basename(file)} is not the app exe (${appExeNames.join(', ') || 'unknown'}), so it is not signed with Overwolf's certificate; pass --app-exe "${basename(file)}" if it is the app exe`,
    );
  }
  if (options.fallback !== undefined) {
    await runFallback(options.fallback, file);
    return 'fallback';
  }
  log.info(`not signing ${basename(file)} (no Overwolf certificate signing for it, no --fallback)`);
  return 'skipped';
}

async function signWithOverwolfCertificate(
  file: string,
  env: Record<string, string | undefined>,
  log: Logger,
): Promise<SignExeOutcome> {
  const creds = credentials(env, log);
  if (!creds) {
    throw new Error(
      '[OW] certificate signing required but OW_CLI_EMAIL/OW_CLI_API_KEY/OW_BUILD_KEY are not set',
    );
  }
  const url = new URL(`${apiUrl(env)}/sign/electron-certificate`);
  log.info(`signing ${basename(file)} with Overwolf certificate`);
  const raw = await postFile(url, file, creds);
  let response: Record<string, unknown>;
  try {
    const parsed: unknown = JSON.parse(raw.toString('utf8'));
    response = isObject(parsed) ? parsed : {};
  } catch (error) {
    throw new Error(
      `[OW] certificate signing API returned a non-JSON response: ${(error as Error).message}`,
      { cause: error },
    );
  }
  if (response['isAlreadySigned'] === true) {
    log.info(`${basename(file)} is already signed - skipping`);
    return 'already-signed';
  }
  const zipUrl = response['zip'];
  if (typeof zipUrl !== 'string') {
    throw new Error('[OW] certificate signing API response missing zip url');
  }
  let zip: Buffer;
  try {
    zip = await download(new URL(zipUrl));
  } catch (error) {
    throw new Error(`[OW] failed to download signed executable: ${(error as Error).message}`, {
      cause: error,
    });
  }
  await writeAtomic(file, extractSignedExe(zip, basename(file)));
  log.info(`${basename(file)} signed with Overwolf certificate`);
  return 'overwolf';
}
