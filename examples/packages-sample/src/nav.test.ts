import { describe, expect, it } from 'vitest';

import { PAGES, pageOf } from './nav';

describe('pageOf', () => {
  it('selects the page of a hash, with or without # and /', () => {
    expect(pageOf('#ads').id).toBe('ads');
    expect(pageOf('settings').id).toBe('settings');
    expect(pageOf('#/updater').id).toBe('updater');
  });

  it('falls back to the logger', () => {
    expect(pageOf('').id).toBe('logger');
    expect(pageOf('#nope').id).toBe('logger');
  });

  it('lists the five pages in navigation order', () => {
    expect(PAGES.map((p) => p.id)).toEqual(['logger', 'ads', 'settings', 'updater', 'packages']);
    for (const page of PAGES) expect(page.title.length).toBeGreaterThan(0);
  });
});
