// @vitest-environment node
/// <reference types="node" />
// CB-1: ow-tauri's typings (CONTRACT B.4) next to an installed
// `@overwolf/ow-electron`, as a project that builds both hosts during a
// migration has it. `@overwolf/ow-electron-packages-types` starts with
// `import '@overwolf/ow-electron'`; when that resolves to the installed
// package, ow-electron's `electron.d.ts` (its own `declare module 'electron'`
// and global `Electron` namespace) joins the program. The documented
// `paths` entry for `@overwolf/ow-electron` keeps it out; ow-tauri's runtime
// entry points declare no globals, so an ow-electron build that imports them
// still compiles against ow-electron's types.
//
// The fixture installs ow-electron's typings into a temporary node_modules:
// the real 42.11.4 declarations when the parity harness has them installed
// (tools/parity-harness), else a stand-in with the same layout (`declare
// namespace Electron`, `declare module 'electron' { export = ... }`, and the
// global `Electron.App` / `overwolf` augmentation).
import { cpSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

import ts from 'typescript';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

const pkg = join(dirname(fileURLToPath(import.meta.url)), '..');
const repo = join(pkg, '..', '..');
const realOwElectron = join(
  repo,
  'tools',
  'parity-harness',
  'node_modules',
  '@overwolf',
  'ow-electron',
);
// OW_TAURI_COEXIST_STUB=1 forces the stand-in (what CI without the harness install runs).
const useReal =
  process.env['OW_TAURI_COEXIST_STUB'] !== '1' &&
  existsSync(join(realOwElectron, 'ow-electron-types.d.ts'));

/** Stand-in for ow-electron's typings, with the same declaration layout. */
const STUB: Record<string, string> = {
  'package.json': JSON.stringify({
    name: '@overwolf/ow-electron',
    version: '0.0.0-stub',
    types: 'ow-electron.d.ts',
  }),
  'ow-electron.d.ts': [
    '/// <reference path="./electron.d.ts" />',
    '/// <reference path="./ow-electron-types.d.ts" />',
    '',
  ].join('\n'),
  'electron.d.ts': `
declare namespace Electron {
  class EventEmitter { on(event: string, listener: (...args: unknown[]) => void): this; }
  interface App extends EventEmitter {
    getPath(name: string): string;
    readonly name: string;
    whenReady(): Promise<void>;
  }
  interface BrowserWindowConstructorOptions { width?: number; height?: number; show?: boolean }
  class BrowserWindow extends EventEmitter {
    constructor(options?: BrowserWindowConstructorOptions);
    readonly id: number;
    readonly webContents: WebContents;
  }
  interface WebContents { send(channel: string, ...args: unknown[]): void }
  interface Display { id: number; workArea: Rectangle }
  interface Rectangle { x: number; y: number; width: number; height: number }
  interface Size { width: number; height: number }
  namespace CrossProcessExports {
    const app: App;
    type App = Electron.App;
    const BrowserWindow: typeof Electron.BrowserWindow;
    type BrowserWindow = Electron.BrowserWindow;
    type BrowserWindowConstructorOptions = Electron.BrowserWindowConstructorOptions;
    type WebContents = Electron.WebContents;
    type Display = Electron.Display;
    type Rectangle = Electron.Rectangle;
    type Size = Electron.Size;
  }
}
declare module 'electron' {
  export = Electron.CrossProcessExports;
}
`,
  'ow-electron-types.d.ts': `
import { App } from 'electron';
declare global {
  namespace Electron {
    interface App { overwolf: overwolf.OverwolfApi }
  }
  namespace overwolf {
    interface OverwolfApp extends App { overwolf: OverwolfApi }
    interface OverwolfApi { disableAnonymousAnalytics(): void; packages: packages.OverwolfPackageManager }
    interface EmailHashes { sha256?: string }
    namespace packages {
      type PackageName = 'gep' | 'overlay' | 'recorder' | 'utility' | 'crn';
      interface OverwolfPackageManager { readonly gep: unknown }
    }
  }
}
`,
};

/** A port (Tauri build): `electron` and the global names are ow-tauri's. */
const TAURI_CONSUMER = `/// <reference path="${posix(join(pkg, 'src', 'types', 'index.d.ts'))}" />
import type { OverwolfGameEventPackage } from '@overwolf/ow-electron-packages-types';
import electron, { BrowserWindow, app } from 'electron';

export function port(): void {
  const gep: OverwolfGameEventPackage | undefined = app.overwolf.packages.gep;
  const legacy = app as overwolf.OverwolfApp;
  const dir: string = legacy.getPath('userData');
  const api: overwolf.OverwolfApi = electron.app.overwolf;
  const win: Electron.BrowserWindow = new BrowserWindow({ show: false });
  const name: overwolf.packages.PackageName = 'gep';
  const ad: overwolf.AdviewTag = document.createElement('owadview');
  win.setVibrancy('sidebar');
  ad.customTracking = '{}';
  void [gep, dir, api, win.id, name];
}
`;

/** An ow-electron build of the same project that shares code using ow-tauri's runtime entries. */
const ELECTRON_CONSUMER = `import type { OverwolfGameEventPackage } from '@overwolf/ow-electron-packages-types';
import { BrowserWindow, app } from 'electron';
import type * as tauriMain from 'ow-tauri/main';
import type * as tauriRenderer from 'ow-tauri/renderer';

export function electronBuild(main: typeof tauriMain, renderer: typeof tauriRenderer): void {
  const gep = app.overwolf.packages.gep as OverwolfGameEventPackage | undefined;
  const legacy = app as overwolf.OverwolfApp;
  const dir: string = legacy.getPath('userData');
  const api: overwolf.OverwolfApi = app.overwolf;
  const win: Electron.BrowserWindow = new BrowserWindow({ show: false });
  const uid: string = main.overwolf.uid;
  void [gep, dir, api, win.id, uid, renderer.ipcRenderer];
}
`;

function posix(path: string): string {
  return path.split(sep).join('/');
}

let dir = '';

beforeAll(() => {
  dir = mkdtempSync(join(tmpdir(), 'ow-tauri-coexist-'));
  const owElectron = join(dir, 'node_modules', '@overwolf', 'ow-electron');
  mkdirSync(owElectron, { recursive: true });
  if (useReal) {
    for (const name of [
      'package.json',
      'ow-electron.d.ts',
      'electron.d.ts',
      'ow-electron-types.d.ts',
    ]) {
      cpSync(join(realOwElectron, name), join(owElectron, name));
    }
  } else {
    for (const [name, text] of Object.entries(STUB)) writeFileSync(join(owElectron, name), text);
  }
  // A copy, not a link: its `import '@overwolf/ow-electron'` must resolve
  // from inside this node_modules, as in an app that installed both.
  const typesPkg = [pkg, repo]
    .map((root) => join(root, 'node_modules', '@overwolf', 'ow-electron-packages-types'))
    .find((path) => existsSync(join(path, 'types.d.ts')));
  if (typesPkg === undefined)
    throw new Error('@overwolf/ow-electron-packages-types is not installed');
  cpSync(typesPkg, join(dir, 'node_modules', '@overwolf', 'ow-electron-packages-types'), {
    recursive: true,
    filter: (from) =>
      !from.includes(
        `${sep}node_modules${sep}@overwolf${sep}ow-electron-packages-types${sep}node_modules`,
      ),
  });
  writeFileSync(
    join(dir, 'package.json'),
    JSON.stringify({ name: 'coexist-fixture', private: true, type: 'module' }),
  );
  writeFileSync(join(dir, 'tauri.ts'), TAURI_CONSUMER);
  writeFileSync(join(dir, 'electron.ts'), ELECTRON_CONSUMER);
});

afterAll(() => {
  if (dir) rmSync(dir, { recursive: true, force: true });
});

interface Result {
  errors: string[];
  files: string[];
}

function compile(entry: string, paths: Record<string, string[]>, skipLibCheck: boolean): Result {
  const base = ts.readConfigFile(join(pkg, 'tsconfig.json'), (path) => ts.sys.readFile(path));
  const parsed = ts.parseJsonConfigFileContent(base.config, ts.sys, pkg);
  const options: ts.CompilerOptions = {
    ...parsed.options,
    noEmit: true,
    declaration: false,
    declarationMap: false,
    sourceMap: false,
    skipLibCheck,
    types: ['node'],
    typeRoots: [join(pkg, 'node_modules', '@types'), join(repo, 'node_modules', '@types')],
    paths,
  };
  delete options.rootDir;
  delete options.outDir;
  const program = ts.createProgram([join(dir, entry)], options);
  const host: ts.FormatDiagnosticsHost = {
    getCanonicalFileName: (name) => name,
    getCurrentDirectory: () => dir,
    getNewLine: () => '\n',
  };
  const errors = ts
    .getPreEmitDiagnostics(program)
    // The upstream packages-types declaration has one implicit-any member
    // (`registerGames`) that only a full lib check reports; not ours.
    .filter((d) => !d.file?.fileName.includes('ow-electron-packages-types'))
    .map((d) => ts.formatDiagnostic(d, host).trim());
  const files = program.getSourceFiles().map((f) => posix(f.fileName));
  return { errors, files };
}

// A full program with Electron's typings takes a few seconds on a slow machine.
const TIMEOUT_MS = 60_000;

const src = (rel: string): string[] => [join(pkg, 'src', rel)];

const DOCUMENTED_PATHS = {
  electron: src('types/electron.d.ts'),
  '@overwolf/ow-electron': src('types/ow-electron.d.ts'),
  'ow-tauri/electron': src('electron/index.ts'),
};

describe(`ow-tauri typings with @overwolf/ow-electron installed (${useReal ? 'real 42.x typings' : 'stand-in'})`, () => {
  it(
    'a port with the documented types + paths compiles, without ow-electron typings in the program',
    () => {
      const { errors, files } = compile('tauri.ts', DOCUMENTED_PATHS, false);
      expect(errors).toEqual([]);
      expect(files.filter((f) => f.includes('/node_modules/@overwolf/ow-electron/'))).toEqual([]);
      expect(files.some((f) => f.endsWith('/src/types/ow-electron.d.ts'))).toBe(true);
    },
    TIMEOUT_MS,
  );

  it(
    'without the @overwolf/ow-electron path, the installed typings join through packages-types',
    () => {
      const { electron, 'ow-tauri/electron': facade } = DOCUMENTED_PATHS;
      const { files } = compile('tauri.ts', { electron, 'ow-tauri/electron': facade }, true);
      // This is why the documented tsconfig keeps that path: two
      // `declare module 'electron'` would merge into one inconsistent module.
      expect(
        files.some((f) => f.endsWith('/node_modules/@overwolf/ow-electron/electron.d.ts')),
      ).toBe(true);
    },
    TIMEOUT_MS,
  );

  it(
    'an ow-electron build importing ow-tauri/main and ow-tauri/renderer compiles against ow-electron types',
    () => {
      const { errors, files } = compile(
        'electron.ts',
        { 'ow-tauri/main': src('main/index.ts'), 'ow-tauri/renderer': src('renderer/index.ts') },
        true,
      );
      expect(errors).toEqual([]);
      // Nothing from ow-tauri's B.4 declarations leaks into that program.
      expect(files.filter((f) => f.includes('/src/types/'))).toEqual([]);
      expect(
        files.some((f) => f.endsWith('/node_modules/@overwolf/ow-electron/electron.d.ts')),
      ).toBe(true);
    },
    TIMEOUT_MS,
  );
});
