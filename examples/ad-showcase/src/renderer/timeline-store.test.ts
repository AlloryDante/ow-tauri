import { describe, expect, it, vi } from 'vitest';

import { clock, matches, spreadString, TimelineStore, type NewEntry } from './timeline-store.js';

const row = (cid: string, name: string, family: NewEntry['family'] = 'display'): NewEntry => ({
  t: 10,
  cid,
  name,
  family,
  payload: {},
  sinceMount: null,
});

describe('TimelineStore', () => {
  it('numbers rows, counts names and lists cids in first-seen order', () => {
    const store = new TimelineStore();
    store.add(row('b', 'impression'));
    store.add(row('a', 'display_ad_loaded'));
    store.add(row('b', 'display_ad_loaded'));
    expect(store.entries.map((e) => e.seq)).toEqual([1, 2, 3]);
    expect(store.count('display_ad_loaded')).toBe(2);
    expect(store.count('missing')).toBe(0);
    expect(store.counts()).toEqual([
      ['display_ad_loaded', 2],
      ['impression', 1],
    ]);
    expect(store.cids()).toEqual(['b', 'a']);
  });

  it('notifies subscribers until they unsubscribe', () => {
    const store = new TimelineStore();
    const listener = vi.fn();
    const off = store.subscribe(listener);
    store.add(row('a', 'x'));
    off();
    store.add(row('a', 'y'));
    expect(listener).toHaveBeenCalledTimes(1);
    expect(listener.mock.calls[0]?.[0]).toMatchObject({ seq: 1, name: 'x' });
  });

  it('exports metadata, counts and rows', () => {
    const store = new TimelineStore();
    store.add(row('a', 'x'));
    const doc = store.toExport(
      {
        host: 'ow-tauri',
        hostVersion: '0.1.0',
        mode: 'test',
        uidMasked: 'abcd…wxyz',
        platform: 'darwin',
      },
      new Date('2026-10-07T00:00:00.000Z'),
    );
    expect(doc).toMatchObject({
      kind: 'ow-tauri-ad-showcase-timeline',
      version: 1,
      exportedAt: '2026-10-07T00:00:00.000Z',
      host: 'ow-tauri',
      uidMasked: 'abcd…wxyz',
      counts: { x: 1 },
    });
    expect(doc['entries']).toHaveLength(1);
  });
});

describe('page visits', () => {
  it('scopes element rows to the visit that created the element, app rows to their arrival', () => {
    const store = new TimelineStore();
    const visits: number[] = [];
    store.onVisit((v) => visits.push(v));
    expect(store.beginVisit()).toBe(1);
    store.add(row('app', 'control:page', 'control'));
    store.add(row('sz1', 'did-attach', 'lifecycle'));
    store.add(row('sz1', 'display_ad_loaded'));
    expect(store.beginVisit()).toBe(2);
    store.add(row('app', 'control:page', 'control'));
    // A late row of the first page's element stays on the first page.
    store.add(row('sz1', 'destroyed', 'lifecycle'));
    store.add(row('ly1', 'display_ad_loaded'));
    expect(visits).toEqual([1, 2]);
    expect(store.visit).toBe(2);
    expect(store.entries.map((e) => [e.cid, e.visit])).toEqual([
      ['app', 1],
      ['sz1', 1],
      ['sz1', 1],
      ['app', 2],
      ['sz1', 1],
      ['ly1', 2],
    ]);
    expect(store.cids(2)).toEqual(['app', 'ly1']);
    expect(store.cids()).toEqual(['app', 'sz1', 'ly1']);
    expect(store.countsIn(2)).toEqual([
      ['control:page', 1],
      ['display_ad_loaded', 1],
    ]);
    expect(store.countsIn(1)).toEqual([
      ['control:page', 1],
      ['destroyed', 1],
      ['did-attach', 1],
      ['display_ad_loaded', 1],
    ]);
    expect(store.countsIn(null)).toEqual(store.counts());
  });

  it('puts the rows of a page visited again on the new visit', () => {
    const store = new TimelineStore();
    store.beginVisit();
    store.bindElement('sz1');
    store.add(row('sz1', 'display_ad_loaded'));
    store.beginVisit();
    store.add(row('sz1', 'destroyed', 'lifecycle'));
    store.beginVisit();
    // The page comes back and creates a new element with the same cid.
    store.bindElement('sz1');
    store.add(row('sz1', 'did-attach', 'lifecycle'));
    store.bindElement('app');
    store.add(row('app', 'control:page', 'control'));
    expect(store.entries.map((e) => [e.name, e.visit])).toEqual([
      ['display_ad_loaded', 1],
      ['destroyed', 1],
      ['did-attach', 3],
      ['control:page', 3],
    ]);
    expect(store.cids(3)).toEqual(['sz1', 'app']);
  });
});

describe('matches', () => {
  const entry = { seq: 1, visit: 3, ...row('a', 'x', 'video') };

  it('filters by visit, cid and family', () => {
    expect(matches(entry, { visit: null, cid: null, family: null })).toBe(true);
    expect(matches(entry, { visit: 3, cid: 'a', family: 'video' })).toBe(true);
    expect(matches(entry, { visit: 2, cid: null, family: null })).toBe(false);
    expect(matches(entry, { visit: null, cid: 'b', family: null })).toBe(false);
    expect(matches(entry, { visit: null, cid: null, family: 'display' })).toBe(false);
  });
});

describe('clock', () => {
  it('formats mm:ss.mmm', () => {
    expect(clock(0)).toBe('00:00.000');
    expect(clock(61_234)).toBe('01:01.234');
    expect(clock(-5)).toBe('00:00.000');
  });
});

describe('spreadString', () => {
  it('reads back a string spread one character per key', () => {
    expect(
      spreadString(Object.fromEntries(Array.from('hello', (c) => c).map((c, i) => [String(i), c]))),
    ).toBe('hello');
  });

  it('is null for other payloads', () => {
    expect(spreadString({})).toBeNull();
    expect(spreadString({ a: 'b' })).toBeNull();
    expect(spreadString({ 0: 'ab' })).toBeNull();
  });
});
