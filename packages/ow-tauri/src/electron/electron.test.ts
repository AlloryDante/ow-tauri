import { afterEach, describe, expect, it, vi } from 'vitest';

import { OwTauriError, OwTauriUnsupportedError } from '../shared/errors.js';
import type { Display, HostMessage } from '../shared/protocol.js';
import {
  defaultSnapshot,
  mockHost,
  setHostContext,
  settle,
  type MockHost,
  type MockHostOptions,
} from '../testing/index.js';
import electron, {
  BrowserWindow,
  Menu,
  Tray,
  WebContents,
  app,
  clipboard,
  contextBridge,
  crashReporter,
  dialog,
  globalShortcut,
  ipcMain,
  ipcRenderer,
  nativeTheme,
  process as processShim,
  screen,
  shell,
} from './index.js';
import { completeDisplay } from './screen.js';
import { parseColor, windowHooks } from './browser-window.js';
import { normalizeAccelerator } from './shell-dialog.js';
import { toAssetPath } from './runtime.js';
import { copyForBridge, deepFreeze } from './context-bridge.js';

let host: MockHost;

async function start(options: MockHostOptions = {}): Promise<MockHost> {
  host = mockHost(options);
  await settle();
  host.clearCalls();
  return host;
}

function windowEvent(id: number, event: string, extra: Record<string, unknown> = {}): HostMessage {
  return { type: 'window', id, event, ...extra };
}

function names(): string[] {
  return host.calls.map((c) => c.command);
}

afterEach(() => {
  setHostContext(null);
  host.dispose();
  vi.restoreAllMocks();
});

describe('app (B.2.1)', () => {
  it('emits ready once the host is ready and resolves whenReady', async () => {
    host = mockHost();
    const ready = vi.fn();
    app.on('ready', ready);
    expect(app.isReady()).toBe(false);
    await app.whenReady();
    await settle();
    expect(app.isReady()).toBe(true);
    expect(ready).toHaveBeenCalledTimes(1);
  });

  it('reads paths, name, version and locale from the snapshot', async () => {
    await start();
    expect(app.getAppPath()).toBe('/app');
    expect(app.getPath('userData')).toBe('/data/Test App');
    expect(app.getPath('temp')).toBe('/tmp');
    expect(() => app.getPath('nope')).toThrow("Failed to get 'nope' path");
    expect(() => app.getPath('module')).toThrow(OwTauriUnsupportedError);
    app.setPath('logs', '/elsewhere');
    expect(app.getPath('logs')).toBe('/elsewhere');
    expect(app.getVersion()).toBe('1.0.0');
    expect(typeof app.getName()).toBe('string');
    app.setName('Renamed');
    expect(app.getName()).toBe('Renamed');
    expect(app.name).toBe('Renamed');
    app.name = 'Again';
    expect(app.getName()).toBe('Again');
    expect(app.isPackaged).toBe(false);
    expect(app.getLocale()).toBe('en-US');
    expect(typeof app.getSystemLocale()).toBe('string');
  });

  it('maps quit, exit, relaunch and focus to commands', async () => {
    await start();
    app.quit();
    app.exit(3);
    app.relaunch({ args: ['--x'] });
    app.focus({ steal: true });
    await settle();
    expect(host.callsOf('app_quit')).toHaveLength(1);
    expect(host.callsOf('app_exit')[0]).toEqual({ code: 3 });
    expect(host.callsOf('app_relaunch')[0]).toEqual({ args: ['--x'] });
    expect(host.callsOf('app_focus')[0]).toEqual({ steal: true });
    expect(() => {
      app.relaunch({ execPath: '/x' });
    }).toThrow(OwTauriUnsupportedError);
  });

  it('answers before-quit and will-quit with preventDefault', async () => {
    await start();
    app.on('before-quit', (event: { preventDefault(): void }) => {
      event.preventDefault();
    });
    const quit = vi.fn();
    app.on('quit', quit);
    host.push(
      { type: 'lifecycle', event: 'before-quit', requestId: 7 },
      { type: 'lifecycle', event: 'will-quit', requestId: 8 },
    );
    host.push({ type: 'lifecycle', event: 'quit', exitCode: 2 });
    await settle();
    expect(host.callsOf('app_quit_reply')).toEqual([
      { requestId: 7, prevent: true },
      { requestId: 8, prevent: false },
    ]);
    expect(quit).toHaveBeenCalledWith(expect.anything(), 2);
  });

  it('emits second-instance and activate', async () => {
    await start();
    const second = vi.fn();
    const activate = vi.fn();
    app.on('second-instance', second);
    app.on('activate', activate);
    host.push({
      type: 'lifecycle',
      event: 'second-instance',
      argv: ['a', '--b'],
      cwd: '/w',
      additionalData: { k: 1 },
    });
    host.push({ type: 'lifecycle', event: 'activate', hasVisibleWindows: false });
    host.push({ type: 'lifecycle', event: 'mystery' });
    await settle();
    expect(second).toHaveBeenCalledWith(expect.anything(), ['a', '--b'], '/w', { k: 1 });
    expect(activate).toHaveBeenCalledWith(expect.anything(), false);
    expect(app.requestSingleInstanceLock()).toBe(true);
    expect(app.hasSingleInstanceLock()).toBe(true);
    app.releaseSingleInstanceLock();
  });

  it('reads switches and records appended ones', async () => {
    await start({
      snapshot: {
        switches: {
          argv: ['app', '--foo=bar', '--flag', '--port', '9', '--', '--after=1'],
          testAd: false,
        },
      },
    });
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    expect(app.commandLine.hasSwitch('foo')).toBe(true);
    expect(app.commandLine.getSwitchValue('foo')).toBe('bar');
    expect(app.commandLine.getSwitchValue('flag')).toBe('');
    // Chromium: a separate argument is not a switch value; nothing after `--` is a switch.
    expect(app.commandLine.getSwitchValue('port')).toBe('');
    expect(app.commandLine.hasSwitch('port')).toBe(true);
    expect(app.commandLine.hasSwitch('after')).toBe(false);
    expect(app.commandLine.hasSwitch('none')).toBe(false);
    app.commandLine.appendSwitch('disable-gpu');
    app.commandLine.appendSwitch('lang', 'de');
    app.commandLine.appendArgument('extra');
    expect(app.commandLine.hasSwitch('disable-gpu')).toBe(true);
    expect(app.commandLine.getSwitchValue('lang')).toBe('de');
    app.commandLine.removeSwitch('lang');
    expect(app.commandLine.hasSwitch('lang')).toBe(false);
    app.disableHardwareAcceleration();
    app.setAppUserModelId('x');
    expect(warn).toHaveBeenCalled();
  });

  it('throws for unsupported members and in UI windows', async () => {
    await start();
    expect(() => {
      (app as unknown as { getGPUInfo(): void }).getGPUInfo();
    }).toThrow(OwTauriUnsupportedError);
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    expect((app as unknown as { dock: unknown }).dock).toBeUndefined();
    expect(warn).toHaveBeenCalled();
    setHostContext('ui');
    expect(() => {
      app.quit();
    }).toThrow(OwTauriError);
  });

  it('quits when the last window closes and nobody listens to window-all-closed', async () => {
    await start();
    const win = new BrowserWindow({ show: false });
    await win.whenCreated();
    host.push(windowEvent(1, 'closed'));
    await settle();
    expect(host.callsOf('app_quit')).toHaveLength(1);
  });
});

