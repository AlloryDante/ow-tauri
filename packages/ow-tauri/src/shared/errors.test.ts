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
