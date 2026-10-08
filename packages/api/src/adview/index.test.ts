// Tests of the `./adview` entry: the page-wide singleton and the inert paths.
// The entry starts a runtime when imported, so each test imports a fresh
// copy (`vi.resetModules()`), the way a second bundle would.
import { afterEach, describe, expect, it, vi } from 'vitest';

import { mockOverwolf, settle } from '../testing/index.js';
import type * as EntryModule from './index.js';
import { RUNTIME_KEY, installRuntime, registeredRuntime, type AdviewApi } from './singleton.js';

type Entry = typeof EntryModule;

async function importEntry(): Promise<Entry> {
  vi.resetModules();
  return await import('./index.js');
}

function fakeApi(version: string): AdviewApi {
  return { version, upgrade: () => undefined, elements: () => [] };
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('installRuntime / registeredRuntime', () => {
  it('registers one runtime per scope and hands it to every later install', () => {
    const scope = {};
    const create = vi.fn(() => fakeApi('0.1.0'));
    const first = installRuntime('0.1.0', create, scope);
    const second = installRuntime('0.1.0', create, scope);
    expect(second).toBe(first);
    expect(create).toHaveBeenCalledTimes(1);
    expect(registeredRuntime(scope)).toBe(first);
    expect(Object.keys(scope)).toEqual([]);
  });

  it('warns once when a different major is bundled in the same page', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const scope = {};
    const first = installRuntime('0.1.0', () => fakeApi('0.1.0'), scope);
    expect(installRuntime('1.0.0', () => fakeApi('1.0.0'), scope)).toBe(first);
    expect(installRuntime('1.2.0', () => fakeApi('1.2.0'), scope)).toBe(first);
    expect(warn).toHaveBeenCalledTimes(1);
    expect(String(warn.mock.calls[0]?.[0])).toContain('two copies of tauri-plugin-overwolf-api');
  });

  it('ignores a registration without the runtime shape', () => {
    const scope = {};
    Reflect.set(scope, RUNTIME_KEY, { version: 1 });
    expect(registeredRuntime(scope)).toBeUndefined();
    expect(registeredRuntime({ [RUNTIME_KEY]: null })).toBeUndefined();
  });
});

describe('the ./adview entry', () => {
  it('stays inert outside a Tauri webview, with a warning', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const { adview } = await importEntry();
    expect(adview.version).toBe('0.1.0');
    expect(adview.elements()).toEqual([]);
    adview.upgrade(document.createElement('owadview'));
    expect(registeredRuntime()).toBeUndefined();
    expect(warn.mock.calls.flat().join('\n')).toContain('inert outside a Tauri webview');
  });

  it('stays inert in a reserved (ad guest or CMP) webview', async () => {
    for (const label of ['owad-3', 'ow-cmp']) {
      const overwolf = mockOverwolf({ label });
      const { adview } = await importEntry();
      expect(adview.elements()).toEqual([]);
      expect(registeredRuntime()).toBeUndefined();
      overwolf.restore();
    }
  });

  it('starts one runtime for two bundles of the package (two bundles, one runtime)', async () => {
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(() => ({
      x: 0,
      y: 0,
      width: 300,
      height: 250,
      left: 0,
      top: 0,
      right: 300,
      bottom: 250,
      toJSON: () => ({}),
    }));
    Object.defineProperty(HTMLElement.prototype, 'checkVisibility', {
      configurable: true,
      writable: true,
      value: () => true,
    });
    const overwolf = mockOverwolf();
    const one = await importEntry();
    const two = await importEntry();
    expect(two.adview).toBe(one.adview);
    expect(registeredRuntime()).toBe(one.adview);
    const el = document.createElement('owadview');
    el.setAttribute('cid', 'main');
    el.setAttribute('slotsize', '300x250');
    document.body.append(el);
    await settle();
    expect(overwolf.callsOf('adview_mount')).toHaveLength(1);
    expect(two.adview.elements()).toEqual([el]);
    two.adview.upgrade(el);
    await settle();
    expect(overwolf.callsOf('adview_mount')).toHaveLength(1);
    el.remove();
    await settle();
    expect(overwolf.callsOf('adview_unmount')).toHaveLength(1);
    overwolf.restore();
  });
});
