/**
 * The ad guest shim (`docs/CONTRACT.md` D.1 to D.5): `window.__overwolf__`,
 * `window.gc` and the host API Rust calls in the main frame of
 * `https://www.overwolf.com/monsdk/electron/latest/adview.html`. Built into
 * `crates/tauri-plugin-overwolf/js/adview-host.js` by `adview-host.ts`.
 *
 * The host API lives on a non-enumerable window property whose name is
 * random per guest (`hostKey` of the configuration), so the page cannot
 * look it up by a fixed name. The shim's functions are bound functions:
 * their source text reads `function () { [native code] }`, like the
 * functions ow-electron exposes to the page.
 *
 * @packageDocumentation
 */
import { Outbox, copy, deepFreeze, hostFunction, sanitize } from './outbox.js';

/** The only origin the ad shim runs on (D.1). */
export const ADVIEW_ORIGIN = 'https://www.overwolf.com';

/** The command ad guests send with (A.2.6). */
export const ADVIEW_COMMAND = 'plugin:overwolf|adview_event';

/**
 * The host API's window property when the configuration has no valid
 * `hostKey` (only in tests: Rust always sets one).
 */
export const DEFAULT_HOST_KEY = '__owTauriHost';

/** A valid `hostKey`: a plain identifier of at most 64 characters. */
const HOST_KEY_PATTERN = /^[A-Za-z_$][\w$]{0,63}$/;

/**
 * The window property of the host API, and the `sessionStorage` key of the
 * `pageurl` for the next load (B.3.3), for a guest configuration.
 *
 * @param config - the guest configuration
 * @returns its `hostKey`, or {@link DEFAULT_HOST_KEY}
 */
export function hostKeyOf(config: Record<string, unknown>): string {
  const key = config['hostKey'];
  return typeof key === 'string' && HOST_KEY_PATTERN.test(key) ? key : DEFAULT_HOST_KEY;
}

/** Most `onmessage` handlers kept (D.3). */
export const MAX_HANDLERS = 16;

/** Gesture reports closer than this are merged (D.4). */
export const GESTURE_DEBOUNCE_MS = 100;

/** The D.2 data keys, in the order the page enumerates them after the functions. */
export const DATA_KEYS = [
  'uid',
  'name',
  'owVersion',
  'version',
  'windowName',
  'windowTitle',
  'windowFocused',
  'testAd',
  'consent',
  'consentFull',
  'slotSize',
  'containerId',
  'systemInfo',
  'settings',
  'muidV2',
  'phasePercent',
  'pageUrl',
  'performanceAd',
  'adStyle',
  'unit',
  'customTracking',
] as const;

/** A host message (D.5). */
export interface HostMessage {
  /** Message type, e.g. `consent`. */
  type: string;
  /** Message data. */
  data?: unknown;
}

/** The host API: what Rust calls in the guest (D.5). */
export interface AdviewHostApi {
  /** Passes a host message to the page's `onmessage` handlers. */
  deliver: (...args: unknown[]) => boolean;
  /** Sets the embedder focus behind `hasWindowFocus()` and `document.hasFocus()`. */
  setEmbedderFocus: (...args: unknown[]) => void;
  /** Sets `document.visibilityState` (`visible` or `hidden`). */
  setVisibility: (...args: unknown[]) => void;
  /** Stores the `pageUrl` the next load of this guest starts with. */
  setNextPageUrl: (...args: unknown[]) => void;
}

type Visibility = 'visible' | 'hidden';

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isTopFrame(win: Window): boolean {
  try {
    return win.top === win;
  } catch {
    return false;
  }
}

function readStoredPageUrl(win: Window, key: string): string | undefined {
  try {
    const stored = win.sessionStorage.getItem(key);
    return stored ?? undefined;
  } catch {
    return undefined;
  }
}

/**
 * Installs the ad shim in `win` with the configuration Rust spliced in.
 * Does nothing (and returns `false`) outside the main frame of
 * {@link ADVIEW_ORIGIN}, without a configuration object, or when the shim
 * already ran in this document (D.1).
 *
 * @param win - the guest window
 * @param config - the D.2 data plus `slotId` and `visibilityState`
 * @returns whether the shim was installed
 */
