import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { OwTauriError } from '../shared/errors.js';
import {
  browserChromeEnvironment,
  installWindowChrome,
  type ChromeEnvironment,
  type ChromeServices,
} from './window-chrome.js';

interface FakeServices extends ChromeServices {
  calls: { name: string; args: Record<string, unknown> | undefined }[];
  fail: Record<string, unknown>;
  logs: string[];
  warnings: string[];
}

function fakeServices(context: ChromeServices['context'] = 'ui'): FakeServices {
  const services: FakeServices = {
    context,
    calls: [],
    fail: {},
    logs: [],
    warnings: [],
    command: async (name, args) => {
      services.calls.push({ name, args });
      if (name in services.fail) throw services.fail[name];
      return await Promise.resolve(null);
    },
    raw: async (name, args) => {
      services.calls.push({ name, args });
      if (name in services.fail) throw services.fail[name];
      return await Promise.resolve(null);
    },
    log: (level, message) => {
      services.logs.push(`${level}: ${message}`);
    },
    warnOnce: (key, message) => {
      services.warnings.push(`${key}: ${message}`);
    },
  };
  return services;
}

let services: FakeServices;
let navigated: string[];
let top: boolean;
let href: string;
let navListeners: (() => void)[];
let remove: () => void = () => undefined;

function environment(overrides: Partial<ChromeEnvironment> = {}): ChromeEnvironment {
  return {
    document,
    platform: 'darwin',
    appRegion: true,
    region: (el) => el.getAttribute('data-region') ?? '',
    isTop: () => top,
    origin: () => 'http://localhost:3000',
    navigate: (url) => navigated.push(url),
    href: () => href,
    onSameDocumentNavigation: (listener) => {
      navListeners.push(listener);
      return () => {
        navListeners = navListeners.filter((l) => l !== listener);
      };
    },
    ...overrides,
  };
}

function navigateInPage(url: string): void {
  href = url;
  for (const listener of navListeners) listener();
}

function install(overrides: Partial<ChromeEnvironment> = {}): void {
  remove = installWindowChrome(services, environment(overrides));
}

async function tick(): Promise<void> {
  for (let i = 0; i < 3; i++) await new Promise<void>((resolve) => setTimeout(resolve, 0));
}

function press(el: Element, init: MouseEventInit = {}): MouseEvent {
  const event = new MouseEvent('mousedown', {
    bubbles: true,
    cancelable: true,
    button: 0,
    ...init,
  });
  el.dispatchEvent(event);
  return event;
}

function click(el: Element, init: MouseEventInit = {}): MouseEvent {
  const event = new MouseEvent('click', { bubbles: true, cancelable: true, button: 0, ...init });
  el.dispatchEvent(event);
  return event;
}

beforeEach(() => {
  // A link click the runtime leaves alone navigates the happy-dom window.
  (window as unknown as { happyDOM: { setURL(url: string): void } }).happyDOM.setURL(
    'http://localhost:3000/',
  );
  services = fakeServices();
  navigated = [];
  top = true;
  href = 'http://localhost:3000/index.html';
  navListeners = [];
  document.head.innerHTML = '';
  document.body.innerHTML = '';
});

afterEach(() => {
  remove();
  remove = () => undefined;
  document.head.innerHTML = '';
  document.body.innerHTML = '';
});

describe('app-region dragging (B.3.6)', () => {
  it('starts a drag on a drag region and toggles maximize on a double-click', async () => {
    install();
    document.body.innerHTML =
      '<header data-region="drag"><span id="title">x</span><div data-region="no-drag"><i id="menu"></i></div><button id="close">x</button></header><main id="body"></main>';
    press(document.getElementById('title') as Element);
    press(document.getElementById('title') as Element, { detail: 2 });
    press(document.getElementById('title') as Element, { button: 2 });
    press(document.getElementById('menu') as Element);
    press(document.getElementById('close') as Element);
    press(document.getElementById('body') as Element);
    await tick();
    expect(services.calls.map((c) => c.name)).toEqual([
      'plugin:window|start_dragging',
      'plugin:window|toggle_maximize',
    ]);
  });

  it('finds regions across a shadow root and text nodes', async () => {
    install();
    document.body.innerHTML =
      '<div id="host" data-region="drag"></div><p data-region="drag">text</p>';
    const host = document.getElementById('host')!;
    const inner = document.createElement('span');
    host.attachShadow({ mode: 'open' }).append(inner);
    press(inner, { composed: true });
    const text = (document.querySelector('p') as HTMLElement).firstChild as Text;
    text.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0 }));
    await tick();
    expect(services.calls.map((c) => c.name)).toEqual([
      'plugin:window|start_dragging',
      'plugin:window|start_dragging',
    ]);
  });

  it('warns once when the window may not drag, and does nothing without engine support', async () => {
    services.fail['plugin:window|start_dragging'] = new Error('not allowed');
    install();
    document.body.innerHTML = '<header data-region="drag"></header>';
    const header = document.querySelector('header') as Element;
    press(header);
    press(header);
    await tick();
    expect(services.warnings).toHaveLength(2);
    expect(services.warnings[0]).toContain('start_dragging failed');
    remove();
    services = fakeServices();
    install({ appRegion: false });
    press(header);
    await tick();
    expect(services.calls).toEqual([]);
  });
});

