// @vitest-environment node
/// <reference types="node" />
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import type { IncomingMessage, Server, ServerResponse } from 'node:http';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { deflateRawSync } from 'node:zlib';

import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { cargoBinaryName, tauriAppExeNames } from './app-exe.js';
import {
  deepAssign,
  envOn,
  isCertSigningEnabled,
  isSigningRequired,
  packagedForm,
} from './package-json.js';
import { USAGE, parseArgs, run } from './run.js';
import type { CliIo } from './run.js';
import { sign, signingPackageJson, uidMismatchMessage } from './sign.js';
import type { Logger, SignOptions } from './sign.js';
import { extractSignedExe, signExe, splitCommand } from './sign-exe.js';
import { computeUid, loadTauriConfig } from './tauri-config.js';
import { readZipEntries } from './zip.js';

/** A ZIP of `entries`, stored or deflated (CRCs are not checked by the reader). */
function makeZip(entries: Record<string, Buffer | string>, deflate = false): Buffer {
  const locals: Buffer[] = [];
  const centrals: Buffer[] = [];
  let offset = 0;
  for (const [name, content] of Object.entries(entries)) {
    const raw = Buffer.from(content);
    const data = deflate ? deflateRawSync(raw) : raw;
    const nameBuf = Buffer.from(name, 'utf8');
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(deflate ? 8 : 0, 8);
    local.writeUInt32LE(data.length, 18);
    local.writeUInt32LE(raw.length, 22);
    local.writeUInt16LE(nameBuf.length, 26);
    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(deflate ? 8 : 0, 10);
    central.writeUInt32LE(data.length, 20);
    central.writeUInt32LE(raw.length, 24);
    central.writeUInt16LE(nameBuf.length, 28);
    central.writeUInt32LE(offset, 42);
    locals.push(local, nameBuf, data);
    centrals.push(central, nameBuf);
    offset += local.length + nameBuf.length + data.length;
  }
  const cd = Buffer.concat(centrals);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(Object.keys(entries).length, 8);
  eocd.writeUInt16LE(Object.keys(entries).length, 10);
  eocd.writeUInt32LE(cd.length, 12);
  eocd.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, cd, eocd]);
}

interface Seen {
  method: string;
  url: string;
  headers: IncomingMessage['headers'];
  body: Buffer;
}

type Handler = (req: Seen, res: ServerResponse) => void;

/** A local stand-in for the signing service; stand-in values only. */
class MockService {
  readonly seen: Seen[] = [];
  routes = new Map<string, Handler>();
  #server: Server | null = null;
  base = '';

  async start(): Promise<void> {
    this.#server = createServer((req, res) => {
      const chunks: Buffer[] = [];
      req.on('data', (c: Buffer) => chunks.push(c));
      req.on('end', () => {
        const seen = {
          method: req.method ?? '',
          url: req.url ?? '',
          headers: req.headers,
          body: Buffer.concat(chunks),
        };
        this.seen.push(seen);
        const handler = this.routes.get(`${seen.method} ${seen.url}`);
        if (handler) handler(seen, res);
        else res.writeHead(404).end('{"message":"no route"}');
      });
    });
    await new Promise<void>((resolve) => this.#server?.listen(0, '127.0.0.1', resolve));
    const { port } = this.#server.address() as AddressInfo;
    this.base = `http://127.0.0.1:${String(port)}`;
  }

  async stop(): Promise<void> {
    await new Promise<void>((resolve) =>
      this.#server?.close(() => {
        resolve();
      }),
    );
  }
}

function json(res: ServerResponse, status: number, body: unknown): void {
  res.writeHead(status, { 'Content-Type': 'application/json' }).end(JSON.stringify(body));
}

function collectLog(): Logger & { lines: string[] } {
  const lines: string[] = [];
  return {
    lines,
    info: (m) => lines.push(`info: ${m}`),
    warn: (m) => lines.push(`warn: ${m}`),
  };
}

