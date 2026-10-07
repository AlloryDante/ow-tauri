/**
 * Window behaviour of UI windows that Electron gets from Chromium and the
 * renderer bootstrap emulates (`docs/CONTRACT.md` B.3.6 and A.2.3.1):
 *
 * - `app-region` dragging: on engines that expose `app-region` /
 *   `-webkit-app-region` in computed style (WebView2), a primary-button press
 *   on a `drag` region starts a native window drag, and a double-click
 *   toggles maximize.
 * - External links on macOS and Linux: a top-frame link click or form
 *   submission to an `http(s)` URL outside the app origin is cancelled and
 *   sent to `navigation_external`, which opens the system browser, as the
 *   Windows navigation hook does natively.
 * - In-page navigations: a top-document URL change without a new load (a
 *   fragment change, `history.pushState` / `replaceState`, back or forward
 *   between such entries) is sent to `navigation_in_page`, so
 *   `webContents.getURL()` follows it and the app gets
 *   `did-navigate-in-page`, as Chromium reports them in Electron.
 *
 * @packageDocumentation
 */
import type { LogLevel } from '../bootstrap/services.js';
import { OwTauriError } from '../shared/errors.js';
import type { HostContext } from '../shared/protocol.js';

/** Elements that stay clickable inside a drag region (B.3.6). */
const INTERACTIVE = 'button, input, select, textarea, a[href]';

/** The kernel services the window behaviour uses. */
export interface ChromeServices {
  /** Where the runtime runs; only `'ui'` installs anything. */
  readonly context: HostContext;
  /**
   * Invokes `plugin:overwolf|<name>`.
   *
   * @param name - the command
   * @param args - its arguments
   * @returns the response
   */
  command(name: string, args?: Record<string, unknown>): Promise<unknown>;
  /**
   * Invokes another Tauri command (`plugin:window|start_dragging`).
   *
   * @param command - the full command name
   * @param args - its arguments
   * @returns the response
   */
  raw(command: string, args?: Record<string, unknown>): Promise<unknown>;
  /**
   * Logs a message.
   *
   * @param level - the level
   * @param message - the message
   */
  log(level: LogLevel, message: string): void;
  /**
   * Logs a warning once per key.
   *
   * @param key - deduplication key
   * @param message - the message
   */
  warnOnce(key: string, message: string): void;
}

/** Platform hooks, replaceable in tests. */
export interface ChromeEnvironment {
  /** The document. */
  readonly document: Document;
  /** Node's `process.platform` of the host (`win32`, `darwin`, `linux`). */
  readonly platform: string;
  /** Whether the engine exposes `app-region` in computed style. */
  readonly appRegion: boolean;
  /**
   * The computed `app-region` of an element (`drag`, `no-drag`, or anything else).
   *
   * @param el - the element
   * @returns the value
   */
  region(el: Element): string;
  /**
   * Whether the document is the top frame.
   *
   * @returns `true` in the top frame
   */
  isTop(): boolean;
  /**
   * The document's origin.
   *
   * @returns the origin
   */
  origin(): string;
  /**
   * Navigates the document (the fallback when the plugin cannot open the URL).
   *
   * @param url - the URL
   */
  navigate(url: string): void;
  /**
   * The document's current URL (`location.href`).
   *
   * @returns the URL
   */
  href(): string;
  /**
   * Calls `listener` after every same-document navigation: `hashchange`,
   * `popstate`, `history.pushState` and `history.replaceState`.
   *
   * @param listener - called after the URL may have changed
   * @returns a function that stops the calls
   */
  onSameDocumentNavigation(listener: () => void): () => void;
}

/**
 * The environment of the current document.
 *
 * @param platform - the host platform
 * @returns the environment
 */
