import { describe, expect, it } from 'vitest';

import { EVENT_NAMES, FAMILIES, familyOf, payloadOf } from './events.js';

describe('familyOf', () => {
  it('sorts events into families', () => {
    expect(familyOf('display_ad_loaded')).toBe('display');
    expect(familyOf('video_ad_ready')).toBe('reward');
    expect(familyOf('play')).toBe('video');
    expect(familyOf('play', 'reward')).toBe('reward');
    expect(familyOf('high-impact-ad-loaded')).toBe('high-impact');
    expect(familyOf('performance_ad_shown')).toBe('performance');
    expect(familyOf('shutdown')).toBe('performance');
    expect(familyOf('house_ad_action')).toBe('house');
    expect(familyOf('house-ad-action')).toBe('house');
    expect(familyOf('control:mute')).toBe('control');
    expect(familyOf('player_loaded')).toBe('video');
    expect(familyOf('dom-ready')).toBe('lifecycle');
    expect(familyOf('destroyed')).toBe('lifecycle');
  });

  it('gives every listed event a known family', () => {
    for (const name of EVENT_NAMES) expect(FAMILIES).toContain(familyOf(name));
  });
});

describe('payloadOf', () => {
  it('copies own properties, without isTrusted', () => {
    const event = Object.assign(new Event('impression'), { a: 1, b: 'x', c: undefined });
    Object.defineProperty(event, 'isTrusted', { value: false, enumerable: true });
    expect(payloadOf(event)).toEqual({ a: 1, b: 'x', c: null });
  });

  it('turns values JSON cannot hold into strings', () => {
    const cyclic: Record<string, unknown> = {};
    cyclic['self'] = cyclic;
    const event = Object.assign(new Event('x'), { cyclic, n: 1n });
    const payload = payloadOf(event);
    expect(payload['cyclic']).toBe('[object Object]');
    expect(payload['n']).toBe('1');
  });

  it('is empty for a plain event', () => {
    expect(payloadOf(new Event('x'))).toEqual({});
  });
});
