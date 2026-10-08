/// <reference types="node" />
/**
 * `ow-tauri sign` (`docs/CONTRACT.md` G.4, DESIGN §2.7, §4.15): Overwolf
 * signing for a Tauri build, after the frontend build.
 *
 * 1. `POST /sign/electron` with a `packageJson` body synthesised from the
 *    merged Tauri configuration (`name`, `productName`, `version`, `author`,
 *    `overwolf.uid` when configured, `main`) and the SHA-256 of the entry
 *    file (`--main`, else `plugins.overwolf.signing.entry`);
 * 2. the signed `package.json` and `_metadata.json` from the response ZIP;
 * 3. `integrity.dll` from `integrityDllUrl`;
 * 4. `owe.json` (`{"appUid": ...}`) for the `OWEINTEGRITY/OWE` resource the
 *    build step links.
 *
 * The uid in Overwolf's signed response must equal the uid the app resolves
 * from `plugins.overwolf` (PAR-B2); otherwise nothing is written, unless
 * `--write-uid` pins the signed uid in `tauri.conf.json`.
 *
 * Signing is opt-in (`plugins.overwolf.signing.enabled`). Never calls
 * `/sign/asar`: Tauri has no asar (G.4 step f).
 *
 * @packageDocumentation
 */

import { createHash } from 'node:crypto';
import { mkdir, readFile } from 'node:fs/promises';
import { isAbsolute, join, relative, resolve, sep } from 'node:path';

import { findAppExeNames } from './app-exe.js';
import type { Credentials } from './http.js';
import { download, postJson } from './http.js';
import {
  formatJson,
  isObject,
  mergePatch,
  parseJsonObject,
  writeAtomic,
  type JsonObject,
} from './json.js';
import { envOn, isCertSigningEnabled, isSigningRequired } from './package-json.js';
import {
  BASE_FILE,
  UNPINNED_MESSAGE,
  cargoPackage,
  child,
  overwolfBlock,
  projectDirOf,
  resolveIdentity,
  type Identity,
  type LoadedConfig,
} from './tauri-config.js';
import { readZipEntries } from './zip.js';

export { writeAtomic } from './json.js';

/** The default signing service. */
export const DEFAULT_API_URL = 'https://console-be.overwolf.com';
/**
 * The folder `ow-tauri sign` writes into, in the project folder (the parent
 * of `src-tauri`): the bundle maps `../signed/integrity.dll` (DESIGN §2.7).
 */
export const SIGNED_DIR = 'signed';
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
  /** The merged Tauri configuration. */
  readonly loaded: LoadedConfig;
  /** The entry file to hash, relative to {@link SignOptions.cwd}; default `signing.entry`. */
  readonly main?: string | undefined;
  /** The working directory (`--main` and `--out` are relative to it). */
  readonly cwd: string;
  /** Output folder; default `<project>/signed`. */
  readonly outDir?: string | undefined;
  /** The target OS (`win32`, `darwin`, `linux`); signing is required (gating) for `win32` only. */
  readonly platform: string;
  /** Print the request instead of sending it. */
  readonly dryRun: boolean;
  /** Pin the signed uid in `tauri.conf.json` when it differs from the resolved one. */
  readonly writeUid: boolean;
  /** The environment (credentials, URL, gating switches). */
  readonly env: Readonly<Record<string, string | undefined>>;
  /** Messages. */
  readonly log: Logger;
}

