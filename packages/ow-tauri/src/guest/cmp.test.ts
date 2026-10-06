import { Window as HappyWindow } from 'happy-dom';
import { afterEach, describe, expect, it } from 'vitest';

import { CMP_COMMAND, consentString, installCmp } from './cmp-core.js';

const CMP_URL = 'https://content.overwolf.com/monsdk/electron/latest/cmp/22.3.27/cmp.html';

const windows: HappyWindow[] = [];
afterEach(async () => {
  for (const w of windows.splice(0)) await w.happyDOM.close();
});

function consentWindow(url = CMP_URL, config: unknown = { adOptimization: false }) {
  const happy = new HappyWindow({ url });
  windows.push(happy);
  const win = happy as unknown as Window;
  const sent: { command: string; args: Record<string, unknown> }[] = [];
  Reflect.set(win, '__TAURI_INTERNALS__', {
    invoke: (command: string, args: Record<string, unknown>) => {
      sent.push({ command, args });
      return Promise.resolve();
    },
  });
  const installed = installCmp(win, config);
  return { win, sent, installed };
}

interface CmpGlobals {
  cmp: { saveConsent: (v: unknown) => void; saveUnifiedConsent: (v: unknown) => void };
  privacy: {
    enableAdOptimization: (v: unknown) => Promise<void>;
    getIsAdOptimizationEnabled: () => Promise<boolean>;
  };
}

describe('installCmp (D.6.6)', () => {
  it('bridges the consent page API to cmp_event', async () => {
    const { win, sent, installed } = consentWindow();
    expect(installed).toBe(true);
    const g = win as unknown as CmpGlobals & Window;
    g.cmp.saveUnifiedConsent(encodeURIComponent('cmp=T&ac=2~1'));
    g.cmp.saveConsent({ tcString: 'TCFONLY', cmpId: 1 });
    g.cmp.saveConsent(42);
    expect(await g.privacy.getIsAdOptimizationEnabled()).toBe(false);
    await g.privacy.enableAdOptimization(true);
    expect(await g.privacy.getIsAdOptimizationEnabled()).toBe(true);
    win.close();
    expect(sent.every((s) => s.command === CMP_COMMAND)).toBe(true);
    expect(sent.map((s) => s.args)).toEqual([
      { name: 'ready' },
      { name: 'saveUnifiedConsent', data: { consent: 'cmp%3DT%26ac%3D2~1' } },
      { name: 'saveConsent', data: { consent: 'TCFONLY' } },
      { name: 'enableAdOptimization', data: { enabled: true } },
      { name: 'close' },
    ]);
  });

  it('defines frozen globals whose functions have length 0', () => {
    const { win } = consentWindow();
    const g = win as unknown as CmpGlobals;
    expect(Object.isFrozen(g.cmp)).toBe(true);
    expect(Object.isFrozen(g.privacy)).toBe(true);
    for (const f of [g.cmp.saveConsent, g.cmp.saveUnifiedConsent, g.privacy.enableAdOptimization]) {
      expect(f.length).toBe(0);
      expect(Object.isFrozen(f)).toBe(true);
    }
    expect('overwolf' in win).toBe(false);
  });

  it('reads the stored ad-optimisation value', async () => {
    const { win } = consentWindow(CMP_URL, { adOptimization: true });
    expect(await (win as unknown as CmpGlobals).privacy.getIsAdOptimizationEnabled()).toBe(true);
  });

  it('is inert off content.overwolf.com and on a second run', () => {
    for (const url of [
      'https://www.overwolf.com/x',
      'https://evil.example/',
      'http://content.overwolf.com/',
    ]) {
      const { win, sent, installed } = consentWindow(url);
      expect(installed, url).toBe(false);
      expect('cmp' in win).toBe(false);
      expect(sent).toEqual([]);
    }
    const { win } = consentWindow();
    expect(installCmp(win, {})).toBe(false);
  });

  it('reduces consent arguments to strings', () => {
    expect(consentString('CQ')).toBe('CQ');
    expect(consentString({ tcString: 'TC' })).toBe('TC');
    expect(consentString({})).toBe('');
    expect(consentString(null)).toBe('');
    expect(consentString('x'.repeat(16 * 1024 + 1))).toBe('');
  });
});