const UID = 'abcdefghijklmnopabcdefghijklmnopabcdefgh';
const CREDS = {
  OW_CLI_EMAIL: 'dev@example.com',
  OW_CLI_API_KEY: 'k-test',
  OW_BUILD_KEY: 'pk-test',
};

let service: MockService;
let dir: string;
let tauriDir: string;

/** The test app's tauri.conf.json `plugins.overwolf` block. */
function writeConfig(overwolf: Record<string, unknown>, extra: Record<string, unknown> = {}): void {
  writeFileSync(
    join(tauriDir, 'tauri.conf.json'),
    `${JSON.stringify(
      { productName: 'Demo App', version: '1.2.3', ...extra, plugins: { overwolf } },
      null,
      2,
    )}\n`,
  );
}

const SIGNING = { enabled: true, owCertSigning: true };

beforeEach(async () => {
  service = new MockService();
  await service.start();
  dir = mkdtempSync(join(tmpdir(), 'ow-tauri-cli-'));
  tauriDir = join(dir, 'src-tauri');
  mkdirSync(join(dir, 'dist'));
  mkdirSync(tauriDir);
  writeFileSync(join(dir, 'dist', 'index.html'), '<!doctype html>\n');
  writeFileSync(
    join(tauriDir, 'Cargo.toml'),
    '[package]\nname = "demo-app-shell"\nversion = "0.1.0"\n',
  );
  writeConfig({ author: 'Example Studio', uid: UID, signing: SIGNING });
  const signedPackage = {
    name: 'demo-app-shell',
    productName: 'Demo App',
    version: '1.2.3',
    overwolf: { uid: UID },
  };
  service.routes.set('POST /sign/electron', (_req, res) => {
    json(res, 200, {
      zip: makeZip(
        {
          'package.json': JSON.stringify(signedPackage),
          '_metadata.json': JSON.stringify({ signature: 'stand-in' }),
        },
        true,
      ).toString('base64'),
      integrityDllUrl: `${service.base}/files/integrity.dll`,
      isOwCertificateEnabled: true,
    });
  });
  service.routes.set('GET /files/integrity.dll', (_req, res) => {
    res.writeHead(200).end(Buffer.from('MZ-stand-in-dll'));
  });
});

afterEach(async () => {
  await service.stop();
  rmSync(dir, { recursive: true, force: true });
});

async function options(extra: Partial<SignOptions> = {}): Promise<SignOptions> {
  return {
    loaded: await loadTauriConfig({ tauriDir, target: 'windows', env: {}, cwd: dir }),
    main: 'dist/index.html',
    cwd: dir,
    platform: 'win32',
    dryRun: false,
    writeUid: false,
    env: { ...CREDS, OW_CLI_API_URL: `${service.base}/` },
    log: collectLog(),
    ...extra,
  };
}

describe('packaged form', () => {
  it('merges extraMetadata and removes build-only keys like the builder', () => {
    const out = packagedForm({
      name: 'a',
      _id: 'x',
      build: { extraMetadata: { main: 'b.js', nested: { y: 2 } }, removePackageKeywords: false },
      scripts: {},
      keywords: ['k'],
      devDependencies: {},
      dependencies: { react: '1' },
      babel: {},
      gitHead: 'g',
      nested: { x: 1 },
    });
    expect(out).toEqual({
      name: 'a',
      main: 'b.js',
      keywords: ['k'],
      dependencies: { react: '1' },
      nested: { x: 1, y: 2 },
    });
    expect(packagedForm({ dependencies: { 'babel-core': '1' }, babel: {} })).toHaveProperty(
      'babel',
    );
    expect(deepAssign({ a: [1] }, { a: [2, 3] })).toEqual({ a: [2, 3] });
    expect(deepAssign({}, { a: { b: 1 } })).toEqual({ a: { b: 1 } });
  });

  it('reads the gating switches as the builder does', () => {
    expect(['1', 'true', 'yes'].map(envOn)).toEqual([true, true, true]);
    expect([undefined, '', '0', 'FALSE'].map(envOn)).toEqual([false, false, false, false]);
    expect(isSigningRequired(undefined, {})).toBe(true);
    expect(isSigningRequired(false, {})).toBe(false);
    expect(isSigningRequired(false, { OW_REQUIRE_SIGNING: '1' })).toBe(true);
    expect(isCertSigningEnabled(true, {})).toBe(true);
    expect(isCertSigningEnabled(undefined, { OW_ENABLE_CERT_SIGNING: '1' })).toBe(true);
    expect(isCertSigningEnabled('yes', {})).toBe(false);
  });
});

