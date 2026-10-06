/**
 * `ow-tauri/electron`: an Electron-compatible subset for a bundler alias
 * `electron -> ow-tauri/electron`.
 *
 * Main-process code (in the main webview) and preload and renderer code (in
 * UI windows) keep their imports. Members ow-tauri does not implement throw
 * {@link OwTauriUnsupportedError}; members used in the wrong context throw
 * `OwTauriError('forbidden')`. The member-by-member table is in
 * `docs/CONTRACT.md` section B.2.
 *
 * @packageDocumentation
 */
/* eslint-disable @typescript-eslint/no-deprecated -- the facade re-exports the
   unsupported modules it marks deprecated, so ported imports keep compiling */
import type { IpcMain } from '../bootstrap/ipc-main.js';
import type { IpcRenderer } from '../bootstrap/ipc-renderer.js';
import { app } from './app.js';
import { BrowserWindow, WebContents } from './browser-window.js';
import { contextBridge } from './context-bridge.js';
import * as misc from './misc.js';
import { kernel } from './runtime.js';
import { screen } from './screen.js';
import { dialog, globalShortcut, shell } from './shell-dialog.js';

/**
 * Electron's `ipcMain` (main webview only).
 *
 * @example
 * ```ts
 * ipcMain.handle('games:list', async () => loadGames());
 * ipcMain.on('log', (event, line: string) => console.log(event.sender.id, line));
 * ```
 */
export const ipcMain: IpcMain = kernel.server.ipcMain;

/**
 * Electron's `ipcRenderer` (UI windows only).
 *
 * @example
 * ```ts
 * const games = await ipcRenderer.invoke('games:list');
 * ipcRenderer.on('game-launched', (_event, game) => render(game));
 * ```
 */
export const ipcRenderer: IpcRenderer = kernel.ipcRenderer;

export { app, App } from './app.js';
export type { CommandLine, RelaunchOptions } from './app.js';
export { BrowserWindow, WebContents } from './browser-window.js';
export { contextBridge } from './context-bridge.js';
export type { ContextBridge } from './context-bridge.js';
export { screen, Screen } from './screen.js';
export { dialog, shell, globalShortcut, GlobalShortcut } from './shell-dialog.js';
export type { Dialog, Shell } from './shell-dialog.js';
export {
  crashReporter,
  nativeTheme,
  NativeTheme,
  process,
  Menu,
  MenuItem,
  Tray,
  Notification,
  session,
  protocol,
  net,
  netLog,
  powerMonitor,
  powerSaveBlocker,
  autoUpdater,
  clipboard,
  nativeImage,
  systemPreferences,
  desktopCapturer,
  webFrame,
  webFrameMain,
  utilityProcess,
  MessageChannelMain,
  BrowserView,
  WebContentsView,
  BaseWindow,
  TouchBar,
  inAppPurchase,
  pushNotifications,
  safeStorage,
  contentTracing,
} from './misc.js';
export type { CrashReporter } from './misc.js';
export type * from './unsupported-types.js';
export type { ProcessShim } from '../bootstrap/process-shim.js';
export { IpcMain } from '../bootstrap/ipc-main.js';
export type {
  IpcMainEvent,
  IpcMainEventBase,
  IpcMainInvokeEvent,
  IpcHandler,
  IpcSenderFrame,
} from '../bootstrap/ipc-main.js';
export { IpcRenderer } from '../bootstrap/ipc-renderer.js';
export type { IpcRendererEvent } from '../bootstrap/ipc-renderer.js';
export type {
  Display,
  Event,
  FileFilter,
  HandlerDetails,
  LoadFileOptions,
  MessageBoxOptions,
  MessageBoxReturnValue,
  OpenDialogOptions,
  OpenDialogReturnValue,
  Point,
  Rectangle,
  SaveDialogOptions,
  SaveDialogReturnValue,
  Size,
  WebPreferences,
  BrowserWindowConstructorOptions,
  BrowserWindowLike,
  WindowOpenHandlerResponse,
  WindowOpenAllow,
  WindowOpenDeny,
} from './types.js';
export { OwTauriError, OwTauriUnsupportedError } from '../shared/errors.js';
export type { OwTauriErrorCode, OwTauriErrorOptions } from '../shared/errors.js';

/**
 * The module object, for `import electron from 'electron'` and
 * `require('electron')` style access.
 */
const electron = {
  /** {@inheritDoc app} */
  app,
  /** {@inheritDoc BrowserWindow} */
  BrowserWindow,
  /** {@inheritDoc WebContents} */
  WebContents,
  /** {@inheritDoc ipcMain} */
  ipcMain,
  /** {@inheritDoc ipcRenderer} */
  ipcRenderer,
  /** {@inheritDoc contextBridge} */
  contextBridge,
  /** {@inheritDoc screen} */
  screen,
  /** {@inheritDoc shell} */
  shell,
  /** {@inheritDoc dialog} */
  dialog,
  /** {@inheritDoc globalShortcut} */
  globalShortcut,
  ...misc,
};

export default electron;
