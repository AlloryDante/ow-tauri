import { OverwolfError } from 'tauri-plugin-overwolf-api';
import { describe, expect, it } from 'vitest';

import { describeError, logged } from './logged';
import { createLogStore } from './store';

describe('logged', () => {
  it('logs the call and its result', async () => {
    const log = createLogStore();
    const outcome = await logged(log, 'getInfo()', () => Promise.resolve({ uid: 'u' }));
    expect(outcome).toEqual({ ok: true, value: { uid: 'u' } });
    expect(log.entries().map((e) => [e.level, e.source, e.message, e.args])).toEqual([
      ['info', 'api', 'getInfo()', []],
      ['result', 'api', 'getInfo() →', [{ uid: 'u' }]],
    ]);
  });

  it('logs "done" for calls without a value', async () => {
    const log = createLogStore();
    await logged(log, 'disableAdsFPD()', () => Promise.resolve(undefined));
    await logged(log, 'check()', () => Promise.resolve(null), { source: 'updater' });
    expect(log.entries().map((e) => e.message)).toEqual([
      'disableAdsFPD()',
      'disableAdsFPD() → done',
      'check()',
      'check() → done',
    ]);
    expect(log.entries()[3]?.source).toBe('updater');
  });

  it('logs what `shown` makes of the result, and returns the real value', async () => {
    const log = createLogStore();
    const outcome = await logged(log, 'secret()', () => Promise.resolve('abcdef'), {
      shown: (v) => v.slice(0, 2),
    });
    expect(outcome).toEqual({ ok: true, value: 'abcdef' });
    expect(log.entries()[1]?.args).toEqual(['ab']);
  });

  it('logs an OverwolfError with its code and never rejects', async () => {
    const log = createLogStore();
    const outcome = await logged(log, 'getMachineIds()', () =>
      Promise.reject(new OverwolfError('forbidden', 'not granted')),
    );
    expect(outcome).toEqual({ ok: false, code: 'forbidden', message: 'not granted' });
    const last = log.entries().at(-1);
    expect(last?.level).toBe('error');
    expect(last?.message).toBe('getMachineIds() failed: forbidden');
    expect(last?.args).toEqual(['not granted']);
  });
});

describe('describeError', () => {
  it('names any rejection', () => {
    expect(describeError(new OverwolfError('unsupported', 'no'))).toEqual({
      code: 'unsupported',
      message: 'no',
    });
    expect(describeError(new Error('boom'))).toEqual({ code: 'error', message: 'boom' });
    expect(describeError('plain')).toEqual({ code: 'error', message: 'plain' });
  });
});
