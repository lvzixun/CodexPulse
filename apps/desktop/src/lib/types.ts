export interface Settings {
  theme: 'system' | 'light' | 'dark';
  glass: boolean;
  floating: boolean;
  always_on_top: boolean;
  windows_enabled: boolean;
  wsl_enabled: boolean;
  windows_home: string | null;
  timezone: string;
  compact_position: [number, number] | null;
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
