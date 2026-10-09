import { describe, expect, it } from 'vitest';

import {
  DEFAULT_LAYOUT,
  LAYOUTS,
  PERFORMANCE_END_EVENTS,
  PERFORMANCE_EVENTS,
  isVideoSize,
  layoutOf,
  sizeText,
} from './formats';

describe('layouts', () => {
  it('keeps the upstream layouts with unique ids', () => {
    expect(LAYOUTS).toHaveLength(15);
    expect(new Set(LAYOUTS.map((l) => l.id)).size).toBe(LAYOUTS.length);
    expect(LAYOUTS.filter((l) => l.highImpact).map((l) => l.id)).toEqual([
      'tower-plus-high-impact',
    ]);
  });

  it('has at most one video slot per layout (Overwolf ad policy)', () => {
    for (const l of LAYOUTS)
      expect([l.ad1, l.ad2].filter(isVideoSize).length, l.id).toBeLessThanOrEqual(1);
    expect(LAYOUTS.some((l) => isVideoSize(l.ad1) || isVideoSize(l.ad2))).toBe(true);
  });

  it('finds a layout or falls back to the default', () => {
    expect(layoutOf('studio-left').ad1).toEqual([400, 300]);
    expect(layoutOf('nope').id).toBe(DEFAULT_LAYOUT);
  });

  it('writes slot sizes as WxH', () => {
    expect(sizeText([728, 90])).toBe('728x90');
  });

  it('removes the performance ad only on events it dispatches', () => {
    for (const name of PERFORMANCE_END_EVENTS) expect(PERFORMANCE_EVENTS).toContain(name);
  });
});
