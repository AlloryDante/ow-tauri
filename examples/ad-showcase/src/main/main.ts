/**
 * The showcase's main process, written against the Electron API only. On
 * ow-electron `electron` is the real module; on ow-tauri the bundler aliases
 * it to `ow-tauri/electron` and this file runs unchanged in the plugin's
 * hidden main webview. The one per-host piece is `#host` (file access and
 * version strings, see `host/contract.ts`).
 *
 * It opens one window (1280x860, the size that meets every format's
 * documented minimum) and answers the window's `showcase:*` requests with
 * `app.overwolf` and the window API.
 *
 * @packageDocumentation
 */
import { app, BrowserWindow, ipcMain } from 'electron';
import { host } from '#host';
import { authorName, exportFileName, formulaUid, relaunchArgs } from '../shared/identity.js';
import { PAGE_SWITCH, formatRoute, parseRoute, withPageSwitch } from '../shared/route.js';
import {
  Channel,
  type AdMode,
  type ExportResult,
  type HostInfo,
  type ParityLookup,
  type ParityReport,
  type WindowAction,
  type WindowEvent,
} from '../shared/ipc.js';

/** The window size and its minimum. */
const SIZE = { width: 1280, height: 860, minWidth: 1100, minHeight: 700 } as const;

/** Test ads when launched with `--test-ad` (both hosts read the same switch). */
const mode: AdMode = app.commandLine.hasSwitch('test-ad') ? 'test' : 'live';

let mainWindow: BrowserWindow | null = null;

/**
 * Joins path segments with the separator `base` already uses, so the result
 * is a native absolute path on Windows and on POSIX hosts alike.
 */
function joinPath(base: string, ...parts: string[]): string {
  const sep = base.includes('\\') ? '\\' : '/';
  return [base.replace(/[\\/]+$/, ''), ...parts].join(sep);
}

/** Sends a window state change to the window, if it is still open. */
function report(event: WindowEvent): void {
  if (mainWindow && !mainWindow.isDestroyed()) {
    mainWindow.webContents.send(Channel.windowEvent, event);
  }
}

async function hostInfo(): Promise<HostInfo> {
  const manifestText = await host.readText(joinPath(app.getAppPath(), 'package.json'));
  const manifest = (manifestText ? JSON.parse(manifestText) : {}) as Record<string, unknown>;
  const productName =
    typeof manifest['productName'] === 'string' && manifest['productName'] !== ''
      ? manifest['productName']
      : typeof manifest['name'] === 'string'
        ? manifest['name']
        : app.getName();
  const overwolf = app.overwolf;
  return {
    host: host.name,
    ...host.versions(),
    platform: process.platform,
    mode,
    uid: overwolf.uid,
    cuid: await formulaUid(authorName(manifest['author']), productName),
    muid: overwolf.muid,
    phasePercent: overwolf.phasePercent,
    productName: app.getName(),
    appVersion: app.getVersion(),
    exportsDir: joinPath(app.getPath('userData'), 'exports'),
  };
}

async function readParity(): Promise<ParityLookup> {
  const path = joinPath(app.getPath('userData'), 'parity-report.json');
  try {
    const text = await host.readText(path);
    return { path, report: text === null ? null : (JSON.parse(text) as ParityReport) };
  } catch (error) {
    return { path, report: null, error: String(error) };
  }
}

function runWindowAction(win: BrowserWindow, action: WindowAction): void {
  switch (action) {
    case 'hide-3s':
      win.hide();
      setTimeout(() => {
        if (!win.isDestroyed()) win.show();
      }, 3000);
      break;
    case 'minimize-3s':
      win.minimize();
      setTimeout(() => {
        if (!win.isDestroyed()) win.restore();
      }, 3000);
      break;
    case 'shrink-900x500':
      // Below the interstitial's minimum: the ad page answers with
      // performance_ad_error, then shutdown [OBS].
      win.setMinimumSize(800, 450);
      win.setSize(900, 500);
      break;
    case 'restore-size':
      win.setSize(SIZE.width, SIZE.height);
      win.setMinimumSize(SIZE.minWidth, SIZE.minHeight);
      break;
  }
}

const WINDOW_ACTIONS: ReadonlySet<string> = new Set<WindowAction>([
  'hide-3s',
  'minimize-3s',
  'shrink-900x500',
  'restore-size',
]);

function registerIpc(): void {
  ipcMain.handle(Channel.info, () => hostInfo());
  ipcMain.handle(Channel.cmpRequired, () => app.overwolf.isCMPRequired());
  ipcMain.handle(Channel.openPrivacy, () => app.overwolf.openAdPrivacySettingsWindow());
  ipcMain.handle(Channel.emailHashes, (_event, email: unknown) => {
    if (typeof email !== 'string') throw new TypeError('email must be a string');
    return { ...app.overwolf.generateUserEmailHashes(email) };
  });
  ipcMain.handle(
    Channel.exportTimeline,
    async (_event, request: unknown): Promise<ExportResult> => {
      const json = (request as { json?: unknown } | null)?.json;
      if (typeof json !== 'string') throw new TypeError('json must be a string');
      const path = joinPath(
        app.getPath('userData'),
        'exports',
        exportFileName(host.name, mode, new Date()),
      );
      await host.writeText(path, json);
      return { path };
    },
  );
  ipcMain.handle(Channel.parity, () => readParity());
  ipcMain.handle(Channel.restart, (_event, next: unknown, route: unknown) => {
    if (next !== 'test' && next !== 'live') throw new TypeError('mode must be test or live');
    // Come back on the same page, without mounting page 1 first.
    const page = typeof route === 'string' ? route : null;
    app.relaunch({ args: withPageSwitch(relaunchArgs(process.argv, next), page) });
    app.exit(0);
  });
  ipcMain.handle(Channel.windowAction, (_event, action: unknown) => {
    if (typeof action !== 'string' || !WINDOW_ACTIONS.has(action)) {
      throw new TypeError('unknown window action');
    }
    if (mainWindow && !mainWindow.isDestroyed()) {
      runWindowAction(mainWindow, action as WindowAction);
    }
  });
}

async function createWindow(): Promise<void> {
  const root = app.getAppPath();
  const win = new BrowserWindow({
    ...SIZE,
    show: false,
    title: app.getName(),
    backgroundColor: '#0e1014',
    webPreferences: { preload: joinPath(root, 'preload', 'preload.js') },
  });
  mainWindow = win;
  for (const name of ['show', 'hide', 'minimize', 'restore'] as const) {
    win.on(name, () => {
      report({ name });
    });
  }
  let resizeTimer: ReturnType<typeof setTimeout> | undefined;
  win.on('resize', () => {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(() => {
      if (!win.isDestroyed()) report({ name: 'resize', detail: { ...win.getBounds() } });
    }, 250);
  });
  win.on('closed', () => {
    mainWindow = null;
  });
  // `--showcase-page=<route>` opens that page first (see shared/route.ts).
  const start = parseRoute(app.commandLine.getSwitchValue(PAGE_SWITCH));
  await win.loadFile(
    joinPath(root, 'renderer', 'index.html'),
    start ? { hash: formatRoute(start) } : {},
  );
  win.show();
}

registerIpc();
app.on('window-all-closed', () => {
  app.quit();
});
void app.whenReady().then(createWindow);
