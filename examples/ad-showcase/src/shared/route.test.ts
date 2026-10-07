import { describe, expect, it } from 'vitest';

import { formatRoute, parseRoute, withPageSwitch } from './route.js';

describe('parseRoute', () => {
  it('reads a page with or without an argument and a leading #', () => {
    expect(parseRoute('#sizes')).toEqual({ page: 'sizes', arg: null });
    expect(parseRoute('layouts/tower-plus')).toEqual({ page: 'layouts', arg: 'tower-plus' });
    expect(parseRoute('#sizes/300x250')).toEqual({ page: 'sizes', arg: '300x250' });
  });

  it('rejects empty, nested and unsafe routes', () => {
    expect(parseRoute('')).toBeNull();
    expect(parseRoute('#')).toBeNull();
    expect(parseRoute('a/b/c')).toBeNull();
    expect(parseRoute('Sizes')).toBeNull();
    expect(parseRoute('sizes/<script>')).toBeNull();
    expect(parseRoute('sizes/')).toBeNull();
    expect(parseRoute('x'.repeat(41))).toBeNull();
  });

  it('round-trips through formatRoute', () => {
    for (const text of ['reward', 'layouts/studio']) {
      const route = parseRoute(text);
      expect(route).not.toBeNull();
      expect(formatRoute(route!)).toBe(text);
    }
  });
});

describe('withPageSwitch', () => {
  it('replaces any page switch and appends the new route', () => {
    expect(
      withPageSwitch(
        ['--test-ad', '--showcase-page=sizes', 'app', '--showcase-page'],
        'layouts/tower',
      ),
    ).toEqual(['--test-ad', 'app', '--showcase-page=layouts/tower']);
  });

  it('only removes the switch when the route is null or malformed', () => {
    expect(withPageSwitch(['--showcase-page=sizes', 'app'], null)).toEqual(['app']);
    expect(withPageSwitch(['app'], 'a/b/c')).toEqual(['app']);
  });
});
