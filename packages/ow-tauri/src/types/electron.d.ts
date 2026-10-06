// The `electron` module as ow-tauri provides it (CONTRACT B.4): the B.2
// surface of `ow-tauri/electron`, plus the same types under the global
// `Electron` namespace. Point `paths.electron` at this file.

/// <reference path="./ow-electron.d.ts" />
import type * as facade from '../electron/index.js';

export * from '../electron/index.js';

/**
 * Electron's `app` with ow-electron's augmentation: `app.overwolf` is the
 * global {@link overwolf.OverwolfApi}, so `@overwolf/ow-electron-packages-types`
 * applies (`app.overwolf.packages.gep`, `.recorder`, ...) and the object is
 * assignable to `overwolf.OverwolfApi`. Package objects are `undefined` at run
 * time while no package runtime exists, as in ow-electron.
 */
export type ElectronApp = facade.App & { readonly overwolf: overwolf.OverwolfApi };

/** Electron's `app` (CONTRACT B.2.1), typed with the `app.overwolf` augmentation. */
export declare const app: ElectronApp;

declare const electron: Omit<typeof facade.default, 'app'> & { readonly app: ElectronApp };
export default electron;

declare global {
  namespace Electron {
    type App = ElectronApp;
    type BrowserWindow = facade.BrowserWindow;
    type BrowserWindowConstructorOptions = facade.BrowserWindowConstructorOptions;
    type WebContents = facade.WebContents;
    type WebPreferences = facade.WebPreferences;
    type Display = facade.Display;
    type Rectangle = facade.Rectangle;
    type Size = facade.Size;
    type Point = facade.Point;
    type Event = facade.Event;
    type IpcMain = facade.IpcMain;
    type IpcMainEvent = facade.IpcMainEvent;
    type IpcMainInvokeEvent = facade.IpcMainInvokeEvent;
    type IpcRenderer = facade.IpcRenderer;
    type IpcRendererEvent = facade.IpcRendererEvent;
    type OpenDialogOptions = facade.OpenDialogOptions;
    type OpenDialogReturnValue = facade.OpenDialogReturnValue;
    type SaveDialogOptions = facade.SaveDialogOptions;
    type SaveDialogReturnValue = facade.SaveDialogReturnValue;
    type MessageBoxOptions = facade.MessageBoxOptions;
    type MessageBoxReturnValue = facade.MessageBoxReturnValue;
    type FileFilter = facade.FileFilter;
    type HandlerDetails = facade.HandlerDetails;
    type Screen = facade.Screen;
  }
}
