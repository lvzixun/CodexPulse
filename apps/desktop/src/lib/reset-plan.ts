import type { Snapshot } from './types';
import { translateFor, currentLanguage, type Language } from './i18n.ts';

/** Only an announced Tibo plan is confirmed; forecasts stay in ResetWatch. */
export function resetPlan(news: Snapshot['news'], now: number) {
  const item = news.scheduled_reset;
  if (
    !item ||
    item.kind !== 'scheduled' ||
    item.source_type !== 'x_post' ||
    item.author !== 'thsottiaux' ||
    !Number.isFinite(now)
  )
    return null;
  const at = Date.parse(item.scheduled_for ?? '');
  if (!Number.isFinite(at)) return { item, state: 'pending' as const };
  // A completed reset supersedes the cached plan, even before the next fetch.
  const latestReset = news.latest_reset;
  const latest = Date.parse(latestReset?.occurred_at ?? '');
  if (
    latestReset?.kind === 'announcement' &&
    latestReset.source_type === 'x_post' &&
    latestReset.author === 'thsottiaux' &&
    item.reset_type != null &&
    latestReset.reset_type === item.reset_type &&
    latest >= at &&
    latest <= now
  )
    return null;
  return { item, state: at > now ? ('upcoming' as const) : ('overdue' as const) };
}

export function resetPlanCountdown(
  at: string,
  now: number,
  language: Language = currentLanguage(),
) {
  const t = (key: string, values?: Record<string, string | number>) =>
    translateFor(language, key, values);
  const remaining = Date.parse(at) - now;
  if (!Number.isFinite(remaining) || remaining <= 0) return t('计划时间已过，待确认执行');
  const minutes = Math.ceil(remaining / 60000);
  const hours = Math.floor(minutes / 60);
  const days = Math.floor(hours / 24);
  if (days > 0) return t('{_0} 天 {_1} 小时后重置', { _0: days, _1: hours % 24 });
  return hours > 0
    ? t('{_0} 小时 {_1} 分钟后重置', { _0: hours, _1: minutes % 60 })
    : t('{_0} 分钟后重置', { _0: minutes });
}
