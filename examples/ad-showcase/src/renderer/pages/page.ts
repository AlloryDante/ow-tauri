/**
 * What every page gets, and the page shape.
 *
 * @packageDocumentation
 */
import type { HostInfo, ShowcaseApi, WindowEvent } from '../../shared/ipc.js';
import type { AdContext } from '../slots.js';

/** The context a page mounts with. */
export interface PageContext extends AdContext {
  /** Host, mode and identity. */
  info: HostInfo;
  /** The preload API. */
  api: ShowcaseApi;
  /** Collapses or expands the timeline rail. */
  setRailCollapsed(collapsed: boolean): void;
  /** Whether the rail is collapsed. */
  railCollapsed(): boolean;
  /**
   * Subscribes to window state changes for the page's lifetime.
   *
   * @returns an unsubscribe function
   */
  onWindowEvent(listener: (event: WindowEvent) => void): () => void;
}

/** Mounts a page into `root`; returns its cleanup (remove elements, stop timers). */
export type MountPage = (root: HTMLElement, ctx: PageContext) => () => void;

/** A page of the sidebar. */
export interface PageDef {
  /** Keyboard shortcut and position, 1 to 9. */
  n: number;
  /** Stable id (`data-page`). */
  id: string;
  /** Sidebar label. */
  label: string;
  /** Mounts the page. */
  mount: MountPage;
}
