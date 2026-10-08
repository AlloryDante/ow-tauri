import { describe, expect, it } from 'vitest';

import { SILENT_DIALOGS, silenceDialogs } from './dialogs.js';

function fakeWindow(): Window & Record<string, unknown> {
  return {
    alert: () => {
      throw new Error('a dialog was shown');
    },
    confirm: () => true,
    prompt: () => 'typed',
  } as unknown as Window & Record<string, unknown>;
}

describe('silenceDialogs', () => {
  it('answers alert, confirm and prompt at once without showing anything', () => {
    const win = fakeWindow();
    expect(silenceDialogs(win)).toEqual(['alert', 'confirm', 'prompt']);
    const alert: (message: string) => unknown = win.alert;
    expect(alert('hi')).toBeUndefined();
    expect(win.confirm('sure?')).toBe(false);
    expect(win.prompt('name?', 'x')).toBeNull();
    expect(SILENT_DIALOGS).toEqual({ alert: undefined, confirm: false, prompt: null });
  });

  it('installs native-looking, frozen functions that the page can still replace', () => {
    const win = fakeWindow();
    silenceDialogs(win);
    expect(win.alert.name).toBe('');
    expect(Object.isFrozen(win.alert)).toBe(true);
    const descriptor = Object.getOwnPropertyDescriptor(win, 'confirm');
    expect(descriptor).toMatchObject({ writable: true, configurable: true, enumerable: true });
  });

  it('works on a real window (its dialogs come from the prototype)', () => {
    expect(silenceDialogs(window)).toEqual(['alert', 'confirm', 'prompt']);
    expect(window.confirm('x')).toBe(false);
  });

  it('leaves a non-configurable dialog alone', () => {
    const win = fakeWindow();
    const original = win.prompt;
    Object.defineProperty(win, 'prompt', { value: original, configurable: false });
    expect(silenceDialogs(win)).toEqual(['alert', 'confirm']);
    expect(win.prompt).toBe(original);
  });

  it('keeps the dialogs of a frame that refuses the property', () => {
    const win = new Proxy(fakeWindow(), {
      defineProperty: () => {
        throw new TypeError('refused');
      },
    });
    expect(silenceDialogs(win)).toEqual([]);
  });
});