export function installAdviewHost(win: Window, config: unknown): boolean {
  if (!isTopFrame(win)) return false;
  if (win.location.origin !== ADVIEW_ORIGIN) return false;
  if (Object.prototype.hasOwnProperty.call(win, '__overwolf__')) return false;
  if (!isRecord(config)) return false;

  const doc = win.document;
  const hostKey = hostKeyOf(config);
  const outbox = new Outbox(win, ADVIEW_COMMAND);
  const slotId = typeof config['slotId'] === 'string' ? config['slotId'] : '';
  const post = (name: string, data?: unknown): void => {
    outbox.post({ slotId, name, data: sanitize(data) });
  };

  const systemInfo = copy(config['systemInfo']) ?? null;
  let customTracking: unknown = isRecord(config['customTracking'])
    ? copy(config['customTracking'])
    : null;
  let embedderFocus = config['windowFocused'] === true;
  let visibility: Visibility = config['visibilityState'] === 'hidden' ? 'hidden' : 'visible';
  const handlers: ((message: unknown) => void)[] = [];

  // `pageUrl` of this load: a `setPageUrl()` made before the reload wins (B.3.3).
  const pageUrl = readStoredPageUrl(win, hostKey) ?? config['pageUrl'];

  const fn = hostFunction;

  const functions = {
    setMute: fn((...args: unknown[]) => {
      post('__host:setMute', { muted: args[0] === true });
    }),
    triggerEvent: fn((...args: unknown[]) => {
      const name = String(args[0]);
      if (name === 'message' || name === 'messageerror') return;
      const rest = args.slice(1);
      post(name, rest.length > 1 ? rest : rest[0]);
    }),
    applySetting: fn((...args: unknown[]) => {
      post('__host:applySetting', args[0]);
    }),
    crash: fn(() => {
      post('__host:crash');
    }),
    reload: fn(() => {
      post('__host:reload');
    }),
    getSystemInformation: fn(() => copy(systemInfo)),
    getCustomTracking: fn(() => copy(customTracking)),
    hasWindowFocus: fn(() => embedderFocus),
    onmessage: fn((...args: unknown[]) => {
      const handler = args[0];
      if (typeof handler === 'function' && handlers.length < MAX_HANDLERS) {
        handlers.push(handler as (message: unknown) => void);
      }
    }),
  };

  const overwolf: Record<string, unknown> = {
    muid: typeof config['muid'] === 'string' ? config['muid'] : '',
  };
  Object.assign(overwolf, functions);
  for (const key of DATA_KEYS) {
    overwolf[key] = key === 'pageUrl' ? pageUrl : copy(config[key]);
  }
  Object.defineProperty(win, '__overwolf__', {
    value: deepFreeze(overwolf),
    writable: false,
    configurable: false,
    enumerable: true,
  });

  if (typeof Reflect.get(win, 'gc') !== 'function') {
    Object.defineProperty(win, 'gc', {
      value: fn(() => undefined),
      writable: true,
      configurable: true,
      enumerable: false,
    });
  }

  // Visibility and focus as the page reads them (D.5).
  try {
    Object.defineProperty(doc, 'visibilityState', { configurable: true, get: () => visibility });
    Object.defineProperty(doc, 'hidden', {
      configurable: true,
      get: () => visibility === 'hidden',
    });
    Object.defineProperty(doc, 'hasFocus', {
      configurable: true,
      writable: true,
      value: fn(() => embedderFocus),
    });
  } catch {
    // A page that redefined them first keeps its own.
  }

  const dispatch = (message: HostMessage): void => {
    for (const handler of handlers.slice()) {
      try {
        handler(copy(message));
      } catch {
        // ow-electron swallows handler errors.
      }
    }
  };

  const host: AdviewHostApi = {
    deliver: fn((...args: unknown[]) => {
      const raw = args[0];
      if (!isRecord(raw) || typeof raw['type'] !== 'string') return false;
      const message: HostMessage = { type: raw['type'] };
      if ('data' in raw) message.data = copy(raw['data']);
      if (message.type === 'customTracking') {
        customTracking = isRecord(message.data) ? message.data : null;
      }
      dispatch(message);
      return true;
    }),
    setEmbedderFocus: fn((...args: unknown[]) => {
      embedderFocus = args[0] === true;
    }),
    setVisibility: fn((...args: unknown[]) => {
      const next = args[0];
      if (next !== 'visible' && next !== 'hidden') return;
      if (next === visibility) return;
      visibility = next;
      const EventType = (Reflect.get(win, 'Event') as typeof Event | undefined) ?? Event;
      doc.dispatchEvent(new EventType('visibilitychange'));
    }),
    setNextPageUrl: fn((...args: unknown[]) => {
      const url = args[0];
      if (typeof url !== 'string' || url.length > 2048) return;
      try {
        win.sessionStorage.setItem(hostKey, url);
      } catch {
        // No storage: the next load keeps the mount value.
      }
    }),
  };
  Object.defineProperty(win, hostKey, {
    value: Object.freeze(host),
    writable: false,
    configurable: false,
    enumerable: false,
  });

  // Guest focus and user gestures (D.4).
  win.addEventListener('focus', () => {
    post('__host:focus', { focused: true });
  });
  win.addEventListener('blur', () => {
    post('__host:focus', { focused: false });
  });
  let lastGesture = Number.NEGATIVE_INFINITY;
  const gesture = (kind: string): void => {
    const now = Date.now();
    if (now - lastGesture < GESTURE_DEBOUNCE_MS) return;
    lastGesture = now;
    post('__host:gesture', { kind });
  };
  win.addEventListener(
    'pointerdown',
    (event) => {
      if (event.isTrusted) gesture('pointerdown');
    },
    true,
  );
  win.addEventListener(
    'keydown',
    (event) => {
      if (event.isTrusted) gesture('keydown');
    },
    true,
  );
  win.addEventListener('blur', () => {
    // A click in a cross-origin creative moves focus into its iframe.
    win.setTimeout(() => {
      if (doc.activeElement?.tagName === 'IFRAME') gesture('iframe-focus');
    }, 0);
  });
  const domReady = (): void => {
    post('__host:domReady');
  };
  if (doc.readyState === 'loading') {
    doc.addEventListener('DOMContentLoaded', domReady, { once: true });
  } else {
    domReady();
  }

  post('__host:ready', {
    href: win.location.href,
    testAd: config['testAd'] === true,
    visibilityState: visibility,
    pageUrl,
  });
  return true;
}
