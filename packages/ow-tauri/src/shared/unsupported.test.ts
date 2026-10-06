import { describe, expect, it, vi } from 'vitest';

import { OwTauriUnsupportedError } from './errors.js';
import {
  consoleWarnOnce,
  defineUnsupported,
  unsupportedMethod,
  unsupportedModule,
} from './unsupported.js';

describe('unsupported members (B.2 legend "U")', () => {
  it('throws for methods and warns once for properties', () => {
    const target = {};
    defineUnsupported(target, 'thing.', ['run'], ['prop']);
    expect(() => {
      (target as { run(): void }).run();
    }).toThrow(OwTauriUnsupportedError);
    expect(Object.keys(target)).toEqual([]);
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    expect((target as { prop: unknown }).prop).toBeUndefined();
    expect((target as { prop: unknown }).prop).toBeUndefined();
    expect(warn).toHaveBeenCalledTimes(1);
    consoleWarnOnce('k-test', 'm');
    consoleWarnOnce('k-test', 'm');
    expect(warn).toHaveBeenCalledTimes(2);
  });

  it('names the api in the error', () => {
    expect(() => unsupportedMethod('a.b')()).toThrow('ow-tauri does not support a.b');
  });

  it('builds module stand-ins that throw on every use', () => {
    const mod = unsupportedModule('Thing', 'because') as Record<string, unknown> & (() => void);
    expect(mod.name).toBe('Thing');
    expect(mod['then']).toBeUndefined();
    expect(mod[Symbol.toStringTag as unknown as string]).toBeUndefined();
    expect(() => {
      (mod['doIt'] as () => void)();
    }).toThrow('ow-tauri does not support Thing.doIt: because');
    expect(() => {
      mod();
    }).toThrow(OwTauriUnsupportedError);
    expect(() => new (mod as unknown as new () => unknown)()).toThrow(OwTauriUnsupportedError);
    expect(() => {
      mod['x'] = 1;
    }).toThrow(OwTauriUnsupportedError);
    expect('x' in mod).toBe(false);
  });
});
