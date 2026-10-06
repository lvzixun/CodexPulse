import type { QuotaWindow } from './types';
export function windowLabel(window: QuotaWindow) {
  const minutes = window.duration_minutes;
  if (minutes === null) return '额度窗口';
  if (minutes === 10080) return '每周';
  if (minutes >= 1440 && minutes % 1440 === 0) return `${minutes / 1440} 天`;
  if (minutes >= 60 && minutes % 60 === 0) return `${minutes / 60} 小时`;
  return `${minutes} 分钟`;
}
export function untilReset(window: QuotaWindow, now: number) {
  if (window.resets_at === null) return '恢复时间未知';
  const seconds = window.resets_at * 1000 - now;
  if (seconds <= 0) return '等待刷新确认';
  const minutes = Math.ceil(seconds / 60000),
    days = Math.floor(minutes / 1440),
    hours = Math.floor((minutes % 1440) / 60);
  return days > 0
    ? `${days} 天 ${hours} 小时后刷新`
    : hours > 0
      ? `${hours} 小时 ${minutes % 60} 分钟后刷新`
      : `${minutes} 分钟后刷新`;
}
import type { RecentSession, Snapshot } from './types';
export function sessionLabel(meta: RecentSession['meta'] | undefined): string {
  if (!meta) return '暂无 session';
  return meta.title?.trim() || `未命名会话 · ${meta.id.slice(0, 8)}`;
}
export function sessionActivity(data: Pick<Snapshot, 'recent' | 'sources'>, now: number) {
  const connected = (id: string) =>
    data.sources.some((s) => s.id === id && s.status === 'connected');
  const sessions = data.recent.filter(
    (s) =>
      s.meta.status === 'active' &&
      now - Date.parse(s.meta.last_activity) >= 0 &&
      now - Date.parse(s.meta.last_activity) <= 5 * 60 * 1000 &&
      s.sources.some(connected),
  );
  const working = sessions[0];
  const state = working
    ? 'busy'
    : data.recent.some((s) => s.meta.status === 'active') ||
        !data.sources.some((s) => s.status === 'connected')
      ? 'unknown'
      : 'idle';
  return { working, sessions, state };
}
export function sessionTokens(
  session: RecentSession | undefined,
  format: (n: number) => string,
): string {
  if (!session || session.events === 0 || session.events === session.unknown_totals) return '—';
  return format(session.total) + (session.unknown_totals ? '*' : '');
}
