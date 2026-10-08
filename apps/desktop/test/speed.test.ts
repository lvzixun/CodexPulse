import test from 'node:test';
import assert from 'node:assert/strict';
import { rateValue, tierKey, speedGeometry, speedTime } from '../src/lib/speed.ts';
import type { SpeedPoint } from '../src/lib/types.ts';
const point = (at: string, output = 100, elapsed = 1000): SpeedPoint => ({
  started_at: at,
  measured_at: at,
  output_tokens: output,
  elapsed_ms: elapsed,
  samples: 1,
  service_tier: null,
});
test('speed requires positive output and duration; mode never inferred from magnitude', () => {
  assert.equal(rateValue(null), null);
  assert.equal(rateValue(point('x', 0)), null);
  assert.equal(rateValue(point('x', 1, 0)), null);
  assert.equal(rateValue(point('x', 200, 10000)), 20);
  assert.equal(tierKey(null), '未知');
  assert.equal(tierKey('priority'), 'Fast');
  assert.equal(tierKey('default'), '标准');
  assert.equal(tierKey('mixed'), '混合');
  assert.equal(tierKey('custom'), '未知');
});
test('curve uses real elapsed local time and continuous non-overshooting segments', () => {
  const data = [
    point('2026-10-08T00:00:00Z', 10),
    point('2026-10-08T01:00:00Z', 20),
    point('2026-10-08T10:00:00Z', 30),
  ];
  const g = speedGeometry(data);
  assert.equal(g.points[1].x, 58.1);
  assert.equal(g.points[2].x, 311);
  assert.equal((g.path.match(/ C/g) || []).length, 2);
  assert.doesNotMatch(g.path, /NaN|Infinity/);
  assert.equal(speedTime(g.first, 'en-US', 'Asia/Shanghai'), '08:00');
  assert.equal(speedTime(g.last, 'en-US', 'America/Los_Angeles'), '03:00');
});
test('duplicate timestamps are weighted, invalid samples omitted, singleton remains visible', () => {
  const at = '2026-10-08T00:00:00Z',
    g = speedGeometry([point(at, 100, 1000), point(at, 100, 9000), point('invalid'), point(at, 0)]);
  assert.equal(g.points.length, 1);
  assert.equal(rateValue(g.points[0].sample), 20);
  assert.equal(g.points[0].x, 173);
  assert.equal(g.points[0].sample.samples, 2);
  assert.ok(g.path.startsWith('M'));
  assert.equal(speedGeometry([]).path, '');
});