describe('external links on macOS and Linux (A.2.3.1)', () => {
  it('sends top-frame clicks on external http(s) links to navigation_external', async () => {
    install();
    document.body.innerHTML = [
      '<a id="ext" href="https://example.com/a?b=1">go</a>',
      '<a id="local" href="/page">local</a>',
      '<a id="mail" href="mailto:x@example.com">mail</a>',
      '<a id="blank" href="https://example.com/" target="_blank">blank</a>',
      '<a id="self" href="https://example.com/self" target="_TOP">self</a>',
      '<a id="dl" href="https://example.com/f" download>file</a>',
      '<a id="bad" href="http://[">bad</a>',
      '<a id="nohref">none</a>',
    ].join('');
    // happy-dom runs the activation of a link's descendant before the click
    // reaches the document (browsers run it after dispatch), so the tests
    // click links directly.
    const event = click(document.getElementById('ext') as Element);
    expect(event.defaultPrevented).toBe(true);
    for (const id of ['local', 'mail', 'blank', 'dl', 'bad', 'nohref'])
      expect(click(document.getElementById(id) as Element).defaultPrevented).toBe(false);
    expect(click(document.getElementById('self') as Element).defaultPrevented).toBe(true);
    expect(
      click(document.getElementById('ext') as Element, { metaKey: true }).defaultPrevented,
    ).toBe(false);
    expect(click(document.getElementById('ext') as Element, { button: 1 }).defaultPrevented).toBe(
      false,
    );
    const handled = new MouseEvent('click', { bubbles: true, cancelable: true });
    handled.preventDefault();
    document.getElementById('ext')?.dispatchEvent(handled);
    top = false;
    expect(click(document.getElementById('ext') as Element).defaultPrevented).toBe(false);
    await tick();
    expect(services.calls).toEqual([
      { name: 'navigation_external', args: { url: 'https://example.com/a?b=1' } },
      { name: 'navigation_external', args: { url: 'https://example.com/self' } },
    ]);
    expect(navigated).toEqual([]);
  });

  it('honours <base target> and leaves the navigation alone on Windows', async () => {
    document.head.innerHTML = '<base target="_blank">';
    install();
    document.body.innerHTML = '<a id="ext" href="https://example.com/">x</a>';
    expect(click(document.getElementById('ext') as Element).defaultPrevented).toBe(false);
    remove();
    document.head.innerHTML = '';
    install({ platform: 'win32' });
    expect(click(document.getElementById('ext') as Element).defaultPrevented).toBe(false);
    await tick();
    expect(services.calls).toEqual([]);
  });

  it('falls back to navigating when the plugin cannot open the URL, but not when it refuses it', async () => {
    services.fail['navigation_external'] = new OwTauriError('backend', 'unknown command');
    install();
    document.body.innerHTML = '<a id="ext" href="https://example.com/">x</a>';
    click(document.getElementById('ext') as Element);
    await tick();
    expect(navigated).toEqual(['https://example.com/']);
    expect(services.warnings[0]).toContain('navigation_external failed');
    services.fail['navigation_external'] = new OwTauriError('invalid-argument', 'blocked url');
    click(document.getElementById('ext') as Element);
    await tick();
    expect(navigated).toHaveLength(1);
    expect(services.logs.join('\n')).toContain('blocked url');
  });

  it('intercepts external form submissions with the GET query or the POST action', async () => {
    install();
    document.body.innerHTML = [
      '<form id="get" action="https://example.com/search"><input name="q" value="a b"><button id="go" name="s" value="1">go</button></form>',
      '<form id="post" method="post" action="https://example.com/post"><input name="x" value="1"></form>',
      '<form id="local" action="/local"></form>',
      '<form id="dialog" method="dialog" action="https://example.com/"></form>',
      '<form id="blank" target="_blank" action="https://example.com/"></form>',
      '<form id="over" action="/local"><button id="ob" formaction="https://example.com/o" formmethod="post">o</button></form>',
    ].join('');
    const submit = (id: string, submitter?: HTMLElement | null): boolean => {
      const event = new Event('submit', { bubbles: true, cancelable: true }) as SubmitEvent;
      Object.defineProperty(event, 'submitter', { value: submitter ?? null });
      document.getElementById(id)?.dispatchEvent(event);
      return event.defaultPrevented;
    };
    expect(submit('get', document.getElementById('go'))).toBe(true);
    expect(submit('post')).toBe(true);
    expect(submit('local')).toBe(false);
    expect(submit('dialog')).toBe(false);
    expect(submit('blank')).toBe(false);
    expect(submit('over', document.getElementById('ob'))).toBe(true);
    top = false;
    expect(submit('post')).toBe(false);
    const div = document.createElement('div');
    document.body.append(div);
    div.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await tick();
    expect(services.calls.map((c) => c.args?.['url'])).toEqual([
      'https://example.com/search?q=a+b&s=1',
      'https://example.com/post',
      'https://example.com/o',
    ]);
  });

  it('falls back to a native submission or navigation for forms', async () => {
    services.fail['navigation_external'] = new Error('unknown command');
    install();
    document.body.innerHTML =
      '<form id="get" action="https://example.com/s"><input name="q" value="1"></form><form id="post" method="post" action="https://example.com/p"></form>';
    const native = vi
      .spyOn(HTMLFormElement.prototype, 'submit')
      .mockImplementation(() => undefined);
    for (const id of ['get', 'post'])
      document
        .getElementById(id)
        ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await tick();
    expect(navigated).toEqual(['https://example.com/s?q=1']);
    expect(native).toHaveBeenCalledTimes(1);
  });
});

