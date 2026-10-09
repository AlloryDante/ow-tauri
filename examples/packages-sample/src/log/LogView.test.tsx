import { act } from 'react';
import { describe, expect, it } from 'vitest';

import { button, click, flush, render } from '../test/render';
import { JsonNode } from './JsonNode';
import { LogView } from './LogView';
import { createLogStore } from './store';

describe('LogView', () => {
  it('shows every entry with its values, and new ones as they come', async () => {
    const log = createLogStore();
    log.push('info', 'app', 'started');
    const view = await render(<LogView label="Log" />, log);
    expect(view.container.querySelectorAll('.log-entry')).toHaveLength(1);
    await act(async () => {
      log.push('error', 'api', 'getMachineIds() failed: forbidden', 'not granted');
      await Promise.resolve();
    });
    const entries = view.container.querySelectorAll('.log-entry');
    expect(entries).toHaveLength(2);
    expect(entries[1]?.className).toBe('log-entry error');
    expect(entries[1]?.textContent).toContain('"not granted"');
    await view.unmount();
  });

  it('filters by source and by search, and clears the log', async () => {
    const log = createLogStore();
    log.push('info', 'app', 'started');
    log.push('success', 'ad', 'ad1 300x250: display_ad_loaded');
    log.push('success', 'ad', 'ad1 300x250: impression');
    const ads = await render(<LogView label="Ads" sources={['ad']} search={false} />, log);
    expect(ads.container.querySelectorAll('.log-entry')).toHaveLength(2);
    expect(ads.container.querySelector('.log-search')).toBeNull();
    await ads.unmount();

    const view = await render(<LogView label="Log" />, log);
    const input = view.container.querySelector<HTMLInputElement>('.log-search')!;
    await act(async () => {
      // React tracks the value it set; the prototype setter bypasses that.
      Reflect.set(HTMLInputElement.prototype, 'value', 'IMPRESSION', input);
      input.dispatchEvent(new Event('input', { bubbles: true }));
      await Promise.resolve();
    });
    expect(view.container.querySelectorAll('.log-entry')).toHaveLength(1);
    expect(view.container.querySelector('.log-search-count')?.textContent).toBe('1 / 3');
    await click(button(view.container, 'Clear'));
    await flush();
    expect(log.entries()).toEqual([]);
    expect(view.container.querySelectorAll('.log-entry')).toHaveLength(0);
    await view.unmount();
  });
});

describe('JsonNode', () => {
  it('shows primitives in colour classes', async () => {
    const view = await render(
      <>
        <JsonNode value="text" />
        <JsonNode value={3} />
        <JsonNode value={true} />
        <JsonNode value={null} />
        <JsonNode value={[]} />
      </>,
    );
    const text = view.container.textContent;
    expect(text).toBe('"text"3truenull[]');
    expect(view.container.querySelector('.jv-number')?.textContent).toBe('3');
    await view.unmount();
  });

  it('starts collapsed with a preview and expands on click', async () => {
    const value = { a: 1, b: 'a long string that is cut in the preview', c: [1], d: {}, e: 5 };
    const view = await render(<JsonNode value={value} />);
    const toggle = view.container.querySelector<HTMLButtonElement>('.jv-toggle')!;
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(view.container.textContent).toContain("'a long string that is cu…'");
    expect(view.container.querySelector('.jv-preview-more')).not.toBeNull();
    await click(toggle);
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    expect(view.container.querySelectorAll('.jv-row').length).toBeGreaterThanOrEqual(5);
    expect(view.container.textContent).toContain('"a long string that is cut in the preview"');
    await view.unmount();
  });
});
