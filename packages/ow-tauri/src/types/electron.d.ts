// The `electron` module as ow-tauri provides it (CONTRACT B.4): the B.2
// surface of `ow-tauri/electron`, plus the same types under the global
// `Electron` namespace. Point `paths.electron` at this file.

import type * as facade from '../electron/index.js';

export * from '../electron/index.js';
export { default } from '../electron/index.js';

declare global {
  namespace Electron {
    type App = facade.App;
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
