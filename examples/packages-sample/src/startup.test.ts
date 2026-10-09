import { mockOverwolf, DEFAULT_INFO } from 'tauri-plugin-overwolf-api/testing';
import { describe, expect, it } from 'vitest';

import { createLogStore } from './log/store';
import { startup } from './startup';

describe('startup', () => {
  it('logs the launch, getInfo() and isCMPRequired() and returns the info', async () => {
    const overwolf = mockOverwolf({ cmpRequired: true });
    const log = createLogStore();
    expect(await startup(log)).toEqual(DEFAULT_INFO);
    expect(log.entries().map((e) => [e.source, e.message])).toEqual([
      ['app', 'ow-tauri Packages Sample started'],
      ['app', 'getInfo()'],
      ['app', 'getInfo() →'],
      ['app', 'isCMPRequired()'],
      ['app', 'isCMPRequired() →'],
    ]);
    expect(log.entries()[4]?.args).toEqual([true]);
    overwolf.restore();
  });

  it('returns null when getInfo() fails', async () => {
    const overwolf = mockOverwolf({
      commands: {
        get_info: () => {
          throw new Error('get_info not allowed');
        },
      },
    });
    const log = createLogStore();
    expect(await startup(log)).toBeNull();
    expect(log.entries().some((e) => e.message === 'getInfo() failed: forbidden')).toBe(true);
    overwolf.restore();
  });
});
