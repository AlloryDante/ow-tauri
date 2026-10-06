import { describe, expect, it } from 'vitest';

import { OwTauriError, OwTauriUnsupportedError } from './errors.js';

describe('OwTauriUnsupportedError', () => {
  it('is an OwTauriError with code "unsupported" and names the api', () => {
    const error = new OwTauriUnsupportedError('app.dock', 'macOS dock menus are out of scope');
    expect(error).toBeInstanceOf(OwTauriError);
    expect(error).toBeInstanceOf(Error);
    expect(error.code).toBe('unsupported');
    expect(error.api).toBe('app.dock');
    expect(error.name).toBe('OwTauriUnsupportedError');
    expect(error.message).toBe(
      'ow-tauri does not support app.dock: macOS dock menus are out of scope',
    );
  });
});

describe('OwTauriError', () => {
  it('keeps the cause', () => {
    const cause = new Error('socket closed');
    const error = new OwTauriError('backend', 'package runtime stopped', { cause });
    expect(error.cause).toBe(cause);
    expect(error.code).toBe('backend');
  });
});

describe('instanceof across runtime copies', () => {
  const BRANDS = Symbol.for('ow-tauri.error.brands');

  it('recognises an error branded by another copy of the runtime', () => {
    const foreign = Object.defineProperty(new Error('from another bundle'), BRANDS, {
      value: ['OwTauriUnsupportedError', 'OwTauriError'],
    });
    expect(foreign instanceof OwTauriError).toBe(true);
    expect(foreign instanceof OwTauriUnsupportedError).toBe(true);
  });

  it('keeps subclass checks precise', () => {
    const base = new OwTauriError('io', 'disk full');
    expect(base instanceof OwTauriError).toBe(true);
    expect(base instanceof OwTauriUnsupportedError).toBe(false);
    expect(new Error('plain') instanceof OwTauriError).toBe(false);
    expect(OwTauriError[Symbol.hasInstance]('text')).toBe(false);
  });

  it('brands instances with their class chain', () => {
    const error = new OwTauriUnsupportedError('app.dock', 'out of scope');
    expect((error as unknown as Record<symbol, unknown>)[BRANDS]).toEqual([
      'OwTauriUnsupportedError',
      'OwTauriError',
    ]);
  });
});
