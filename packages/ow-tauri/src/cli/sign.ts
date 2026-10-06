/// <reference types="node" />
/**
 * `ow-tauri sign` (`docs/CONTRACT.md` G.4, ADR 0016): the Overwolf signing
 * step of the published builder for a Tauri build.
 *
 * Steps a, b and the data of step c, with the gating of step e:
 *
 * 1. `POST /sign/electron` with the packaged `package.json` and the SHA-256
 *    of the main entry file;
 * 2. the signed `package.json` and `_metadata.json` from the response ZIP;
 * 3. `integrity.dll` from `integrityDllUrl`;
 * 4. `owe.json` (`{"appUid": ...}`) for the `OWEINTEGRITY/OWE` resource that
 *    the app's `build.rs` compiles.
 *
 * It then touches `package.json`, so the next `cargo build` re-runs the
 * app's build script and picks the signed output up.
 *
 * Never calls `/sign/asar` and never writes an asar integrity token: Tauri has
 * no asar (step f).
 *
 * @packageDocumentation
 */

import { createHash } from 'node:crypto';
import { mkdir, readFile, rename, utimes, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';

import { findAppExeNames } from './app-exe.js';
import type { Credentials } from './http.js';
import { download, postJson } from './http.js';
import type { JsonObject } from './package-json.js';
import {
  buildOverwolf,
  isCertSigningEnabled,
  isObject,
  isSigningRequired,
  packagedForm,
} from './package-json.js';
import { readZipEntries } from './zip.js';

/** The default signing service. */
export const DEFAULT_API_URL = 'https://console-be.overwolf.com';
/** The folder `ow-tauri sign` writes into, next to `package.json`. */
export const SIGNED_DIR = 'ow-tauri-signed';
/** The summary file inside {@link SIGNED_DIR}. */
export const RESULT_FILE = 'sign-result.json';

/** Where the CLI writes its messages. */
export interface Logger {
  /** Progress. */
  info(message: string): void;
  /** Something the developer should fix; the build goes on. */
  warn(message: string): void;
}

/** `ow-tauri sign` options. */
export interface SignOptions {
  /** Path of `package.json`. */
  readonly packageJson: string;
  /** The entry file to hash, relative to {@link SignOptions.projectDir}; default `main`. */
  readonly main?: string | undefined;
  /** The folder `main` is relative to; default the folder of `package.json`. */
  readonly projectDir?: string | undefined;
  /** Output folder; default `<package dir>/ow-tauri-signed`. */
  readonly outDir?: string | undefined;
  /** The target OS; signing is required (gating) for `win32` only. */
  readonly platform: string;
  /** Print the request instead of sending it. */
  readonly dryRun: boolean;
  /** The environment (credentials, URL, gating switches). */
  readonly env: Record<string, string | undefined>;
  /** Messages. */
  readonly log: Logger;
}

/** `sign-result.json`: what later steps (`build.rs`, `sign-exe`) read. */
export interface SignResult {
  /** The console-assigned uid from the signed `package.json`. */
  readonly uid: string;
  /** Whether the service allows Overwolf certificate signing for this app. */
  readonly isOwCertificateEnabled: boolean;
  /** Whether `enableOWCertSigning` (or `OW_ENABLE_CERT_SIGNING`) asked for it. */
  readonly enableOWCertSigning: boolean;
  /** The hashed entry file, relative to the project folder (the `fileHashes` key). */
  readonly mainFile: string;
  /** The hashed entry file, absolute, for the build's staleness check. */
  readonly mainPath: string;
  /**
   * The app exe file names of the Tauri project next to `package.json`
   * (`mainBinaryName`, else Cargo's binary name and `productName`), for
   * `sign-exe`; empty when no `tauri.conf.json` was found.
   */
  readonly appExeNames: readonly string[];
  /** Its SHA-256 (hex). */
  readonly mainSha256: string;
  /** The signed app version. */
  readonly version: string;
}

/** The outcome of {@link sign}. */
export type SignOutcome =
  | { readonly status: 'signed'; readonly result: SignResult; readonly outDir: string }
  | { readonly status: 'dry-run' }
  | { readonly status: 'unsigned'; readonly reason: string };

/**
 * The credentials from the environment, or `null` with the builder's
 * warning when any is missing.
 *
 * @param env - the environment
 * @param log - messages
 * @returns the credentials, or `null`
 */
export function credentials(
  env: Record<string, string | undefined>,
  log: Logger,
): Credentials | null {
  const email = env['OW_CLI_EMAIL'];
  const apiKey = env['OW_CLI_API_KEY'];
  if (!email || !apiKey) {
    log.warn('Missing OW_CLI_EMAIL / OW_CLI_API_KEY - package.json will not be signed');
    return null;
  }
  const appKey = env['OW_BUILD_KEY'];
  if (!appKey) {
    log.warn('Missing OW_BUILD_KEY - needed for runtime signature verification');
    return null;
  }
  return { email, apiKey, appKey };
}

/**
 * The signing service base URL: `OW_CLI_API_URL` or the default, without a
 * trailing `/`.
 *
 * @param env - the environment
 * @returns the base URL
 */
export function apiUrl(env: Record<string, string | undefined>): string {
  return (env['OW_CLI_API_URL'] ?? DEFAULT_API_URL).replace(/\/$/, '');
}

/**
 * Writes a file through a temporary sibling and a rename.
 *
 * @param path - the target
 * @param data - the content
 */
export async function writeAtomic(path: string, data: string | Buffer): Promise<void> {
  const temp = `${path}.ow-tmp`;
  await writeFile(temp, data);
  await rename(temp, path);
}

function sha256Hex(data: Buffer): string {
  return createHash('sha256').update(data).digest('hex');
}

interface SignResponse {
  readonly zip?: unknown;
  readonly integrityDllUrl?: unknown;
  readonly isOwCertificateEnabled?: unknown;
}

/**
 * Runs the signing step. With `required` (a Windows target whose
 * `requireSigning` is not `false`, or `OW_REQUIRE_SIGNING`), missing
 * credentials and any failure reject; otherwise they resolve `unsigned`
 * with the builder's warning.
 *
 * @param options - the options
 * @returns the outcome
 */
export async function sign(options: SignOptions): Promise<SignOutcome> {
  const { env, log } = options;
  const packagePath = resolve(options.packageJson);
  const pkgDir = dirname(packagePath);
  let pkg: JsonObject;
  try {
    const parsed: unknown = JSON.parse(await readFile(packagePath, 'utf8'));
    if (!isObject(parsed)) throw new Error('package.json is not a JSON object');
    pkg = parsed;
  } catch (error) {
    throw new Error(`[OW] cannot read ${packagePath}: ${(error as Error).message}`, {
      cause: error,
    });
  }
  const owBuild = buildOverwolf(pkg);
  const required =
    options.platform === 'win32' && isSigningRequired(owBuild['requireSigning'], env);
  try {
    const outcome = await signInner(options, pkg, pkgDir, owBuild);
    if (outcome.status === 'signed') {
      // Cargo watches package.json: a build after signing re-runs the
      // app's build script, which then applies the signed output.
      const now = new Date();
      await utimes(packagePath, now, now);
    }
    return outcome;
  } catch (error) {
    if (required) {
      log.warn('Overwolf signing failed - aborting build (requireSigning is enabled)');
      throw error;
    }
    const reason = (error as Error).message;
    log.warn(`${reason}\nOverwolf signing failed - building unsigned`);
    return { status: 'unsigned', reason };
  }
}

async function signInner(
  options: SignOptions,
  pkg: JsonObject,
  pkgDir: string,
  owBuild: JsonObject,
): Promise<SignOutcome> {
  const { env, log } = options;
  const packaged = packagedForm(pkg);
  const mainRel = options.main ?? (typeof packaged['main'] === 'string' ? packaged['main'] : '');
  const creds = credentials(env, log);
  if (!creds && !options.dryRun) {
    throw new Error(
      '[OW] signing required but OW_CLI_EMAIL/OW_CLI_API_KEY/OW_BUILD_KEY are not set',
    );
  }
  if (!mainRel) {
    throw new Error(
      '[OW] Missing main field in package.json — cannot determine entry file to sign',
    );
  }
  const projectDir = resolve(options.projectDir ?? pkgDir);
  let mainBytes: Buffer;
  try {
    mainBytes = await readFile(join(projectDir, mainRel));
  } catch (error) {
    throw new Error(`[OW] cannot load entry file to sign: ${(error as Error).message}`, {
      cause: error,
    });
  }
  const mainHash = sha256Hex(mainBytes);
  const url = new URL(`${apiUrl(env)}/sign/electron`);
  const body = { packageJson: packaged, fileHashes: { [mainRel]: mainHash } };
  if (options.dryRun) {
    log.info(
      [
        `dry run: POST ${url.href}`,
        `Authorization: Key ${creds ? `${creds.email}:<OW_CLI_API_KEY>` : '<missing credentials>'}`,
        `x-ow-app-key: ${creds ? '<OW_BUILD_KEY>' : '<missing>'}`,
        JSON.stringify(body, null, 2),
      ].join('\n'),
    );
    return { status: 'dry-run' };
  }
  if (!creds) throw new Error('unreachable: credentials checked above');
  log.info(`sending to Overwolf signing API ${url.href}`);
  let response: SignResponse;
  try {
    const raw = await postJson(url, body, creds);
    const parsed: unknown = JSON.parse(raw.toString('utf8'));
    response = isObject(parsed) ? parsed : {};
  } catch (error) {
    throw new Error(`[OW] Signing API call failed: ${(error as Error).message}`, { cause: error });
  }
  if (typeof response.zip !== 'string' || typeof response.integrityDllUrl !== 'string') {
    throw new Error('[OW] Signing API response missing zip or integrityDllUrl fields');
  }
  const entries = readZipEntries(Buffer.from(response.zip, 'base64'));
  const pkgData = entries.get('package.json');
  const metaData = entries.get('_metadata.json');
  if (!pkgData || !metaData) {
    throw new Error('[OW] Signing API response ZIP is missing package.json or _metadata.json');
  }
  const signedPackage: unknown = JSON.parse(pkgData.toString('utf8'));
  const metadata: unknown = JSON.parse(metaData.toString('utf8'));
  if (!isObject(signedPackage)) throw new Error('[OW] signed package.json is not an object');
  log.info('downloading signed integrity.dll');
  let dll: Buffer;
  try {
    dll = await download(new URL(response.integrityDllUrl));
  } catch (error) {
    throw new Error(`[OW] Failed to download integrity.dll: ${(error as Error).message}`, {
      cause: error,
    });
  }
  const ow = signedPackage['overwolf'];
  const uid = isObject(ow) && typeof ow['uid'] === 'string' ? ow['uid'] : '';
  const result: SignResult = {
    uid,
    isOwCertificateEnabled: response.isOwCertificateEnabled === true,
    enableOWCertSigning: isCertSigningEnabled(owBuild['enableOWCertSigning'], env),
    mainFile: mainRel,
    mainPath: join(projectDir, mainRel),
    appExeNames: await findAppExeNames([pkgDir]),
    mainSha256: mainHash,
    version: typeof signedPackage['version'] === 'string' ? signedPackage['version'] : '',
  };
  const outDir = resolve(options.outDir ?? join(pkgDir, SIGNED_DIR));
  await mkdir(outDir, { recursive: true });
  await writeAtomic(join(outDir, 'package.json'), `${JSON.stringify(signedPackage, null, 2)}\n`);
  await writeAtomic(join(outDir, '_metadata.json'), JSON.stringify(metadata, null, 2));
  await writeAtomic(join(outDir, 'integrity.dll'), dll);
  if (uid) await writeAtomic(join(outDir, 'owe.json'), JSON.stringify({ appUid: uid }));
  await writeAtomic(join(outDir, RESULT_FILE), `${JSON.stringify(result, null, 2)}\n`);
  const certNote =
    result.isOwCertificateEnabled || result.enableOWCertSigning
      ? ` isOwCertificateEnabled=${String(result.isOwCertificateEnabled)}`
      : '';
  log.info(`signing complete: uid ${uid || '(none)'}${certNote}; wrote ${outDir}`);
  if (result.enableOWCertSigning && !result.isOwCertificateEnabled) {
    log.warn(
      'this app is not eligible for Overwolf certificate signing - the Overwolf signing service did not enable it for this app.',
    );
  }
  return { status: 'signed', result, outDir };
}
