import { currentLanguage, type Language } from './i18n.ts';
import type { ResetDate } from './types';

export function localDay(timestamp: string | number, timezone: string): string {
  const parts = new Intl.DateTimeFormat('en-CA', {
    timeZone: timezone,
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
  }).formatToParts(new Date(timestamp));
  const value = (type: string) => parts.find((p) => p.type === type)?.value;
  return `${value('year')}-${value('month')}-${value('day')}`;
}

export function resetCalendar(
  items: ResetDate[],
  now: number,
  timezone: string,
  complete: boolean,
  language: Language = currentLanguage(),
) {
  const events = items.filter((item) => ['regular', 'banked'].includes(item.reset_type ?? ''));
  const byDay = new Map<string, ResetDate[]>();
  for (const event of events) {
    const day = localDay(event.occurred_at, timezone);
    byDay.set(day, [...(byDay.get(day) ?? []), event]);
  }
  const today = Date.parse(localDay(now, timezone));
  // Keep the recent half year readable within the menu bar panel.
  const first = today - 182 * 86400000;
  const start = first - new Date(first).getUTCDay() * 86400000;
  const earliest = [...byDay.keys()].sort()[0];
  const cells = [];
  for (let at = start, index = 0; at <= today; at += 86400000, index++) {
    const date = new Date(at).toISOString().slice(0, 10);
    const entries = byDay.get(date) ?? [];
    const regular = entries.some((e) => e.reset_type === 'regular');
    const banked = entries.some((e) => e.reset_type === 'banked');
    cells.push({
      date,
      entries,
      column: Math.floor(index / 7) + 1,
      row: (index % 7) + 2,
      month:
        date.endsWith('-01') || index === 0
          ? new Intl.DateTimeFormat(language === 'zh' ? 'zh-CN' : 'en-US', {
              month: 'short',
              timeZone: 'UTC',
            }).format(new Date(at))
          : '',
      type:
        regular && banked
          ? 'both'
          : regular
            ? 'regular'
            : banked
              ? 'banked'
              : !complete && (!earliest || date < earliest)
                ? 'unknown'
                : 'none',
    });
  }
  return cells;
}