describe('zip reader', () => {
  it('reads stored and deflated entries and skips folders', () => {
    for (const deflate of [false, true]) {
      const entries = readZipEntries(
        makeZip({ 'a.txt': 'hello', 'dir/': '', 'b/c.json': '{}' }, deflate),
      );
      expect([...entries.keys()]).toEqual(['a.txt', 'b/c.json']);
      expect(entries.get('a.txt')?.toString()).toBe('hello');
    }
    expect(() => readZipEntries(Buffer.from('not a zip at all, definitely not'))).toThrow(
      /end of central/,
    );
    const zip = makeZip({ 'a.txt': 'hello' });
    zip.writeUInt16LE(12, zip.length - 22 - 46 - 5 + 10);
    expect(() => readZipEntries(zip)).toThrow(/compression method 12/);
  });

  it('picks the signed exe by name, else the only exe', () => {
    expect(extractSignedExe(makeZip({ 'App.exe': 'S' }), 'App.exe').toString()).toBe('S');
    expect(extractSignedExe(makeZip({ 'other.exe': 'S', 'x.txt': '' }), 'App.exe').toString()).toBe(
      'S',
    );
    expect(() => extractSignedExe(makeZip({ 'x.txt': '' }), 'App.exe')).toThrow(/entries: x.txt/);
  });
});

/**
 * The sign cases run the whole signing round (zip, hashes, two fake service
 * calls) several times; on the Windows runners they take longer than
 * vitest's 5 s default.
 */
const SIGN_TIMEOUT_MS = 20_000;

