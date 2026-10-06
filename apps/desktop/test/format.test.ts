import { test } from 'node:test';
import assert from 'node:assert/strict';
import { sessionActivity, sessionTokens, sessionRunState } from '../src/lib/format.ts';
import type { RecentSession, SourceHealth } from '../src/lib/types.ts';
const now = Date.parse('2026-10-06T09:00:00Z');
const source: SourceHealth = {
  id: 'windows',
  label: 'Windows',
  path: 'local',
  status: 'connected',
  last_read: null,
  files: 1,
  issues: 0,
};
const session: RecentSession = {
  meta: {
    id: 's',
    title: null,
    project: null,
    source_kind: null,
    source_version: null,
    parent_id: null,
    status: 'active',
    last_activity: '2026-10-06T08:59:00Z',
  },
  sources: ['windows'],
  models: [],
  total: 100,
  events: 1,
  unknown_totals: 0,
  cost_nanousd: 0,
  unpriced_tokens: 100,
};
test('working requires recent owned logs and a connected source', () => {
  assert.equal(sessionActivity({ recent: [session], sources: [source] }, now).state, 'busy');
  assert.equal(
    sessionActivity(
      {
        recent: [session, { ...session, meta: { ...session.meta, id: 'parallel' } }],
        sources: [source],
      },
      now,
    ).sessions.length,
    2,
  );
  assert.equal(
    sessionActivity({ recent: [session], sources: [source] }, now + 6 * 60000).state,
    'unknown',
  );
  assert.equal(
    sessionActivity({ recent: [session], sources: [{ ...source, status: 'read_error' }] }, now)
      .state,
    'unknown',
  );
  assert.equal(
    sessionActivity(
      {
        recent: [{ ...session, meta: { ...session.meta, status: 'completed' } }],
        sources: [source],
      },
      now,
    ).state,
    'idle',
  );
  assert.equal(
    sessionActivity(
      {
        recent: [{ ...session, meta: { ...session.meta, last_activity: 'bad' } }],
        sources: [source],
      },
      now,
    ).state,
    'unknown',
  );
});
test('metadata-only and unknown totals are not shown as zero', () => {
  assert.equal(sessionTokens({ ...session, events: 0, total: 0 }, String), '—');
  assert.equal(sessionTokens({ ...session, unknown_totals: 1, total: 0 }, String), '—');
  assert.equal(sessionTokens({ ...session, events: 2, unknown_totals: 1 }, String), '100*');
  assert.equal(sessionTokens(session, String), '100');
});
test('session rows distinguish ended sessions from stale or disconnected activity', () => {
  const ended = {
    ...session,
    meta: {
      ...session.meta,
      id: 'ended',
      status: 'completed',
      last_activity: '2026-10-06T08:59:30Z',
    },
  };
  const stale = {
    ...session,
    meta: { ...session.meta, id: 'stale', last_activity: '2026-10-06T08:00:00Z' },
  };
  assert.equal(sessionRunState(ended, [source], now), 'completed');
  assert.equal(sessionRunState(stale, [source], now), 'unknown');
  assert.equal(sessionRunState(session, [{ ...source, status: 'offline' }], now), 'unknown');
  assert.equal(sessionRunState(session, [source], now + 6 * 60000), 'unknown');
});
