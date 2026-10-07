/**
 * A tiny element builder, so the pages stay plain DOM without a framework.
 *
 * @packageDocumentation
 */

/** Attributes and properties `h()` understands. */
export interface Props {
  /** `className`. */
  class?: string;
  /** Text content (set before children). */
  text?: string;
  /** `data-*` attributes, without the prefix. */
  data?: Record<string, string>;
  /** Inline style text. */
  style?: string;
  /** Other attributes. */
  attrs?: Record<string, string>;
  /** Click handler. */
  onClick?: (event: MouseEvent) => void;
}

/**
 * Creates an element.
 *
 * @param tag - the tag name
 * @param props - attributes, text and a click handler
 * @param children - child nodes or strings
 * @returns the element
 */
export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Props = {},
  ...children: (Node | string | null | undefined | false)[]
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  if (props.class) el.className = props.class;
  if (props.text !== undefined) el.textContent = props.text;
  if (props.style) el.setAttribute('style', props.style);
  for (const [k, v] of Object.entries(props.data ?? {})) el.dataset[k] = v;
  for (const [k, v] of Object.entries(props.attrs ?? {})) el.setAttribute(k, v);
  const onClick = props.onClick;
  if (onClick) {
    el.addEventListener('click', (event) => {
      onClick(event as MouseEvent);
    });
  }
  for (const child of children) {
    if (child === null || child === undefined || child === false) continue;
    el.append(child);
  }
  return el;
}

/**
 * Creates a `<button type="button">`.
 *
 * @param label - the visible label
 * @param action - a stable `data-action` id (the lab driver presses buttons by it)
 * @param onClick - the click handler
 * @param cls - extra classes
 * @returns the button
 */
export function button(
  label: string,
  action: string,
  onClick: (event: MouseEvent) => void,
  cls = '',
): HTMLButtonElement {
  return h('button', {
    class: `btn ${cls}`.trim(),
    text: label,
    data: { action },
    attrs: { type: 'button' },
    onClick,
  });
}

/**
 * A page heading with a one-line description, and optional controls on the
 * same row (pages whose ads must fit the window height put their toolbar
 * there).
 *
 * @param title - the page title
 * @param description - what the page shows
 * @param aside - controls shown to the right of the heading
 * @returns the header element
 */
export function pageHeader(title: string, description: string, aside?: Node): HTMLElement {
  const text = h(
    'div',
    { class: 'page-head-text' },
    h('h1', { text: title }),
    h('p', { text: description }),
  );
  if (!aside) return h('header', { class: 'page-head' }, text);
  return h('header', { class: 'page-head page-head-row' }, text, aside);
}

/**
 * A note box (`info`, `warn`).
 *
 * @param kind - the tone
 * @param children - the content
 * @returns the note element
 */
export function note(kind: 'info' | 'warn', ...children: (Node | string)[]): HTMLElement {
  return h('div', { class: `note note-${kind}`, attrs: { role: 'note' } }, ...children);
}
