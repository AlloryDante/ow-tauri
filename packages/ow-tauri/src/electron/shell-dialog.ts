/**
 * Electron's `shell`, `dialog` and `globalShortcut` (`docs/CONTRACT.md`
 * section B.2.5), main webview only. Each call is a plugin command; the
 * plugin uses the opener, dialog and global-shortcut plugins from Rust.
 *
 * @packageDocumentation
 */
import type { Kernel } from '../bootstrap/kernel.js';
import { OwTauriError } from '../shared/errors.js';
import type { GlobalShortcutMessage } from '../shared/protocol.js';
import { defineUnsupported } from '../shared/unsupported.js';
import { kernel } from './runtime.js';
import type {
  BrowserWindowLike,
  MessageBoxOptions,
  MessageBoxReturnValue,
  OpenDialogOptions,
  OpenDialogReturnValue,
  SaveDialogOptions,
  SaveDialogReturnValue,
} from './types.js';

/** Electron's `shell` (CONTRACT B.2.5). */
export interface Shell {
  /**
   * Opens a URL in the system browser or mail client (`http`, `https`, `mailto` only).
   *
   * @param url - an absolute URL
   * @param options - ignored (Electron's `activate`, `workingDirectory`)
   * @returns resolves once handed to the OS
   */
  openExternal(url: string, options?: unknown): Promise<void>;
  /**
   * Opens a file or folder with its default app. Paths outside the file
   * scope and executables are refused (CONTRACT A.2.3.2).
   *
   * @param path - the path
   * @returns `''` on success, else the error text (never rejects)
   */
  openPath(path: string): Promise<string>;
  /**
   * Shows a file in the OS file manager.
   *
   * @param path - the path
   */
  showItemInFolder(path: string): void;
}

function main(api: string): void {
  kernel.require('main', api);
}

/** Electron's `shell` (main webview only). */
export const shell: Shell = {
  async openExternal(url) {
    main('shell.openExternal');
    await kernel.command('shell_open_external', { url });
  },
  async openPath(path) {
    main('shell.openPath');
    try {
      const result = await kernel.command('shell_open_path', { path });
      return typeof result === 'string' ? result : '';
    } catch (error) {
      return (error as Error).message;
    }
  },
  showItemInFolder(path) {
    main('shell.showItemInFolder');
    kernel.command('shell_show_item_in_folder', { path }).catch((error: unknown) => {
      kernel.log('warn', `shell.showItemInFolder failed: ${(error as Error).message}`);
    });
  },
};
defineUnsupported(shell, 'shell.', ['trashItem', 'beep', 'writeShortcutLink', 'readShortcutLink']);

/** Electron's `dialog` (CONTRACT B.2.5). */
export interface Dialog {
  /**
   * Shows an open dialog.
   *
   * @param windowOrOptions - the parent window, or the options
   * @param options - the options when a window is given
   * @returns the chosen paths
   */
  showOpenDialog(
    windowOrOptions: BrowserWindowLike | OpenDialogOptions,
    options?: OpenDialogOptions,
  ): Promise<OpenDialogReturnValue>;
  /**
   * Shows a save dialog.
   *
   * @param windowOrOptions - the parent window, or the options
   * @param options - the options when a window is given
   * @returns the chosen path
   */
  showSaveDialog(
    windowOrOptions: BrowserWindowLike | SaveDialogOptions,
    options?: SaveDialogOptions,
  ): Promise<SaveDialogReturnValue>;
  /**
   * Partial: shows a message box with up to three buttons; more rejects
   * with `invalid-argument`.
   *
   * @param windowOrOptions - the parent window, or the options
   * @param options - the options when a window is given
   * @returns the clicked button and check box state
   */
  showMessageBox(
    windowOrOptions: BrowserWindowLike | MessageBoxOptions,
    options?: MessageBoxOptions,
  ): Promise<MessageBoxReturnValue>;
  /**
   * Shows an error message box (does not wait for it).
   *
   * @param title - the title
   * @param content - the message
   */
  showErrorBox(title: string, content: string): void;
}

function split<T extends object>(
  windowOrOptions: BrowserWindowLike | T,
  options: T | undefined,
): [Record<string, unknown>, number | undefined] {
  if (options !== undefined) {
    const id = (windowOrOptions as BrowserWindowLike | null)?.id;
    return [
      { ...(options as Record<string, unknown>) },
      typeof id === 'number' ? kernel.windowIds.toHost(id) : undefined,
    ];
  }
  return [{ ...(windowOrOptions as Record<string, unknown>) }, undefined];
}

function withWindow(
  args: Record<string, unknown>,
  windowId: number | undefined,
): Record<string, unknown> {
  return windowId === undefined ? args : { ...args, windowId };
}

