import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

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

describe('scripts/run.mjs', () => {
  it('is the same file in every example that has one', () => {
    const here = dirname(fileURLToPath(import.meta.url));
    const examples = join(here, '..', '..');
    const own = readFileSync(join(here, 'run.mjs'), 'utf8');
    const copies = readdirSync(examples)
      .map((name) => join(examples, name, 'scripts', 'run.mjs'))
      .filter((path) => existsSync(path));
    expect(copies.length).toBeGreaterThan(1);
    for (const path of copies) expect(readFileSync(path, 'utf8'), path).toBe(own);
  });
});
