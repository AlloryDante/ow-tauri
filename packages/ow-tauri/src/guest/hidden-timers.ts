/**
 * Chromium's timer throttling for hidden pages, emulated for the ad
 * guest's long timers. Chromium runs the timers of a hidden page only at
 * aligned wake-ups, once per second ("Timer throttling in Chrome 88",
 * developer.chrome.com); WebKit runs the shim-hidden document's timers on
 * time. The ad page gives up its ad with a timer about 2 s after `hidden`;
 * in ow-electron that comes 2.6 to 4.8 s after `hidden`, so a 2 s hide keeps
 * the ad there [OBS].
 *
 * Only timers of at least {@link MIN_ALIGNED_TIMEOUT_MS} are aligned [DEC]:
 * aligning every timer of the main frame alone (its cross-origin ad frames
 * keep running on time) left the ad silent after a 1-frame hide
 * [OBS: lab, reward-optin]. Chromium's intensive throttling of long hidden
 * pages (one wake-up per minute after 5 minutes) is not emulated.
 */

/** Wake-up alignment of a hidden page's timers (Chromium). */
export const HIDDEN_WAKE_UP_MS = 1_000;

/** Shorter timers run on time even while hidden [DEC]. */
export const MIN_ALIGNED_TIMEOUT_MS = 1_000;

/** A timer due this close to a wake-up runs at once. */
const WAKE_UP_SLACK_MS = 4;

type Handler = (...args: unknown[]) => void;

/** The timer functions of a window. */
export interface TimerHost {
  setTimeout: (handler: Handler, timeout?: number, ...args: unknown[]) => number;
  clearTimeout: (id?: number) => void;
  clearInterval: (id?: number) => void;
  performance: { now: () => number };
}

/**
 * How long a callback due `now` waits for the next aligned wake-up while
 * the page is hidden (0: run now).
 */
export function wakeUpDelay(now: number): number {
  const wait = Math.ceil(now / HIDDEN_WAKE_UP_MS) * HIDDEN_WAKE_UP_MS - now;
  return wait < WAKE_UP_SLACK_MS ? 0 : wait;
}

/**
 * Replaces `setTimeout`, `clearTimeout` and `clearInterval` of `win`: the
 * callback of a timer of at least {@link MIN_ALIGNED_TIMEOUT_MS} that falls
 * due while `hidden()` holds waits for the next aligned wake-up. Ids stay stable across the
 * wait (clearing works at any time); shorter timers and string handlers go
 * to the original functions untouched.
 *
 * @param win - the guest window
 * @param hidden - whether the page is hidden now
 * @param wrap - makes a replacement look like a host function
 */
export function installHiddenTimerAlignment(
  win: TimerHost,
  hidden: () => boolean,
  wrap: <A extends unknown[], R>(body: (...args: A) => R) => (...args: A) => R,
): void {
  const native = {
    setTimeout: win.setTimeout.bind(win),
    clearTimeout: win.clearTimeout.bind(win),
    clearInterval: win.clearInterval.bind(win),
  };
  /** Our id → the native timer that runs it now. */
  const live = new Map<number, number>();

  win.setTimeout = wrap((handler: Handler, timeout?: number, ...args: unknown[]): number => {
    const delay = Math.max(0, Number(timeout) || 0);
    if (typeof handler !== 'function' || delay < MIN_ALIGNED_TIMEOUT_MS)
      return native.setTimeout(handler, timeout, ...args);
    const run = (): void => {
      live.delete(id);
      handler(...args);
    };
    const id = native.setTimeout(() => {
      const wait = hidden() ? wakeUpDelay(win.performance.now()) : 0;
      if (wait === 0) {
        run();
        return;
      }
      live.set(id, native.setTimeout(run, wait));
    }, delay);
    live.set(id, id);
    return id;
  });
  // Timeouts and intervals share one id space: either function clears a
  // timer that waits for its wake-up.
  const clear =
    (original: (id?: number) => void) =>
    (id?: number): void => {
      if (id === undefined) return;
      const timer = live.get(id);
      live.delete(id);
      original(timer ?? id);
    };
  win.clearTimeout = wrap(clear(native.clearTimeout));
  win.clearInterval = wrap(clear(native.clearInterval));
}
