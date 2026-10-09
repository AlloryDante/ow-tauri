import { describe, expect, it } from 'vitest';

import { formatTime, jsonSafe, matches, payloadOf, valueText } from './format';
import type { LogEntry } from './store';

const entry = (message: string, ...args: unknown[]): LogEntry => ({
  id: 1,
  at: 0,
  level: 'info',
  source: 'api',
  message,
  args,
});

describe('formatTime', () => {
  it('prints local hours, minutes, seconds and milliseconds', () => {
    const at = new Date(2026, 9, 9, 7, 5, 3, 42).getTime();
    expect(formatTime(at)).toBe('07:05:03.042');
  });
});

describe('valueText', () => {
  it('keeps strings and writes everything else as JSON', () => {
    expect(valueText('a b')).toBe('a b');
    expect(valueText({ a: 1 })).toBe('{"a":1}');
    expect(valueText(undefined)).toBe('undefined');
    expect(valueText(() => 1)).toBe('function');
    const cycle: Record<string, unknown> = {};
    cycle['self'] = cycle;
    expect(valueText(cycle)).toBe('[object Object]');
  });
});

describe('matches', () => {
  it('finds the query in the message or the values, ignoring case', () => {
    const e = entry('getInfo() →', { uid: 'AbCd' });
    expect(matches(e, '')).toBe(true);
    expect(matches(e, '  ')).toBe(true);
    expect(matches(e, 'GETINFO')).toBe(true);
    expect(matches(e, 'abcd')).toBe(true);
    expect(matches(e, 'muid')).toBe(false);
  });
});

describe('jsonSafe', () => {
  it('copies JSON values and makes the rest readable', () => {
    const value = { a: [1, 'x'], b: null };
    const copy = jsonSafe(value);
    expect(copy).toEqual(value);
    expect(copy).not.toBe(value);
    expect(jsonSafe(undefined)).toBeNull();
    expect(jsonSafe(10n)).toBe('10');
    const cycle: Record<string, unknown> = {};
    cycle['self'] = cycle;
    expect(jsonSafe(cycle)).toBe('[object Object]');
  });
});

describe('payloadOf', () => {
  it('copies the own properties of an event except isTrusted', () => {
    const event = new Event('display_ad_loaded');
    Object.assign(event, { slotSize: '300x250', detail: { n: 1 }, gone: undefined });
    expect(payloadOf(event)).toEqual({ slotSize: '300x250', detail: { n: 1 }, gone: null });
  });
});
