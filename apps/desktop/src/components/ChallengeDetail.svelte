<script lang="ts">
  import { translator as t, locale } from '../lib/i18n';

  import type { Snapshot } from '../lib/types';
  import { challengeKey, newsEntries } from '../lib/news';
  import { localDay } from '../lib/reset-calendar';
  import NewsCard from './NewsCard.svelte';
  import Icon from './Icon.svelte';
  let {
    news,
    now,
    back,
  }: {
    news: Snapshot['news'];
    now: number;
    back: () => void;
  } = $props();
  const entries = $derived(new Map(newsEntries(news, $locale).map((e) => [e.key, e])));
  const currentDate = $derived(news.challenge ? localDay(now, news.challenge.timezone) : '');
</script>

<section aria-label={$t('28 天挑战完整记录')}>
  <button class="cp-textbutton cp-challenge-back" onclick={back}
    ><span class="cp-back-icon"><Icon name="right" /></span>{$t('返回 Tibo')}</button
  >
  <div class="cp-sectionhead">
    <strong>{$t('Tibo 的 28 天挑战')}</strong><small>{$t('美西日期')}</small>
  </div>
  {#if news.challenge}
    {#each [...news.challenge.records]
      .filter((r) => r.date <= currentDate)
      .sort((a, b) => b.day - a.day) as record (record.day)}
      <section class="cp-challenge-day" aria-label={$t('第 {_0} 天', { _0: record.day })}>
        <div class="cp-sectionhead">
          <span>{$t('第 {day} 天', { day: record.day })}</span><small>{record.date}</small>
        </div>
        {#each record.entries as entry}
          {@const key = challengeKey(news, record.day, entry)}
          {@const row = entries.get(key)}
          {#if row}<NewsCard entry={row} {now} unread={news.unread_keys.includes(key)} />{/if}
        {:else}<p class="cp-note">
            {record.date > currentDate ? $t('尚未到这一天') : $t('暂无公布记录')}
          </p>{/each}
      </section>
    {/each}
    {#if news.challenge.records.some((r) => r.date > currentDate)}<p class="cp-note">
        {$t('后续挑战日期尚未到来，公布后自动更新。')}
      </p>{/if}
  {:else}<p class="cp-note">{$t('等待挑战记录同步。')}</p>{/if}
</section>
