import type { Snapshot } from './types';
import { translateFor, currentLanguage, type Language } from './i18n.ts';

/** Forecasts expire without network activity and stop after a newer executed reset. */
export function activeResetWatch(news: Snapshot['news'], now: number) {
  const watch = news.active_watch;
  if (!watch || watch.item.kind !== 'forecast') return null;
  const observed = Date.parse(watch.item.occurred_at);
  const deadline = Date.parse(watch.item.expires_at ?? '');
  const latest = Date.parse(news.latest_reset?.occurred_at ?? '');
  if (
    !Number.isFinite(now) ||
    !Number.isFinite(observed) ||
    !Number.isFinite(deadline) ||
    observed > now ||
    deadline <= now ||
    deadline <= observed ||
    (Number.isFinite(latest) && latest >= observed)
  )
    return null;
  return watch;
}

export function resetWatchCountdown(
  deadline: string,
  now: number,
  language: Language = currentLanguage(),
) {
  const t = (key: string, values?: Record<string, string | number>) =>
    translateFor(language, key, values);
  const remaining = Date.parse(deadline) - now;
  if (!Number.isFinite(remaining) || remaining <= 0) return t('观察窗口已结束');
  const minutes = Math.ceil(remaining / 60000);
  const hours = Math.floor(minutes / 60);
  return hours > 0
    ? t('{_0} 小时 {_1} 分钟内可能重置', { _0: hours, _1: minutes % 60 })
    : t('{_0} 分钟内可能重置', { _0: minutes });
}
