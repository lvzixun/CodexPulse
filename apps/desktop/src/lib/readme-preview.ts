/** Synthetic fixtures for documentation screenshots, imported only by Vite's dev preview. */
import type {
  Snapshot,
  RecentSession,
  ModelPage,
  ViewState,
  SessionDetail,
  UsageBreakdown,
} from './types';

const readKeys = new Set<string>();
export function previewReadNews(keys: string[]): void {
  for (const key of keys) readKeys.add(key);
}

export function previewSnapshot(base: Snapshot): Snapshot {
  const now = Date.now();
  const at = new Date(now).toISOString();
  const reset = new Date(now - 3 * 86400000).toISOString();
  const day = (offset: number) => new Date(now - offset * 86400000).toISOString().slice(0, 10);
  const reference = {
    price_date: day(0),
    cost_nanousd: 248.76e9,
    unpriced_tokens: 1.2e6,
    unpriced_events: 3,
    pending: false,
  };
  const meta = {
    id: 'example-session',
    title: 'Build a personal dashboard',
    project: 'SampleProject',
    source_kind: 'cli',
    source_version: null,
    parent_id: null,
    status: 'active',
    current_model: 'gpt-6.1-sol',
    last_activity: new Date(now - 2000).toISOString(),
    output_rate: { output_tokens: 1240, elapsed_ms: 40000, measured_at: at, completed: false },
  };
  const session: RecentSession = {
    meta,
    reference: { ...reference, cost_nanousd: 12.34e9, unpriced_tokens: 0, unpriced_events: 0 },
    models: ['gpt-6.1-sol'],
    sources: ['macos'],
    total: 2.8e6,
    events: 40,
    unknown_totals: 0,
    cost_nanousd: 0,
    unpriced_tokens: 0,
  };
  const latest = {
    id: 'sample-reset',
    kind: 'announcement' as const,
    text: 'Quota reset completed. Have a great week!',
    occurred_at: reset,
    reset_type: 'regular',
    scheduled_for: null,
    expires_at: null,
    forecast_window: null,
    probability: null,
    source_type: 'x_post',
    author: 'thsottiaux',
    source_url: null,
  };
  const bucket = {
    source_id: 'macos',
    identity_key: 'example',
    identity_confirmed: true,
    limit_id: 'codex',
    name: 'Codex',
    plan: 'pro',
    primary: null,
    secondary: {
      remaining_percent: 72,
      duration_minutes: 10080,
      resets_at: Math.floor(now / 1000) + 2 * 86400 + 9 * 3600,
    },
    captured_at: at,
  };
  return {
    ...structuredClone(base),
    collecting: false,
    updated_at: at,
    error: null,
    settings: {
      ...base.settings,
      floating: false,
      theme: 'dark',
      timezone: 'UTC',
      wsl_enabled: false,
    },
    sources: [
      {
        id: 'macos',
        label: 'macOS App / CLI',
        path: '/Users/example/.codex',
        status: 'connected',
        files: 24,
        issues: 0,
        last_read: at,
      },
    ],
    recent: [
      session,
      ...['Improve search performance', 'Review the settings flow', 'Add export support'].map(
        (title, i): RecentSession => ({
          ...session,
          total: [1.6e6, 920000, 640000][i],
          models: [['gpt-6-astra'], ['gpt-6.1-sol'], ['gpt-6-luna']][i],
          reference: { ...session.reference, cost_nanousd: [8.2e9, 4.1e9, 1.8e9][i] },
          meta: {
            ...meta,
            id: `example-session-${i + 2}`,
            title,
            status: 'completed',
            last_activity: new Date(now - (i + 1) * 3600000).toISOString(),
            output_rate: null,
          },
        }),
      ),
    ],
    usage: {
      ...base.usage,
      reference,
      from_day: day(29),
      through_day: day(0),
      total: 48.6e6,
      input: 38.6e6,
      cached: 30e6,
      output: 10e6,
      sessions: 24,
      model_count: 3,
      fact_revision: '1',
      days: Array.from({ length: 30 }, (_, i) => ({
        day: day(29 - i),
        total: Math.round((0.4 + ((i * 7) % 13) / 8) * 1e6),
        cost_nanousd: 0,
        unpriced_tokens: 0,
      })),
    },
    quota: {
      request_status: '',
      last_success: at,
      next_attempt: null,
      buckets: [bucket],
      sources: {
        macos: {
          home: '/Users/example/.codex',
          allowance: {
            balance: 12500,
            unlimited: false,
            reset_cards: 2,
            applicable_reset_cards: 1,
            next_expiration: new Date(now + 14 * 86400000).toISOString(),
            next_expiring_count: 1,
            expiration_status: 'connected',
          },
          profile: {
            display_name: 'Alex Chen',
            username: 'example',
            lifetime_tokens: 980e6,
            peak_daily_tokens: 48e6,
            longest_running_turn_sec: 5400,
            longest_streak_days: 42,
            current_streak_days: 12,
            stats_as_of: day(0),
            stats_unavailable: false,
          },
          profile_status: 'connected',
          profile_last_success: at,
          status: 'connected',
          last_attempt: at,
          failures: 0,
          buckets: [bucket],
          identity: 'example',
          last_success: at,
          retry_at: 0,
          proxy_source: '',
        },
      },
    },
    news: {
      ...base.news,
      status: 'connected',
      last_success: at,
      latest_reset: latest,
      important_unread: readKeys.has('Announcement:sample-reset') ? 0 : 1,
      unread_keys: readKeys.has('Announcement:sample-reset') ? [] : ['Announcement:sample-reset'],
      items: [latest],
      reset_stats: {
        total: 18,
        average_interval_days: 7.2,
        longest_wait_days: 21.5,
        history_complete: true,
      },
      reset_history: Array.from({ length: 18 }, (_, i) => ({
        occurred_at: new Date(now - (3 + i * 8) * 86400000).toISOString(),
        reset_type: i % 5 === 0 ? 'banked' : 'regular',
      })),
      challenge_status: 'connected',
      challenge: {
        start_date: day(1),
        days: 28,
        timezone: 'America/Los_Angeles',
        records: [
          {
            day: 2,
            date: day(0),
            entries: [
              {
                kind: 'product_update',
                title: 'A smoother review experience',
                text: 'Today’s update makes reviewing long tasks easier. Open the challenge to explore earlier updates.',
                source_url: null,
              },
            ],
          },
        ],
      },
    },
  };
}

