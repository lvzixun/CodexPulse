import { test } from 'node:test';
import assert from 'node:assert/strict';
import { activeResetWatch, resetWatchCountdown } from '../src/lib/reset-watch.ts';
import type { Snapshot } from '../src/lib/types.ts';

const now = Date.parse('2026-10-07T01:30:00Z');
const news = {
  items: [],
  latest_reset: null,
  scheduled_reset: null,
  active_watch: {
    level: 'elevated',
    item: {
      id: 'watch-example',
      kind: 'forecast',
      occurred_at: '2026-10-06T22:00:00Z',
      expires_at: '2026-10-07T07:00:00Z',
      probability: 40,
    },
  },
} as unknown as Snapshot['news'];

test('watch is independent of history rows and is never a confirmed plan', () => {
  assert.equal(activeResetWatch(news, now)?.item.id, 'watch-example');
  assert.equal(news.scheduled_reset, null);
  assert.match(
    resetWatchCountdown(news.active_watch!.item.expires_at!, now, 'zh'),
    /5 小时 30 分钟内可能重置/,
  );
  assert.match(
    resetWatchCountdown(news.active_watch!.item.expires_at!, now, 'en'),
    /Possible reset within 5h 30m/,
  );
});

test('watch expires at the deadline and after a newer reset without a network refresh', () => {
  assert.equal(activeResetWatch(news, Date.parse('2026-10-07T07:00:00Z')), null);
  assert.equal(activeResetWatch(news, Date.parse('2026-10-06T21:59:59Z')), null);
  assert.equal(
    activeResetWatch(
      {
        ...news,
        latest_reset: {
          ...news.active_watch!.item,
          kind: 'announcement',
          occurred_at: '2026-10-07T00:00:00Z',
        },
      },
      now,
    ),
    null,
  );
  assert.ok(
    activeResetWatch(
      {
        ...news,
        latest_reset: {
          ...news.active_watch!.item,
          kind: 'announcement',
          occurred_at: '2026-10-06T20:00:00Z',
        },
      },
      now,
    ),
  );
});

test('invalid and missing watch windows do not show a guessed countdown', () => {
  for (const expires_at of ['invalid', null, '2026-10-06T20:00:00Z']) {
    assert.equal(
      activeResetWatch(
        {
          ...news,
          active_watch: { ...news.active_watch!, item: { ...news.active_watch!.item, expires_at } },
        },
        now,
      ),
      null,
    );
  }
  assert.equal(activeResetWatch({ ...news, active_watch: null }, now), null);
  assert.equal(activeResetWatch(news, NaN), null);
  assert.equal(resetWatchCountdown('invalid', now, 'en'), 'Watch window ended');
});
