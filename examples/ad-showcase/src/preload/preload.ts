/**
 * The window's preload: exposes the `showcase:*` IPC as `window.showcase`.
 * On ow-electron it runs as a sandboxed Electron preload; on ow-tauri the
 * `ow-tauri/electron` facade injects it as an initialization script. The
 * page itself never imports `electron`.
 *
 * @packageDocumentation
 */
import { contextBridge, ipcRenderer } from 'electron';
import {
  Channel,
  type AdMode,
  type ExportRequest,
  type ShowcaseApi,
  type WindowAction,
  type WindowEvent,
} from '../shared/ipc.js';

/** `ipcRenderer.invoke` with the reply type of the channel (see `Channel`). */
function invoke<T>(channel: string, ...args: unknown[]): Promise<T> {
  return ipcRenderer.invoke(channel, ...args) as Promise<T>;
}

const api: ShowcaseApi = {
  info: () => invoke(Channel.info),
  cmpRequired: () => invoke(Channel.cmpRequired),
  openPrivacySettings: () => invoke(Channel.openPrivacy),
  emailHashes: (email: string) => invoke(Channel.emailHashes, email),
  exportTimeline: (request: ExportRequest) =>
    invoke(Channel.exportTimeline, { json: request.json }),
  parity: () => invoke(Channel.parity),
  restart: (mode: AdMode, route?: string) => invoke(Channel.restart, mode, route),
  windowAction: (action: WindowAction) => invoke(Channel.windowAction, action),
  onWindowEvent(listener: (event: WindowEvent) => void) {
    const wrapped = (_event: unknown, payload: WindowEvent): void => {
      listener(payload);
    };
    ipcRenderer.on(Channel.windowEvent, wrapped);
    return () => {
      ipcRenderer.removeListener(Channel.windowEvent, wrapped);
    };
  },
};

contextBridge.exposeInMainWorld('showcase', api);
