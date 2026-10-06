/**
 * Electron's `screen` (`docs/CONTRACT.md` section B.2.5), served
 * synchronously from the state cache (`displays`, `primaryDisplayId`).
 *
 * @packageDocumentation
 */
import type { FacadeKernel } from '../bootstrap/facade-kernel.js';
import { EventEmitter, emitFromHost } from '../shared/emitter.js';
import type { Display, Point, Rectangle } from '../shared/protocol.js';
import { createEvent, kernel } from './runtime.js';

/** Cursor refresh interval (CONTRACT B.2.5: at most every 100 ms). */
const CURSOR_TTL_MS = 100;

/** Metrics compared for `display-metrics-changed`. */
const METRICS = ['bounds', 'workArea', 'scaleFactor', 'rotation'] as const;

/**
 * Fills the optional fields of a cached display with Electron's defaults.
 *
 * @param display - the cached display
 * @returns a fresh, complete copy
 */
export function completeDisplay(display: Display): Required<Display> {
  const { bounds, workArea, scaleFactor } = display;
  return {
    id: display.id,
    label: display.label,
    bounds: { ...bounds },
    workArea: { ...workArea },
    scaleFactor,
    size: { width: bounds.width, height: bounds.height },
    workAreaSize: { width: workArea.width, height: workArea.height },
    rotation: display.rotation ?? 0,
    internal: display.internal ?? false,
    monochrome: display.monochrome ?? false,
    accelerometerSupport: display.accelerometerSupport ?? 'unknown',
    touchSupport: display.touchSupport ?? 'unknown',
    displayFrequency: display.displayFrequency ?? 0,
    colorDepth: display.colorDepth ?? 24,
    depthPerComponent: display.depthPerComponent ?? 8,
    colorSpace: display.colorSpace ?? '',
    detected: display.detected ?? true,
    maximumCursorSize: display.maximumCursorSize
      ? { ...display.maximumCursorSize }
      : { width: 0, height: 0 },
    nativeOrigin: display.nativeOrigin
      ? { ...display.nativeOrigin }
      : { x: Math.round(bounds.x * scaleFactor), y: Math.round(bounds.y * scaleFactor) },
  };
}

function contains(rect: Rectangle, p: Point): boolean {
  return p.x >= rect.x && p.y >= rect.y && p.x < rect.x + rect.width && p.y < rect.y + rect.height;
}

function distance(rect: Rectangle, p: Point): number {
  const dx = Math.max(rect.x - p.x, 0, p.x - (rect.x + rect.width));
  const dy = Math.max(rect.y - p.y, 0, p.y - (rect.y + rect.height));
  return Math.hypot(dx, dy);
}

function overlap(a: Rectangle, b: Rectangle): number {
  const w = Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x);
  const h = Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y);
  return w > 0 && h > 0 ? w * h : 0;
}

function physicalRect(d: Required<Display>): Rectangle {
  return {
    x: d.nativeOrigin.x,
    y: d.nativeOrigin.y,
    width: Math.round(d.bounds.width * d.scaleFactor),
    height: Math.round(d.bounds.height * d.scaleFactor),
  };
}

const FALLBACK: Display = {
  id: 0,
  label: '',
  bounds: { x: 0, y: 0, width: 1920, height: 1080 },
  workArea: { x: 0, y: 0, width: 1920, height: 1080 },
  scaleFactor: 1,
};

/**
 * Electron's `Screen` (CONTRACT B.2.5). Events: `display-added`,
 * `display-removed`, `display-metrics-changed` (from the cached display
 * list, which the plugin refreshes by polling).
 */
export class Screen extends EventEmitter {
  readonly #kernel: FacadeKernel;
  #cursor: Point = { x: 0, y: 0 };
  #cursorAt = 0;
  #generation = 0;

