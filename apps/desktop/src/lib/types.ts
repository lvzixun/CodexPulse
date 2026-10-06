export interface Settings {
  theme: 'system' | 'light' | 'dark';
  accent: 'blue' | 'violet' | 'teal' | 'amber' | 'rose';
  glass: boolean;
  floating: boolean;
  always_on_top: boolean;
  windows_enabled: boolean;
  wsl_enabled: boolean;
  windows_home: string | null;
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
  cost_nanousd: number;
  unpriced_tokens: number;
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
  news: {
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
    buckets: QuotaBucket[];
    sources: Record<
      string,
      {
        home: string;
        status: string;
        last_attempt: string;
        failures: number;
        buckets: QuotaBucket[];
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
