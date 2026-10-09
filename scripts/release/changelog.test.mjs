// node --test scripts/release/*.test.mjs
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, it } from 'node:test';

import { checkSection, findSection, isDated, stamp } from './changelog.mjs';
import { check, REPO_ROOT } from './version-sync.mjs';

const TEXT = `# Changelog

## [Unreleased]

## [1.0.0-rc.2] - Unreleased

### Fixed

- Two.

## [1.0.0-rc.1] - 2026-10-10

### Added

- One.

## [0.9.0] - 2026-01-01

[Unreleased]: https://example.invalid/compare/v1.0.0-rc.1...HEAD
[1.0.0-rc.1]: https://example.invalid/releases/tag/v1.0.0-rc.1
`;

describe('findSection', () => {
  it('returns the body up to the next heading', () => {
    const s = findSection(TEXT, '1.0.0-rc.1');
    assert.equal(s.date, '2026-10-10');
    assert.equal(s.body, '### Added\n\n- One.');
  });
  it('stops at the link references', () => {
    assert.equal(findSection(TEXT, '0.9.0').body, '');
  });
  it('does not match a version that only starts the same', () => {
    assert.equal(findSection(TEXT, '1.0.0'), undefined);
    assert.equal(findSection(TEXT, '1.0.0-rc'), undefined);
  });
});

describe('checkSection', () => {
  it('passes a dated section with content', () => {
    assert.deepEqual(checkSection(TEXT, '1.0.0-rc.1', { dated: true }), []);
  });
  it('reports a missing, empty or undated section', () => {
    assert.match(checkSection(TEXT, '2.0.0')[0], /no "## \[2\.0\.0\]" section/);
    assert.match(checkSection(TEXT, '0.9.0')[0], /empty/);
    assert.deepEqual(checkSection(TEXT, '1.0.0-rc.2'), []);
    assert.match(checkSection(TEXT, '1.0.0-rc.2', { dated: true })[0], /no release date/);
  });
});

describe('stamp', () => {
  it('dates an Unreleased heading and nothing else', () => {
    const out = stamp(TEXT, '1.0.0-rc.2', '2026-10-11');
    assert.equal(out, TEXT.replace('## [1.0.0-rc.2] - Unreleased', '## [1.0.0-rc.2] - 2026-10-11'));
  });
  it('leaves a dated heading alone', () => {
    assert.equal(stamp(TEXT, '1.0.0-rc.1', '2026-10-11'), TEXT);
  });
  it('refuses bad dates and missing sections', () => {
    assert.throws(() => stamp(TEXT, '1.0.0-rc.2', '2026-02-30'), /date/);
    assert.throws(() => stamp(TEXT, '3.0.0', '2026-10-11'), /no "## \[3\.0\.0\]"/);
  });
  it('validates dates', () => {
    assert.equal(isDated('2026-10-09'), true);
    assert.equal(isDated('Unreleased'), false);
    assert.equal(isDated('2026-13-01'), false);
    assert.equal(isDated(undefined), false);
  });
});

describe('this repository', () => {
  it('has an Unreleased section and, once the version is a release, its section', () => {
    const text = readFileSync(join(REPO_ROOT, 'CHANGELOG.md'), 'utf8');
    assert.ok(findSection(text, 'Unreleased'), 'an [Unreleased] heading');
    const { version } = check(REPO_ROOT);
    // Before the first release bump the manifests still carry 0.1.0, which
    // was never released and has no section.
    if (version !== '0.1.0') assert.deepEqual(checkSection(text, version), []);
  });
});
