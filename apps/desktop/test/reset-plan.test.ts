import { test } from 'node:test';
import assert from 'node:assert/strict';
import { resetPlan, resetPlanCountdown } from '../src/lib/reset-plan.ts';
import type { Snapshot } from '../src/lib/types.ts';

const now = Date.parse('2026-10-08T01:30:00Z');
const news = {
  latest_reset: {
    kind: 'announcement',
    source_type: 'x_post',
    author: 'thsottiaux',
    reset_type: 'regular',
    occurred_at: '2026-10-07T03:35:00Z',
  },
  scheduled_reset: {
    kind: 'scheduled',
    source_type: 'x_post',
    author: 'thsottiaux',
    reset_type: 'regular',
    scheduled_for: '2026-10-08T07:00:00Z',
  },
} as Snapshot['news'];

test('confirmed future plans show a countdown; the deadline becomes pending confirmation', () => {
  assert.equal(resetPlan(news, now)?.state, 'upcoming');
  const at = news.scheduled_reset!.scheduled_for!;
  assert.equal(resetPlanCountdown(at, now, 'zh'), '5 小时 30 分钟后重置');
  assert.equal(resetPlanCountdown(at, now, 'en'), 'Reset in 5h 30m');
  assert.equal(resetPlanCountdown(at, Date.parse(at) - 1, 'en'), 'Reset in 1m');
  assert.equal(resetPlan(news, Date.parse(at))?.state, 'overdue');
  assert.equal(resetPlanCountdown(at, Date.parse(at), 'zh'), '计划时间已过，待确认执行');
  assert.equal(resetPlanCountdown('2026-10-10T04:30:00Z', now, 'en'), 'Reset in 2d 3h');
});

test('forecasts and untrusted sources cannot become confirmed reset reminders', () => {
  for (const change of [
    { kind: 'forecast' },
    { kind: 'announcement' },
    { source_type: 'community' },
    { author: 'other' },
  ]) {
    assert.equal(
      resetPlan({ ...news, scheduled_reset: { ...news.scheduled_reset!, ...change } }, now),
      null,
    );
  }
  assert.equal(resetPlan({ ...news, scheduled_reset: null }, now), null);
  assert.equal(resetPlan(news, NaN), null);
});

test('missing times do not fabricate countdowns and completed resets supersede cached plans', () => {
  for (const scheduled_for of [null, '', 'invalid']) {
    assert.equal(
      resetPlan({ ...news, scheduled_reset: { ...news.scheduled_reset!, scheduled_for } }, now)
        ?.state,
      'pending',
    );
  }
  assert.equal(
    resetPlan(
      {
        ...news,
        latest_reset: { ...news.latest_reset!, occurred_at: '2026-10-08T07:00:00Z' },
      },
      Date.parse('2026-10-08T07:01:00Z'),
    ),
    null,
  );
  for (const change of [{ reset_type: 'banked' }, { kind: 'observation' }]) {
    assert.equal(
      resetPlan(
        {
          ...news,
          latest_reset: {
            ...news.latest_reset!,
            occurred_at: '2026-10-08T07:00:00Z',
            ...change,
          },
        },
        Date.parse('2026-10-08T07:01:00Z'),
      )?.state,
      'overdue',
      'A banked card or community observation cannot confirm the planned regular reset',
    );
  }
  assert.equal(
    resetPlanCountdown('invalid', now, 'en'),
    'Scheduled time passed; awaiting confirmation',
  );
});
