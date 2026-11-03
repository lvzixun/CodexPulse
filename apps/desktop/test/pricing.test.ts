import test from 'node:test';
import assert from 'node:assert/strict';
import { referenceMoney, referenceExplanation } from '../src/lib/pricing.ts';
import type { ReferenceEstimate } from '../src/lib/types';

const quote = (overrides: Partial<ReferenceEstimate> = {}): ReferenceEstimate => ({
  price_date: '2026-11-03',
  cost_nanousd: 1e9,
  max_cost_nanousd: 2e9,
  unpriced_tokens: 0,
  unpriced_events: 0,
  pending: false,
  ...overrides,
});
test('pricing ranges, partial coverage, zero costs and unavailable models remain distinct', () => {
  assert.equal(referenceMoney(quote(), 'en'), '$1.00–$2.00');
  assert.equal(referenceMoney(quote({ unpriced_events: 2 }), 'en'), '$1.00–$2.00*');
  assert.equal(referenceMoney(quote({ cost_nanousd: 0, max_cost_nanousd: 0 }), 'en'), '$0.00');
  const missing = quote({
    cost_nanousd: 0,
    max_cost_nanousd: 0,
    unpriced_events: 1,
    reasons: ['unknown_model_id'],
  });
  assert.equal(referenceMoney(missing, 'en'), 'Model ID not mapped');
  assert.equal(referenceMoney(missing, 'zh'), '模型标识未关联');
  assert.equal(referenceMoney(quote({ pending: true }), 'en'), 'Calculating…');
  assert.equal(referenceMoney(quote({ pending: true, error: true }), 'en'), 'Estimate failed');
});
test('price assumptions and missing-data reasons are visible and localized', () => {
  const value = quote({ estimate_flags: 1 | 2 | 4 | 16 | 32, reasons: ['token_details_missing'] });
  const en = referenceExplanation(value, 'en');
  assert.match(en, /Standard/);
  assert.match(en, /context/);
  assert.match(en, /cache write/);
  assert.match(en, /peak/);
  assert.doesNotMatch(en, /\p{Script=Han}/u);
  assert.match(referenceExplanation(value, 'zh'), /标准档/);
});