describe('installWindowChrome', () => {
  it('does nothing outside UI windows', async () => {
    services = fakeServices('main');
    install();
    document.body.innerHTML = '<a id="ext" href="https://example.com/" data-region="drag">x</a>';
    press(document.getElementById('ext') as Element);
    expect(click(document.getElementById('ext') as Element).defaultPrevented).toBe(false);
    await tick();
    expect(services.calls).toEqual([]);
  });

  it('reports each in-page navigation of the top document once', async () => {
    install({ appRegion: false, platform: 'win32' });
    navigateInPage('http://localhost:3000/index.html#/');
    navigateInPage('http://localhost:3000/index.html#/');
    navigateInPage('http://localhost:3000/settings?tab=1');
    await tick();
    expect(services.calls).toEqual([
      { name: 'navigation_in_page', args: { url: 'http://localhost:3000/index.html#/' } },
      { name: 'navigation_in_page', args: { url: 'http://localhost:3000/settings?tab=1' } },
    ]);
    remove();
    expect(navListeners).toEqual([]);
  });

  it('reports no in-page navigation from a subframe, and survives a refused report', async () => {
    top = false;
    install({ appRegion: false, platform: 'win32' });
    expect(navListeners).toEqual([]);
    remove();
    top = true;
    services.fail['navigation_in_page'] = new OwTauriError('forbidden', 'no capability');
    install({ appRegion: false, platform: 'win32' });
    navigateInPage('http://localhost:3000/index.html#/a');
    await tick();
    expect(services.warnings).toEqual([
      'window-chrome:navigation_in_page: navigation_in_page failed (the window capability may lack it): no capability',
    ]);
  });

  it('hears every same-document navigation of the current document', () => {
    const env = browserChromeEnvironment('darwin');
    const heard: string[] = [];
    const stop = env.onSameDocumentNavigation(() => heard.push(env.href()));
    window.history.pushState({}, '', '/pushed');
    window.history.replaceState({}, '', '/replaced#x');
    window.dispatchEvent(new Event('hashchange'));
    window.dispatchEvent(new Event('popstate'));
    expect(heard).toEqual([
      'http://localhost:3000/pushed',
      'http://localhost:3000/replaced#x',
      'http://localhost:3000/replaced#x',
      'http://localhost:3000/replaced#x',
    ]);
    stop();
    window.history.pushState({}, '', '/after');
    window.dispatchEvent(new Event('hashchange'));
    expect(heard).toHaveLength(4);
    expect(Object.hasOwn(window.history, 'pushState')).toBe(false);
  });

  it('reads the platform hooks of the current document', () => {
    const env = browserChromeEnvironment('darwin');
    expect(env.platform).toBe('darwin');
    expect(typeof env.appRegion).toBe('boolean');
    expect(env.isTop()).toBe(true);
    expect(env.origin()).toBe(window.location.origin);
    const el = document.createElement('div');
    document.body.append(el);
    expect(env.region(el)).toBe('');
    const assign = vi.spyOn(window.location, 'assign').mockImplementation(() => undefined);
    env.navigate('https://example.com/');
    expect(assign).toHaveBeenCalledWith('https://example.com/');
  });
});
