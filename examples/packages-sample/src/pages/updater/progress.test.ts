import { describe, expect, it } from 'vitest';

import { INITIAL, explain, percent, updaterReducer, type UpdaterState } from './progress';

const update = { version: '1.1.0', currentVersion: '1.0.0' };

describe('updaterReducer', () => {
  it('goes from check to the result', () => {
    const checking = updaterReducer(INITIAL, { type: 'check' });
    expect(checking).toEqual({ phase: 'checking' });
    expect(updaterReducer(checking, { type: 'checked', update: null })).toEqual({
      phase: 'up-to-date',
    });
    expect(updaterReducer(checking, { type: 'checked', update })).toEqual({
      phase: 'available',
      update,
    });
  });

  it('counts the download from Started, Progress and Finished', () => {
    let s: UpdaterState = { phase: 'available', update };
    s = updaterReducer(s, { type: 'download' });
    expect(s).toEqual({ phase: 'downloading', update, downloaded: 0 });
    s = updaterReducer(s, {
      type: 'event',
      event: { event: 'Started', data: { contentLength: 1000 } },
    });
    expect(s).toEqual({ phase: 'downloading', update, downloaded: 0, total: 1000 });
    s = updaterReducer(s, {
      type: 'event',
      event: { event: 'Progress', data: { chunkLength: 300 } },
    });
    s = updaterReducer(s, {
      type: 'event',
      event: { event: 'Progress', data: { chunkLength: 200 } },
    });
    expect(percent(s)).toBe(50);
    s = updaterReducer(s, { type: 'event', event: { event: 'Finished' } });
    expect(s).toEqual({ phase: 'installing', update, downloaded: 500 });
    expect(percent(s)).toBe(100);
  });

  it('has no percentage without a size', () => {
    let s: UpdaterState = updaterReducer({ phase: 'available', update }, { type: 'download' });
    s = updaterReducer(s, { type: 'event', event: { event: 'Started', data: {} } });
    s = updaterReducer(s, {
      type: 'event',
      event: { event: 'Progress', data: { chunkLength: 10 } },
    });
    expect(s).toEqual({ phase: 'downloading', update, downloaded: 10 });
    expect(percent(s)).toBeNull();
    expect(percent(INITIAL)).toBeNull();
  });

  it('ignores actions that do not apply', () => {
    const downloading: UpdaterState = { phase: 'downloading', update, downloaded: 5 };
    expect(updaterReducer(downloading, { type: 'check' })).toBe(downloading);
    expect(updaterReducer(INITIAL, { type: 'download' })).toBe(INITIAL);
    expect(updaterReducer(INITIAL, { type: 'event', event: { event: 'Finished' } })).toBe(INITIAL);
  });

  it('keeps the error code and message', () => {
    expect(
      updaterReducer({ phase: 'checking' }, { type: 'failed', code: 'unsupported', message: 'no' }),
    ).toEqual({
      phase: 'error',
      code: 'unsupported',
      message: 'no',
    });
  });
});

describe('explain', () => {
  it('explains the codes an integrator meets', () => {
    expect(explain('unsupported')).toContain('Windows only');
    expect(explain('forbidden')).toContain('overwolf:updater');
    expect(explain('network')).toContain('feed');
    expect(explain('verification')).toContain('not started');
    expect(explain('io')).toBeNull();
  });
});
