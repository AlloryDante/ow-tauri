import { act } from 'react';
import { mockOverwolf, DEFAULT_INFO, type MockOverwolf } from 'tauri-plugin-overwolf-api/testing';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

import { App } from './App';
import { layoutForAds } from './test/adview';
import { button, click, flush, render } from './test/render';

let overwolf: MockOverwolf;
beforeAll(async () => {
  layoutForAds();
  overwolf = mockOverwolf();
  await import('tauri-plugin-overwolf-api/adview');
});
afterAll(() => {
  overwolf.restore();
  window.location.hash = '';
});

async function go(hash: string): Promise<void> {
  await act(async () => {
    window.location.hash = hash;
    window.dispatchEvent(new HashChangeEvent('hashchange'));
    await Promise.resolve();
  });
  await flush();
}

describe('App', () => {
  it('shows the host and switches pages with the URL hash', async () => {
    window.location.hash = '';
    const view = await render(<App info={Promise.resolve(DEFAULT_INFO)} />);
    await flush();
    expect(view.container.querySelector('.host-version')?.textContent).toBe(
      'tauri 2.12.1 · test ads',
    );
    expect(view.container.querySelector('h2')?.textContent).toBe('Logger');
    expect(view.container.querySelector('[aria-current="page"]')?.textContent).toBe('Logger');
    for (const [hash, title] of [
      ['#settings', 'CMP & Settings'],
      ['#updater', 'Updater'],
      ['#packages', 'Packages'],
      ['#ads', 'Ads Tester'],
    ] as const) {
      await go(hash);
      expect(view.container.querySelector('h2')?.textContent).toBe(title);
    }
    expect(view.container.querySelectorAll('[data-package]')).toHaveLength(0);
    await go('#packages');
    expect(view.container.querySelectorAll('[data-package]')).toHaveLength(4);
    expect(view.container.textContent).toContain('not available on Tauri');
    await view.unmount();
  });

  it('runs the ads tester: layouts, slots and the performance ad', async () => {
    await go('#ads');
    const view = await render(<App info={Promise.resolve(null)} />);
    await flush();
    const slots = (): string[] =>
      [...view.container.querySelectorAll('.ad-container')].map(
        (c) => c.getAttribute('aria-label') ?? '',
      );
    expect(slots()).toEqual(['ad1 160x600 ad slot', 'ad2 400x600 ad slot']);

    const select = view.container.querySelector<HTMLSelectElement>('.select-layout select')!;
    await act(async () => {
      select.value = 'studio-right';
      select.dispatchEvent(new Event('change', { bubbles: true }));
      await Promise.resolve();
    });
    expect(slots()).toEqual(['ad1 728x90 ad slot', 'ad2 400x300 (video) ad slot']);

    await act(async () => {
      select.value = 'tower-plus-high-impact';
      select.dispatchEvent(new Event('change', { bubbles: true }));
      await Promise.resolve();
    });
    const hi = view.container.querySelector('[data-slot="ad2"]')!;
    await click(button(hi, 'Start ad'));
    await flush();
    expect(hi.querySelector('owadview')?.getAttribute('adstyle')).toBe('high-impact-ad;');
    const mount = overwolf.mounts().at(-1)!;
    overwolf.emit(mount.elementId, 'high-impact-ad-loaded', {});
    await flush();
    const cells = [...view.container.querySelectorAll<HTMLElement>('.slot-cell')];
    expect(cells.map((c) => [c.hidden, c.classList.contains('expanded')])).toEqual([
      [true, false],
      [false, true],
    ]);
    await click(button(hi, 'Remove ad'));
    expect(view.container.querySelectorAll<HTMLElement>('.slot-cell')[0]?.hidden).toBe(false);

    await click(button(view.container, 'Performance ad'));
    await click(button(view.container, 'Performance ad'));
    expect(document.querySelectorAll('owadview[performance]')).toHaveLength(1);
    const panel = view.container.querySelector('[aria-label="Ad events"]')?.textContent ?? '';
    expect(panel).toContain('performance ad: appended to the page');
    expect(panel).toContain('performance ad: one is already showing');
    document.querySelector('owadview[performance]')?.remove();
    await view.unmount();
  });
});
