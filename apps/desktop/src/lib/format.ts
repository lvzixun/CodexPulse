import { translateFor, currentLanguage, type Language } from './i18n.ts';
import type { QuotaWindow } from './types';
const usd = new Intl.NumberFormat('en-US', {
  style: 'currency',
  currency: 'USD',
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
});
export const formatUsd = (nanousd: number) => usd.format(nanousd / 1e9);
export function resetAge(at: string, now: number, language: Language = currentLanguage()): string {
  const t = (key: string, values?: Record<string, string | number>) =>
    translateFor(language, key, values);
  const elapsed = now - Date.parse(at);
  if (!Number.isFinite(elapsed) || elapsed < 0) return '—';
  if (elapsed < 3600000) return t('刚刚');
  if (elapsed < 86400000) return t('{_0} 小时前', { _0: Math.floor(elapsed / 3600000) });
  return t('{_0} 天前', { _0: Math.ceil(elapsed / 86400000) });
}
export function windowLabel(window: QuotaWindow, language: Language = currentLanguage()) {
  const t = (key: string, values?: Record<string, string | number>) =>
    translateFor(language, key, values);
  const minutes = window.duration_minutes;
  if (minutes === null) return t('额度窗口');
  if (minutes === 10080) return t('每周');
  if (minutes >= 1440 && minutes % 1440 === 0) return t('{_0} 天', { _0: minutes / 1440 });
  if (minutes >= 60 && minutes % 60 === 0) return t('{_0} 小时', { _0: minutes / 60 });
  return t('{_0} 分钟', { _0: minutes });
}
export function untilReset(
  window: QuotaWindow,
  now: number,
  language: Language = currentLanguage(),
) {
  const t = (key: string, values?: Record<string, string | number>) =>
    translateFor(language, key, values);
  if (window.resets_at === null) return t('恢复时间未知');
  const seconds = window.resets_at * 1000 - now;
  if (seconds <= 0) return t('等待刷新确认');
  const minutes = Math.ceil(seconds / 60000),
    days = Math.floor(minutes / 1440),
    hours = Math.floor((minutes % 1440) / 60);
  return days > 0
    ? t('{_0} 天 {_1} 小时后刷新', { _0: days, _1: hours })
    : hours > 0
      ? t('{_0} 小时 {_1} 分钟后刷新', { _0: hours, _1: minutes % 60 })
      : t('{_0} 分钟后刷新', { _0: minutes });
}
import type { RecentSession, Snapshot, SourceHealth } from './types';
export function sourceNames(ids: string[], sources: SourceHealth[]): string {
  return ids
    .map(
      (id) =>
        sources.find((s) => s.id === id)?.label ??
        (id === 'windows'
          ? 'Windows App / CLI'
          : id.startsWith('windows:')
            ? `Windows · ${id.slice(8, 16)}`
            : id.startsWith('wsl:')
              ? `WSL · ${id.split(':')[1]}`
              : id),
    )
    .join(' / ');
}
export function sessionLabel(
  meta: RecentSession['meta'] | undefined,
  hideTitle = false,
  language: Language = currentLanguage(),
): string {
  const t = (key: string, values?: Record<string, string | number>) =>
    translateFor(language, key, values);
  if (!meta) return t('暂无 session');
  if (hideTitle) return t('会话 · {_0}', { _0: meta.id.slice(0, 8) });
  return meta.title?.trim() || t('未命名会话 · {_0}', { _0: meta.id.slice(0, 8) });
}
export function sessionActivity(data: Pick<Snapshot, 'recent' | 'sources'>, now: number) {
  const sessions = data.recent.filter((s) => sessionRunState(s, data.sources, now) === 'running');
  const working = sessions[0];
  const state = working
    ? 'busy'
    : data.recent.some((s) => s.meta.status === 'active') ||
        !data.sources.some((s) => s.status === 'connected')
      ? 'unknown'
      : 'idle';
  return { working, sessions, state };
}
export function sessionRunState(session: RecentSession, sources: SourceHealth[], now: number) {
  const age = now - Date.parse(session.meta.last_activity);
  if (session.meta.status === 'completed') return 'completed';
  if (
    session.meta.status === 'active' &&
    age >= 0 &&
    age <= 5 * 60 * 1000 &&
    session.sources.some((id) => sources.some((s) => s.id === id && s.status === 'connected'))
  ) {
    return 'running';
  }
  return 'unknown';
}
export function sessionTokens(
  session: RecentSession | undefined,
  format: (n: number) => string,
): string {
  if (!session || session.events === 0 || session.events === session.unknown_totals) return '—';
  return format(session.total) + (session.unknown_totals ? '*' : '');
}