describe('BrowserWindow (B.2.2)', () => {
  it('creates a window with a synchronous id and forwards options and preload', async () => {
    await start();
    const created = vi.fn();
    app.on('browser-window-created', created);
    const win = new BrowserWindow({
      width: 400,
      height: 300,
      show: false,
      title: 'T',
      icon: '/app/icon.png',
      webPreferences: { preload: '/app/preload/index.js', devTools: true, contextIsolation: true },
    });
    expect(win.id).toBe(1);
    expect(created).toHaveBeenCalledWith(expect.anything(), win);
    expect(BrowserWindow.fromId(1)).toBe(win);
    expect(BrowserWindow.getAllWindows()).toEqual([win]);
    expect(BrowserWindow.fromWebContents(win.webContents)).toBe(win);
    expect(win instanceof BrowserWindow).toBe(true);
    expect(win.webContents instanceof WebContents).toBe(true);
    await win.whenCreated();
    const [request] = host.callsOf('window_create');
    expect(request).toMatchObject({
      preload: 'preload/index.js',
      windowClass: 'ui',
      options: {
        width: 400,
        height: 300,
        show: false,
        title: 'T',
        icon: 'icon.png',
        webPreferences: { devTools: true },
      },
    });
    expect(win.isVisible()).toBe(false);
    expect(win.getBounds()).toMatchObject({ width: 400, height: 300 });
    expect(win.getTitle()).toBe('T');
  });

  it('rejects unsupported options and methods', async () => {
    await start();
    expect(() => new BrowserWindow({ titleBarStyle: 'hidden' })).toThrow(OwTauriUnsupportedError);
    expect(() => new BrowserWindow({ titleBarStyle: 'default', show: false })).not.toThrow();
    const win = BrowserWindow.getAllWindows()[0]!;
    expect(() => {
      (win as unknown as { setOpacity(v: number): void }).setOpacity(0.5);
    }).toThrow(OwTauriUnsupportedError);
    expect(() => {
      (win.webContents as unknown as { print(): void }).print();
    }).toThrow(OwTauriUnsupportedError);
    setHostContext('ui');
    expect(() => new BrowserWindow()).toThrow(OwTauriError);
  });

  it('updates the cache at once and sends window commands in order', async () => {
    await start();
    const win = new BrowserWindow({ show: false });
    win.show();
    expect(win.isVisible()).toBe(true);
    expect(win.isFocused()).toBe(true);
    win.setBounds({ x: 10, y: 20, width: 300, height: 200 });
    expect(win.getBounds()).toEqual({ x: 10, y: 20, width: 300, height: 200 });
    win.setSize(320, 240);
    expect(win.getSize()).toEqual([320, 240]);
    win.setPosition(5, 6);
    expect(win.getPosition()).toEqual([5, 6]);
    win.setTitle('Hello');
    win.setAlwaysOnTop(true);
    expect(win.isAlwaysOnTop()).toBe(true);
    win.minimize();
    expect(win.isMinimized()).toBe(true);
    win.restore();
    win.maximize();
    expect(win.isMaximized()).toBe(true);
    win.unmaximize();
    win.setResizable(false);
    expect(win.isResizable()).toBe(false);
    win.setBackgroundColor('#112233');
    win.hide();
    expect(win.isVisible()).toBe(false);
    await win.whenCreated();
    await settle();
    const windowCommands = names().filter((n) => n.startsWith('plugin:window|'));
    expect(windowCommands.slice(0, 2)).toEqual(['plugin:window|show', 'plugin:window|set_focus']);
    expect(windowCommands).toContain('plugin:window|set_title');
    expect(windowCommands.at(-1)).toBe('plugin:window|hide');
    expect(host.calls.find((c) => c.command === 'plugin:window|set_title')?.args).toEqual({
      label: 'bw-1',
      value: 'Hello',
    });
  });

  it('resolves loadFile on did-finish-load and rejects loadURL on did-fail-load', async () => {
    await start();
    const win = new BrowserWindow({ show: false });
    const loaded = win.loadFile('/app/index.html', { query: { a: '1' }, hash: '#top' });
    await settle();
    expect(host.callsOf('window_load')[0]).toEqual({
      id: 1,
      target: { kind: 'file', path: 'index.html', query: { a: '1' }, hash: 'top' },
    });
    expect(win.webContents.isLoading()).toBe(true);
    const finished = vi.fn();
    win.webContents.on('did-finish-load', finished);
    host.push(windowEvent(1, 'did-finish-load', { data: { url: 'app://index.html' } }));
    await loaded;
    expect(finished).toHaveBeenCalled();
    expect(win.webContents.getURL()).toBe('app://index.html');

    const failing = win.loadURL('https://example.com/');
    host.push(
      windowEvent(1, 'did-fail-load', {
        data: {
          errorCode: -105,
          errorDescription: 'ERR_NAME_NOT_RESOLVED',
          validatedURL: 'https://example.com/',
        },
      }),
    );
    await expect(failing).rejects.toMatchObject({ code: 'io' });
  });

  it('answers close with preventDefault and cleans up on closed', async () => {
    await start();
    app.on('window-all-closed', () => undefined);
    const win = new BrowserWindow({ show: false });
    await win.whenCreated();
    win.on('close', (event: { preventDefault(): void }) => {
      event.preventDefault();
    });
    const closed = vi.fn();
    win.on('closed', closed);
    win.close();
    host.push(windowEvent(1, 'close', { requestId: 4 }));
    await settle();
    expect(names()).toContain('plugin:window|close');
    expect(host.callsOf('window_close_reply')[0]).toEqual({ id: 1, requestId: 4, prevent: true });
    host.push(windowEvent(1, 'closed'));
    await settle();
    expect(closed).toHaveBeenCalled();
    expect(win.isDestroyed()).toBe(true);
    expect(BrowserWindow.getAllWindows()).toEqual([]);
    expect(() => win.loadURL('https://x.test/')).toThrow('Object has been destroyed');
    expect(host.callsOf('app_quit')).toHaveLength(0);
  });

  it('tracks focus, window state events and the app focus events', async () => {
    await start();
    const focus = vi.fn();
    const blur = vi.fn();
    app.on('browser-window-focus', focus);
    app.on('browser-window-blur', blur);
    const win = new BrowserWindow({ show: false });
    await win.whenCreated();
    host.push(windowEvent(1, 'focus'));
    host.push(windowEvent(1, 'resize', { data: { bounds: { x: 1, y: 2, width: 3, height: 4 } } }));
    host.push(windowEvent(1, 'maximize'));
    await settle();
    expect(BrowserWindow.getFocusedWindow()).toBe(win);
    expect(focus).toHaveBeenCalledWith(expect.anything(), win);
    expect(win.getBounds()).toEqual({ x: 1, y: 2, width: 3, height: 4 });
    expect(win.isMaximized()).toBe(true);
    host.push(windowEvent(1, 'blur'));
    await settle();
    expect(BrowserWindow.getFocusedWindow()).toBeNull();
    expect(blur).toHaveBeenCalled();
  });

  it('adopts windows created by the host and holds early events', async () => {
    let release: () => void = () => undefined;
    await start({
      commands: {
        window_create: () =>
          new Promise((resolve) => {
            release = () => {
              resolve({ id: 5, label: 'bw-5' });
            };
          }),
      },
    });
    const win = new BrowserWindow({ show: false });
    const shown = vi.fn();
    win.on('show', shown);
    host.push(windowEvent(5, 'show'));
    await settle();
    expect(shown).not.toHaveBeenCalled();
    release();
    await win.whenCreated();
    await settle();
    expect(shown).toHaveBeenCalled();
    host.push(windowEvent(9, 'created', { data: { options: {} } }));
    await settle();
    expect(BrowserWindow.getAllWindows()).toHaveLength(2);
  });

  it('sends to the window, evaluates code and handles window.open', async () => {
    await start({ commands: { window_eval: () => ({ $otj: 'undefined' }) } });
    const win = new BrowserWindow({ show: false });
    win.webContents.send('ping', 1, new Map([[1, 2]]));
    await settle();
    expect(host.callsOf('ipc_emit')[0]).toMatchObject({ target: 1, channel: 'ping', seq: 1 });
    await expect(win.webContents.executeJavaScript('1 + 1')).resolves.toBeUndefined();
    expect(host.callsOf('window_eval')[0]).toEqual({ id: 1, code: '1 + 1', wantResult: true });
    win.webContents.setWindowOpenHandler(({ url }) =>
      url.includes('ok') ? { action: 'allow' } : { action: 'deny' },
    );
    host.push(windowEvent(1, 'new-window', { data: { url: 'https://ok.test/' } }));
    host.push(windowEvent(1, 'new-window', { data: { url: 'https://no.test/' } }));
    await settle();
    expect(host.callsOf('shell_open_external')).toEqual([{ url: 'https://ok.test/' }]);
    win.webContents.openDevTools();
    expect(win.webContents.isDevToolsOpened()).toBe(true);
    win.webContents.setZoomFactor(1.5);
    expect(win.webContents.getZoomFactor()).toBe(1.5);
    await settle();
    expect(host.callsOf('window_devtools')[0]).toEqual({ id: 1, open: true });
    expect(host.callsOf('plugin:webview|set_webview_zoom')).toEqual([
      { label: 'bw-1', value: 1.5 },
    ]);
  });

  it('targets the bwr- webview after a remote load (A.2.3.1)', async () => {
    await start();
    const win = new BrowserWindow({ show: false });
    const local = win.loadURL(`${location.origin}/index.html`);
    win.webContents.setZoomFactor(1.25);
    const remote = win.loadURL('https://example.test/page');
    win.webContents.setZoomFactor(2);
    await settle();
    host.push(windowEvent(1, 'did-finish-load'));
    await Promise.all([local, remote]);
    expect(host.callsOf('plugin:webview|set_webview_zoom')).toEqual([
      { label: 'bw-1', value: 1.25 },
      { label: 'bwr-1', value: 2 },
    ]);
  });

  it('routes webContents.ipc and ipcMain handlers', async () => {
    await start();
    const win = new BrowserWindow({ show: false });
    await win.whenCreated();
    win.webContents.ipc.handle('scoped', () => 'from-scope');
    ipcMain.handle('global', (event) =>
      event.sender === win.webContents ? 'sender-ok' : 'sender-wrong',
    );
    host.push(
      {
        type: 'ipc',
        kind: 'invoke',
        id: 1,
        channel: 'scoped',
        args: [],
        sender: { windowId: 1, url: 'app://index.html' },
      } as HostMessage,
      {
        type: 'ipc',
        kind: 'invoke',
        id: 2,
        channel: 'global',
        args: [],
        sender: { windowId: 1, url: 'app://index.html' },
      } as HostMessage,
    );
    await settle();
    const replies = host
      .callsOf('ipc_reply')
      .sort((a, b) => (a['id'] as number) - (b['id'] as number));
    expect(replies.map((r) => r['value'])).toEqual(['from-scope', 'sender-ok']);
    ipcMain.removeHandler('global');
  });

  it('covers the remaining window members', async () => {
    await start();
    const win = new BrowserWindow({
      show: true,
      minWidth: 100,
      minHeight: 50,
      x: 3,
      y: 4,
      backgroundColor: '#000',
    });
    expect(win.isFocused()).toBe(true);
    expect(win.getContentBounds()).toEqual(win.getBounds());
    win.setContentSize(500, 400);
    expect(win.getContentSize()).toEqual([500, 400]);
    expect(win.getMinimumSize()).toEqual([100, 50]);
    win.setMinimumSize(0, 0);
    win.setMaximumSize(900, 800);
    expect(win.getMaximumSize()).toEqual([900, 800]);
    win.center();
    expect(win.getPosition()).toEqual([710, 320]);
    win.setMovable(false);
    expect(win.isMovable()).toBe(false);
    win.setMinimizable(false);
    expect(win.isMinimizable()).toBe(false);
    win.setMaximizable(false);
    expect(win.isMaximizable()).toBe(false);
    win.setClosable(false);
    expect(win.isClosable()).toBe(false);
    win.setFocusable(false);
    expect(win.isFocusable()).toBe(false);
    win.setSkipTaskbar(true);
    win.setIgnoreMouseEvents(true, { forward: true });
    win.setFullScreen(true);
    expect(win.isFullScreen()).toBe(true);
    win.setFullScreen(false);
    expect(win.getBackgroundColor()).toMatch(/^#/);
    win.setProgressBar(0.5);
    win.setProgressBar(-1);
    win.flashFrame(true);
    win.setVisibleOnAllWorkspaces(true);
    win.setContentProtection(true);
    win.moveTop();
    win.showInactive();
    win.focus();
    win.blur();
    expect(win.isFocused()).toBe(false);
    win.setMenu();
    win.removeMenu();
    win.setMenuBarVisibility();
    win.setAutoHideMenuBar();
    win.startDragging();
    win.webContents.closeDevTools();
    win.webContents.toggleDevTools();
    win.webContents.reload();
    expect(win.webContents.getTitle()).toBe('');
    expect(win.webContents.isDestroyed()).toBe(false);
    await win.whenCreated();
    await settle();
    const commands = new Set(names());
    for (const name of [
      'set_size',
      'set_min_size',
      'set_max_size',
      'center',
      'set_skip_taskbar',
      'set_ignore_cursor_events',
      'set_fullscreen',
    ]) {
      expect(commands.has(`plugin:window|${name}`)).toBe(true);
    }
    expect(host.calls.find((c) => c.command === 'plugin:window|set_min_size')?.args).toEqual({
      label: 'bw-1',
      value: null,
    });
    win.destroy();
    await settle();
    expect(host.callsOf('window_destroy')).toEqual([{ id: 1 }]);
  });

  it('emits contents events and reports failed creation', async () => {
    await start({
      commands: { window_create: () => Promise.reject({ code: 'io', message: 'no display' }) },
    });
    const allClosed = vi.fn();
    app.on('window-all-closed', allClosed);
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const win = new BrowserWindow({ show: false });
    const loaded = win.loadFile('index.html');
    await expect(win.whenCreated()).rejects.toMatchObject({ code: 'io' });
    await expect(loaded).rejects.toMatchObject({ code: 'io' });
    expect(win.isDestroyed()).toBe(true);
    expect(BrowserWindow.getAllWindows()).toEqual([]);
    expect(error).toHaveBeenCalled();
    // The window never existed: no window-all-closed, and never an implicit quit.
    app.removeAllListeners('window-all-closed');
    const lone = new BrowserWindow({ show: false });
    await expect(lone.whenCreated()).rejects.toMatchObject({ code: 'io' });
    await settle();
    expect(allClosed).not.toHaveBeenCalled();
    expect(host.callsOf('app_quit')).toEqual([]);

    await start();
    const live = new BrowserWindow({ show: false });
    await live.whenCreated();
    const events = vi.fn();
    for (const name of ['dom-ready', 'render-process-gone', 'will-navigate', 'destroyed'])
      live.webContents.on(name, events);
    host.push(windowEvent(1, 'dom-ready'));
    host.push(windowEvent(1, 'render-process-gone', { data: { exitCode: 9 } }));
    host.push(windowEvent(1, 'will-navigate', { data: { url: 'https://x.test/' } }));
    host.push(windowEvent(1, 'mystery'));
    host.push(windowEvent(99, 'show'));
    await settle();
    expect(events).toHaveBeenCalledWith(expect.anything(), { reason: 'crashed', exitCode: 9 });
    expect(events).toHaveBeenCalledWith(expect.anything(), 'https://x.test/');
    host.push(windowEvent(1, 'closed'));
    await settle();
    expect(events).toHaveBeenCalledTimes(4);
    expect(live.webContents.isDestroyed()).toBe(true);
    expect(() => {
      live.webContents.send('x');
    }).toThrow('Object has been destroyed');
  });

  it('keeps parent and modal for a child created in the same tick as its parent', async () => {
    let release: () => void = () => undefined;
    let next = 1;
    await start({
      commands: {
        window_create: () => {
          const id = next++;
          if (id > 1) return { id, label: `bw-${String(id)}` };
          return new Promise((resolve) => {
            release = () => {
              resolve({ id, label: `bw-${String(id)}` });
            };
          });
        },
      },
    });
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const main = new BrowserWindow({ show: false });
    const child = new BrowserWindow({ parent: main, modal: true, show: false });
    await settle();
    expect(host.callsOf('window_create')).toHaveLength(1);
    release();
    await child.whenCreated();
    expect(host.callsOf('window_create')[1]).toMatchObject({
      options: { parentId: 1, modal: true },
    });
    expect(warn).not.toHaveBeenCalled();
  });

  it('shares one registry between two copies of the package (ADR 0012)', async () => {
    await start();
    vi.resetModules();
    const copy = await import('./index.js');
    expect(copy.BrowserWindow).not.toBe(BrowserWindow);
    expect(copy.app).toBe(app);
    const other = new copy.BrowserWindow({ show: false });
    await other.whenCreated();
    const focus = vi.fn();
    other.on('focus', focus);
    expect(BrowserWindow.fromId(other.id)).toBe(other);
    expect(other instanceof BrowserWindow).toBe(true);
    host.push(windowEvent(1, 'focus'));
    host.push(windowEvent(1, 'close', { requestId: 3 }));
    await settle();
    expect(focus).toHaveBeenCalledTimes(1);
    expect(host.callsOf('window_close_reply')).toEqual([{ id: 1, requestId: 3, prevent: false }]);
    const loaded = other.loadFile('index.html');
    await settle();
    host.push(windowEvent(1, 'did-finish-load'));
    await expect(loaded).resolves.toBeUndefined();
  });

  it('quits by default and answers closes of unknown windows without the app module', async () => {
    await start();
    const hooks = windowHooks();
    const saved = hooks.allClosed;
    delete hooks.allClosed;
    try {
      const win = new BrowserWindow({ show: false });
      await win.whenCreated();
      host.push(windowEvent(42, 'close', { requestId: 8 }));
      host.push(windowEvent(1, 'closed'));
      await settle();
      expect(host.callsOf('window_close_reply')).toEqual([
        { id: 42, requestId: 8, prevent: false },
      ]);
      expect(host.callsOf('app_quit')).toEqual([{}]);
    } finally {
      if (saved) hooks.allClosed = saved;
    }
  });

  it('parses colours and asset paths', () => {
    expect(parseColor('#fff')).toEqual([255, 255, 255, 255]);
    expect(parseColor('#80112233')).toEqual([0x11, 0x22, 0x33, 0x80]);
    expect(parseColor('nope')).toBeNull();
    expect(toAssetPath('file:///app/a%20b/./c/../d.html', '/app')).toBe('a b/d.html');
    expect(toAssetPath('C:\\app\\x.js', 'C:\\app')).toBe('x.js');
    expect(toAssetPath('rel/x.js', undefined)).toBe('rel/x.js');
  });
});

const second: Display = {
  id: 2,
  label: 'Display 2',
  bounds: { x: 1920, y: 0, width: 1280, height: 720 },
  workArea: { x: 1920, y: 0, width: 1280, height: 700 },
  scaleFactor: 2,
  nativeOrigin: { x: 1920, y: 0 },
};

describe('screen (B.2.5)', () => {
  it('serves displays synchronously with Electron defaults', async () => {
    await start({ snapshot: { displays: [...defaultSnapshot().displays, second] } });
    const all = screen.getAllDisplays();
    expect(all).toHaveLength(2);
    expect(all[0]).toMatchObject({
      id: 1,
      size: { width: 1920, height: 1080 },
      rotation: 0,
      internal: false,
    });
    expect(screen.getPrimaryDisplay().id).toBe(1);
    expect(screen.getDisplayNearestPoint({ x: 2000, y: 10 }).id).toBe(2);
    expect(screen.getDisplayNearestPoint({ x: 99999, y: 10 }).id).toBe(2);
    expect(screen.getDisplayMatching({ x: 1900, y: 0, width: 400, height: 100 }).id).toBe(2);
    expect(screen.getDisplayMatching({ x: -5000, y: -5000, width: 1, height: 1 }).id).toBe(1);
    expect(screen.dipToScreenPoint({ x: 1930, y: 10 })).toEqual({ x: 1940, y: 20 });
    expect(screen.screenToDipPoint({ x: 1940, y: 20 })).toEqual({ x: 1930, y: 10 });
    expect(screen.dipToScreenRect(null, { x: 1930, y: 10, width: 100, height: 50 })).toEqual({
      x: 1940,
      y: 20,
      width: 200,
      height: 100,
    });
    expect(screen.screenToDipRect(null, { x: 1940, y: 20, width: 200, height: 100 })).toEqual({
      x: 1930,
      y: 10,
      width: 100,
      height: 50,
    });
    setHostContext('ui');
    expect(() => screen.getAllDisplays()).toThrow(OwTauriError);
  });

  it('emits display events from state patches', async () => {
    await start();
    const added = vi.fn();
    const removed = vi.fn();
    const changed = vi.fn();
    screen.on('display-added', added);
    screen.on('display-removed', removed);
    screen.on('display-metrics-changed', changed);
    const first = defaultSnapshot().displays[0]!;
    host.push({
      type: 'state',
      seq: 1,
      patches: [{ path: 'displays', value: [{ ...first, scaleFactor: 1.5 }, second] }],
    });
    host.push({ type: 'state', seq: 2, patches: [{ path: 'displays', value: [first] }] });
    await settle();
    expect(added).toHaveBeenCalledWith(expect.anything(), expect.objectContaining({ id: 2 }));
    expect(changed).toHaveBeenCalledWith(expect.anything(), expect.objectContaining({ id: 1 }), [
      'scaleFactor',
    ]);
    expect(removed).toHaveBeenCalledWith(expect.anything(), expect.objectContaining({ id: 2 }));
  });

  it('primes the cursor when ready and refreshes it at most every 100 ms', async () => {
    let cursor = { x: 12, y: 34 };
    await start({ commands: { screen_snapshot: () => ({ cursor }) } });
    // Fetched once when the app became ready (start() cleared that call).
    expect(screen.getCursorScreenPoint()).toEqual({ x: 12, y: 34 });
    cursor = { x: 5, y: 6 };
    await settle();
    expect(host.callsOf('screen_snapshot')).toHaveLength(0);
    vi.spyOn(Date, 'now').mockReturnValue(Date.now() + 1000);
    expect(screen.getCursorScreenPoint()).toEqual({ x: 12, y: 34 });
    await settle();
    expect(screen.getCursorScreenPoint()).toEqual({ x: 5, y: 6 });
    expect(host.callsOf('screen_snapshot')).toHaveLength(1);
  });

  it('seeds the cursor from the snapshot when the host provides one', async () => {
    await start({
      snapshot: { cursor: { x: 7, y: 8 } },
      commands: {
        screen_snapshot: () => {
          throw { code: 'io', message: 'no cursor' };
        },
      },
    });
    vi.spyOn(console, 'debug').mockImplementation(() => undefined);
    expect(screen.getCursorScreenPoint()).toEqual({ x: 7, y: 8 });
  });

  it('completes partial displays', () => {
    expect(completeDisplay(second).nativeOrigin).toEqual({ x: 1920, y: 0 });
    expect(
      completeDisplay({ ...second, nativeOrigin: undefined } as unknown as Display).nativeOrigin,
    ).toEqual({ x: 3840, y: 0 });
  });
});

describe('shell, dialog, globalShortcut (B.2.5)', () => {
  it('maps shell members to commands', async () => {
    await start({
      commands: {
        shell_open_path: (args) => (args['path'] === '/bad' ? 'refused' : ''),
        shell_show_item_in_folder: () => Promise.reject(new Error('io')),
      },
    });
    await shell.openExternal('https://example.com/');
    expect(await shell.openPath('/ok')).toBe('');
    expect(await shell.openPath('/bad')).toBe('refused');
    shell.showItemInFolder('/x');
    await settle();
    expect(host.callsOf('shell_open_external')).toEqual([{ url: 'https://example.com/' }]);
    expect(() => {
      (shell as unknown as { beep(): void }).beep();
    }).toThrow(OwTauriUnsupportedError);
    setHostContext('ui');
    await expect(shell.openExternal('https://example.com/')).rejects.toMatchObject({
      code: 'forbidden',
    });
  });

  it('maps dialogs to commands with the parent window id', async () => {
    await start({
      commands: {
        dialog_open: () => ({ canceled: false, filePaths: ['/a'] }),
        dialog_save: () => ({ canceled: true, filePath: '' }),
        dialog_message: () => ({ response: 1, checkboxChecked: true }),
      },
    });
    const win = new BrowserWindow({ show: false });
    await win.whenCreated();
    await expect(dialog.showOpenDialog(win, { properties: ['openFile'] })).resolves.toEqual({
      canceled: false,
      filePaths: ['/a'],
    });
    await expect(dialog.showSaveDialog({ title: 's' })).resolves.toEqual({
      canceled: true,
      filePath: '',
    });
    await expect(dialog.showMessageBox({ message: 'm', buttons: ['a', 'b'] })).resolves.toEqual({
      response: 1,
      checkboxChecked: true,
    });
    await expect(
      dialog.showMessageBox({ message: 'm', buttons: ['a', 'b', 'c', 'd'] }),
    ).rejects.toMatchObject({ code: 'invalid-argument' });
    dialog.showErrorBox('T', 'C');
    await settle();
    expect(host.callsOf('dialog_open')[0]).toEqual({ properties: ['openFile'], windowId: 1 });
    expect(host.callsOf('dialog_message')[1]).toEqual({ type: 'error', title: 'T', message: 'C' });
    expect(() => {
      (dialog as unknown as { showMessageBoxSync(): void }).showMessageBoxSync();
    }).toThrow(OwTauriUnsupportedError);
  });

  it('parents a dialog to a window constructed in the same tick', async () => {
    await start({ commands: { dialog_message: () => ({ response: 0 }) } });
    const win = new BrowserWindow({ show: false });
    await dialog.showMessageBox(win, { message: 'hi' });
    expect(host.callsOf('dialog_message')).toEqual([{ message: 'hi', windowId: 1 }]);
  });

  it('registers shortcuts and dispatches presses', async () => {
    await start({
      commands: { global_shortcut_register: (args) => args['accelerator'] !== 'Bad+Key' },
    });
    const pressed = vi.fn();
    expect(globalShortcut.register('Ctrl+K', pressed)).toBe(true);
    expect(globalShortcut.register('Ctrl+K', pressed)).toBe(false);
    globalShortcut.registerAll(['Bad+Key'], pressed);
    await settle();
    expect(globalShortcut.isRegistered('Ctrl+K')).toBe(true);
    expect(globalShortcut.isRegistered('Bad+Key')).toBe(false);
    expect(host.callsOf('global_shortcut_register')[0]).toEqual({ accelerator: 'Ctrl+K', id: 1 });
    host.push({ type: 'global-shortcut', id: 1, accelerator: 'Ctrl+K', state: 'pressed' });
    host.push({ type: 'global-shortcut', id: 1, accelerator: 'Ctrl+K', state: 'released' });
    await settle();
    expect(pressed).toHaveBeenCalledTimes(1);
    globalShortcut.unregister('Ctrl+K');
    globalShortcut.unregister('Ctrl+K');
    globalShortcut.unregisterAll();
    await settle();
    expect(host.callsOf('global_shortcut_unregister')).toEqual([{ accelerator: 'Ctrl+K' }, {}]);
    expect(globalShortcut.isRegistered('Ctrl+K')).toBe(false);
  });

  it('treats accelerator aliases as one shortcut and allows a retry after a failure', async () => {
    let refuse = true;
    await start({ commands: { global_shortcut_register: () => !refuse } });
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    expect(globalShortcut.register('CmdOrCtrl+Shift+X', vi.fn())).toBe(true);
    await settle();
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('refused'));
    expect(globalShortcut.isRegistered('CmdOrCtrl+Shift+X')).toBe(false);
    refuse = false;
    expect(globalShortcut.register('CmdOrCtrl+Shift+X', vi.fn())).toBe(true);
    await settle();
    expect(globalShortcut.isRegistered('shift+commandorcontrol+x')).toBe(true);
    expect(globalShortcut.register('CommandOrControl+Shift+X', vi.fn())).toBe(false);
    globalShortcut.unregister('Shift+CommandOrControl+X');
    await settle();
    expect(host.callsOf('global_shortcut_unregister')).toEqual([
      { accelerator: 'CmdOrCtrl+Shift+X' },
    ]);
    expect(normalizeAccelerator('Option+Meta+Ctrl++')).toBe('control+alt+super+plus');
    expect(normalizeAccelerator('Cmd+A')).toBe(normalizeAccelerator('command+a'));
  });
});

