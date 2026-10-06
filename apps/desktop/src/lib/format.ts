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
