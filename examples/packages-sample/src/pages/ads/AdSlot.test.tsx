import { mockOverwolf, type MockOverwolf } from 'tauri-plugin-overwolf-api/testing';
import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';

import { layoutForAds } from '../../test/adview';
import { button, click, flush, render } from '../../test/render';
import { AdSlot, CUSTOM_TRACKING } from './AdSlot';

let overwolf: MockOverwolf;
beforeAll(async () => {
  layoutForAds();
  overwolf = mockOverwolf();
  // The <owadview> runtime starts on import, in the mocked Tauri webview.
  await import('tauri-plugin-overwolf-api/adview');
});
afterAll(() => {
  overwolf.restore();
});

describe('AdSlot', () => {
  it('mounts an <owadview> on Start with the slot attributes', async () => {
    const view = await render(<AdSlot name="ad1" cid="sample-ad1" size={[300, 250]} />);
    expect(view.container.querySelector('owadview')).toBeNull();
    const before = overwolf.callsOf('adview_mount').length;
    await click(button(view.container, 'Start ad'));
    await flush();
    const el = view.container.querySelector('owadview');
    expect(el?.getAttribute('cid')).toBe('sample-ad1');
    expect(el?.getAttribute('slotsize')).toBe('300x250');
    expect(el?.getAttribute('customtracking')).toBe(CUSTOM_TRACKING);
    expect(el?.hasAttribute('adstyle')).toBe(false);
    expect(overwolf.callsOf('adview_mount').length).toBe(before + 1);
    expect(button(view.container, 'Start ad').disabled).toBe(true);
    await view.unmount();
  });

  it('logs the ad events with their payload', async () => {
    const view = await render(<AdSlot name="ad1" cid="sample-log" size={[300, 250]} autoStart />);
    await flush();
    const mount = overwolf
      .mounts()
      .find((m) => m.request['attributes'] && JSON.stringify(m.request).includes('sample-log'));
    expect(mount).toBeDefined();
    overwolf.emit(mount!.elementId, 'display_ad_loaded', { slot: 'x' });
    await flush();
    const entry = view.log.entries().find((e) => e.message === 'ad1 300x250: display_ad_loaded');
    expect(entry?.source).toBe('ad');
    expect(entry?.level).toBe('success');
    expect(JSON.stringify(entry?.args)).toContain('"slot":"x"');
    await view.unmount();
  });

  it('removes and recreates the element', async () => {
    const view = await render(<AdSlot name="ad2" cid="sample-rec" size={[160, 600]} autoStart />);
    await flush();
    const first = view.container.querySelector('owadview');
    const unmounts = overwolf.callsOf('adview_unmount').length;
    await click(button(view.container, 'Recreate'));
    await flush();
    const second = view.container.querySelector('owadview');
    expect(second).not.toBeNull();
    expect(second).not.toBe(first);
    expect(overwolf.callsOf('adview_unmount').length).toBe(unmounts + 1);
    await click(button(view.container, 'Remove ad'));
    await flush();
    expect(view.container.querySelector('owadview')).toBeNull();
    expect(overwolf.callsOf('adview_unmount').length).toBe(unmounts + 2);
    expect(view.log.entries().map((e) => e.message)).toEqual(
      expect.arrayContaining(['ad2 160x600: recreate', 'ad2 160x600: remove']),
    );
    await view.unmount();
  });

  it('mutes and unmutes the ad with setAudioMuted', async () => {
    const view = await render(<AdSlot name="ad1" cid="sample-mute" size={[400, 300]} autoStart />);
    await flush();
    const commands = overwolf.callsOf('adview_command').length;
    await click(button(view.container, 'Mute'));
    await flush();
    await click(button(view.container, 'Unmute'));
    await flush();
    const sent = overwolf.callsOf('adview_command').slice(commands);
    expect(sent.map((c) => [c['command'], c['args']])).toEqual([
      ['setAudioMuted', [true]],
      ['setAudioMuted', [false]],
    ]);
    expect(
      view.log.entries().some((e) => e.message === 'ad1 400x300 (video): setAudioMuted(true)'),
    ).toBe(true);
    await view.unmount();
  });

  it('asks for a high impact ad and reports its state', async () => {
    const onHighImpact = vi.fn();
    const view = await render(
      <AdSlot
        name="ad2"
        cid="sample-hi"
        size={[400, 600]}
        highImpact
        autoStart
        onHighImpact={onHighImpact}
      />,
    );
    await flush();
    expect(view.container.querySelector('owadview')?.getAttribute('adstyle')).toBe(
      'high-impact-ad;',
    );
    const mount = overwolf.mounts().find((m) => JSON.stringify(m.request).includes('sample-hi'));
    overwolf.emit(mount!.elementId, 'high-impact-ad-loaded', {});
    overwolf.emit(mount!.elementId, 'high-impact-ad-removed', {});
    await flush();
    expect(onHighImpact.mock.calls).toEqual([[true], [false]]);
    await view.unmount();
  });
});
