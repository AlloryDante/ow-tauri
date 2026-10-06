import { afterEach, describe, expect, it, vi } from 'vitest';

import { emitFromHost, errorMonitor, EventEmitter, reportUncaught, safeCall } from './emitter.js';

describe('EventEmitter (Node semantics)', () => {
  afterEach(() => {
    EventEmitter.defaultMaxListeners = 10;
  });

  it('calls listeners synchronously in registration order with this = emitter', () => {
    const e = new EventEmitter();
    const order: string[] = [];
    e.on('x', function (this: unknown, a: number) {
      expect(this).toBe(e);
      order.push(`a${String(a)}`);
    });
    e.addListener('x', (a: number) => order.push(`b${String(a)}`));
    e.prependListener('x', (a: number) => order.push(`p${String(a)}`));
    expect(e.emit('x', 1)).toBe(true);
    expect(order).toEqual(['p1', 'a1', 'b1']);
    expect(e.emit('nothing')).toBe(false);
  });

  it('removes once listeners before calling them and supports prependOnceListener', () => {
    const e = new EventEmitter();
    const calls: string[] = [];
    e.once('x', () => {
      calls.push('once');
      expect(e.listenerCount('x')).toBe(1);
    });
    e.on('x', () => calls.push('on'));
    e.prependOnceListener('x', () => calls.push('first'));
    e.emit('x');
    e.emit('x');
    expect(calls).toEqual(['first', 'once', 'on', 'on']);
  });

  it('does not call a once listener twice when emit recurses', () => {
    const e = new EventEmitter();
    const fn = vi.fn(() => e.emit('x'));
    e.once('x', fn);
    e.emit('x');
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it('snapshots listeners during emit', () => {
    const e = new EventEmitter();
    const late = vi.fn();
    e.on('x', () => e.on('x', late));
    e.emit('x');
    expect(late).not.toHaveBeenCalled();
    e.emit('x');
    expect(late).toHaveBeenCalledTimes(1);
  });

  it('removeListener removes the last matching registration, including once wrappers', () => {
    const e = new EventEmitter();
    const fn = vi.fn();
    e.on('x', fn);
    e.once('x', fn);
    expect(e.listenerCount('x', fn)).toBe(2);
    e.off('x', fn);
    expect(e.rawListeners('x')).toEqual([fn]);
    e.removeListener('x', fn);
    expect(e.eventNames()).toEqual([]);
    e.removeListener('missing', fn);
    e.removeListener('x', fn);
  });

  it('listeners() unwraps once wrappers; rawListeners() keeps them', () => {
    const e = new EventEmitter();
    const fn = vi.fn();
    e.once('x', fn);
    expect(e.listeners('x')).toEqual([fn]);
    const [raw] = e.rawListeners('x');
    expect(raw).not.toBe(fn);
    expect((raw as unknown as { listener: unknown }).listener).toBe(fn);
    raw?.();
    expect(fn).toHaveBeenCalledTimes(1);
    expect(e.listenerCount('x')).toBe(0);
    expect(e.listeners('none')).toEqual([]);
  });

  it('emits newListener before adding and removeListener after removing', () => {
    const e = new EventEmitter();
    const events: unknown[] = [];
    const fn = (): void => undefined;
    e.on('newListener', (name: string, listener: unknown) => {
      events.push(['new', name, listener === fn, e.listenerCount(name)]);
    });
    e.on('removeListener', (name: string, listener: unknown) =>
      events.push(['remove', name, listener === fn]),
    );
    e.once('x', fn);
    e.removeListener('x', fn);
    expect(events).toEqual([
      ['new', 'removeListener', false, 0],
      ['new', 'x', true, 0],
      ['remove', 'x', true],
    ]);
  });

  it('removeAllListeners with and without removeListener listeners', () => {
    const e = new EventEmitter();
    e.on('a', vi.fn());
    e.on('b', vi.fn());
    e.removeAllListeners('a');
    expect(e.eventNames()).toEqual(['b']);
    e.removeAllListeners();
    expect(e.eventNames()).toEqual([]);

    const removed: unknown[] = [];
    e.on('removeListener', (name: unknown) => removed.push(name));
    e.on('a', vi.fn());
    e.on('a', vi.fn());
    e.on('b', vi.fn());
    e.removeAllListeners('a');
    expect(removed).toEqual(['a', 'a']);
    e.removeAllListeners();
    // Like Node, removing the last 'removeListener' listener emits nothing.
    expect(removed).toEqual(['a', 'a', 'b']);
    expect(e.eventNames()).toEqual([]);
    e.removeAllListeners('none');
  });

  it("throws an unhandled 'error' after errorMonitor listeners ran", () => {
    const e = new EventEmitter();
    const monitor = vi.fn();
    e.on(errorMonitor, monitor);
    const boom = new Error('boom');
    expect(() => e.emit('error', boom)).toThrow(boom);
    expect(monitor).toHaveBeenCalledWith(boom);
    expect(() => e.emit('error', 'text')).toThrow("Unhandled error. ('text')");
    expect(() => e.emit('error', { code: 1 })).toThrow('Unhandled error. ({"code":1})');
    const cyclic: Record<string, unknown> = {};
    cyclic['self'] = cyclic;
    expect(() => e.emit('error', cyclic)).toThrow('Unhandled error. ([object Object])');
    e.on('error', vi.fn());
    expect(e.emit('error', boom)).toBe(true);
    expect(monitor).toHaveBeenCalledTimes(5);
    expect(EventEmitter.errorMonitor).toBe(errorMonitor);
  });

  it('validates listeners', () => {
    const e = new EventEmitter();
    expect(() => e.on('x', 5 as never)).toThrow(TypeError);
    expect(() => e.removeListener('x', null as never)).toThrow(TypeError);
  });

  it('warns once when the max listener count is exceeded', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const e = new EventEmitter();
    expect(e.getMaxListeners()).toBe(10);
    e.setMaxListeners(1);
    e.on('x', vi.fn());
    e.on('x', vi.fn());
    e.on('x', vi.fn());
    expect(warn).toHaveBeenCalledTimes(1);
    expect(warn.mock.calls[0]?.[0]).toContain('MaxListenersExceededWarning');
    e.setMaxListeners(0);
    for (let i = 0; i < 20; i++) e.on('y', vi.fn());
    expect(warn).toHaveBeenCalledTimes(1);
    expect(() => e.setMaxListeners(-1)).toThrow(RangeError);
    EventEmitter.defaultMaxListeners = 2;
    expect(new EventEmitter().getMaxListeners()).toBe(2);
    expect(() => {
      EventEmitter.defaultMaxListeners = 1.5;
    }).toThrow(RangeError);
  });

  it('accepts symbol event names', () => {
    const e = new EventEmitter();
    const s = Symbol('s');
    const fn = vi.fn();
    e.on(s, fn);
    e.emit(s, 1);
    expect(fn).toHaveBeenCalledWith(1);
    expect(e.eventNames()).toEqual([s]);
  });
});

describe('emitFromHost', () => {
  it("logs an 'error' without listeners instead of throwing", () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const e = new EventEmitter();
    const monitor = vi.fn();
    e.on(errorMonitor, monitor);
    expect(emitFromHost(e, 'error', new Error('x'))).toBe(false);
    expect(warn).toHaveBeenCalled();
    expect(monitor).toHaveBeenCalled();
  });

  it('reports listener exceptions without throwing', () => {
    const report = vi.fn();
    vi.stubGlobal('reportError', report);
    const e = new EventEmitter();
    e.on('x', () => {
      throw new Error('listener failed');
    });
    expect(emitFromHost(e, 'x')).toBe(false);
    expect(report).toHaveBeenCalledWith(new Error('listener failed'));
    e.on('y', vi.fn());
    expect(emitFromHost(e, 'y')).toBe(true);
    vi.unstubAllGlobals();
  });

  it('falls back to console.error without reportError', () => {
    vi.stubGlobal('reportError', undefined);
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    reportUncaught('x');
    safeCall(() => {
      throw new Error('y');
    });
    expect(error).toHaveBeenCalledTimes(2);
    vi.unstubAllGlobals();
  });
});
