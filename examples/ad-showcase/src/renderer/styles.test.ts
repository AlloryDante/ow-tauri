import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

const css = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8');

/** The declarations of every rule whose selector names `owadview`. */
const owadviewRules = (): string[] =>
  [...css.replace(/\/\*[\s\S]*?\*\//g, '').matchAll(/([^{}]+)\{([^{}]*)\}/g)]
    .filter(([, selector]) => /\bowadview\b/.test(selector ?? ''))
    .map(([, , body]) => body ?? '');

/** Every rule as [selector list, declarations], comments removed. */
const rules = (): [string, string][] =>
  [...css.replace(/\/\*[\s\S]*?\*\//g, '').matchAll(/([^{}]+)\{([^{}]*)\}/g)].map(
    ([, sel, body]) => [(sel ?? '').trim(), body ?? ''],
  );

/** Declarations of every rule whose selector list contains `selector` exactly. */
const declarationsOf = (selector: string): string =>
  rules()
    .filter(([sel]) => sel.split(',').some((part) => part.trim() === selector))
    .map(([, body]) => body)
    .join(';');

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

  it('never cuts a slot header chip, size or adstyle; it wraps instead', () => {
    expect(declarationsOf('.slot-head')).toMatch(/flex-wrap:\s*wrap/);
    expect(declarationsOf('.slot-head')).not.toMatch(/(^|[;\s])height:/);
    for (const selector of ['.slot-head > *', '.chip', '.tag', '.slot-size']) {
      const body = declarationsOf(selector);
      expect(body, selector).not.toMatch(/text-overflow|overflow:\s*hidden/);
    }
    expect(declarationsOf('.slot-head > *')).toMatch(/flex:\s*none/);
    expect(declarationsOf('.slot-head > .chip')).toBe('');
    // Only the cid gives way, with an ellipsis (its title has the full text).
    expect(declarationsOf('.slot-head > .slot-cid')).toMatch(/text-overflow:\s*ellipsis/);
  });

  it('hides the "no fill yet" words once the slot has an ad', () => {
    for (const status of ['loaded', 'playing', 'ready']) {
      expect(declarationsOf(`.slot[data-status='${status}'] .slot-fallback`), status).toMatch(
        /visibility:\s*hidden/,
      );
    }
  });

  it('gives the timeline event name priority over the cid', () => {
    const shrink = (selector: string): number =>
      Number(/flex:\s*\S+\s+(\d+)/.exec(declarationsOf(selector))?.[1] ?? NaN);
    expect(shrink('.tl-name')).toBeLessThan(shrink('.tl-cid'));
    expect(declarationsOf('.tl-time')).toMatch(/flex:\s*none/);
  });
});

/** The custom properties a rule with exactly this selector sets. */
const tokensOf = (selector: string): Map<string, string> =>
  new Map(
    [...declarationsOf(selector).matchAll(/(--[\w-]+)\s*:\s*([^;]+)/g)].map(([, name, value]) => [
      name ?? '',
      (value ?? '').trim(),
    ]),
  );

/** WCAG 2 contrast ratio of two `#rrggbb` colours. */
const contrast = (a: string, b: string): number => {
  const luminance = (hex: string): number => {
    const [r, g, bl] = [1, 3, 5].map((i) => {
      const c = parseInt(hex.slice(i, i + 2), 16) / 255;
      return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * (r ?? 0) + 0.7152 * (g ?? 0) + 0.0722 * (bl ?? 0);
  };
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return ((hi ?? 0) + 0.05) / ((lo ?? 0) + 0.05);
};

describe('light theme', () => {
  const chosen = tokensOf(":root[data-theme='light']");
  const system = tokensOf(":root:not([data-theme='dark'])");

  it('is the same whether chosen or taken from the system', () => {
    expect([...system.entries()]).toEqual([...chosen.entries()]);
  });

  it('overrides every colour the dark theme sets', () => {
    const dark = [...tokensOf(':root').entries()].filter(([, v]) => v.startsWith('#'));
    const kept = new Set(['--badge-test', '--badge-live', '--on-badge']);
    for (const [name] of dark) if (!kept.has(name)) expect(chosen.has(name), name).toBe(true);
  });

  it('keeps every status and family colour readable as text on its surfaces', () => {
    const text = [
      '--text',
      '--muted',
      '--accent',
      '--ok',
      '--warn',
      '--err',
      '--info',
      ...[...chosen.keys()].filter((name) => name.startsWith('--fam-')),
    ];
    for (const surface of ['--bg', '--surface', '--surface-2']) {
      for (const name of text) {
        const ratio = contrast(chosen.get(name) ?? '', chosen.get(surface) ?? '');
        expect(ratio, `${name} on ${surface}`).toBeGreaterThanOrEqual(4.5);
      }
    }
    // White text on the accent (pressed buttons, the scope toggle).
    expect(
      contrast(chosen.get('--on-accent') ?? '', chosen.get('--accent') ?? ''),
    ).toBeGreaterThanOrEqual(4.5);
  });
});
