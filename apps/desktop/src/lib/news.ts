import { translateFor, currentLanguage, type Language } from './i18n.ts';
import type { NewsItem, Challenge, Snapshot } from './types';

export const newsKey = (item: NewsItem) =>
  `${item.kind[0].toUpperCase()}${item.kind.slice(1)}:${item.id}`;
export const isResetNews = (item: NewsItem) =>
  (item.kind === 'announcement' || item.kind === 'scheduled') &&
  item.author === 'thsottiaux' &&
  item.source_type === 'x_post' &&
  (item.reset_type === 'regular' || item.reset_type === 'banked');
export const challengeKey = (
  news: Snapshot['news'],
  day: number,
  entry: Challenge['records'][number]['entries'][number],
) => {
  const match = entry.source_url && news.items.find((item) => item.source_url === entry.source_url);
  return match ? newsKey(match) : `challenge:${day}:${entry.source_url ?? entry.title}`;
};

export interface NewsEntry {
  key: string;
  title: string;
  text: string;
  date: string;
  dateOnly: boolean;
  label: string;
  sourceUrl: string | null;
  item?: NewsItem;
  latestReset: boolean;
}

/** One reading list, retaining the original IDs for read acknowledgements and translation. */
export function newsEntries(
  news: Snapshot['news'],
  language: Language = currentLanguage(),
): NewsEntry[] {
  const t = (key: string) => translateFor(language, key);
  const entries = new Map<string, NewsEntry>();
  for (const item of news.items) {
    const reset = isResetNews(item);
    const latestReset = reset && news.latest_reset?.id === item.id;
    entries.set(newsKey(item), {
      key: newsKey(item),
      title:
        item.kind === 'forecast'
          ? t('重置预测')
          : reset
            ? item.kind === 'scheduled'
              ? t('重置计划')
              : item.reset_type === 'banked'
                ? t('重置卡发放')
                : t('额度已重置')
            : t('社区动态'),
      text: item.text,
      date: item.occurred_at,
      dateOnly: false,
      label: latestReset ? t('最近重置') : reset ? t('Tibo · 重置公告') : t('社区观察'),
      sourceUrl: item.source_url,
      item,
      latestReset,
    });
  }
  for (const record of news.challenge?.records ?? []) {
    for (const entry of record.entries) {
      const key = challengeKey(news, record.day, entry);
      const existing = entries.get(key);
      // Keep the precise time and reset evidence when a post appears in both feeds.
      if (existing && (entry.kind === 'reset' || (existing.item && isResetNews(existing.item))))
        continue;
      if (existing?.dateOnly && existing.date > record.date) continue;
      entries.set(key, {
        key,
        title: entry.title,
        text: entry.text,
        date: existing?.date ?? record.date,
        dateOnly: existing?.dateOnly ?? true,
        label: entry.kind === 'reset' ? t('Tibo · 重置公告') : t('Tibo · 产品动态'),
        sourceUrl: entry.source_url,
        latestReset: false,
      });
    }
  }
  const unread = new Set(news.unread_keys ?? []);
  return [...entries.values()].sort(
    (a, b) =>
      Number(b.latestReset) - Number(a.latestReset) ||
      Number(unread.has(b.key)) - Number(unread.has(a.key)) ||
      b.date.localeCompare(a.date) ||
      a.key.localeCompare(b.key),
  );
}
