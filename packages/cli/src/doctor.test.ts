// @vitest-environment node
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { WEBVIEW2_MINIMUM, coversOverwolf, doctor, formatFindings, minorOf } from './doctor.js';
import type { Finding } from './doctor.js';
import { computeUid, loadTauriConfig } from './tauri-config.js';

let dir: string;
let tauriDir: string;

function write(path: string, content: unknown): void {
  writeFileSync(path, typeof content === 'string' ? content : JSON.stringify(content));
}

async function run(): Promise<Finding[]> {
  return await doctor(await loadTauriConfig({ tauriDir, target: 'windows', env: {}, cwd: dir }));
}

function messages(findings: Finding[], level: Finding['level']): string[] {
  return findings.filter((f) => f.level === level).map((f) => f.message);
}

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'ow-tauri-doctor-'));
  tauriDir = join(dir, 'src-tauri');
  mkdirSync(join(tauriDir, 'src', 'commands'), { recursive: true });
  mkdirSync(join(tauriDir, 'capabilities'));
  write(join(tauriDir, 'tauri.conf.json'), {
    productName: 'My Game App',
    version: '1.0.0',
    plugins: { overwolf: { author: 'Example Studio', name: 'My Game App', ads: { testAd: true } } },
  });
  write(
    join(tauriDir, 'Cargo.toml'),
    '[package]\nname = "my-game-app"\nversion = "0.1.0"\n\n[dependencies]\ntauri = { version = "2.12.1", features = [] }\ntauri-plugin-overwolf = "1"\n',
  );
  write(join(tauriDir, 'capabilities', 'default.json'), {
    identifier: 'default',
    webviews: ['main'],
    permissions: ['core:default', 'overwolf:default'],
  });
  write(
    join(tauriDir, 'src', 'lib.rs'),
    'pub fn run() {\n    builder.on_web_content_process_terminate(hook);\n}\n',
  );
  write(join(dir, 'package.json'), { dependencies: { '@tauri-apps/api': '^2.12.0' } });
});

afterEach(() => {
  rmSync(dir, { recursive: true, force: true });
});

describe('ow-tauri doctor', () => {
  it('reports the identity and a healthy setup without warnings', async () => {
    const findings = await run();
    expect(messages(findings, 'error')).toEqual([]);
    expect(messages(findings, 'warn')).toEqual([]);
    expect(messages(findings, 'info')).toEqual([
      `uid ${computeUid('Example Studio', 'My Game App')} (computed from author and name)`,
      `cuid ${computeUid('Example Studio', 'My Game App')}`,
      'name "My Game App" (from plugins.overwolf.name)',
      'author "Example Studio"',
      'version 1.0.0',
      'test ads are on (plugins.overwolf.ads.testAd); remove it before shipping',
      `Windows: ads need the WebView2 Runtime ${WEBVIEW2_MINIMUM} or newer`,
    ]);
    expect(messages(findings, 'ok')).toEqual([
      'the uid is pinned for release builds',
      'the macOS web content terminate hook is wired',
      'tauri 2.12.1 and @tauri-apps/api ^2.12.0 share the minor 2.12',
    ]);
    expect(formatFindings(findings.slice(0, 1))).toMatch(/^info {2}uid [a-p]{40} /);
  });

  it('flags capabilities, WebviewWindow uses, versions and two updaters', async () => {
    write(join(tauriDir, 'tauri.conf.json'), { productName: 'App' });
    write(join(tauriDir, 'capabilities', 'default.json'), {
      capabilities: [
        { identifier: 'main', windows: ['*'], permissions: ['core:default'] },
        {
          identifier: 'remote',
          webviews: ['x'],
          remote: { urls: ['https://*.overwolf.com', 'https://*'] },
        },
      ],
    });
    write(join(tauriDir, 'capabilities', 'notes.txt'), 'ignored');
    write(
      join(tauriDir, 'src', 'commands', 'win.rs'),
      'fn a(w: tauri::WebviewWindow) {}\nfn b() { app.get_webview_window("main"); }\n',
    );
    write(join(tauriDir, 'src', 'lib.rs'), 'pub fn run() {}\n');
    write(
      join(tauriDir, 'Cargo.toml'),
      '[package]\nname = "app"\n[dependencies]\ntauri = "2.10.0"\ntauri-plugin-updater = "2"\ntauri-plugin-overwolf = { version = "1", features = ["updater"] }\n',
    );
    mkdirSync(join(dir, 'node_modules', '@tauri-apps', 'api'), { recursive: true });
    write(join(dir, 'node_modules', '@tauri-apps', 'api', 'package.json'), { version: '2.12.1' });
    const findings = await run();
    const warn = messages(findings, 'warn').join('\n');
    expect(warn).toContain('set "uid", or both "author" and "name", before a release build');
    expect(warn).toContain('plugins.overwolf.author is not set');
    expect(warn).toContain('capability "main" (capabilities/default.json) selects "windows"');
    expect(warn).toContain('no capability grants "overwolf:default"');
    expect(warn).toContain(
      `${join('src', 'commands', 'win.rs')}:1: WebviewWindow does not see a window`,
    );
    expect(warn).toContain(`${join('src', 'commands', 'win.rs')}:2: get_webview_window`);
    expect(warn).toContain('tauri 2.10.0 and @tauri-apps/api 2.12.1 differ in the minor version');
    expect(warn).toContain('both tauri-plugin-updater and the overwolf "updater" feature are on');
    expect(messages(findings, 'error')).toEqual([
      'capability "remote" (capabilities/default.json) allows the remote URL "https://*.overwolf.com", which covers Overwolf ad pages; remove it',
      'capability "remote" (capabilities/default.json) allows the remote URL "https://*", which covers Overwolf ad pages; remove it',
    ]);
    expect(messages(findings, 'info').join('\n')).toContain(
      'macOS: wire builder.on_web_content_process_terminate',
    );
  });

  it('reads the tauri version from Cargo.lock, and says when it cannot compare', async () => {
    write(
      join(dir, 'Cargo.lock'),
      'version = 4\n\n[[package]]\nname = "tauri"\nversion = "2.12.3"\nsource = "registry"\n',
    );
    write(join(dir, 'package.json'), { devDependencies: { '@tauri-apps/api': '2.12.1' } });
    expect(messages(await run(), 'ok')).toContain(
      'tauri 2.12.3 and @tauri-apps/api 2.12.1 share the minor 2.12',
    );
    rmSync(join(dir, 'package.json'));
    expect(messages(await run(), 'info').join('\n')).toContain(
      'could not compare the tauri crate (2.12.3) with @tauri-apps/api (not found)',
    );
  });

  it('turns an identity error into an error finding', async () => {
    write(join(tauriDir, 'tauri.conf.json'), {
      productName: 'A',
      plugins: { overwolf: { uid: '../x' } },
    });
    expect(messages(await run(), 'error')).toEqual([
      '[OW] plugins.overwolf.uid: must be 1 to 64 ASCII letters or digits',
    ]);
  });

  it('knows which remote URLs cover Overwolf pages', () => {
    for (const url of ['https://*', 'http://*', 'https://*/x', '*', 'https://www.overwolf.com/*']) {
      expect(coversOverwolf(url)).toBe(true);
    }
    for (const url of ['https://example.com/*', 'https://*.example.com', 'tauri://localhost']) {
      expect(coversOverwolf(url)).toBe(false);
    }
    expect(minorOf('^2.12.1')).toBe('2.12');
    expect(minorOf('latest')).toBeUndefined();
  });
});
