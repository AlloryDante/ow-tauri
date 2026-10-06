// Type-level check of the B.4 declarations: code written for ow-electron
// compiles against ow-tauri once `paths` points at dist/types.
import '@overwolf/ow-electron';
import type { OverwolfGameEventPackage } from '@overwolf/ow-electron-packages-types';
import electron, { BrowserWindow, app, ipcMain, screen, type IpcMainInvokeEvent } from 'electron';

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
