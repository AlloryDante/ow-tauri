import { describe, expect, it } from 'vitest';

import { binaryName } from './run.mjs';

describe('binaryName', () => {
  it('prefers the [[bin]] name', () => {
    const toml = '[package]\nname = "pkg"\n\n[[bin]]\nname = "app-bin"\npath = "src/main.rs"\n';
    expect(binaryName(toml)).toBe('app-bin');
  });

  it('falls back to the package name', () => {
    expect(binaryName('[package]\nname = "my-app"\nversion = "1.0.0"\n')).toBe('my-app');
  });

  it('refuses a manifest without a name', () => {
    expect(() => binaryName('[workspace]\n')).toThrow('no package name');
  });
});
