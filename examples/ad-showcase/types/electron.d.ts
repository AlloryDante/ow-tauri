/**
 * The part of ow-electron's `electron` module the ow-electron twin uses
 * (`src/main`, `src/preload`), for the type checker only: at run time the
 * twin runs on `@overwolf/ow-electron` (`scripts/ow-electron.mjs`), whose
 * full typings ship with that package. Keeping this subset here lets the
 * example type-check without installing the ow-electron binary.
 *
 * @packageDocumentation
 */
declare module 'electron' {
  /** `app.overwolf` of ow-electron (the members the twin uses). */
  interface OverwolfApp {
    readonly uid: string;
    readonly muid: string;
    readonly phasePercent: number;
    isCMPRequired(): Promise<boolean>;
    openAdPrivacySettingsWindow(): Promise<void>;
    generateUserEmailHashes(email: string): Record<string, string>;
  }

  interface CommandLine {
    hasSwitch(name: string): boolean;
    getSwitchValue(name: string): string;
  }

  interface App {
    readonly overwolf: OverwolfApp;
    readonly commandLine: CommandLine;
    getPath(name: 'home' | 'userData'): string;
    getAppPath(): string;
    getName(): string;
    getVersion(): string;
    relaunch(options?: { args?: string[] }): void;
    exit(code?: number): void;
    quit(): void;
    whenReady(): Promise<void>;
    on(event: 'window-all-closed', listener: () => void): this;
  }

  interface Rectangle {
    x: number;
    y: number;
    width: number;
    height: number;
  }

  interface WebContents {
    send(channel: string, ...args: unknown[]): void;
  }

  interface BrowserWindowOptions {
    width?: number;
    height?: number;
    minWidth?: number;
    minHeight?: number;
    show?: boolean;
    title?: string;
    backgroundColor?: string;
    webPreferences?: { preload?: string };
  }

  class BrowserWindow {
    constructor(options?: BrowserWindowOptions);
    readonly webContents: WebContents;
    isDestroyed(): boolean;
    show(): void;
    hide(): void;
    minimize(): void;
    restore(): void;
    setSize(width: number, height: number): void;
    setMinimumSize(width: number, height: number): void;
    getBounds(): Rectangle;
    loadFile(path: string, options?: { hash?: string }): Promise<void>;
    on(
      event: 'show' | 'hide' | 'minimize' | 'restore' | 'resize' | 'closed',
      listener: () => void,
    ): this;
  }

  interface IpcMainInvokeEvent {
    readonly sender: WebContents;
  }

  interface IpcMain {
    handle(
      channel: string,
      listener: (event: IpcMainInvokeEvent, ...args: unknown[]) => unknown,
    ): void;
  }

  interface IpcRendererEvent {
    readonly sender: unknown;
  }

  interface IpcRenderer {
    invoke(channel: string, ...args: unknown[]): Promise<unknown>;
    on(channel: string, listener: (event: IpcRendererEvent, ...args: never[]) => void): this;
    removeListener(
      channel: string,
      listener: (event: IpcRendererEvent, ...args: never[]) => void,
    ): this;
  }

  interface ContextBridge {
    exposeInMainWorld(key: string, api: unknown): void;
  }

  const app: App;
  const ipcMain: IpcMain;
  const ipcRenderer: IpcRenderer;
  const contextBridge: ContextBridge;
}
