import { describe, expect, it } from 'vitest';

import type { HostInfo, WindowEvent } from '../shared/ipc.js';
import { WINDOW_EVENT, createShowcaseApi, type ShowcaseDeps } from './showcase-api.js';

const HOME = '/home/me';

const INFO: HostInfo = {
  host: 'ow-tauri',
  hostVersion: '0.1.0',
  engine: 'Tauri 2.12.1',
  platform: 'darwin',
  mode: 'test',
  uid: 'uid',
  cuid: 'cuid',
  muid: 'muid',
  phasePercent: 42,
  productName: 'ow-tauri Ad Showcase',
  appVersion: '1.0.0',
  exportsDir: '~/data/exports',
};

/** Fake deps: commands answer from `answers`; every call is recorded. */
function fake(answers: Record<string, (args?: Record<string, unknown>) => unknown> = {}) {
  const calls: { command: string; args: Record<string, unknown> | undefined }[] = [];
  const handlers = new Map<string, (event: { payload: WindowEvent }) => void>();
  let unlistened = 0;
  const defaults: Record<string, (args?: Record<string, unknown>) => unknown> = {
    showcase_info: () => INFO,
    showcase_paths: () => ({ home: HOME }),
    showcase_write_export: (args) => `~/data/exports/${String(args?.['name'])}`,
    showcase_read_parity: () => ({ path: '~/data/parity-report.json', text: null }),
    showcase_restart: () => null,
    showcase_window_action: () => null,
  };
  const deps: ShowcaseDeps = {
    invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
      calls.push({ command, args });
      const answer = answers[command] ?? defaults[command];
      if (!answer) return Promise.reject(new Error(`unknown command ${command}`));
      try {
        return Promise.resolve(answer(args) as T);
      } catch (error) {
        return Promise.reject(error instanceof Error ? error : new Error(String(error)));
      }
    },
    listen(event: string, handler: (event: { payload: WindowEvent }) => void) {
      handlers.set(event, handler);
      return Promise.resolve(() => {
        unlistened += 1;
        handlers.delete(event);
      });
    },
    isCMPRequired: () => Promise.resolve(true),
    openAdPrivacySettingsWindow: () => Promise.resolve(),
    generateUserEmailHashes: () => Promise.resolve({ sha1: 'a', md5: 'b', sha256: undefined }),
    now: () => new Date('2026-10-09T10:00:00.000Z'),
  };
  return { deps, calls, handlers, unlistened: () => unlistened };
}

describe('createShowcaseApi', () => {
  it('asks the app for host info once', async () => {
    const f = fake();
    const api = createShowcaseApi(f.deps);
    expect(await api.info()).toEqual(INFO);
    await api.info();
    expect(f.calls.filter((c) => c.command === 'showcase_info')).toHaveLength(1);
  });

  it('passes consent and hashes through the plugin, without empty hashes', async () => {
    const api = createShowcaseApi(fake().deps);
    expect(await api.cmpRequired()).toBe(true);
    await expect(api.openPrivacySettings()).resolves.toBeUndefined();
    expect(await api.emailHashes('a@example.com')).toEqual({ sha1: 'a', md5: 'b' });
  });

  it('writes exports under a timestamped name without the home folder', async () => {
    const f = fake();
    const api = createShowcaseApi(f.deps);
    const result = await api.exportTimeline({
      json: JSON.stringify({ url: `file://${HOME}/x.html` }),
    });
    const write = f.calls.find((c) => c.command === 'showcase_write_export');
    expect(write?.args).toEqual({
      name: 'timeline-ow-tauri-test-2026-10-09T10-00-00-000Z.json',
      json: '{"url":"file://~/x.html"}',
    });
    expect(result.path).toBe('~/data/exports/timeline-ow-tauri-test-2026-10-09T10-00-00-000Z.json');
  });

  it('reads the parity report without the home folder', async () => {
    const text = JSON.stringify({ bugs: 0, diffs: [{ class: 'variance', section: `${HOME}/c` }] });
    const api = createShowcaseApi(
      fake({ showcase_read_parity: () => ({ path: '~/data/parity-report.json', text }) }).deps,
    );
    const lookup = await api.parity();
    expect(lookup.path).toBe('~/data/parity-report.json');
    expect(lookup.report?.diffs?.[0]?.section).toBe('~/c');
  });

  it('reports a missing, broken or unreadable parity report', async () => {
    expect(await createShowcaseApi(fake().deps).parity()).toEqual({
      path: '~/data/parity-report.json',
      report: null,
    });
    const broken = await createShowcaseApi(
      fake({ showcase_read_parity: () => ({ path: '~/p.json', text: '{' }) }).deps,
    ).parity();
    expect(broken.report).toBeNull();
    expect(broken.error).toMatch(/JSON|Unexpected|Expected/);
    const failed = await createShowcaseApi(
      fake({
        showcase_read_parity: () => {
          throw new Error(`denied ${HOME}/x`);
        },
      }).deps,
    ).parity();
    expect(failed.error).toBe('Error: denied ~/x');
  });

  it('restarts with the route, and surfaces a refusal', async () => {
    const f = fake();
    await createShowcaseApi(f.deps).restart('live', 'layouts/tower');
    await createShowcaseApi(f.deps).restart('test');
    expect(f.calls.filter((c) => c.command === 'showcase_restart').map((c) => c.args)).toEqual([
      { mode: 'live', route: 'layouts/tower' },
      { mode: 'test', route: null },
    ]);
    const refusing = fake({
      showcase_restart: () => {
        throw new Error('Restart needs a built app');
      },
    });
    await expect(createShowcaseApi(refusing.deps).restart('test')).rejects.toThrow(
      'Restart needs a built app',
    );
  });

  it('runs window actions', async () => {
    const f = fake();
    await createShowcaseApi(f.deps).windowAction('hide-3s');
    expect(f.calls.at(-1)).toEqual({
      command: 'showcase_window_action',
      args: { action: 'hide-3s' },
    });
  });

  it('delivers window events until unsubscribed', async () => {
    const f = fake();
    const seen: WindowEvent[] = [];
    const stop = createShowcaseApi(f.deps).onWindowEvent((e) => seen.push(e));
    await Promise.resolve();
    await Promise.resolve();
    f.handlers.get(WINDOW_EVENT)?.({ payload: { name: 'hide' } });
    stop();
    expect(f.unlistened()).toBe(1);
    f.handlers.get(WINDOW_EVENT)?.({ payload: { name: 'show' } });
    expect(seen).toEqual([{ name: 'hide' }]);
  });

  it('unsubscribes even when stopped before listening started', async () => {
    const f = fake();
    const stop = createShowcaseApi(f.deps).onWindowEvent(() => undefined);
    stop();
    await Promise.resolve();
    await Promise.resolve();
    expect(f.unlistened()).toBe(1);
  });
});
