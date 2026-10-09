import { mockOverwolf, type MockOverwolf } from 'tauri-plugin-overwolf-api/testing';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { button, click, flush, render } from '../../test/render';
import { Updater } from './Updater';

let overwolf: MockOverwolf;
beforeEach(() => {
  overwolf = mockOverwolf();
});
afterEach(() => {
  overwolf.restore();
});

/** Sends `message` on the channel the page passed as `onEvent`. */
function send(channel: unknown, index: number, message: unknown): void {
  const internals = Reflect.get(globalThis, '__TAURI_INTERNALS__') as {
    runCallback(id: number, data: unknown): void;
  };
  internals.runCallback((channel as { id: number }).id, { message, index });
}

describe('Updater', () => {
  it('shows "unsupported" plainly with the reason', async () => {
    overwolf.setCommand('updater_check', () => {
      // eslint-disable-next-line @typescript-eslint/only-throw-error -- the plugin rejects with wire objects
      throw { code: 'unsupported', message: 'the updater runs on Windows only' };
    });
    const view = await render(<Updater />);
    await click(button(view.container, 'Check for updates'));
    await flush();
    const status = view.container.querySelector('[role="status"]')?.textContent ?? '';
    expect(status).toContain('unsupported: the updater runs on Windows only');
    expect(status).toContain('Windows only');
    expect(view.log.entries().some((e) => e.message === 'check() failed: unsupported')).toBe(true);
    await view.unmount();
  });

  it('says up to date when the feed has nothing newer', async () => {
    const view = await render(<Updater />);
    await click(button(view.container, 'Check for updates'));
    await flush();
    expect(view.container.textContent).toContain('Up to date.');
    expect(overwolf.callsOf('updater_check')).toEqual([{ options: null }]);
    await view.unmount();
  });

  it('downloads an update and follows its progress events', async () => {
    overwolf.setCommand('updater_check', () => ({
      rid: 7,
      version: '1.1.0',
      currentVersion: '1.0.0',
      body: 'Fixes',
      raw: {},
    }));
    let finish: (() => void) | undefined;
    overwolf.setCommand('updater_download_and_install', (args) => {
      send(args['onEvent'], 0, { event: 'Started', data: { contentLength: 200 } });
      send(args['onEvent'], 1, { event: 'Progress', data: { chunkLength: 50 } });
      return new Promise((resolve) => {
        finish = () => {
          send(args['onEvent'], 2, { event: 'Progress', data: { chunkLength: 150 } });
          send(args['onEvent'], 3, { event: 'Finished' });
          resolve(null);
        };
      });
    });
    const view = await render(<Updater />);
    await click(button(view.container, 'Check for updates'));
    await flush();
    expect(view.container.textContent).toContain('Version 1.1.0 is available (running 1.0.0)');
    expect(view.container.querySelector('.notes')?.textContent).toBe('Fixes');
    await click(button(view.container, 'Download and install'));
    await flush();
    expect(overwolf.callsOf('updater_download_and_install')[0]?.['rid']).toBe(7);
    expect(view.container.textContent).toContain('50 bytes of 200');
    expect(view.container.querySelector('progress')?.getAttribute('value')).toBe('25');
    finish?.();
    await flush();
    expect(view.container.textContent).toContain('starting the installer');
    expect(view.container.querySelector('progress')?.getAttribute('value')).toBe('100');
    await view.unmount();
  });

  it('shows a failed download', async () => {
    overwolf.setCommand('updater_check', () => ({
      rid: 1,
      version: '2.0.0',
      currentVersion: '1.0.0',
      raw: {},
    }));
    overwolf.setCommand('updater_download_and_install', () => {
      // eslint-disable-next-line @typescript-eslint/only-throw-error -- the plugin rejects with wire objects
      throw { code: 'verification', message: 'bad signature' };
    });
    const view = await render(<Updater />);
    await click(button(view.container, 'Check for updates'));
    await flush();
    await click(button(view.container, 'Download and install'));
    await flush();
    expect(view.container.textContent).toContain('verification: bad signature');
    await view.unmount();
  });
});
