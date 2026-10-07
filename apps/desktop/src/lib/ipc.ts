import { translate as t, currentLocale } from './i18n.ts';
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
  ModelPageRequest,
  ModelPage,
  StartupStatus,
  AppUpdateInfo,
} from './types';
export const native = isTauri();
export const checkAppUpdates = (manual = false) =>
  native
    ? invoke<AppUpdateInfo>('check_updates', { manual })
    : Promise.reject(new Error(t('更新检查仅在桌面应用中可用')));
export const openAppRelease = () => (native ? invoke<void>('open_app_release') : Promise.resolve());
export const installAppUpdate = () => invoke<void>('install_app_update');
export const readmePreview =
  import.meta.env.DEV &&
  !native &&
  new URLSearchParams(location.search).get('preview') === 'readme';
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
    active_watch: null,
    challenge: null,
    history_status: '',
    challenge_status: '',
    challenge_fetched_at: null,
    reset_stats: {
      total: null,
      average_interval_days: null,
      longest_wait_days: null,
      history_complete: false,
    },
    reset_history: [],
  },
  quota: { buckets: [], sources: {}, request_status: '', last_success: null, next_attempt: null },
  settings: {
    theme: 'system',
    language: 'system',
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
    reference: {
      price_date: '',
      cost_nanousd: 0,
      unpriced_tokens: 0,
      unpriced_events: 0,
      pending: false,
    },
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
    model_count: 0,
    fact_revision: '0',
    days: [],
  },
  recent: [],
  sources: [],
  updated_at: null,
  collecting: true,
  error: null,
};
export const snapshot = async (): Promise<Snapshot> => {
  if (native) return invoke<Snapshot>('get_snapshot');
  if (readmePreview) {
    const { previewSnapshot } = await import('./readme-preview');
    return previewSnapshot(empty);
  }
  return {
    ...empty,
    collecting: false,
    error: t('浏览器预览未连接桌面采集器。请通过 pnpm dev 启动应用。'),
  };
};
export const saveSettings = (settings: Settings) =>
  native
    ? invoke<void>('set_settings', { settings })
    : Promise.reject(new Error(t('设置仅在桌面应用中可用')));
export const saveUiPreferences = (preferences: Partial<Pick<Settings, 'theme' | 'language'>>) =>
  native
    ? invoke<Settings>('set_ui_preferences', {
        theme: preferences.theme ?? null,
        language: preferences.language ?? null,
      })
    : Promise.reject(new Error(t('设置仅在桌面应用中可用')));
export const setRefresh = (group: 'quota' | 'news', config: RefreshConfig) =>
  native
    ? invoke<void>('set_refresh', { group, config })
    : Promise.reject(new Error(t('刷新设置仅在桌面应用中可用')));
export const refreshNow = (group: 'quota' | 'news') =>
  native
    ? invoke<number | null>('refresh_now', { group })
    : Promise.reject(new Error(t('刷新仅在桌面应用中可用')));
export const windowAction = (action: string) =>
  native ? invoke<void>('window_action', { action }) : Promise.resolve();
export const getStartupStatus = () =>
  native
    ? invoke<StartupStatus>('get_startup_status')
    : Promise.resolve(readmePreview ? ('disabled' as const) : ('unsupported' as const));
export const setStartupEnabled = (enabled: boolean) =>
  native
    ? invoke<StartupStatus>('set_startup_enabled', { enabled })
    : Promise.reject(new Error(t('登录启动仅在桌面应用中可用')));
export const openStartupSettings = () =>
  native
    ? invoke<void>('open_startup_settings')
    : Promise.reject(new Error(t('登录项仅在桌面应用中可用')));
export const getDiagnostics = () =>
  native
    ? invoke<string>('get_diagnostics')
    : Promise.reject(new Error(t('诊断仅在桌面应用中可用')));
export const copyDiagnostics = () =>
  native
    ? invoke<string>('copy_diagnostics')
    : Promise.reject(new Error(t('诊断仅在桌面应用中可用')));
export const onEvent = <T>(name: string, callback: (value: T) => void) =>
  native ? listen<T>(name, (e) => callback(e.payload)) : Promise.resolve(() => {});
export const openSource = (url: string) =>
  native
    ? invoke<void>('open_source', { url })
    : Promise.reject(new Error(t('请在桌面应用中打开来源')));
export const translateNews = (id: string) =>
  native
    ? invoke<string>('translate_news', { id })
    : Promise.reject(new Error(t('翻译仅在桌面应用中可用')));
export const readNews = async (keys: string[]): Promise<void> => {
  if (native) return invoke<void>('read_news', { keys });
  if (readmePreview) (await import('./readme-preview')).previewReadNews(keys);
};
export const getViewState = async () => {
  if (native) return invoke<ViewState>('get_view_state');
  if (readmePreview) return (await import('./readme-preview')).previewView();
  return null;
};
export const rememberView = (view: ViewState) =>
  native ? invoke<void>('remember_view', { view }) : Promise.resolve();
export const sessionPage = async (request: SessionPageRequest): Promise<SessionPage> => {
  if (native) return invoke<SessionPage>('get_session_page', { request });
  return {
    items: readmePreview
      ? (await snapshot()).recent.filter(
          (s) => !request.filter.model || s.models.includes(request.filter.model),
        )
      : [],
    older: null,
    newer: null,
  };
};
export const modelPage = async (request: ModelPageRequest): Promise<ModelPage> => {
  if (native) return invoke<ModelPage>('get_model_page', { request });
  if (readmePreview) return (await import('./readme-preview')).previewModels(await snapshot());
  return {
    items: [],
    next: null,
    previous: null,
    watermark: '0',
    total_models: 0,
    known_total: 0,
    unknown_total_events: 0,
  };
};
export const sessionDetail = async (
  request: SessionDetailRequest,
): Promise<SessionDetail | null> => {
  if (native) return invoke<SessionDetail | null>('get_session_detail', { request });
  if (readmePreview)
    return (await import('./readme-preview')).previewSessionDetail(await snapshot(), request.id);
  return null;
};
export const defaultSessionQuery = (): SessionPageRequest => ({
  filter: { model: null, from_day: null, through_day: null },
  cursor: null,
  direction: 'older',
});