/** Electron's `dialog` (main webview only). */
export const dialog: Dialog = {
  async showOpenDialog(windowOrOptions, options) {
    main('dialog.showOpenDialog');
    const [args, windowId] = split(windowOrOptions, options);
    const result = (await kernel.command(
      'dialog_open',
      withWindow(args, windowId),
    )) as Partial<OpenDialogReturnValue> | null;
    return {
      canceled: result?.canceled === true,
      filePaths: Array.isArray(result?.filePaths) ? result.filePaths : [],
    };
  },
  async showSaveDialog(windowOrOptions, options) {
    main('dialog.showSaveDialog');
    const [args, windowId] = split(windowOrOptions, options);
    const result = (await kernel.command(
      'dialog_save',
      withWindow(args, windowId),
    )) as Partial<SaveDialogReturnValue> | null;
    return {
      canceled: result?.canceled === true,
      filePath: typeof result?.filePath === 'string' ? result.filePath : '',
    };
  },
  async showMessageBox(windowOrOptions, options) {
    main('dialog.showMessageBox');
    const [args, windowId] = split(windowOrOptions, options);
    if (Array.isArray(args['buttons']) && args['buttons'].length > 3) {
      throw new OwTauriError(
        'invalid-argument',
        'dialog.showMessageBox supports at most three buttons in ow-tauri',
        {
          data: { buttons: args['buttons'].length },
        },
      );
    }
    const result = (await kernel.command(
      'dialog_message',
      withWindow(args, windowId),
    )) as Partial<MessageBoxReturnValue> | null;
    return {
      response: typeof result?.response === 'number' ? result.response : 0,
      checkboxChecked: result?.checkboxChecked === true,
    };
  },
  showErrorBox(title, content) {
    main('dialog.showErrorBox');
    kernel
      .command('dialog_message', { type: 'error', title, message: content })
      .catch((error: unknown) => {
        kernel.log('warn', `dialog.showErrorBox failed: ${(error as Error).message}`);
      });
  },
};
defineUnsupported(dialog, 'dialog.', [
  'showOpenDialogSync',
  'showSaveDialogSync',
  'showMessageBoxSync',
  'showCertificateTrustDialog',
]);

interface Shortcut {
  id: number;
  callback: () => void;
  registered: boolean;
}

/**
 * Electron's `globalShortcut` (CONTRACT B.2.5). Presses arrive as
 * `global-shortcut` host messages.
 */
export class GlobalShortcut {
  readonly #kernel: Kernel;
  readonly #shortcuts = new Map<string, Shortcut>();
  #nextId = 1;

  /**
   * @param k - the kernel
   * @internal
   */
  constructor(k: Kernel) {
    this.#kernel = k;
    k.on('global-shortcut', (message) => {
      const { id, state } = message as GlobalShortcutMessage;
      if (state !== 'pressed') return;
      for (const shortcut of this.#shortcuts.values()) {
        if (shortcut.id === id) {
          try {
            shortcut.callback();
          } catch (error) {
            k.log('error', `global shortcut callback threw: ${(error as Error).message}`);
          }
        }
      }
    });
    k.onReset(() => {
      this.#shortcuts.clear();
      this.#nextId = 1;
    });
  }

  /**
   * Partial: registers a system-wide shortcut. Returns `true` synchronously
   * (`false` when this app already registered it); a later failure is logged
   * and {@link GlobalShortcut.isRegistered} turns `false`.
   *
   * @param accelerator - Electron accelerator syntax, e.g. `CommandOrControl+Shift+X`
   * @param callback - called on each press
   * @returns whether registration was requested
   */
  register(accelerator: string, callback: () => void): boolean {
    this.#kernel.require('main', 'globalShortcut.register');
    if (this.#shortcuts.has(accelerator)) return false;
    const shortcut: Shortcut = { id: this.#nextId++, callback, registered: true };
    this.#shortcuts.set(accelerator, shortcut);
    this.#kernel
      .command('global_shortcut_register', { accelerator, id: shortcut.id })
      .then((ok) => {
        if (ok === false) throw new Error('the OS refused the shortcut');
      })
      .catch((error: unknown) => {
        shortcut.registered = false;
        this.#kernel.log(
          'warn',
          `globalShortcut.register('${accelerator}') failed: ${(error as Error).message}`,
        );
      });
    return true;
  }

  /**
   * Registers several accelerators with one callback.
   *
   * @param accelerators - the accelerators
   * @param callback - called on each press
   */
  registerAll(accelerators: string[], callback: () => void): void {
    for (const accelerator of accelerators) this.register(accelerator, callback);
  }

  /**
   * Whether this app holds the accelerator.
   *
   * @param accelerator - the accelerator
   * @returns `true` while registered
   */
  isRegistered(accelerator: string): boolean {
    this.#kernel.require('main', 'globalShortcut.isRegistered');
    return this.#shortcuts.get(accelerator)?.registered === true;
  }

  /**
   * Unregisters one accelerator.
   *
   * @param accelerator - the accelerator
   */
  unregister(accelerator: string): void {
    this.#kernel.require('main', 'globalShortcut.unregister');
    if (!this.#shortcuts.delete(accelerator)) return;
    this.#fire({ accelerator });
  }

  /** Unregisters every accelerator of this app. */
  unregisterAll(): void {
    this.#kernel.require('main', 'globalShortcut.unregisterAll');
    this.#shortcuts.clear();
    this.#fire({});
  }

  #fire(args: Record<string, unknown>): void {
    this.#kernel.command('global_shortcut_unregister', args).catch((error: unknown) => {
      this.#kernel.log('warn', `globalShortcut.unregister failed: ${(error as Error).message}`);
    });
  }
}

/** Electron's `globalShortcut` (main webview only). */
export const globalShortcut: GlobalShortcut = kernel.singleton(
  'electron.globalShortcut',
  () => new GlobalShortcut(kernel),
);
