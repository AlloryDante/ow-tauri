// Type-level check of the B.4 declarations: code written for ow-electron
// compiles against ow-tauri once `paths` points at dist/types.
import '@overwolf/ow-electron';
import type { OverwolfGameEventPackage } from '@overwolf/ow-electron-packages-types';
import electron, {
  BrowserWindow,
  Menu,
  Tray,
  app,
  clipboard,
  dialog,
  ipcMain,
  ipcRenderer,
  powerMonitor,
  screen,
  shell,
  type IpcMainInvokeEvent,
} from 'electron';

export function sample(): void {
  const display: Electron.Display = screen.getPrimaryDisplay();
  const win: Electron.BrowserWindow = new BrowserWindow({
    width: display.workArea.width,
    show: false,
  });
  ipcMain.handle('x', (event: IpcMainInvokeEvent, value: number) => [event.frameId, value]);
  const name: overwolf.packages.PackageName = 'gep';
  const hashes: overwolf.EmailHashes = { sha256: 'a' };
  const legacy: overwolf.OverwolfApp | undefined = undefined;
  const ad: overwolf.AdviewTag = document.createElement('owadview');
  ad.customTracking = '{}';
  void [win.id, app.getVersion(), name, hashes, legacy, electron.dialog];
  const gep: OverwolfGameEventPackage | undefined = undefined;
  void gep;
}

// Ported code that still uses unsupported members compiles (editors show them
// as deprecated) and throws OwTauriUnsupportedError at run time.
export function unsupported(win: BrowserWindow): void {
  Menu.setApplicationMenu(null);
  powerMonitor.on('suspend', () => undefined);
  void clipboard.readText();
  void new Tray('icon.png');
  win.setVibrancy('sidebar');
  win.webContents.print();
  void win.webContents.session;
  void app.dock;
  app.setBadgeCount(1);
  void shell.trashItem('/tmp/x');
  void dialog.showMessageBoxSync({ message: 'x' });
  void ipcRenderer.sendSync('x');
}

// `app.overwolf` is the global `overwolf.OverwolfApi` (CONTRACT B.4), so the
// packages-types augmentations apply, as in the sample's controllers.
export function packages(): void {
  const api: overwolf.OverwolfApi = app.overwolf;
  const gep: OverwolfGameEventPackage = app.overwolf.packages.gep;
  const recorder = app.overwolf.packages.recorder;
  const viaDefault: overwolf.OverwolfApi = electron.app.overwolf;
  const typed: Electron.App = app;
  void [api, gep, recorder, viaDefault, typed.overwolf.uid, app.overwolf.uid, app.getName()];
}
