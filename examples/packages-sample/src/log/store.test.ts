import { describe, expect, it, vi } from 'vitest';

import { MAX_ENTRIES, createLogStore } from './store';

describe('createLogStore', () => {
  it('adds entries with increasing ids, the time and the values', () => {
    let t = 100;
    const log = createLogStore({ now: () => t++ });
    const a = log.push('info', 'app', 'started');
    const b = log.push('result', 'api', 'getInfo() →', { uid: 'x' });
    expect(log.entries()).toEqual([a, b]);
    expect(a).toEqual({
      id: 1,
      at: 100,
      level: 'info',
      source: 'app',
      message: 'started',
      args: [],
    });
    expect(b.id).toBe(2);
    expect(b.args).toEqual([{ uid: 'x' }]);
  });

  it('keeps the snapshot until the next change', () => {
    const log = createLogStore();
    const before = log.entries();
    expect(log.entries()).toBe(before);
    log.push('info', 'app', 'x');
    expect(log.entries()).not.toBe(before);
    expect(before).toEqual([]);
  });

  it('drops the oldest entries beyond the limit', () => {
    const log = createLogStore({ max: 3 });
    for (let i = 1; i <= 5; i++) log.push('info', 'app', String(i));
    expect(log.entries().map((e) => e.message)).toEqual(['3', '4', '5']);
    expect(MAX_ENTRIES).toBe(1000);
    expect(createLogStore({ max: 0 }).push('info', 'app', 'one').id).toBe(1);
  });

  it('notifies subscribers on push and clear, until they unsubscribe', () => {
    const log = createLogStore();
    const listener = vi.fn();
    const off = log.subscribe(listener);
    log.push('info', 'app', 'x');
    log.clear();
    expect(listener).toHaveBeenCalledTimes(2);
    expect(log.entries()).toEqual([]);
    log.clear(); // already empty: no change, no call
    expect(listener).toHaveBeenCalledTimes(2);
    off();
    log.push('info', 'app', 'y');
    expect(listener).toHaveBeenCalledTimes(2);
  });
});
