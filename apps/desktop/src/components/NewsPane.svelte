<script lang="ts">
  import { translator as t, locale, localizeError } from '../lib/i18n';

  import type { Snapshot } from '../lib/types';
  import { openSource } from '../lib/ipc';
  import { newsEntries } from '../lib/news';
  import NewsCard from './NewsCard.svelte';
  import Icon from './Icon.svelte';
  import ResetOverview from './ResetOverview.svelte';
  import ResetWatch from './ResetWatch.svelte';
  import { activeResetWatch } from '../lib/reset-watch';
  import ChallengeSummary from './ChallengeSummary.svelte';
  import ChallengeDetail from './ChallengeDetail.svelte';
  let {
    news,
    now,
    timezone,
    detail = false,
    limit = $bindable(5),
    navigate,
  }: {
    timezone: string;
    detail: boolean;
    limit?: number;
    navigate: (detail: boolean) => void;
    news: Snapshot['news'];
    now: number;
  } = $props();
  let error = $state('');
  const unread = $derived(news.unread_keys ?? []);
  const productUrls = $derived(
    new Set(
      news.challenge?.records.flatMap((r) =>
        r.entries
          .filter((e) => e.kind !== 'reset')
          .map((e) => e.source_url)
          .filter(Boolean),
      ) ?? [],
    ),
  );
  const watch = $derived(activeResetWatch(news, now));
  const entries = $derived(
    newsEntries(news, $locale).filter(
      (e) => e.item && e.item.id !== watch?.item.id && !productUrls.has(e.sourceUrl),
    ),
  );
  const visible = $derived(entries.slice(0, limit));
  async function source() {
    try {
      await openSource('https://codex-resets.com');
    } catch (e) {
      error = String(e);
    }
  }
</script>

<section class="cp-news-pane" aria-label={$t('Tibo 动态')}>
  {#if detail}<ChallengeDetail {news} {now} back={() => navigate(false)} />{:else}
    <div class="cp-sectionhead">
      <span>{unread.length ? $t('Tibo · {_0} 条未读新消息', { _0: unread.length }) : 'Tibo'}</span>
      <small
        >{news.status === 'connected'
          ? $t('已同步')
          : news.last_success
            ? $t('离线缓存')
            : $t('等待同步')}</small
      >
    </div>
    <ResetWatch {news} {now} />
    <ResetOverview {news} {now} {timezone} />
    <ChallengeSummary {news} {now} open={() => navigate(true)} />
    <div class="cp-sectionhead cp-news-feed-heading"><span>{$t('最新公告与消息')}</span></div>
    <div class="cp-news-list">
      {#each visible as entry (entry.key)}<NewsCard
          {entry}
          {now}
          unread={unread.includes(entry.key)}
        />{:else}<p class="cp-note">
          {news.last_success ? $t('暂无消息。') : $t('等待首次同步。')}
        </p>{/each}
    </div>
    {#if entries.length > limit}<button
        class="cp-textbutton cp-news-history"
        onclick={() => (limit = Math.min(105, limit + 5))}
        >{$t('查看更早消息')} <Icon name="down" /></button
      >{/if}
    {#if news.challenge && news.challenge_status !== 'connected'}<p class="cp-note">
        {$t('产品动态显示离线缓存')}
      </p>{/if}
  {/if}
  <div class="cp-news-source">
    <button class="cp-textbutton" onclick={() => void source()}
      >Codex Resets <Icon name="external" /></button
    ><small
      >{news.last_success
        ? $t('同步于 {_0}', {
            _0: new Date(news.last_success).toLocaleString($locale === 'zh' ? 'zh-CN' : 'en-US', {
              month: 'numeric',
              day: 'numeric',
              hour: '2-digit',
              minute: '2-digit',
            }),
          })
        : $t('尚未同步')}</small
    >
  </div>
  {#if error}<p class="cp-note" role="status">{localizeError(error, $locale)}</p>{/if}
</section>
