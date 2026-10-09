/**
 * Test helpers: render a React tree into a happy-dom container inside
 * `act()`, with the app's log store.
 *
 * @packageDocumentation
 */
import { act, StrictMode, type ReactElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';

import { LogContext } from '../log/context';
import { createLogStore, type LogStore } from '../log/store';

// React's act() warns unless the environment says it is a test.
Reflect.set(globalThis, 'IS_REACT_ACT_ENVIRONMENT', true);

/** A rendered tree. */
export interface Rendered {
  /** The container element (in the document). */
  container: HTMLElement;
  /** The log store the tree writes to. */
  log: LogStore;
  /** Unmounts the tree and removes the container. */
  unmount: () => Promise<void>;
}

/**
 * Renders `ui` under StrictMode with a log store.
 *
 * @param ui - the element
 * @param log - the store (default: a new one)
 * @returns the rendered tree
 */
export async function render(
  ui: ReactElement,
  log: LogStore = createLogStore(),
): Promise<Rendered> {
  const container = document.createElement('div');
  document.body.append(container);
  let root: Root | undefined;
  await act(async () => {
    root = createRoot(container);
    root.render(
      <StrictMode>
        <LogContext value={log}>{ui}</LogContext>
      </StrictMode>,
    );
    await Promise.resolve();
  });
  return {
    container,
    log,
    unmount: async () => {
      await act(async () => {
        root?.unmount();
        await Promise.resolve();
      });
      container.remove();
    },
  };
}

/**
 * Clicks `el` inside `act()`.
 *
 * @param el - the element
 */
export async function click(el: Element | null | undefined): Promise<void> {
  if (!(el instanceof HTMLElement)) throw new Error('nothing to click');
  await act(async () => {
    el.click();
    await Promise.resolve();
  });
}

/**
 * Lets pending promises, timers and React updates run.
 *
 * @param rounds - timer turns (default 4)
 */
export async function flush(rounds = 4): Promise<void> {
  await act(async () => {
    for (let i = 0; i < rounds; i++) {
      await new Promise<void>((resolve) => {
        setTimeout(resolve, 0);
      });
    }
  });
}

/**
 * The button whose text is `text`.
 *
 * @param root - where to look
 * @param text - the exact button text
 * @returns the button
 */
export function button(root: ParentNode, text: string): HTMLButtonElement {
  const found = [...root.querySelectorAll('button')].find((b) => b.textContent === text);
  if (!found) throw new Error(`no button "${text}"`);
  return found;
}
