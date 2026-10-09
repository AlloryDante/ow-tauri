// Tests of the scenario index (lib/scenarios.mjs).
//
//   node --test scenarios.test.mjs

import assert from 'node:assert/strict';
import { test } from 'node:test';

import { LAB_SCENARIOS } from './ci/windows-lab.mjs';
import { SCENARIOS, SECTION_5_2 } from './lib/scenarios.mjs';

test('SECTION_5_2 lists the checks 1 to 23 once each', () => {
  assert.deepEqual(
    SECTION_5_2.map((c) => c.n),
    Array.from({ length: 23 }, (_, i) => i + 1),
  );
});

test('every scenario of SECTION_5_2 is defined, with a description and defaults', () => {
  for (const check of SECTION_5_2) {
    assert.ok(check.scenarios.length || check.missing, `#${check.n} has no scenario`);
    for (const name of check.scenarios) {
      const s = SCENARIOS[name];
      assert.ok(s, `#${check.n}: ${name} is not defined`);
      assert.equal(typeof s.describe, 'string');
      assert.equal(typeof s.defaults.duration, 'number');
      assert.ok(Array.isArray(s.config.actions), `${name} has no actions list`);
    }
  }
});

test('Windows-only checks are marked and run in the Windows CI lab', () => {
  for (const check of SECTION_5_2.filter((c) => c.lab === 'windows-ci'))
    for (const name of check.scenarios) assert.equal(SCENARIOS[name].windowsOnly, true, name);
  assert.ok(LAB_SCENARIOS.includes('local-frame'));
});

test('no scenario clicks: only the gesture fixture sends input', () => {
  for (const [name, s] of Object.entries(SCENARIOS)) {
    for (const action of s.config?.actions ?? []) {
      if (action.do === 'hit-probe' && action.click) assert.notEqual(action.click, 'ad', name);
      if (action.do === 'gesture-case') assert.equal(s.config.gestureFixture, true, name);
    }
  }
});

test('guest-eval code is an expression (both hosts evaluate it as one)', () => {
  for (const [name, s] of Object.entries(SCENARIOS))
    for (const action of s.config?.actions ?? [])
      if (action.do === 'guest-eval')
        assert.ok(!/^\s*(\(\)|async\s*\(\))\s*=>/.test(action.code), `${name}: ${action.label}`);
});
