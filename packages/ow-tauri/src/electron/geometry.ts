/**
 * Electron's window geometry (CONTRACT B.2.2), mirrored from the plugin's
 * `window::geometry` so the facade's synchronous getters agree with the
 * native window before `window_create` answers.
 *
 * `width` / `height` size the outer frame unless `useContentSize`; Windows
 * and Linux centre a new window in the primary work area with its size
 * clamped to it, macOS centres it on the primary display's full frame and
 * moves it back into the work area when it reaches past its bottom or right
 * edge; then `x` / `y` move it.
 *
 * @packageDocumentation
 */
import type { Rectangle } from '../shared/protocol.js';

/** The frame around the content area, in logical pixels. */
export interface Insets {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/** No frame (frameless windows, or before the frame is known). */
export const NO_INSETS: Readonly<Insets> = Object.freeze({ left: 0, top: 0, right: 0, bottom: 0 });

/** How the platform's Electron places a new window. */
export type Placement = 'fit-work-area' | 'screen-center';

/**
 * The placement of a Node `process.platform` value.
 *
 * @param platform - `darwin`, `win32`, `linux`, ...
 * @returns `screen-center` on macOS, else `fit-work-area`
 */
export function placementOf(platform: unknown): Placement {
  return platform === 'darwin' ? 'screen-center' : 'fit-work-area';
}

/**
 * The insets of `content` inside `outer` (negative values count as zero).
 *
 * @param outer - the frame
 * @param content - the content area
 * @returns the insets
 */
export function insetsBetween(outer: Rectangle, content: Rectangle): Insets {
  const left = Math.max(0, content.x - outer.x);
  const top = Math.max(0, content.y - outer.y);
  return {
    left,
    top,
    right: Math.max(0, outer.width - content.width - left),
    bottom: Math.max(0, outer.height - content.height - top),
  };
}

/**
 * The content area of a window whose frame is `outer` (never negative).
 *
 * @param outer - the frame
 * @param insets - the frame insets
 * @returns the content area
 */
export function contentOf(outer: Rectangle, insets: Insets): Rectangle {
  return {
    x: outer.x + insets.left,
    y: outer.y + insets.top,
    width: Math.max(0, outer.width - insets.left - insets.right),
    height: Math.max(0, outer.height - insets.top - insets.bottom),
  };
}

/**
 * The frame of a window whose content area is `content`.
 *
 * @param content - the content area
 * @param insets - the frame insets
 * @returns the frame
 */
export function outerOf(content: Rectangle, insets: Insets): Rectangle {
  return {
    x: content.x - insets.left,
    y: content.y - insets.top,
    width: content.width + insets.left + insets.right,
    height: content.height + insets.top + insets.bottom,
  };
}

/**
 * C's `round`: halves away from zero (`Math.round` rounds them up).
 *
 * @param value - the number
 * @returns the rounded number
 */
function round(value: number): number {
  return Math.sign(value) * Math.round(Math.abs(value));
}

/**
 * Moves a span starting at `start` back into `[origin, origin + extent]`:
 * back from the far edge first, then never before `origin`.
 *
 * @param start - the span's start
 * @param size - the span's length
 * @param origin - the area's start
 * @param extent - the area's length
 * @returns the moved start
 */
function intoArea(start: number, size: number, origin: number, extent: number): number {
  return Math.max(origin, Math.min(start, origin + extent - size));
}

/** A new window's frame before and after its `x` / `y` options. */
export interface InitialFrame {
  /** The frame when `browser-window-created` is emitted (centred). */
  created: Rectangle;
  /** The frame once the constructor returns. */
  outer: Rectangle;
}

/**
 * The frame Electron gives a new window of outer size `size`.
 *
 * @param size - the outer `[width, height]`
 * @param position - `[x, y]` when both options are given
 * @param placement - the platform's placement
 * @param workArea - the primary display's work area
 * @param screen - the primary display's full bounds
 * @returns the frame before and after `x` / `y`
 */
export function initialFrame(
  size: [number, number],
  position: [number, number] | null,
  placement: Placement,
  workArea: Rectangle,
  screen: Rectangle,
): InitialFrame {
  const width = Math.max(0, size[0]);
  const height = Math.max(0, size[1]);
  let created: Rectangle;
  if (placement === 'fit-work-area') {
    const w = Math.min(width, workArea.width);
    const h = Math.min(height, workArea.height);
    created = {
      x: workArea.x + Math.trunc((workArea.width - w) / 2),
      y: workArea.y + Math.trunc((workArea.height - h) / 2),
      width: w,
      height: h,
    };
  } else {
    created = {
      x: intoArea(screen.x + round((screen.width - width) / 2), width, workArea.x, workArea.width),
      y: intoArea(
        screen.y + round((screen.height - height) / 2),
        height,
        workArea.y,
        workArea.height,
      ),
      width,
      height,
    };
  }
  const outer = position ? { ...created, x: position[0], y: position[1] } : { ...created };
  return { created, outer };
}
