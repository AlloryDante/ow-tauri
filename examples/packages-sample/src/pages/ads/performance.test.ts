import { mockOverwolf, settle, type MockOverwolf } from 'tauri-plugin-overwolf-api/testing';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

import { layoutForAds } from '../../test/adview';
import { showPerformanceAd } from './performance';

let overwolf: MockOverwolf;
beforeAll(async () => {
  layoutForAds();
  overwolf = mockOverwolf();
  await import('tauri-plugin-overwolf-api/adview');
});
afterAll(() => {
  overwolf.restore();
});

describe('showPerformanceAd', () => {
  it('appends one performance <owadview> to the body and removes it when the ad ends', async () => {
    const seen: string[] = [];
    const el = showPerformanceAd((name) => seen.push(name));
    expect(el?.parentElement).toBe(document.body);
    expect(el?.hasAttribute('performance')).toBe(true);
    expect(showPerformanceAd(() => undefined)).toBeNull();
    await settle();
    const mount = overwolf.mounts().find((m) => JSON.stringify(m.request).includes('performance'));
    expect(mount).toBeDefined();
    overwolf.emit(mount!.elementId, 'performance_ad_loaded', {});
    overwolf.emit(mount!.elementId, 'shutdown', {});
    await settle();
    expect(seen).toEqual(['performance_ad_loaded', 'shutdown']);
    expect(el?.isConnected).toBe(false);
    expect(document.querySelector('owadview[performance]')).toBeNull();
  });
});
