export interface RefreshConfig {
  mode: 'auto' | 'manual';
  interval_seconds: number;
}
export interface Settings {
  theme: 'system' | 'light' | 'dark';
  accent: 'blue' | 'violet' | 'teal' | 'amber' | 'rose';
  glass: boolean;
  floating: boolean;
  always_on_top: boolean;
  windows_enabled: boolean;
  wsl_enabled: boolean;
  windows_home: string | null;
  windows_sources: { id: string; label: string; home: string; enabled: boolean }[];
  wsl_sources: { id: string; distro: string; user: string; home: string; enabled: boolean }[];
  wsl_auto_detect: boolean;
  hide_titles: boolean;
  hide_projects: boolean;
  quota_refresh: RefreshConfig;
  news_refresh: RefreshConfig;
  timezone: string;
  compact_position: [number, number] | null;
  compact_anchor: {
    monitor: string | null;
    x: number;
    y: number;
    right: boolean;
    bottom: boolean;
  } | null;
}
export interface ViewState {
  mode: 'compact' | 'details';
  page: string;
  selected_model: string | null;
  selected_session: string | null;
  session_query: SessionPageRequest;
  scroll: Record<string, number>;
  glass_supported: boolean;
  floating_supported: boolean;
}
export interface ModelUsage {
  model: string;
  total: number;
  input: number;
  cached: number;
  output: number;
  cost_nanousd: number;
  unpriced_tokens: number;
  incomplete_events: number;
  sessions: number;
}
export interface DayUsage {
  day: string;
  total: number;
  cost_nanousd: number;
  unpriced_tokens: number;
}
export interface SessionMeta {
  id: string;
  title: string | null;
  project: string | null;
  source_kind: string | null;
  source_version: string | null;
  parent_id: string | null;
  status: string;
  last_activity: string;
}
export interface RecentSession {
  meta: SessionMeta;
  models: string[];
  sources: string[];
  total: number;
  events: number;
  unknown_totals: number;
  cost_nanousd: number;
  unpriced_tokens: number;
}
export interface SessionCursor {
  activity: string;
  id: string;
}
export interface SessionFilter {
  model: string | null;
  from_day: string | null;
  through_day: string | null;
}
export interface SessionPageRequest {
  filter: SessionFilter;
  cursor: SessionCursor | null;
  direction: 'older' | 'newer';
}
export interface SessionPage {
  items: RecentSession[];
  older: SessionCursor | null;
  newer: SessionCursor | null;
}
export interface TokenMeasure {
  known: number;
  unknown_events: number;
}
export interface UsageBreakdown {
  events: number;
  total: TokenMeasure;
  input: TokenMeasure;
  cached: TokenMeasure;
  output: TokenMeasure;
  reasoning: TokenMeasure;
  cache_write: TokenMeasure;
  cost_nanousd: number;
  unpriced_tokens: number;
  unpriced_events: number;
  started_at: string | null;
  ended_at: string | null;
}
export interface SessionDetailRequest {
  id: string;
  models_after: string | null;
  prices_after: string | null;
}
export interface SessionDetail {
  session: RecentSession;
  usage: UsageBreakdown;
  models: { model: string; usage: UsageBreakdown }[];
  models_next: string | null;
  prices: {
    version: string | null;
    events: number;
    tokens: number;
    unknown_totals: number;
    cost_nanousd: number;
    reference: PriceReference | null;
  }[];
  prices_next: string | null;
}
export interface PriceReference {
  version: string;
  provider: string;
  model: string;
  service_tier: string;
  effective_from: string;
  effective_to: string | null;
  min_input: number | null;
  max_input: number | null;
  input_microusd: number;
  cached_microusd: number;
  cache_write_microusd: number | null;
  output_microusd: number;
  source_url: string;
  checked_at: string;
}
export interface SourceHealth {
  id: string;
  label: string;
  path: string;
  status: string;
  last_read: string | null;
  files: number;
  issues: number;
}
export interface Snapshot {
  timezone_rebuild: { target_timezone: string; processed: number; ready: boolean } | null;
  timezone_error: string | null;
  news: {
    request_status: string;
    next_attempt: number | null;
    unread_keys: string[];
    important_unread: number;
    items: NewsItem[];
    status: string;
    last_success: string | null;
    last_attempt: string | null;
    latest_reset: NewsItem | null;
    scheduled_reset: NewsItem | null;
    challenge: Challenge | null;
    challenge_status: string;
    challenge_fetched_at: string | null;
  };
  quota: {
    request_status: string;
    last_success: string | null;
    next_attempt: number | null;
    buckets: QuotaBucket[];
    sources: Record<
      string,
      {
        home: string;
        status: string;
        last_attempt: string;
        failures: number;
        buckets: QuotaBucket[];
        identity: string | null;
        last_success: string | null;
        retry_at: number;
        proxy_source: string;
      }
    >;
  };
  settings: Settings;
  usage: {
    from_day: string;
    through_day: string;
    timezone: string;
    total: number;
    input: number;
    cached: number;
    output: number;
    cost_nanousd: number;
    unpriced_tokens: number;
    incomplete_events: number;
    sessions: number;
    models: ModelUsage[];
    days: DayUsage[];
  };
  recent: RecentSession[];
  sources: SourceHealth[];
  updated_at: string | null;
  collecting: boolean;
  error: string | null;
}
export interface QuotaWindow {
  remaining_percent: number | null;
  duration_minutes: number | null;
  resets_at: number | null;
}
export interface QuotaBucket {
  source_id: string;
  identity_key: string;
  identity_confirmed: boolean;
  limit_id: string;
  name: string;
  plan: string | null;
  primary: QuotaWindow | null;
  secondary: QuotaWindow | null;
  captured_at: string;
}
export interface NewsItem {
  id: string;
  kind: 'announcement' | 'scheduled' | 'forecast' | 'observation';
  text: string;
  occurred_at: string;
  reset_type: string | null;
  scheduled_for: string | null;
  expires_at: string | null;
  forecast_window: string | null;
  probability: number | null;
  source_type: string;
  author: string | null;
  source_url: string | null;
}
export interface Challenge {
  start_date: string;
  days: number;
  timezone: string;
  records: {
    day: number;
    date: string;
    entries: { kind: string; title: string; text: string; source_url: string | null }[];
  }[];
}
