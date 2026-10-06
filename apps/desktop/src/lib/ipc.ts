import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Snapshot, Settings, ViewState } from './types';
export const native = isTauri();
export const empty: Snapshot = {
  news: { items: [], status: '', last_success: null, last_attempt: null },
  quota: { buckets: [], sources: {} },
  settings: {
    theme: 'system',
    accent: 'blue',
    glass: true,
    floating: true,
    always_on_top: true,
    windows_enabled: true,
    wsl_enabled: true,
    windows_home: null,
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
export const windowAction = (action: string) =>
  native ? invoke<void>('window_action', { action }) : Promise.resolve();
export const onEvent = <T>(name: string, callback: (value: T) => void) =>
  native ? listen<T>(name, (e) => callback(e.payload)) : Promise.resolve(() => {});
export const openSource = (url: string) =>
  native
    ? invoke<void>('open_source', { url })
    : Promise.reject(new Error('请在桌面应用中打开来源'));
export const getViewState = () =>
  native ? invoke<ViewState>('get_view_state') : Promise.resolve(null);
export const rememberView = (view: ViewState) =>
  native ? invoke<void>('remember_view', { view }) : Promise.resolve();
