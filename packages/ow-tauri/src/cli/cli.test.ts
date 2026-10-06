// @vitest-environment node
/// <reference types="node" />
import { createHash } from 'node:crypto';
import {
  mkdtempSync,
  readFileSync,
  writeFileSync,
  mkdirSync,
  existsSync,
  statSync,
  utimesSync,
} from 'node:fs';
import { createServer } from 'node:http';
import type { IncomingMessage, Server, ServerResponse } from 'node:http';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { deflateRawSync } from 'node:zlib';

import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { cargoBinaryName, tauriAppExeNames } from './app-exe.js';
import { deepAssign, envOn, isSigningRequired, packagedForm } from './package-json.js';
import { USAGE, parseArgs, run } from './run.js';
import type { CliIo } from './run.js';
import { sign } from './sign.js';
import type { Logger } from './sign.js';
import { extractSignedExe, signExe, splitCommand } from './sign-exe.js';
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

beforeEach(async () => {
  service = new MockService();
  await service.start();
  dir = mkdtempSync(join(tmpdir(), 'ow-tauri-cli-'));
  mkdirSync(join(dir, 'dist'));
  writeFileSync(join(dir, 'dist', 'main.js'), 'console.log(1);\n');
  writeFileSync(
    join(dir, 'package.json'),
    JSON.stringify({
      name: 'demo-app',
      productName: 'Demo App',
      version: '1.2.3',
      author: 'Example Studio',
      main: 'dist/main.js',
      scripts: { build: 'x' },
      keywords: ['a'],
      devDependencies: { x: '1' },
      _private: true,
      build: {
        overwolf: { enableOWCertSigning: true },
        extraMetadata: { description: 'from extraMetadata', overwolf: { packages: ['gep'] } },
      },
      overwolf: { packages: [] },
    }),
  );
  const signedPackage = {
    name: 'demo-app',
    productName: 'Demo App',
    version: '1.2.3',
    overwolf: { packages: ['gep'], uid: UID },
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
});

function options(extra: Partial<Parameters<typeof sign>[0]> = {}): Parameters<typeof sign>[0] {
  return {
    packageJson: join(dir, 'package.json'),
    platform: 'win32',
    dryRun: false,
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
  });

  it('reads the gating switches as the builder does', () => {
    expect(['1', 'true', 'yes'].map(envOn)).toEqual([true, true, true]);
    expect([undefined, '', '0', 'FALSE'].map(envOn)).toEqual([false, false, false, false]);
    expect(isSigningRequired(undefined, {})).toBe(true);
    expect(isSigningRequired(false, {})).toBe(false);
    expect(isSigningRequired(false, { OW_REQUIRE_SIGNING: '1' })).toBe(true);
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

describe('ow-tauri sign', () => {
  it('signs, writes the artefacts and never asks for an asar token', async () => {
    const log = collectLog();
    const outcome = await sign(options({ log }));
    expect(outcome.status).toBe('signed');
    const out = join(dir, 'ow-tauri-signed');
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
    expect(result).toMatchObject({
      uid: UID,
      isOwCertificateEnabled: true,
      enableOWCertSigning: true,
      mainFile: 'dist/main.js',
      mainPath: join(dir, 'dist/main.js'),
      appExeNames: [],
      version: '1.2.3',
    });

    const post = service.seen.find((s) => s.url === '/sign/electron');
    expect(post?.headers.authorization).toBe('Key dev@example.com:k-test');
    expect(post?.headers['x-ow-app-key']).toBe('pk-test');
    expect(post?.headers['content-type']).toBe('application/json');
    const body = JSON.parse(post?.body.toString() ?? '{}') as {
      packageJson: Record<string, unknown>;
      fileHashes: Record<string, string>;
    };
    expect(Object.keys(body).sort()).toEqual(['fileHashes', 'packageJson']);
    expect(body.packageJson).not.toHaveProperty('build');
    expect(body.packageJson).not.toHaveProperty('scripts');
    expect(body.packageJson).not.toHaveProperty('devDependencies');
    expect(body.packageJson).not.toHaveProperty('_private');
    expect(body.packageJson).not.toHaveProperty('electronVersion');
    expect(body.packageJson['description']).toBe('from extraMetadata');
    expect(body.fileHashes).toEqual({
      'dist/main.js': createHash('sha256').update('console.log(1);\n').digest('hex'),
    });
    expect(service.seen.some((s) => s.url.includes('asar'))).toBe(false);
  });

  it('touches package.json and records the Tauri app exe names', async () => {
    const pkgPath = join(dir, 'package.json');
    const past = new Date('2020-01-01T00:00:00Z');
    utimesSync(pkgPath, past, past);
    mkdirSync(join(dir, 'src-tauri'));
    writeFileSync(
      join(dir, 'src-tauri', 'tauri.conf.json'),
      JSON.stringify({ productName: 'Demo Studio App' }),
    );
    writeFileSync(
      join(dir, 'src-tauri', 'Cargo.toml'),
      '[package]\nname = "demo-app-shell"\nversion = "0.1.0"\n',
    );
    expect((await sign(options({ projectDir: dir }))).status).toBe('signed');
    expect(statSync(pkgPath).mtimeMs).toBeGreaterThan(past.getTime());
    const result = JSON.parse(
      readFileSync(join(dir, 'ow-tauri-signed', 'sign-result.json'), 'utf8'),
    ) as { appExeNames: string[] };
    expect(result.appExeNames).toEqual(['demo-app-shell.exe', 'Demo Studio App.exe']);
    // Not touched when nothing was signed.
    utimesSync(pkgPath, past, past);
    await sign(options({ dryRun: true }));
    expect(statSync(pkgPath).mtimeMs).toBe(past.getTime());
  });

  it('gates on credentials like the builder', async () => {
    const env = { OW_CLI_API_URL: service.base };
    await expect(sign(options({ env }))).rejects.toThrow(/signing required but OW_CLI_EMAIL/);
    const log = collectLog();
    const outcome = await sign(options({ env, platform: 'darwin', log }));
    expect(outcome.status).toBe('unsigned');
    expect(log.lines.join('\n')).toMatch(/Missing OW_CLI_EMAIL \/ OW_CLI_API_KEY/);
    expect(log.lines.join('\n')).toMatch(/building unsigned/);
    const noKey = { ...CREDS, OW_BUILD_KEY: '', OW_CLI_API_URL: service.base };
    const log2 = collectLog();
    expect((await sign(options({ env: noKey, platform: 'linux', log: log2 }))).status).toBe(
      'unsigned',
    );
    expect(log2.lines.join('\n')).toMatch(/Missing OW_BUILD_KEY/);
    // requireSigning: false lets a Windows build go on unsigned.
    const pkgPath = join(dir, 'package.json');
    const pkg = JSON.parse(readFileSync(pkgPath, 'utf8')) as { build: { overwolf: object } };
    pkg.build.overwolf = { requireSigning: false };
    writeFileSync(pkgPath, JSON.stringify(pkg));
    expect((await sign(options({ env }))).status).toBe('unsigned');
    await expect(sign(options({ env: { ...env, OW_REQUIRE_SIGNING: 'true' } }))).rejects.toThrow();
    expect(service.seen).toHaveLength(0);
  });

  it('reports server errors and bad responses', async () => {
    service.routes.set('POST /sign/electron', (_req, res) => {
      json(res, 403, { message: 'app key mismatch' });
    });
    await expect(sign(options())).rejects.toThrow(
      '[OW] Signing API call failed: [OW] Signing API returned 403: app key mismatch',
    );
    service.routes.set('POST /sign/electron', (_req, res) => {
      json(res, 200, { zip: 'x' });
    });
    await expect(sign(options())).rejects.toThrow('missing zip or integrityDllUrl');
    service.routes.set('POST /sign/electron', (_req, res) => {
      json(res, 200, {
        zip: makeZip({ 'package.json': '{}' }).toString('base64'),
        integrityDllUrl: `${service.base}/files/integrity.dll`,
      });
    });
    await expect(sign(options())).rejects.toThrow('missing package.json or _metadata.json');
    service.routes.set('POST /sign/electron', (_req, res) => {
      json(res, 200, {
        zip: makeZip({ 'package.json': '{}', '_metadata.json': '{}' }).toString('base64'),
        integrityDllUrl: 'http://example.com/integrity.dll',
      });
    });
    await expect(sign(options())).rejects.toThrow(/Failed to download integrity.dll: refusing/);
    expect(existsSync(join(dir, 'ow-tauri-signed'))).toBe(false);
  });

  it('needs an entry file to hash', async () => {
    await expect(sign(options({ main: 'missing.js' }))).rejects.toThrow(
      /cannot load entry file to sign/,
    );
    const pkgPath = join(dir, 'package.json');
    const pkg = JSON.parse(readFileSync(pkgPath, 'utf8')) as Record<string, unknown>;
    delete pkg['main'];
    writeFileSync(pkgPath, JSON.stringify(pkg));
    await expect(sign(options())).rejects.toThrow(/Missing main field/);
    expect((await sign(options({ main: 'dist/main.js' }))).status).toBe('signed');
  });

  it('dry run prints the request and sends nothing', async () => {
    const log = collectLog();
    expect((await sign(options({ dryRun: true, log }))).status).toBe('dry-run');
    const text = log.lines.join('\n');
    expect(text).toContain('POST http://127.0.0.1');
    expect(text).toContain('Key dev@example.com:<OW_CLI_API_KEY>');
    expect(text).not.toContain('k-test');
    expect(text).not.toContain('pk-test');
    expect(service.seen).toHaveLength(0);
  });
});

describe('ow-tauri sign-exe', () => {
  beforeEach(async () => {
    await sign(options());
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
  });

  it('only signs when the service enabled it and the app asked', async () => {
    const result = join(dir, 'ow-tauri-signed', 'sign-result.json');
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

  it('finds the app exe from tauri.conf.json when its name differs from package.json', async () => {
    // As in the packages sample: package.json has only `name`, tauri.conf
    // a different productName, and Cargo names the binary.
    const src = join(dir, 'src-tauri');
    mkdirSync(src);
    writeFileSync(join(src, 'tauri.conf.json'), JSON.stringify({ productName: 'Shell App' }));
    writeFileSync(
      join(src, 'Cargo.toml'),
      '[package]\nname = "shell-app"\n\n[[bin]]\nname = "shell-main" # the app\npath = "src/main.rs"\n',
    );
    mkdirSync(join(src, 'target', 'release'), { recursive: true });
    writeFileSync(join(src, 'target', 'release', 'shell-main.exe'), 'UNSIGNED');
    writeFileSync(join(src, 'target', 'release', 'other.exe'), 'UNSIGNED');
    service.routes.set('GET /files/signed.zip', (_req, res) => {
      res.writeHead(200).end(makeZip({ 'shell-main.exe': 'SIGNED' }, true));
    });
    // Tauri runs signCommand in src-tauri.
    const run = (file: string) =>
      signExe({ ...exeOptions(file), file: join('target', 'release', file), cwd: src });
    expect(await run('shell-main.exe')).toBe('overwolf');
    expect(readFileSync(join(src, 'target', 'release', 'shell-main.exe'), 'utf8')).toBe('SIGNED');
    // Another exe where the main binary sits: skipped, with a warning.
    const log = collectLog();
    const outcome = await signExe({
      ...exeOptions('other.exe'),
      file: join('target', 'release', 'other.exe'),
      cwd: src,
      log,
    });
    expect(outcome).toBe('skipped');
    expect(log.lines.join('\n')).toMatch(/other\.exe is not the app exe \(shell-main\.exe/);
    // mainBinaryName wins.
    writeFileSync(
      join(src, 'tauri.windows.conf.json'),
      JSON.stringify({ mainBinaryName: 'Renamed' }),
    );
    expect(await tauriAppExeNames(src)).toEqual(['Renamed.exe']);
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

  it('parses arguments', () => {
    expect(parseArgs(['sign', '--out', 'x', '--dry-run', '--main=a.js'])).toEqual({
      positional: ['sign'],
      options: new Map<string, string | true>([
        ['out', 'x'],
        ['dry-run', true],
        ['main', 'a.js'],
      ]),
    });
    expect(() => parseArgs(['sign', '--out'])).toThrow(/needs a value/);
    expect(splitCommand('"a b" c  "" d')).toEqual(['a b', 'c', '', 'd']);
  });

  it('runs sign and sign-exe and reports errors with exit code 1', async () => {
    const a = io();
    expect(await run(['sign'], a)).toBe(0);
    expect(existsSync(join(dir, 'ow-tauri-signed', 'owe.json'))).toBe(true);
    writeFileSync(join(dir, 'x.dll'), 'x');
    expect(await run(['sign-exe', 'x.dll'], io())).toBe(0);
    const b = io();
    expect(await run([], b)).toBe(1);
    expect(b.text.join('')).toBe(USAGE);
    expect(await run(['help'], io())).toBe(0);
    const c = io();
    expect(await run(['sign', '--bogus', '1'], c)).toBe(1);
    expect(c.log.lines.join('\n')).toContain('unknown option --bogus');
    expect(await run(['nope'], io())).toBe(1);
    expect(await run(['sign-exe'], io())).toBe(1);
    expect(await run(['sign', '--platform', 'win32'], io({ env: {} }))).toBe(1);
  });
});
