<script lang="ts">
  import { translator as t, locale } from '../lib/i18n';
  import { activeResetWatch, resetWatchCountdown } from '../lib/reset-watch';
  import { newsKey, newsEntries } from '../lib/news';
  import type { Snapshot } from '../lib/types';
  import NewsCard from './NewsCard.svelte';
  let {
    news,
    now,
    compact = false,
    acknowledge = async () => {},
  }: {
    news: Snapshot['news'];
    now: number;
    compact?: boolean;
    acknowledge?: (keys?: string[]) => Promise<void>;
  } = $props();
  const watch = $derived(activeResetWatch(news, now));
  const entry = $derived(
    watch ? newsEntries({ ...news, items: [watch.item], challenge: null }, $locale)[0] : null,
  );
  const level = $derived(
    watch?.level === 'elevated' ? $t('有迹象') : watch?.level === 'strong' ? $t('迹象较强') : null,
  );
  const deadline = $derived(
    watch?.item.expires_at
      ? new Date(watch.item.expires_at).toLocaleString($locale === 'zh' ? 'zh-CN' : 'en-US', {
          month: 'numeric',
          day: 'numeric',
          hour: '2-digit',
          minute: '2-digit',
        })
      : '',
  );
</script>

{#if watch}<section
    class="cp-reset-watch"
    class:cp-reset-watch-compact={compact}
    aria-label={$t('重置观察')}
  >
    <div class="cp-watch-kicker">{$t('重置观察')}{level ? ` · ${level}` : ''}</div>
    <strong class="cp-watch-title"
      >{resetWatchCountdown(watch.item.expires_at!, now, $locale)}</strong
    >
    <p class="cp-watch-deadline">{$t('预计在 {_0} 前（本地时间）', { _0: deadline })}</p>
    <p class="cp-note cp-watch-caveat">
      {$t('站点观察，尚未确认；不代表当前账户额度已恢复。')}{#if news.status !== 'connected'}
        {$t('离线缓存')}{/if}
    </p>
    {#if !compact && entry}<NewsCard
        {entry}
        {now}
        unread={(news.unread_keys ?? []).includes(newsKey(watch.item))}
        onread={() => void acknowledge([newsKey(watch.item)])}
      />{/if}
  </section>{/if}