describe('other modules (B.2.5)', () => {
  it('stubs unsupported modules and warns on crashReporter.start', async () => {
    await start();
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    crashReporter.start({ submitURL: 'x' });
    expect(warn).toHaveBeenCalled();
    expect(() => {
      (crashReporter as unknown as { getLastCrashReport(): void }).getLastCrashReport();
    }).toThrow(OwTauriUnsupportedError);
    expect(() => new (Tray as new () => unknown)()).toThrow(OwTauriUnsupportedError);
    expect(() => {
      (Menu as unknown as { buildFromTemplate(t: unknown[]): void }).buildFromTemplate([]);
    }).toThrow(/Menu\.buildFromTemplate/);
    expect(() => {
      (clipboard as unknown as { writeText(t: string): void }).writeText('x');
    }).toThrow(OwTauriUnsupportedError);
    expect((clipboard as { then?: unknown }).then).toBeUndefined();
    expect('writeText' in clipboard).toBe(false);
  });

  it('reports the theme and process shim', async () => {
    await start();
    const updated = vi.fn();
    nativeTheme.on('updated', updated);
    expect(typeof nativeTheme.shouldUseDarkColors).toBe('boolean');
    nativeTheme.themeSource = 'dark';
    expect(nativeTheme.shouldUseDarkColors).toBe(true);
    nativeTheme.themeSource = 'dark';
    nativeTheme.themeSource = 'light';
    expect(nativeTheme.shouldUseDarkColors).toBe(false);
    expect(nativeTheme.themeSource).toBe('light');
    expect(nativeTheme.shouldUseHighContrastColors).toBe(false);
    expect(nativeTheme.shouldUseInvertedColorScheme).toBe(false);
    expect(updated).toHaveBeenCalledTimes(2);
    nativeTheme.themeSource = 'system';
    expect(['win32', 'darwin', 'linux']).toContain(processShim.platform);
    expect(Object.isFrozen(processShim)).toBe(true);
  });

  it('exposes the module object', () => {
    expect(electron.app).toBe(app);
    expect(electron.ipcMain).toBe(ipcMain);
    expect(electron.ipcRenderer).toBe(ipcRenderer);
    expect(electron.screen).toBe(screen);
    expect(electron.Tray).toBe(Tray);
  });
});