export function browserChromeEnvironment(platform: string): ChromeEnvironment {
  const win = globalThis as unknown as Window & typeof globalThis;
  const css = (win as { CSS?: { supports?: (property: string, value: string) => boolean } }).CSS;
  const supports = (property: string): boolean => {
    try {
      return css?.supports?.(property, 'drag') === true;
    } catch {
      return false;
    }
  };
  return {
    document: win.document,
    platform,
    appRegion: supports('app-region') || supports('-webkit-app-region'),
    region: (el) => {
      const style = win.getComputedStyle(el);
      return (
        style.getPropertyValue('app-region') || style.getPropertyValue('-webkit-app-region')
      ).trim();
    },
    isTop: () => {
      try {
        return win.top === win.self;
      } catch {
        return false;
      }
    },
    origin: () => win.location.origin,
    navigate: (url) => {
      win.location.assign(url);
    },
    href: () => win.location.href,
    onSameDocumentNavigation: (listener) => {
      const history = win.history;
      const names = ['pushState', 'replaceState'] as const;
      const restore = names.map((name) => {
        const own = Object.getOwnPropertyDescriptor(history, name);
        const original = Reflect.get(history, name) as History['pushState'];
        // An own property in front of `History.prototype`'s: the page's
        // calls behave the same, then the change is reported.
        history[name] = function (this: History, ...args: Parameters<History['pushState']>): void {
          Reflect.apply(original, this, args);
          listener();
        };
        return () => {
          if (own) Object.defineProperty(history, name, own);
          else Reflect.deleteProperty(history, name);
        };
      });
      win.addEventListener('hashchange', listener);
      win.addEventListener('popstate', listener);
      return () => {
        for (const undo of restore) undo();
        win.removeEventListener('hashchange', listener);
        win.removeEventListener('popstate', listener);
      };
    },
  };
}

/** The element that contains `node`, crossing shadow roots. */
function parentOf(node: Element): Element | null {
  if (node.parentElement) return node.parentElement;
  const root = node.getRootNode() as Node & { host?: Element };
  return root !== node && root.host instanceof Object ? root.host : null;
}

/**
 * Whether a press on `start` lands on a drag region: the nearest element with
 * an `app-region` of `drag` or `no-drag` decides, and an interactive control
 * on the way stops the walk (B.3.6).
 *
 * @param env - the environment
 * @param start - the pressed element
 * @returns `true` on a drag region
 */
function isDragRegion(env: ChromeEnvironment, start: Element): boolean {
  for (let el: Element | null = start; el; el = parentOf(el)) {
    if (el.matches(INTERACTIVE)) return false;
    const value = env.region(el);
    if (value === 'drag') return true;
    if (value === 'no-drag') return false;
  }
  return false;
}

/** The first element of an event's path, or its target. */
function originOf(event: Event): Element | undefined {
  const first = typeof event.composedPath === 'function' ? event.composedPath()[0] : event.target;
  const node = first as Node | null | undefined;
  if (!node) return undefined;
  if (node.nodeType === 1) return node as Element;
  return node.parentElement ?? undefined;
}

/** A browsing-context target that keeps the navigation in this document. */
function isSelfTarget(target: string | null): boolean {
  if (target === null || target === '') return true;
  const lower = target.trim().toLowerCase();
  return lower === '_self' || lower === '_top';
}

/**
 * Installs the window behaviour in a UI window. Does nothing elsewhere.
 *
 * @param services - kernel services
 * @param env - platform hooks
 * @returns a function that removes the listeners
 */
