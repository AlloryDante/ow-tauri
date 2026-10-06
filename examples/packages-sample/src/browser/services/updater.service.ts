import { app } from 'electron';
import { autoUpdater, UpdateCheckResult } from 'ow-tauri/main';

export class UpdaterService {
  constructor() {
    // Initialize the autoUpdater
    autoUpdater.logger = console;
    autoUpdater.autoDownload = false; // Automatically download updates
    autoUpdater.autoInstallOnAppQuit = true; // Install updates on app quit
    autoUpdater.channel = 'testingChannelz'; // Set the channel for updates
    autoUpdater.allowDowngrade = false; // Prevent downgrades
    autoUpdater.setFeedURL({
      provider: 'generic',
      url: 'https://electron-updates.overwolf.com/electron-updates/electron/nmldjbjpjgphngofdaekabelhnliphfdoahdgpkh',
      // url: 'https://electron-updates-qa.overwolf.com/electron-updates/electron/afebjiflnalenhcoojjobefjapnjkpibggbbbjmd'
    });
  }

  async checkForUpdatesAndNotify() {
    if (!app.isPackaged) {
      console.log('Skipping update check: app is not packaged.');
      return;
    }

    try {
      const checkResult = await autoUpdater.checkForUpdatesAndNotify();
      if (checkResult && checkResult.updateInfo) {
        console.log('Update available:', checkResult.updateInfo);
        await this.downloadUpdate();
      } else {
        console.log('No updates available');
      }
    } catch (error) {
      console.error('Failed to check for updates:', error);
    }
  }

  private async downloadUpdate() {
    try {
      const downloadResult = await autoUpdater.downloadUpdate();
      console.log('Update downloaded successfully:', downloadResult);
      if (downloadResult && downloadResult.length > 0) {
        autoUpdater.quitAndInstall();
      }
    } catch (error) {
      console.error('Failed to download update:', error);
    }
  }
}
