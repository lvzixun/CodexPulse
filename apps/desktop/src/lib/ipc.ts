import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type {
  Snapshot,
  Settings,
  RefreshConfig,
  ViewState,
  SessionPageRequest,
  SessionPage,
  SessionDetailRequest,
  SessionDetail,
} from './types';
export const native = isTauri();
export const empty: Snapshot = {
  timezone_rebuild: null,
  timezone_error: null,
  news: {
    request_status: '',
    next_attempt: null,
    unread_keys: [],
    important_unread: 0,
    items: [],
    status: '',
    last_success: null,
    last_attempt: null,
    latest_reset: null,
    scheduled_reset: null,
    challenge: null,
    challenge_status: '',
    challenge_fetched_at: null,
  },
  quota: { buckets: [], sources: {}, request_status: '', last_success: null, next_attempt: null },
  settings: {
    theme: 'system',
    accent: 'blue',
    glass: true,
    floating: true,
    always_on_top: true,
    windows_enabled: true,
    wsl_enabled: true,
    windows_home: null,
    windows_sources: [],
    wsl_sources: [],
    wsl_auto_detect: true,
    hide_titles: false,
    hide_projects: false,
    quota_refresh: { mode: 'auto', interval_seconds: 300 },
    news_refresh: { mode: 'auto', interval_seconds: 300 },
    timezone: 'UTC',
    compact_position: null,
    compact_anchor: null,
  },
  usage: {
    from_day: '',
    through_day: '',
    timezone: 'UTC',
    total: 0,
    input: 0,
    cached: 0,
    output: 0,
    cost_nanousd: 0,
    unpriced_tokens: 0,
    incomplete_events: 0,
    sessions: 0,
    models: [],
    days: [],
  },
  recent: [],
  sources: [],
  updated_at: null,
  collecting: true,
  error: null,
};
export const snapshot = () =>
  native
    ? invoke<Snapshot>('get_snapshot')
    : Promise.resolve({
        ...empty,
        collecting: false,
        error: '浏览器预览未连接桌面采集器。请通过 pnpm dev 启动应用。',
      });
export const saveSettings = (settings: Settings) =>
  native
    ? invoke<void>('set_settings', { settings })
    : Promise.reject(new Error('设置仅在桌面应用中可用'));
export const setRefresh = (group: 'quota' | 'news', config: RefreshConfig) =>
  native
    ? invoke<void>('set_refresh', { group, config })
    : Promise.reject(new Error('刷新设置仅在桌面应用中可用'));
export const refreshNow = (group: 'quota' | 'news') =>
  native
    ? invoke<void>('refresh_now', { group })
    : Promise.reject(new Error('刷新仅在桌面应用中可用'));
export const windowAction = (action: string) =>
  native ? invoke<void>('window_action', { action }) : Promise.resolve();
export const onEvent = <T>(name: string, callback: (value: T) => void) =>
  native ? listen<T>(name, (e) => callback(e.payload)) : Promise.resolve(() => {});
export const openSource = (url: string) =>
  native
    ? invoke<void>('open_source', { url })
    : Promise.reject(new Error('请在桌面应用中打开来源'));
export const translateNews = (id: string) =>
  native
    ? invoke<string>('translate_news', { id })
    : Promise.reject(new Error('翻译仅在桌面应用中可用'));
export const readNews = (keys: string[]) =>
  native ? invoke<void>('read_news', { keys }) : Promise.resolve();
export const getViewState = () =>
  native ? invoke<ViewState>('get_view_state') : Promise.resolve(null);
export const rememberView = (view: ViewState) =>
  native ? invoke<void>('remember_view', { view }) : Promise.resolve();
export const sessionPage = (request: SessionPageRequest) =>
  native
    ? invoke<SessionPage>('get_session_page', { request })
    : Promise.resolve({ items: [], older: null, newer: null });
export const sessionDetail = (request: SessionDetailRequest) =>
  native ? invoke<SessionDetail | null>('get_session_detail', { request }) : Promise.resolve(null);
export const defaultSessionQuery = (): SessionPageRequest => ({
  filter: { model: null, from_day: null, through_day: null },
  cursor: null,
  direction: 'older',
});
