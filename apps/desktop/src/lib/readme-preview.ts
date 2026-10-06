/** Synthetic fixtures for documentation screenshots, imported only by Vite's dev preview. */
import type { Snapshot, RecentSession, ModelPage, ViewState } from './types';

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
    last_activity: at,
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
    recent: [session],
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
  return {
    mode: 'details',
    page: 'overview',
    news_challenge: false,
    news_limit: 5,
    selected_model: null,
    selected_session: null,
    session_query: {
      filter: { model: null, from_day: null, through_day: null },
      cursor: null,
      direction: 'older',
    },
    model_query: { from_day: '', through_day: '', cursor: null, direction: 'next' },
    scroll: {},
    glass_supported: false,
    floating_supported: false,
  };
}

export function previewModels(base: Snapshot): ModelPage {
  return {
    items: ['gpt-6.1-sol', 'gpt-6-astra', 'gpt-6-luna'].map((model, i) => ({
      model,
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