describe('contextBridge (B.2.4)', () => {
  it('exposes a frozen read-only API in UI windows', async () => {
    await start({ label: 'bw-1' });
    const send = vi.fn();
    contextBridge.exposeInMainWorld('bridgeTest', {
      send,
      nested: { list: [1, { deep: true }] },
      when: new Date(0),
    });
    const exposed = (
      globalThis as unknown as Record<
        string,
        { send: () => void; nested: { list: unknown[] }; when: Date }
      >
    )['bridgeTest']!;
    exposed.send();
    expect(send).toHaveBeenCalled();
    expect(Object.isFrozen(exposed)).toBe(true);
    expect(Object.isFrozen(exposed.nested.list[1])).toBe(true);
    expect(Object.isFrozen(exposed.when)).toBe(false);
    const descriptor = Object.getOwnPropertyDescriptor(globalThis, 'bridgeTest');
    expect(descriptor?.writable).toBe(false);
    expect(descriptor?.configurable).toBe(false);
    expect(() => {
      contextBridge.exposeInMainWorld('bridgeTest', {});
    }).toThrow('Cannot bind an API on top of an existing property on the window object');
    expect(() => {
      contextBridge.exposeInMainWorld('', {});
    }).toThrow(OwTauriError);
    expect(() => {
      (contextBridge as unknown as { exposeInIsolatedWorld(): void }).exposeInIsolatedWorld();
    }).toThrow(OwTauriUnsupportedError);
    setHostContext('main');
    expect(() => {
      contextBridge.exposeInMainWorld('bridgeOther', {});
    }).toThrow(OwTauriError);
  });

  it('exposes a copy, so the preload keeps its own objects mutable', async () => {
    await start({ label: 'bw-1' });
    const config = { level: 1, list: [1] };
    contextBridge.exposeInMainWorld('bridgeCopy', { config });
    config.level = 2;
    config.list.push(2);
    const exposed = (globalThis as unknown as Record<string, { config: typeof config }>)[
      'bridgeCopy'
    ]!;
    expect(exposed.config).toEqual({ level: 1, list: [1] });
    expect(Object.isFrozen(exposed.config)).toBe(true);
    const shared = { n: 1 };
    const bare: Record<string, unknown> = Object.create(null) as Record<string, unknown>;
    bare['a'] = shared;
    bare['b'] = shared;
    const copied = copyForBridge(bare);
    expect(copied['a']).toBe(copied['b']);
    expect(copied['a']).not.toBe(shared);
    expect(Object.getPrototypeOf(copied)).toBeNull();
  });

  it('deep-freezes cycles once', () => {
    const a: Record<string, unknown> = {};
    a['self'] = a;
    expect(deepFreeze(a)).toBe(a);
    expect(Object.isFrozen(a)).toBe(true);
    expect(deepFreeze(3)).toBe(3);
  });
});