export function installWindowChrome(services: ChromeServices, env: ChromeEnvironment): () => void {
  if (services.context !== 'ui') return () => undefined;
  const doc = env.document;
  const cleanups: (() => void)[] = [];

  const fail = (what: string) => (error: unknown) => {
    services.warnOnce(
      `window-chrome:${what}`,
      `${what} failed (the window capability may lack it): ${(error as Error).message}`,
    );
  };

  if (env.appRegion) {
    const onMouseDown = (event: MouseEvent): void => {
      if (event.button !== 0) return;
      const el = originOf(event);
      if (!el || !isDragRegion(env, el)) return;
      if (event.detail === 2)
        services.raw('plugin:window|toggle_maximize').catch(fail('toggle_maximize'));
      else services.raw('plugin:window|start_dragging').catch(fail('start_dragging'));
    };
    doc.addEventListener('mousedown', onMouseDown, { capture: true });
    cleanups.push(() => {
      doc.removeEventListener('mousedown', onMouseDown, { capture: true });
    });
  }

  if (env.platform !== 'win32') {
    const external = (raw: string): URL | undefined => {
      let url: URL;
      try {
        url = new URL(raw, doc.baseURI);
      } catch {
        return undefined;
      }
      if (url.protocol !== 'http:' && url.protocol !== 'https:') return undefined;
      return url.origin === env.origin() ? undefined : url;
    };
    const baseTarget = (): string | null =>
      doc.querySelector('base[target]')?.getAttribute('target') ?? null;
    const open = (url: string, fallback: () => void): void => {
      services.command('navigation_external', { url }).catch((error: unknown) => {
        if (error instanceof OwTauriError && error.code === 'invalid-argument') {
          services.log('warn', `navigation to ${url} was blocked: ${error.message}`);
          return;
        }
        services.warnOnce(
          'window-chrome:navigation_external',
          `navigation_external failed, the link opens in the window: ${(error as Error).message}`,
        );
        fallback();
      });
    };

    const onClick = (event: MouseEvent): void => {
      if (event.defaultPrevented || event.button !== 0) return;
      if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
      if (!env.isTop()) return;
      const path = typeof event.composedPath === 'function' ? event.composedPath() : [];
      const anchor = path.find(
        (node): node is HTMLAnchorElement =>
          (node as Element).nodeType === 1 &&
          ((node as Element).localName === 'a' || (node as Element).localName === 'area') &&
          (node as Element).hasAttribute('href'),
      );
      if (!anchor || anchor.hasAttribute('download')) return;
      if (!isSelfTarget(anchor.getAttribute('target') ?? baseTarget())) return;
      const url = external(anchor.getAttribute('href') ?? '');
      if (!url) return;
      event.preventDefault();
      open(url.href, () => {
        env.navigate(url.href);
      });
    };

    const onSubmit = (event: Event): void => {
      if (event.defaultPrevented || !env.isTop()) return;
      const form = event.target as HTMLFormElement | null;
      if (form?.localName !== 'form') return;
      const submitter = (event as SubmitEvent).submitter as
        HTMLButtonElement | HTMLInputElement | null;
      const attr = (name: string): string | null =>
        submitter?.getAttribute(`form${name}`) ?? form.getAttribute(name);
      const method = (attr('method') ?? 'get').trim().toLowerCase();
      if (method === 'dialog') return;
      if (!isSelfTarget(attr('target') ?? baseTarget())) return;
      const url = external(attr('action') ?? doc.baseURI);
      if (!url) return;
      if (method !== 'post') {
        try {
          const data = new FormData(form, submitter ?? undefined);
          url.search = new URLSearchParams(data as unknown as Record<string, string>).toString();
        } catch {
          // keep the action URL as it is
        }
      }
      event.preventDefault();
      open(url.href, () => {
        if (method === 'post') HTMLFormElement.prototype.submit.call(form);
        else env.navigate(url.href);
      });
    };

    doc.addEventListener('click', onClick);
    doc.addEventListener('submit', onSubmit);
    cleanups.push(() => {
      doc.removeEventListener('click', onClick);
      doc.removeEventListener('submit', onSubmit);
    });
  }

  if (env.isTop()) {
    let last = env.href();
    const report = (): void => {
      const url = env.href();
      if (url === last) return;
      last = url;
      services.command('navigation_in_page', { url }).catch(fail('navigation_in_page'));
    };
    cleanups.push(env.onSameDocumentNavigation(report));
  }

  return () => {
    for (const cleanup of cleanups.splice(0)) cleanup();
  };
}
