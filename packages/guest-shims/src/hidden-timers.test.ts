import { describe, expect, it } from 'vitest';

import {
  HIDDEN_WAKE_UP_MS,
  MIN_ALIGNED_TIMEOUT_MS,
  installHiddenTimerAlignment,
  wakeUpDelay,
  type TimerHost,
} from './hidden-timers.js';

interface Fake extends TimerHost {
  advance: (ms: number) => void;
  pending: () => number;
}

/** A window with a manual clock: `advance` runs what falls due, in order. */
function fakeWindow(start = 0): Fake {
  let clock = start;
  let next = 1;
  const timers = new Map<number, { at: number; run: () => void }>();
  const clear = (id?: number): void => {
    if (id !== undefined) timers.delete(id);
  };
  return {
    performance: { now: () => clock },
    setTimeout: (handler, timeout = 0, ...args) => {
      const id = next++;
      timers.set(id, {
        at: clock + Math.max(0, timeout),
        run: () => {
          handler(...args);
        },
      });
      return id;
    },
    clearTimeout: clear,
    clearInterval: clear,
    advance: (ms) => {
      const end = clock + ms;
      for (;;) {
        let due: [number, { at: number; run: () => void }] | undefined;
        for (const entry of timers) {
          if (entry[1].at <= end && (!due || entry[1].at < due[1].at)) due = entry;
        }
        if (!due) break;
        timers.delete(due[0]);
        clock = Math.max(clock, due[1].at);
        due[1].run();
      }
      clock = end;
    },
    pending: () => timers.size,
  };
}

const same = <A extends unknown[], R>(body: (...args: A) => R): ((...args: A) => R) => body;
const at = (win: Fake): string => String(win.performance.now());

describe('hidden-page timer alignment (Chromium)', () => {
  it('waits for the next whole-second wake-up', () => {
    expect(HIDDEN_WAKE_UP_MS).toBe(1_000);
    expect(wakeUpDelay(1_000)).toBe(0);
    expect(wakeUpDelay(1_002)).toBe(998);
    expect(wakeUpDelay(1_995)).toBe(5);
    // Due within the slack of a wake-up: now.
    expect(wakeUpDelay(1_997)).toBe(0);
  });

  it('runs every timer on time while visible', () => {
    const win = fakeWindow(250);
    installHiddenTimerAlignment(win, () => false, same);
    const runs: string[] = [];
    win.setTimeout(() => runs.push(at(win)), 2_000);
    win.advance(2_000);
    expect(runs).toEqual(['2250']);
  });

  it('aligns a long timer that falls due while hidden, in order of due time', () => {
    const win = fakeWindow(250);
    let hidden = true;
    installHiddenTimerAlignment(win, () => hidden, same);
    const runs: string[] = [];
    // Due at 2250 and 2300 while hidden: both wait for the 3000 wake-up.
    win.setTimeout(() => runs.push(`a@${at(win)}`), 2_000);
    win.setTimeout((x: unknown) => runs.push(`b@${at(win)}:${String(x)}`), 2_050, 'x');
    win.advance(2_500);
    expect(runs).toEqual([]);
    win.advance(250);
    expect(runs).toEqual(['a@3000', 'b@3000:x']);
    // Visible again: on time.
    hidden = false;
    win.setTimeout(() => runs.push(`c@${at(win)}`), MIN_ALIGNED_TIMEOUT_MS);
    win.advance(MIN_ALIGNED_TIMEOUT_MS);
    expect(runs.at(-1)).toBe('c@4000');
  });

  it('runs a short timer on time even while hidden (cross-frame handshakes)', () => {
    const win = fakeWindow(250);
    installHiddenTimerAlignment(win, () => true, same);
    const runs: string[] = [];
    win.setTimeout(() => runs.push(at(win)), MIN_ALIGNED_TIMEOUT_MS - 1);
    win.advance(MIN_ALIGNED_TIMEOUT_MS);
    expect(runs).toEqual(['1249']);
  });

  it('clears a timer while it waits for its wake-up, by the id the page got', () => {
    const win = fakeWindow(0);
    installHiddenTimerAlignment(win, () => true, same);
    let ran = 0;
    const a = win.setTimeout(() => (ran += 1), 1_200);
    const b = win.setTimeout(() => (ran += 1), 1_300);
    win.advance(1_500);
    win.clearTimeout(a);
    win.clearInterval(b);
    win.advance(1_000);
    expect(ran).toBe(0);
    expect(win.pending()).toBe(0);
  });

  it('passes string handlers to the original function', () => {
    const win = fakeWindow(0);
    let seen: unknown;
    win.setTimeout = (handler) => {
      seen = handler;
      return 7;
    };
    installHiddenTimerAlignment(win, () => true, same);
    expect(win.setTimeout('x()' as never, 2_000)).toBe(7);
    expect(seen).toBe('x()');
  });
});