describe('ow-tauri sign', () => {
  it(
    'signs a package.json synthesised from the config and writes signed/',
    async () => {
      const outcome = await sign(await options());
      expect(outcome.status).toBe('signed');
      const out = join(dir, 'signed');
      const signed = JSON.parse(readFileSync(join(out, 'package.json'), 'utf8')) as {
        overwolf: { uid: string };
      };
      expect(signed.overwolf.uid).toBe(UID);
      expect(JSON.parse(readFileSync(join(out, '_metadata.json'), 'utf8'))).toEqual({
        signature: 'stand-in',
      });
      expect(readFileSync(join(out, 'integrity.dll'), 'utf8')).toBe('MZ-stand-in-dll');
      expect(JSON.parse(readFileSync(join(out, 'owe.json'), 'utf8'))).toEqual({ appUid: UID });
      const result = JSON.parse(readFileSync(join(out, 'sign-result.json'), 'utf8')) as Record<
        string,
        unknown
      >;
      expect(result).toEqual({
        uid: UID,
        isOwCertificateEnabled: true,
        enableOWCertSigning: true,
        mainFile: 'dist/index.html',
        mainPath: join(dir, 'dist', 'index.html'),
        appExeNames: ['demo-app-shell.exe', 'Demo App.exe'],
        mainSha256: createHash('sha256').update('<!doctype html>\n').digest('hex'),
        version: '1.2.3',
      });

      const post = service.seen.find((s) => s.url === '/sign/electron');
      expect(post?.headers.authorization).toBe('Key dev@example.com:k-test');
      expect(post?.headers['x-ow-app-key']).toBe('pk-test');
      expect(post?.headers['content-type']).toBe('application/json');
      const body = JSON.parse(post?.body.toString() ?? '{}') as Record<string, unknown>;
      expect(body).toEqual({
        packageJson: {
          name: 'demo-app-shell',
          productName: 'Demo App',
          version: '1.2.3',
          author: 'Example Studio',
          overwolf: { uid: UID },
          main: 'dist/index.html',
        },
        fileHashes: {
          'dist/index.html': createHash('sha256').update('<!doctype html>\n').digest('hex'),
        },
      });
      expect(service.seen.some((s) => s.url.includes('asar'))).toBe(false);
    },
    SIGN_TIMEOUT_MS,
  );

  it(
    'refuses a signed uid that differs from the configured one, or pins it with --write-uid',
    async () => {
      writeConfig({ author: 'Example Studio', name: 'Demo App', signing: SIGNING });
      const computed = computeUid('Example Studio', 'Demo App');
      // Even when signing is not required, a mismatch never builds.
      await expect(sign(await options({ platform: 'linux' }))).rejects.toThrow(
        uidMismatchMessage(UID, computed),
      );
      expect(uidMismatchMessage(UID, computed)).toBe(
        `[OW] the console signed uid ${UID} but plugins.overwolf resolves to ${computed}; set plugins.overwolf.uid to "${UID}" (or run ow-tauri sign --write-uid)`,
      );
      expect(existsSync(join(dir, 'signed'))).toBe(false);
      const log = collectLog();
      expect((await sign(await options({ writeUid: true, log }))).status).toBe('signed');
      const config = JSON.parse(readFileSync(join(tauriDir, 'tauri.conf.json'), 'utf8')) as {
        plugins: { overwolf: Record<string, unknown> };
      };
      expect(config.plugins.overwolf).toEqual({
        author: 'Example Studio',
        name: 'Demo App',
        signing: SIGNING,
        uid: UID,
      });
      expect(log.lines.join('\n')).toContain(`wrote plugins.overwolf.uid "${UID}"`);
      expect(JSON.parse(readFileSync(join(dir, 'signed', 'owe.json'), 'utf8'))).toEqual({
        appUid: UID,
      });
    },
    SIGN_TIMEOUT_MS,
  );

  it('sends no uid when none is configured and accepts a matching signed uid', async () => {
    writeConfig({ author: 'Example Studio', name: 'Demo App', signing: SIGNING });
    const computed = computeUid('Example Studio', 'Demo App');
    service.routes.set('POST /sign/electron', (_req, res) => {
      json(res, 200, {
        zip: makeZip({
          'package.json': JSON.stringify({ name: 'demo-app-shell', overwolf: {} }),
          '_metadata.json': '{}',
        }).toString('base64'),
        integrityDllUrl: `${service.base}/files/integrity.dll`,
      });
    });
    const outcome = await sign(await options());
    expect(outcome.status === 'signed' && outcome.result.uid).toBe(computed);
    const body = JSON.parse(service.seen[0]?.body.toString() ?? '{}') as {
      packageJson: Record<string, unknown>;
    };
    expect(body.packageJson).not.toHaveProperty('overwolf');
  });

  it('does nothing while signing is off, and needs a pinned uid', async () => {
    writeConfig({ author: 'Example Studio', uid: UID });
    const log = collectLog();
    expect((await sign(await options({ log }))).status).toBe('disabled');
    expect(log.lines.join('\n')).toContain('signing.enabled is not true');
    writeConfig({ signing: SIGNING });
    await expect(sign(await options())).rejects.toThrow(
      'set "uid", or both "author" and "name", before a release build',
    );
    expect(service.seen).toHaveLength(0);
  });

  it('gates on credentials like the builder', async () => {
    const env = { OW_CLI_API_URL: service.base };
    await expect(sign(await options({ env }))).rejects.toThrow(/signing required but OW_CLI_EMAIL/);
    const log = collectLog();
    const outcome = await sign(await options({ env, platform: 'darwin', log }));
    expect(outcome.status).toBe('unsigned');
    expect(log.lines.join('\n')).toMatch(/Missing OW_CLI_EMAIL \/ OW_CLI_API_KEY/);
    expect(log.lines.join('\n')).toMatch(/building unsigned/);
    const noKey = { ...CREDS, OW_BUILD_KEY: '', OW_CLI_API_URL: service.base };
    const log2 = collectLog();
    expect((await sign(await options({ env: noKey, platform: 'linux', log: log2 }))).status).toBe(
      'unsigned',
    );
    expect(log2.lines.join('\n')).toMatch(/Missing OW_BUILD_KEY/);
    // requireSigning: false lets a Windows build go on unsigned.
    writeConfig({
      author: 'Example Studio',
      uid: UID,
      signing: { ...SIGNING, requireSigning: false },
    });
    expect((await sign(await options({ env }))).status).toBe('unsigned');
    await expect(
      sign(await options({ env: { ...env, OW_REQUIRE_SIGNING: 'true' } })),
    ).rejects.toThrow();
    expect(service.seen).toHaveLength(0);
  });

  it('reports server errors and bad responses', async () => {
    service.routes.set('POST /sign/electron', (_req, res) => {
      json(res, 403, { message: 'app key mismatch' });
    });
    await expect(sign(await options())).rejects.toThrow(
      '[OW] Signing API call failed: [OW] Signing API returned 403: app key mismatch',
    );
    service.routes.set('POST /sign/electron', (_req, res) => {
      json(res, 200, { zip: 'x' });
    });
    await expect(sign(await options())).rejects.toThrow('missing zip or integrityDllUrl');
    service.routes.set('POST /sign/electron', (_req, res) => {
      json(res, 200, {
        zip: makeZip({ 'package.json': '{}' }).toString('base64'),
        integrityDllUrl: `${service.base}/files/integrity.dll`,
      });
    });
    await expect(sign(await options())).rejects.toThrow('missing package.json or _metadata.json');
    service.routes.set('POST /sign/electron', (_req, res) => {
      json(res, 200, {
        zip: makeZip({ 'package.json': '[]', '_metadata.json': '{}' }).toString('base64'),
        integrityDllUrl: `${service.base}/files/integrity.dll`,
      });
    });
    await expect(sign(await options())).rejects.toThrow('signed package.json is not an object');
    service.routes.set('POST /sign/electron', (_req, res) => {
      json(res, 200, {
        zip: makeZip({ 'package.json': '{}', '_metadata.json': '{}' }).toString('base64'),
        integrityDllUrl: 'http://example.com/integrity.dll',
      });
    });
    await expect(sign(await options())).rejects.toThrow(
      /Failed to download integrity.dll: refusing/,
    );
    expect(existsSync(join(dir, 'signed'))).toBe(false);
  });

  it('hashes --main, else signing.entry from the project folder, else fails', async () => {
    await expect(sign(await options({ main: 'missing.js' }))).rejects.toThrow(
      /cannot load entry file to sign/,
    );
    await expect(sign(await options({ main: undefined, platform: 'linux' }))).rejects.toThrow(
      '[OW] no entry file to sign: pass --main or set plugins.overwolf.signing.entry',
    );
    writeConfig({
      author: 'Example Studio',
      uid: UID,
      signing: { ...SIGNING, entry: 'dist/index.html' },
    });
    // signing.entry is relative to the project folder, whatever the working directory.
    const outcome = await sign(await options({ main: undefined, cwd: tauriDir }));
    expect(outcome.status === 'signed' && outcome.result.mainFile).toBe('dist/index.html');
    // An absolute --main is keyed relative to the project folder.
    const absolute = await sign(await options({ main: join(dir, 'dist', 'index.html') }));
    expect(absolute.status === 'signed' && absolute.result.mainFile).toBe('dist/index.html');
    const out = await sign(await options({ outDir: 'out/signed' }));
    expect(out.status === 'signed' && out.outDir).toBe(join(dir, 'out', 'signed'));
  });

  it('synthesises name from the app name when there is no Cargo package', () => {
    expect(
      signingPackageJson(
        {
          uid: UID,
          cuid: UID,
          name: 'Demo App',
          author: 'unknown',
          version: undefined,
          uidConfigured: false,
          pinned: false,
          nameSource: 'productName',
          warnings: [],
        },
        'index.html',
        undefined,
      ),
    ).toEqual({
      name: 'Demo App',
      productName: 'Demo App',
      version: '',
      author: 'unknown',
      main: 'index.html',
    });
  });

  it('dry run prints the request and sends nothing', async () => {
    const log = collectLog();
    expect((await sign(await options({ dryRun: true, log }))).status).toBe('dry-run');
    const text = log.lines.join('\n');
    expect(text).toContain('POST http://127.0.0.1');
    expect(text).toContain('Key dev@example.com:<OW_CLI_API_KEY>');
    expect(text).toContain('"productName": "Demo App"');
    expect(text).not.toContain('k-test');
    expect(text).not.toContain('pk-test');
    const bare = collectLog();
    expect((await sign(await options({ dryRun: true, env: {}, log: bare }))).status).toBe(
      'dry-run',
    );
    expect(bare.lines.join('\n')).toContain('<missing credentials>');
    expect(service.seen).toHaveLength(0);
  });
});