  /**
   * @param k - the kernel
   * @internal
   */
  constructor(k: FacadeKernel) {
    super();
    this.#kernel = k;
    k.state.onChange((path, value, previous) => {
      if (path === 'displays') this.#diff(previous, value);
      else if (path === 'cursor') this.#seed(value);
    });
    this.#seed(k.state.get('cursor'));
    k.onReset(() => {
      this.removeAllListeners();
      this.#cursor = { x: 0, y: 0 };
      this.#cursorAt = 0;
      this.#prime();
    });
    this.#prime();
  }

  /**
   * Seeds the cursor from the snapshot (`cursor`, when the host provides it)
   * and fetches a fresh position once the main webview is ready, so the first
   * `getCursorScreenPoint()` does not report the origin.
   */
  #prime(): void {
    const generation = ++this.#generation;
    void this.#kernel.whenHostReady().then(() => {
      if (generation === this.#generation && this.#kernel.context === 'main') this.#refresh();
    });
  }

  #seed(value: unknown): void {
    const point = value as Partial<Point> | null | undefined;
    if (typeof point?.x === 'number' && typeof point.y === 'number')
      this.#cursor = { x: point.x, y: point.y };
  }

  #refresh(): void {
    this.#cursorAt = Date.now();
    this.#kernel
      .command('screen_snapshot')
      .then((snapshot) => {
        const cursor = (snapshot as { cursor?: Point } | null)?.cursor;
        if (cursor && typeof cursor.x === 'number' && typeof cursor.y === 'number')
          this.#cursor = { x: cursor.x, y: cursor.y };
      })
      .catch((error: unknown) => {
        this.#kernel.log('debug', `screen_snapshot failed: ${(error as Error).message}`);
      });
  }

  /**
   * Every display.
   *
   * @returns fresh copies of the cached displays
   */
  getAllDisplays(): Required<Display>[] {
    this.#kernel.require('main', 'screen.getAllDisplays');
    return this.#displays().map(completeDisplay);
  }

  /**
   * The primary display.
   *
   * @returns a copy of the primary display
   */
  getPrimaryDisplay(): Required<Display> {
    this.#kernel.require('main', 'screen.getPrimaryDisplay');
    const list = this.#displays();
    const primaryId = this.#kernel.state.get('primaryDisplayId');
    return completeDisplay(list.find((d) => d.id === primaryId) ?? list[0] ?? FALLBACK);
  }

  /**
   * The display containing `point`, else the nearest one.
   *
   * @param point - a point in DIP
   * @returns a copy of the display
   */
  getDisplayNearestPoint(point: Point): Required<Display> {
    this.#kernel.require('main', 'screen.getDisplayNearestPoint');
    const list = this.#displays();
    const hit = list.find((d) => contains(d.bounds, point));
    if (hit) return completeDisplay(hit);
    let best = list[0] ?? FALLBACK;
    for (const d of list) if (distance(d.bounds, point) < distance(best.bounds, point)) best = d;
    return completeDisplay(best);
  }

  /**
   * The display that overlaps `rect` the most (or is nearest to its centre).
   *
   * @param rect - a rectangle in DIP
   * @returns a copy of the display
   */
  getDisplayMatching(rect: Rectangle): Required<Display> {
    this.#kernel.require('main', 'screen.getDisplayMatching');
    let best: Display | undefined;
    let bestArea = 0;
    for (const d of this.#displays()) {
      const area = overlap(d.bounds, rect);
      if (area > bestArea) {
        best = d;
        bestArea = area;
      }
    }
    return best
      ? completeDisplay(best)
      : this.getDisplayNearestPoint({ x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 });
  }

  /**
   * Partial: the cursor position from a cache that is filled when the app
   * becomes ready and refreshed at most every 100 ms. Electron reads the
   * position synchronously; here a call returns the cached value and, when
   * it is older than 100 ms, starts a refresh for the next call.
   *
   * @returns the cursor position in DIP
   *
   * @example
   * ```ts
   * const point = screen.getCursorScreenPoint();
   * const display = screen.getDisplayNearestPoint(point);
   * ```
   */
  getCursorScreenPoint(): Point {
    this.#kernel.require('main', 'screen.getCursorScreenPoint');
    if (Date.now() - this.#cursorAt >= CURSOR_TTL_MS) this.#refresh();
    return { ...this.#cursor };
  }

  /**
   * Partial: converts a DIP point to physical screen pixels using the cached
   * display list.
   *
   * @param point - a point in DIP
   * @returns the point in physical pixels
   */
  dipToScreenPoint(point: Point): Point {
    const d = this.getDisplayNearestPoint(point);
    return {
      x: Math.round(d.nativeOrigin.x + (point.x - d.bounds.x) * d.scaleFactor),
      y: Math.round(d.nativeOrigin.y + (point.y - d.bounds.y) * d.scaleFactor),
    };
  }

  /**
   * Partial: converts a physical screen point to DIP.
   *
   * @param point - a point in physical pixels
   * @returns the point in DIP
   */
  screenToDipPoint(point: Point): Point {
    this.#kernel.require('main', 'screen.screenToDipPoint');
    const list = this.#displays().map(completeDisplay);
    const d =
      list.find((x) => contains(physicalRect(x), point)) ?? list[0] ?? completeDisplay(FALLBACK);
    return {
      x: d.bounds.x + (point.x - d.nativeOrigin.x) / d.scaleFactor,
      y: d.bounds.y + (point.y - d.nativeOrigin.y) / d.scaleFactor,
    };
  }

  /**
   * Partial: converts a DIP rectangle to physical pixels.
   *
   * @param _window - ignored (Electron uses it to pick the display)
   * @param rect - a rectangle in DIP
   * @returns the rectangle in physical pixels
   */
  dipToScreenRect(_window: unknown, rect: Rectangle): Rectangle {
    const d = this.getDisplayMatching(rect);
    const origin = this.dipToScreenPoint({ x: rect.x, y: rect.y });
    return {
      ...origin,
      width: Math.round(rect.width * d.scaleFactor),
      height: Math.round(rect.height * d.scaleFactor),
    };
  }

  /**
   * Partial: converts a physical rectangle to DIP.
   *
   * @param _window - ignored
   * @param rect - a rectangle in physical pixels
   * @returns the rectangle in DIP
   */
  screenToDipRect(_window: unknown, rect: Rectangle): Rectangle {
    const origin = this.screenToDipPoint({ x: rect.x, y: rect.y });
    const d = this.getDisplayNearestPoint(origin);
    return { ...origin, width: rect.width / d.scaleFactor, height: rect.height / d.scaleFactor };
  }

  #displays(): Display[] {
    const value = this.#kernel.state.get('displays');
    return Array.isArray(value)
      ? (value as unknown[]).filter(
          (d): d is Display =>
            typeof d === 'object' && d !== null && typeof (d as Display).id === 'number',
        )
      : [];
  }

  #diff(previous: unknown, next: unknown): void {
    const before = new Map(
      (Array.isArray(previous) ? (previous as Display[]) : []).map((d) => [d.id, d]),
    );
    const after = Array.isArray(next) ? (next as Display[]) : [];
    for (const d of after) {
      const old = before.get(d.id);
      before.delete(d.id);
      if (!old) {
        emitFromHost(this, 'display-added', createEvent(), completeDisplay(d));
        continue;
      }
      const changed = METRICS.filter(
        (m) => JSON.stringify(old[m] ?? null) !== JSON.stringify(d[m] ?? null),
      );
      if (changed.length > 0)
        emitFromHost(this, 'display-metrics-changed', createEvent(), completeDisplay(d), changed);
    }
    for (const d of before.values())
      emitFromHost(this, 'display-removed', createEvent(), completeDisplay(d));
  }
}

/**
 * Electron's `screen` (main webview only).
 *
 * @example
 * ```ts
 * const { workArea } = screen.getPrimaryDisplay();
 * win.setBounds({ x: workArea.x + workArea.width - 400, y: workArea.y, width: 400, height: 300 });
 * screen.on('display-removed', () => win.center());
 * ```
 */
export const screen: Screen = kernel.singleton('electron.screen', () => new Screen(kernel));
