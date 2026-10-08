import { describe, expect, it } from 'vitest';

import {
  NO_INSETS,
  contentOf,
  initialFrame,
  insetsBetween,
  outerOf,
  placementOf,
} from './geometry.js';

// The same vectors as the plugin's `window::geometry` tests.
const WORK = { x: 0, y: 0, width: 1024, height: 720 };
const SCREEN = { x: 0, y: 0, width: 1024, height: 768 };

describe('window geometry (B.2.2)', () => {
  it('reproduces the Windows lab observation', () => {
    // [OBS] WE-high-impact: 1000 x 760 at (0, 0), work area 1024 x 720.
    const f = initialFrame([1000, 760], [0, 0], 'fit-work-area', WORK, SCREEN);
    expect(f.created).toEqual({ x: 12, y: 0, width: 1000, height: 720 });
    expect(f.outer).toEqual({ x: 0, y: 0, width: 1000, height: 720 });
    expect(contentOf(f.outer, { left: 8, top: 31, right: 8, bottom: 8 })).toEqual({
      x: 8,
      y: 31,
      width: 984,
      height: 681,
    });
  });

  it('reproduces the macOS lab observations', () => {
    // [OBS] macOS lab: 1470 x 956 display, menu bar 33, Dock 86.
    const screen = { x: 0, y: 0, width: 1470, height: 956 };
    const work = { x: 0, y: 33, width: 1470, height: 837 };
    const f = initialFrame([1000, 324], [0, 0], 'screen-center', work, screen);
    expect(f.created).toEqual({ x: 235, y: 316, width: 1000, height: 324 });
    expect(f.outer).toEqual({ x: 0, y: 0, width: 1000, height: 324 });
    for (const [width, height, x, y] of [
      [1000, 274, 235, 341],
      [1000, 624, 235, 166],
      [1000, 760, 235, 98],
      [1200, 800, 135, 70],
      [1000, 837, 235, 33],
      [800, 800, 335, 70],
    ] as const) {
      expect(initialFrame([width, height], null, 'screen-center', work, screen).created).toEqual({
        x,
        y,
        width,
        height,
      });
    }
  });

  it('resizes to the work area only where Electron does', () => {
    expect(initialFrame([2000, 900], null, 'fit-work-area', WORK, SCREEN).outer).toEqual({
      x: 0,
      y: 0,
      width: 1024,
      height: 720,
    });
    // macOS keeps the size and starts it at the work area's origin.
    expect(initialFrame([2000, 900], null, 'screen-center', WORK, SCREEN).outer).toEqual({
      x: 0,
      y: 0,
      width: 2000,
      height: 900,
    });
    const tall = { x: 0, y: 33, width: 1024, height: 700 };
    expect(initialFrame([600, 900], null, 'screen-center', tall, SCREEN).outer).toEqual({
      x: 212,
      y: 33,
      width: 600,
      height: 900,
    });
    // Halves round away from zero, as C's round.
    const wide = { x: -10, y: -10, width: 1044, height: 788 };
    expect(initialFrame([1025, 769], null, 'screen-center', wide, SCREEN).outer).toMatchObject({
      x: -1,
      y: -1,
    });
  });

  it('centres with truncated offsets from the work area origin', () => {
    const work = { x: 100, y: 40, width: 1001, height: 701 };
    const f = initialFrame([800, 600], null, 'fit-work-area', work, SCREEN);
    expect(f.outer).toEqual({ x: 200, y: 90, width: 800, height: 600 });
    expect(f.created).toEqual(f.outer);
  });

  it('measures insets and converts both ways', () => {
    const outer = { x: 10, y: 20, width: 300, height: 200 };
    const content = { x: 12, y: 50, width: 296, height: 168 };
    const insets = insetsBetween(outer, content);
    expect(insets).toEqual({ left: 2, top: 30, right: 2, bottom: 2 });
    expect(outerOf(content, insets)).toEqual(outer);
    expect(contentOf(outer, insets)).toEqual(content);
    expect(insetsBetween(content, outer)).toEqual(NO_INSETS);
    // [OBS] R3-perf-test: a 1 x 32 window with a 32 px title bar.
    expect(contentOf({ x: 735, y: 478, width: 1, height: 32 }, { ...NO_INSETS, top: 32 })).toEqual({
      x: 735,
      y: 510,
      width: 1,
      height: 0,
    });
    expect(placementOf('darwin')).toBe('screen-center');
    expect(placementOf('win32')).toBe('fit-work-area');
    expect(placementOf('linux')).toBe('fit-work-area');
  });
});