describe('ow-tauri sign-exe', () => {
  beforeEach(async () => {
    await sign(await options());
    mkdirSync(join(dir, 'target'));
    writeFileSync(join(dir, 'target', 'Demo App.exe'), 'UNSIGNED');
    writeFileSync(join(dir, 'target', 'helper.exe'), 'HELPER');
    service.routes.set('POST /sign/electron-certificate', (_req, res) => {
      json(res, 200, { zip: `${service.base}/files/signed.zip` });
    });
    service.routes.set('GET /files/signed.zip', (_req, res) => {
      res.writeHead(200).end(makeZip({ 'Demo App.exe': 'SIGNED' }, true));
    });
  });

  const exeOptions = (file: string, extra: Partial<Parameters<typeof signExe>[0]> = {}) => ({
    file: join('target', file),
    cwd: dir,
    env: { ...CREDS, OW_CLI_API_URL: service.base },
    log: collectLog(),
    ...extra,
  });

  it('signs the app exe with the Overwolf certificate', async () => {
    expect(await signExe(exeOptions('Demo App.exe'))).toBe('overwolf');
    expect(readFileSync(join(dir, 'target', 'Demo App.exe'), 'utf8')).toBe('SIGNED');
    const post = service.seen.find((s) => s.url === '/sign/electron-certificate');
    expect(post?.headers['content-type']).toMatch(
      /^multipart\/form-data; boundary=----owBuilderBoundary/,
    );
    expect(post?.headers.authorization).toBe('Key dev@example.com:k-test');
    const body = post?.body.toString() ?? '';
    expect(body).toContain('Content-Disposition: form-data; name="file"; filename="Demo App.exe"');
    expect(body).toContain('UNSIGNED');
  });

  it('keeps an already signed exe, skips or delegates other files', async () => {
    service.routes.set('POST /sign/electron-certificate', (_req, res) => {
      json(res, 200, { isAlreadySigned: true });
    });
    expect(await signExe(exeOptions('Demo App.exe'))).toBe('already-signed');
    expect(readFileSync(join(dir, 'target', 'Demo App.exe'), 'utf8')).toBe('UNSIGNED');
    expect(await signExe(exeOptions('helper.exe'))).toBe('skipped');
    const fallback = `"${process.execPath}" -e "require('fs').writeFileSync(process.argv[1], 'FALLBACK')" %1`;
    expect(await signExe(exeOptions('helper.exe', { fallback }))).toBe('fallback');
    expect(readFileSync(join(dir, 'target', 'helper.exe'), 'utf8')).toBe('FALLBACK');
    await expect(
      signExe(exeOptions('helper.exe', { fallback: `"${process.execPath}" -e "process.exit(3)"` })),
    ).rejects.toThrow(/exited with 3/);
    await expect(signExe(exeOptions('helper.exe', { fallback: '  ' }))).rejects.toThrow(/empty/);
  });

  it('only signs when the service enabled it and the app asked', async () => {
    const result = join(dir, 'signed', 'sign-result.json');
    const data = JSON.parse(readFileSync(result, 'utf8')) as Record<string, unknown>;
    writeFileSync(result, JSON.stringify({ ...data, isOwCertificateEnabled: false }));
    expect(await signExe(exeOptions('Demo App.exe'))).toBe('skipped');
    expect(service.seen.some((s) => s.url === '/sign/electron-certificate')).toBe(false);
    // Without credentials the enabled path fails the build.
    writeFileSync(result, JSON.stringify(data));
    await expect(signExe(exeOptions('Demo App.exe', { env: {} }))).rejects.toThrow(
      /certificate signing required/,
    );
  });

  it('runs from src-tauri (where Tauri runs signCommand) and finds ../signed', async () => {
    writeFileSync(
      join(tauriDir, 'Cargo.toml'),
      '[package]\nname = "shell-app"\n\n[[bin]]\nname = "shell-main" # the app\npath = "src/main.rs"\n',
    );
    mkdirSync(join(tauriDir, 'target', 'release'), { recursive: true });
    writeFileSync(join(tauriDir, 'target', 'release', 'shell-main.exe'), 'UNSIGNED');
    writeFileSync(join(tauriDir, 'target', 'release', 'other.exe'), 'UNSIGNED');
    service.routes.set('GET /files/signed.zip', (_req, res) => {
      res.writeHead(200).end(makeZip({ 'shell-main.exe': 'SIGNED' }, true));
    });
    const runIn = (file: string, log = collectLog()) =>
      signExe({ ...exeOptions(file), file: join('target', 'release', file), cwd: tauriDir, log });
    expect(await runIn('shell-main.exe')).toBe('overwolf');
    expect(readFileSync(join(tauriDir, 'target', 'release', 'shell-main.exe'), 'utf8')).toBe(
      'SIGNED',
    );
    // Another exe where the main binary sits: skipped, with a warning.
    const log = collectLog();
    expect(await runIn('other.exe', log)).toBe('skipped');
    expect(log.lines.join('\n')).toMatch(/other\.exe is not the app exe \(shell-main\.exe/);
    // mainBinaryName wins.
    writeFileSync(
      join(tauriDir, 'tauri.windows.conf.json'),
      JSON.stringify({ mainBinaryName: 'Renamed' }),
    );
    expect(await tauriAppExeNames(tauriDir)).toEqual(['Renamed.exe']);
    // An explicit signed dir and app exe.
    expect(
      await signExe({
        ...exeOptions('helper.exe'),
        signedDir: join(dir, 'signed'),
        appExe: 'helper.exe',
      }),
    ).toBe('overwolf');
  });

  it('reads the Cargo binary name', () => {
    expect(cargoBinaryName('[package]\nname = "a"\n')).toBe('a');
    expect(cargoBinaryName('[package]\nname = "a"\ndefault-run = "b"\n[[bin]]\nname = "c"\n')).toBe(
      'b',
    );
    expect(
      cargoBinaryName('[[bin]]\nname = "c"\n[[bin]]\nname = "d"\n[package]\nname = "a"\n'),
    ).toBe('c');
    expect(cargoBinaryName('[dependencies]\nname = "x"\n')).toBeUndefined();
  });

  it('reports a bad certificate response', async () => {
    service.routes.set('POST /sign/electron-certificate', (_req, res) => {
      res.writeHead(200).end('not json');
    });
    await expect(signExe(exeOptions('Demo App.exe'))).rejects.toThrow(/non-JSON response/);
    service.routes.set('POST /sign/electron-certificate', (_req, res) => {
      json(res, 500, { message: 'down' });
    });
    await expect(signExe(exeOptions('Demo App.exe'))).rejects.toThrow(
      '[OW] Certificate signing API returned 500: down',
    );
    service.routes.set('POST /sign/electron-certificate', (_req, res) => {
      json(res, 200, {});
    });
    await expect(signExe(exeOptions('Demo App.exe'))).rejects.toThrow(/missing zip url/);
    service.routes.set('POST /sign/electron-certificate', (_req, res) => {
      json(res, 200, { zip: `${service.base}/files/missing.zip` });
    });
    await expect(signExe(exeOptions('Demo App.exe'))).rejects.toThrow(
      /failed to download signed executable/,
    );
  });
});

describe('command line', () => {
  function io(
    extra: Partial<CliIo> = {},
  ): CliIo & { text: string[]; log: Logger & { lines: string[] } } {
    const text: string[] = [];
    const log = collectLog();
    return {
      env: { ...CREDS, OW_CLI_API_URL: service.base },
      cwd: dir,
      platform: 'linux',
      out: (t) => text.push(t),
      ...extra,
      text,
      log,
    };
  }

  it('parses arguments, keeping every value of a repeated option', () => {
    expect(
      parseArgs(['sign', '--out', 'x', '--dry-run', '--main=a.js', '--config', '{}', '--config=b']),
    ).toEqual({
      positional: ['sign'],
      options: new Map<string, string[] | true>([
        ['out', ['x']],
        ['dry-run', true],
        ['main', ['a.js']],
        ['config', ['{}', 'b']],
      ]),
    });
    expect(() => parseArgs(['sign', '--out'])).toThrow(/needs a value/);
    expect(splitCommand('"a b" c  "" d')).toEqual(['a b', 'c', '', 'd']);
  });

  it('runs sign and sign-exe and reports errors with exit code 1', async () => {
    expect(await run(['sign', '--main', 'dist/index.html', '--platform', 'win32'], io())).toBe(0);
    expect(existsSync(join(dir, 'signed', 'owe.json'))).toBe(true);
    writeFileSync(join(dir, 'x.dll'), 'x');
    expect(await run(['sign-exe', 'x.dll'], io())).toBe(0);
    const b = io();
    expect(await run([], b)).toBe(1);
    expect(b.text.join('')).toBe(USAGE);
    expect(await run(['help'], io())).toBe(0);
    expect(await run(['doctor', '--help'], io())).toBe(0);
    const c = io();
    expect(await run(['sign', '--bogus', '1'], c)).toBe(1);
    expect(c.log.lines.join('\n')).toContain('unknown option --bogus');
    expect(await run(['nope'], io())).toBe(1);
    expect(await run(['sign-exe'], io())).toBe(1);
    expect(await run(['migrate'], io())).toBe(1);
    expect(
      await run(['sign', '--main', 'dist/index.html', '--platform', 'win32'], io({ env: {} })),
    ).toBe(1);
  });

  it('applies --config and TAURI_CONFIG to sign like tauri build does', async () => {
    const d = io();
    const off = JSON.stringify({ plugins: { overwolf: { signing: { enabled: false } } } });
    expect(await run(['sign', '--main', 'dist/index.html', '--config', off], d)).toBe(0);
    expect(d.log.lines.join('\n')).toContain('nothing to sign');
    const e = io({ env: { ...io().env, TAURI_CONFIG: off } });
    expect(await run(['sign', '--main', 'dist/index.html'], e)).toBe(0);
    expect(e.log.lines.join('\n')).toContain('nothing to sign');
    expect(existsSync(join(dir, 'signed'))).toBe(false);
  });

  it('runs init, migrate and doctor', async () => {
    writeConfig({});
    expect(await run(['init', '--author', 'Example Studio'], io())).toBe(0);
    const doctorIo = io();
    expect(await run(['doctor'], doctorIo)).toBe(0);
    expect(doctorIo.text.join('')).toContain(`uid ${computeUid('Example Studio', 'Demo App')}`);
    writeFileSync(join(dir, 'package.json'), JSON.stringify({ name: 'demo', author: 'A' }));
    const m = io();
    expect(await run(['migrate', '--from', 'package.json'], m)).toBe(0);
    expect(m.text.join('')).toContain('"author": "A"');
    writeConfig({ uid: 'bad uid!' });
    const broken = io();
    expect(await run(['doctor', '--tauri-dir', 'src-tauri'], broken)).toBe(1);
    expect(broken.text.join('')).toContain('plugins.overwolf.uid: must be 1 to 64');
  });
});
