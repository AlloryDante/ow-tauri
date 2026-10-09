import { describe, expect, it } from 'vitest';

import { formulaUid, reportedAppIds } from './screenshots.mjs';

describe('formulaUid', () => {
  it('matches the documented example', () => {
    expect(formulaUid('Example Studio', 'Parity Harness')).toBe(
      'bijigndkghcikkfmhgkmicdkjpdehpjafgpmdhcc',
    );
  });
});

describe('reportedAppIds', () => {
  const counter = (id) =>
    JSON.stringify({
      path: `/analytics/Counter?Name=x&Extra=${encodeURIComponent(JSON.stringify({ app_id: id }))}`,
    });

  it('collects the app ids of Counter requests only', () => {
    const text = [
      counter('aaa'),
      counter('aaa'),
      JSON.stringify({ path: '/tracking/InsertStats?Stats=true' }),
      'not json',
      '',
    ].join('\n');
    expect([...reportedAppIds(text)]).toEqual(['aaa']);
  });

  it('sees a second uid', () => {
    expect(reportedAppIds([counter('aaa'), counter('bbb')].join('\n')).size).toBe(2);
  });
});
