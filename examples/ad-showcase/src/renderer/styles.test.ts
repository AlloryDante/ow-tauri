import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

const css = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8');

/** The declarations of every rule whose selector names `owadview`. */
const owadviewRules = (): string[] =>
  [...css.replace(/\/\*[\s\S]*?\*\//g, '').matchAll(/([^{}]+)\{([^{}]*)\}/g)]
    .filter(([, selector]) => /\bowadview\b/.test(selector ?? ''))
    .map(([, , body]) => body ?? '');

describe('styles.css', () => {
  it('styles <owadview> somewhere', () => {
    expect(owadviewRules().length).toBeGreaterThan(0);
  });

  it('never overrides the display of <owadview>', () => {
    // ow-electron lays its guest out as a flex child of an inline-flex
    // <owadview>; a display override leaves the guest 150 px tall and the ad
    // page waits with "not valid slot size" forever [OBS].
    for (const body of owadviewRules()) expect(body).not.toMatch(/(^|[;\s])display\s*:/);
  });
});