export function previewView(): ViewState {
  const params = new URLSearchParams(location.search);
  const windows = params.get('platform') === 'windows';
  return {
    mode: params.get('mode') === 'compact' ? 'compact' : 'details',
    page: params.get('page') ?? 'overview',
    news_challenge: false,
    news_limit: 5,
    selected_model: null,
    selected_session: params.get('page') === 'sessions' ? 'example-session' : null,
    session_query: {
      filter: { model: null, from_day: null, through_day: null },
      cursor: null,
      direction: 'older',
    },
    model_query: { from_day: '', through_day: '', cursor: null, direction: 'next' },
    scroll: {},
    glass_supported: false,
    floating_supported: windows,
  };
}

export function previewSessionDetail(base: Snapshot, id: string): SessionDetail | null {
  const session = base.recent.find((s) => s.meta.id === id);
  if (!session) return null;
  const measure = (known: number) => ({ known, unknown_events: 0 });
  const usage: UsageBreakdown = {
    events: session.events,
    total: measure(session.total),
    input: measure(session.total * 0.8),
    cached: measure(session.total * 0.6),
    output: measure(session.total * 0.2),
    reasoning: measure(session.total * 0.1),
    cache_write: measure(0),
    cost_nanousd: 0,
    unpriced_tokens: 0,
    unpriced_events: 0,
    started_at: new Date(Date.parse(session.meta.last_activity) - 1800000).toISOString(),
    ended_at: session.meta.last_activity,
  };
  return {
    session,
    usage,
    models: [{ model: session.models[0], usage }],
    models_next: null,
    prices: [],
    prices_next: null,
  };
}

export function previewModels(base: Snapshot): ModelPage {
  return {
    items: ['gpt-6.1-sol', 'gpt-6-astra', 'gpt-6-luna'].map((model, i) => ({
      model,
      speed: {
        output_tokens: 24600,
        elapsed_ms: 1000000,
        samples: 50,
        service_tier: i === 0 ? 'mixed' : 'standard',
      },
      total: [28e6, 14.6e6, 6e6][i],
      input: [22e6, 11.6e6, 5e6][i],
      cached: [18e6, 8e6, 4e6][i],
      output: [6e6, 3e6, 1e6][i],
      cost_nanousd: 0,
      unpriced_tokens: 0,
      incomplete_events: 0,
      sessions: [14, 7, 3][i],
      reference: { ...base.usage.reference, cost_nanousd: [128.3e9, 92.4e9, 28.06e9][i] },
      events: 40,
      unknown_totals: 0,
      unknown_input: 0,
      unknown_cached: 0,
      unknown_output: 0,
      unpriced_events: 0,
    })),
    next: null,
    previous: null,
    watermark: '1',
    total_models: 3,
    known_total: 48.6e6,
    unknown_total_events: 0,
  };
}

export function previewModelSpeed(
  request: import('./types').ModelSpeedRequest,
): import('./types').ModelSpeed {
  const end = Date.now() - 2000;
  const count = request.range === 'recent100' ? 100 : request.range === 'month' ? 120 : 50;
  const span =
    request.range === 'month'
      ? 29 * 86400000
      : request.range === 'recent100'
        ? 86400000
        : 12 * 3600000;
  const points = Array.from({ length: count }, (_, i) => {
    const at = end - span + (i / (count - 1)) * span;
    const elapsed_ms = 15000 + (i % 5) * 1000;
    const value = 24 + Math.sin(i * 0.5) * 6 + Math.cos(i * 0.17) * 4;
    return {
      started_at: new Date(at - elapsed_ms).toISOString(),
      measured_at: new Date(at).toISOString(),
      output_tokens: Math.round((value * elapsed_ms) / 1000),
      elapsed_ms,
      samples: request.range === 'month' ? 3 : 1,
      service_tier: i % 3 ? 'fast' : 'standard',
    };
  });
  const current = { ...points.at(-1)!, service_tier: 'fast' };
  points[points.length - 1] = current;
  return {
    output_tokens: points.reduce((a, p) => a + p.output_tokens, 0),
    elapsed_ms: points.reduce((a, p) => a + p.elapsed_ms, 0),
    samples: points.reduce((a, p) => a + p.samples, 0),
    service_tier: 'mixed',
    points,
    current,
    history_pending: false,
  };
}
