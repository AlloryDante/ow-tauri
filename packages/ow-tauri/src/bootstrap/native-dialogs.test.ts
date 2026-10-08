import { describe, expect, it } from 'vitest';

import {
  guardNativeDialogs,
  installNativeDialogGuard,
  isDialogPluginOverride,
  isManagedLabel,
  isNativeFunction,
} from './native-dialogs-core.js';

// tauri-plugin-dialog 2.8.1's script, as it assigns the two functions.
function pluginScript(win: Record<string, unknown>): void {
  const n = (cmd: string, args: unknown): Promise<unknown> => Promise.resolve([cmd, args]);
  win['alert'] = function (i: unknown) {
    void n('plugin:dialog|message', { message: String(i) });
  };
  win['confirm'] = async function (i: unknown) {
    return await n('plugin:dialog|confirm', { message: String(i) });
  };
}

function fakeWindow(label?: string): Record<string, unknown> {
  const calls: string[] = [];
  const win: Record<string, unknown> = {
    calls,
    alert: (m: string) => {
      calls.push(`alert:${m}`);
    },
    confirm: (m: string) => {
      calls.push(`confirm:${m}`);
      return true;
    },
    prompt: () => null,
  };
  if (label !== undefined) {
    win['__TAURI_INTERNALS__'] = { metadata: { currentWebview: { label } } };
  }
  return win;
}

const allNative = (): boolean => true;

describe('native dialogs guard (B.2.6)', () => {
  it('keeps the page functions when tauri-plugin-dialog assigns its own', () => {
    const win = fakeWindow();
    const { alert, confirm, prompt } = win;
    expect(guardNativeDialogs(win, allNative)).toEqual(['alert', 'confirm', 'prompt']);
    pluginScript(win);
    expect(win['alert']).toBe(alert);
    expect(win['confirm']).toBe(confirm);
    expect(win['prompt']).toBe(prompt);
    // confirm() stays synchronous: a boolean, not a promise.
    expect((win['confirm'] as (m: string) => unknown)('x')).toBe(true);
    expect(Object.keys(win)).toEqual(expect.arrayContaining(['alert', 'confirm', 'prompt']));
  });

  it("lets the app's own assignment through, as a plain property", () => {
    const win = fakeWindow();
    guardNativeDialogs(win, allNative);
    const mine = (): string => 'mine';
    win['alert'] = mine;
    expect(win['alert']).toBe(mine);
    const descriptor = Object.getOwnPropertyDescriptor(win, 'alert');
    expect(descriptor).toMatchObject({ value: mine, writable: true, configurable: true });
    // Later assignments are ordinary.
    pluginScript(win);
    expect(isDialogPluginOverride(win['alert'])).toBe(true);
  });

  it('leaves functions that are not native, or not configurable, alone', () => {
    const win = fakeWindow();
    Object.defineProperty(win, 'prompt', { value: () => null, configurable: false });
    expect(guardNativeDialogs(win, (v) => v !== win['confirm'])).toEqual(['alert']);
    expect(guardNativeDialogs(fakeWindow())).toEqual([]);
  });

  it('applies to the webviews ow-tauri manages only', () => {
    for (const label of ['ow-main', 'bw-1', 'bwr-12', 'owad-bw-1-1', 'ow-cmp', 'ow-cmp-startup']) {
      expect(isManagedLabel(label)).toBe(true);
    }
    for (const label of ['main', 'ow-main-2', 'bw-', 'bw-x', 'owad-', 'settings', 'ow-cmpx']) {
      expect(isManagedLabel(label)).toBe(false);
    }
    expect(installNativeDialogGuard(fakeWindow('settings'))).toEqual([]);
    expect(installNativeDialogGuard(fakeWindow())).toEqual([]);
  });

  it('tells native functions and the plugin replacement apart', () => {
    expect(isNativeFunction(Math.max)).toBe(true);
    expect(isNativeFunction(() => 1)).toBe(false);
    expect(isNativeFunction('function alert() { [native code] }')).toBe(false);
    const win = fakeWindow();
    pluginScript(win);
    expect(isDialogPluginOverride(win['alert'])).toBe(true);
    expect(isDialogPluginOverride(win['confirm'])).toBe(true);
    expect(isDialogPluginOverride(Math.max)).toBe(false);
  });
});
