import { test } from 'node:test';
import assert from 'node:assert/strict';
import { resetCalendar } from '../src/lib/reset-calendar.ts';

test('the calendar stays within half a year and uses the displayed local date', () => {
  const now = Date.parse('2026-10-06T15:00:00Z');
  const cells = resetCalendar(
    [
      { occurred_at: '2026-10-05T17:00:00Z', reset_type: 'regular' },
      { occurred_at: '2026-10-06T00:00:00Z', reset_type: 'banked' },
      { occurred_at: '2025-01-01T00:00:00Z', reset_type: 'regular' },
    ],
    now,
    'Asia/Shanghai',
    true,
  );
  assert.ok(cells.length >= 183 && cells.length <= 189);
  assert.ok(cells.at(-1)!.column <= 27);
  assert.equal(cells.at(-1)!.date, '2026-10-06');
  assert.equal(cells.at(-1)!.type, 'both');
  assert.equal(cells.at(-1)!.entries.length, 2);
  assert.ok(cells.every((cell) => cell.date >= '2026-04-01'));
});

test('incomplete older history stays unknown and complete empty days are visible', () => {
  const now = Date.parse('2026-10-06T12:00:00Z');
  const history = [{ occurred_at: '2026-10-03T00:00:00Z', reset_type: 'regular' }];
  const partial = resetCalendar(history, now, 'UTC', false);
  assert.equal(partial.find((cell) => cell.date === '2026-10-02')!.type, 'unknown');
  assert.equal(partial.at(-1)!.type, 'none');
  const complete = resetCalendar(history, now, 'UTC', true);
  assert.equal(complete.find((cell) => cell.date === '2026-10-02')!.type, 'none');
});