/** `sign-result.json`: what later steps (`build::run`, `sign-exe`) read. */
export interface SignResult {
  /** The uid of the signed `package.json` (equal to the app's resolved uid). */
  readonly uid: string;
  /** Whether the service allows Overwolf certificate signing for this app. */
  readonly isOwCertificateEnabled: boolean;
  /** Whether `signing.owCertSigning` (or `OW_ENABLE_CERT_SIGNING`) asked for it. */
  readonly enableOWCertSigning: boolean;
  /** The `fileHashes` key: the entry file as given, with `/` separators. */
  readonly mainFile: string;
  /** The hashed entry file, absolute, for the build's staleness check. */
  readonly mainPath: string;
  /**
   * The app exe file names of the Tauri project (`mainBinaryName`, else
   * Cargo's binary name and `productName`), for `sign-exe`.
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
  | { readonly status: 'disabled' }
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
  env: Readonly<Record<string, string | undefined>>,
  log: Logger,
): Credentials | null {
  const email = env['OW_CLI_EMAIL'];
  const apiKey = env['OW_CLI_API_KEY'];
  if (!email || !apiKey) {
    log.warn('Missing OW_CLI_EMAIL / OW_CLI_API_KEY - the app will not be signed');
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
export function apiUrl(env: Readonly<Record<string, string | undefined>>): string {
  return (env['OW_CLI_API_URL'] ?? DEFAULT_API_URL).replace(/\/$/, '');
}

function sha256Hex(data: Buffer): string {
  return createHash('sha256').update(data).digest('hex');
}

/**
 * The message of a signed uid that differs from the configured one (PAR-B2).
 *
 * @param signed - the uid in Overwolf's signed response
 * @param resolved - the uid `plugins.overwolf` resolves to
 * @returns the message
 */
export function uidMismatchMessage(signed: string, resolved: string): string {
  return `[OW] the console signed uid ${signed} but plugins.overwolf resolves to ${resolved}; set plugins.overwolf.uid to "${signed}" (or run ow-tauri sign --write-uid)`;
}

/** A failure that fails the command even when signing is not required. */
class FatalSignError extends Error {}

/**
 * The `packageJson` body Overwolf signs, synthesised from the configuration
 * (DESIGN §4.15). `name` is the Cargo package name when there is one (an
 * npm-style name), else `<PN>`; `productName` is `<PN>`, so the service's
 * uid formula sees the same input as the plugin.
 *
 * @param identity - the resolved identity
 * @param main - the entry file key
 * @param cargoName - the Cargo package name, if any
 * @returns the body
 */
export function signingPackageJson(
  identity: Identity,
  main: string,
  cargoName: string | undefined,
): JsonObject {
  const pkg: JsonObject = {
    name: cargoName ?? identity.name,
    productName: identity.name,
    version: identity.version ?? '',
    author: identity.author,
  };
  if (identity.uidConfigured) pkg['overwolf'] = { uid: identity.uid };
  pkg['main'] = main;
  return pkg;
}

/** The `fileHashes` key of an entry path: relative paths as given, `/` separators. */
function entryKey(path: string): string {
  return path.split(sep).join('/').replace(/^\.\//, '');
}

interface SignResponse {
  readonly zip?: unknown;
  readonly integrityDllUrl?: unknown;
  readonly isOwCertificateEnabled?: unknown;
}

/**
 * Runs the signing step. With `required` (a Windows target whose
 * `signing.requireSigning` is not `false`, or `OW_REQUIRE_SIGNING`), missing
 * credentials and any failure reject; otherwise they resolve `unsigned`
 * with the builder's warning. A uid mismatch and configuration errors always
 * reject.
 *
 * @param options - the options
 * @returns the outcome
 */
export async function sign(options: SignOptions): Promise<SignOutcome> {
  const { env, log, loaded } = options;
  const signing = child(overwolfBlock(loaded.config), 'signing');
  if (signing['enabled'] !== true && !envOn(env['OW_REQUIRE_SIGNING'])) {
    log.warn(
      'Overwolf signing is off (plugins.overwolf.signing.enabled is not true); nothing to sign',
    );
    return { status: 'disabled' };
  }
  const identity = await resolveIdentity(loaded.config, loaded.tauriDir);
  if (!identity.pinned) throw new Error(`[OW] ${UNPINNED_MESSAGE}`);
  const required =
    options.platform === 'win32' && isSigningRequired(signing['requireSigning'], env);
  try {
    return await signInner(options, identity, signing);
  } catch (error) {
    if (required || error instanceof FatalSignError) {
      if (!(error instanceof FatalSignError)) {
        log.warn('Overwolf signing failed - aborting build (requireSigning is enabled)');
      }
      throw error;
    }
    const reason = (error as Error).message;
    log.warn(`${reason}\nOverwolf signing failed - building unsigned`);
    return { status: 'unsigned', reason };
  }
}

async function signInner(
  options: SignOptions,
  identity: Identity,
  signing: JsonObject,
): Promise<SignOutcome> {
  const { env, log, loaded } = options;
  const projectDir = projectDirOf(loaded.tauriDir);
  const entry =
    options.main ?? (typeof signing['entry'] === 'string' ? signing['entry'] : undefined);
  if (entry === undefined || entry === '') {
    throw new FatalSignError(
      '[OW] no entry file to sign: pass --main or set plugins.overwolf.signing.entry',
    );
  }
  // --main is relative to the working directory, signing.entry to the project folder.
  const mainPath = resolve(options.main !== undefined ? options.cwd : projectDir, entry);
  const mainKey = entryKey(isAbsolute(entry) ? relative(projectDir, mainPath) : entry);
  const creds = credentials(env, log);
  if (!creds && !options.dryRun) {
    throw new Error(
      '[OW] signing required but OW_CLI_EMAIL/OW_CLI_API_KEY/OW_BUILD_KEY are not set',
    );
  }
  let mainBytes: Buffer;
  try {
    mainBytes = await readFile(mainPath);
  } catch (error) {
    throw new Error(`[OW] cannot load entry file to sign: ${(error as Error).message}`, {
      cause: error,
    });
  }
  const mainHash = sha256Hex(mainBytes);
  const cargo = cargoPackage(
    await readFile(join(loaded.tauriDir, 'Cargo.toml'), 'utf8').catch(() => ''),
  );
  const url = new URL(`${apiUrl(env)}/sign/electron`);
  const body = {
    packageJson: signingPackageJson(identity, mainKey, cargo.name === '' ? undefined : cargo.name),
    fileHashes: { [mainKey]: mainHash },
  };
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
  const ow = signedPackage['overwolf'];
  const signedUid = isObject(ow) && typeof ow['uid'] === 'string' ? ow['uid'] : '';
  if (signedUid !== '' && signedUid !== identity.uid) {
    if (!options.writeUid) throw new FatalSignError(uidMismatchMessage(signedUid, identity.uid));
    await pinUid(loaded.tauriDir, signedUid);
    log.warn(
      `the console signed uid ${signedUid}; wrote plugins.overwolf.uid "${signedUid}" to ${BASE_FILE} (it was ${identity.uid})`,
    );
  }
  log.info('downloading signed integrity.dll');
  let dll: Buffer;
  try {
    dll = await download(new URL(response.integrityDllUrl));
  } catch (error) {
    throw new Error(`[OW] Failed to download integrity.dll: ${(error as Error).message}`, {
      cause: error,
    });
  }
  const uid = signedUid || identity.uid;
  const result: SignResult = {
    uid,
    isOwCertificateEnabled: response.isOwCertificateEnabled === true,
    enableOWCertSigning: isCertSigningEnabled(signing['owCertSigning'], env),
    mainFile: mainKey,
    mainPath,
    appExeNames: await findAppExeNames([loaded.tauriDir]),
    mainSha256: mainHash,
    version: typeof signedPackage['version'] === 'string' ? signedPackage['version'] : '',
  };
  const outDir = resolve(
    options.outDir !== undefined ? options.cwd : projectDir,
    options.outDir ?? SIGNED_DIR,
  );
  await mkdir(outDir, { recursive: true });
  await writeAtomic(join(outDir, 'package.json'), `${JSON.stringify(signedPackage, null, 2)}\n`);
  await writeAtomic(join(outDir, '_metadata.json'), JSON.stringify(metadata, null, 2));
  await writeAtomic(join(outDir, 'integrity.dll'), dll);
  await writeAtomic(join(outDir, 'owe.json'), JSON.stringify({ appUid: uid }));
  await writeAtomic(join(outDir, RESULT_FILE), `${JSON.stringify(result, null, 2)}\n`);
  const certNote =
    result.isOwCertificateEnabled || result.enableOWCertSigning
      ? ` isOwCertificateEnabled=${String(result.isOwCertificateEnabled)}`
      : '';
  log.info(`signing complete: uid ${uid}${certNote}; wrote ${outDir}`);
  if (result.enableOWCertSigning && !result.isOwCertificateEnabled) {
    log.warn(
      'this app is not eligible for Overwolf certificate signing - the Overwolf signing service did not enable it for this app.',
    );
  }
  return { status: 'signed', result, outDir };
}

/** Writes `plugins.overwolf.uid` into the base `tauri.conf.json`. */
async function pinUid(tauriDir: string, uid: string): Promise<void> {
  const path = join(tauriDir, BASE_FILE);
  const text = await readFile(path, 'utf8');
  const merged = mergePatch(parseJsonObject(text, path), { plugins: { overwolf: { uid } } });
  await writeAtomic(path, formatJson(merged, text));
}
