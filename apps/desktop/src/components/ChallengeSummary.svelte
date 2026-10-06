<script lang="ts">
  import { translator as t } from '../lib/i18n';

  import type { Snapshot } from '../lib/types';
  import Icon from './Icon.svelte';
  import { challengeKey } from '../lib/news';
  import { localDay } from '../lib/reset-calendar';
  let { news, now, open }: { news: Snapshot['news']; now: number; open: () => void } = $props();
  const challenge = $derived(news.challenge);
  const latest = $derived(challenge?.records.find((record) => record.entries.length > 0));
  const entry = $derived(latest?.entries[0]);
  const day = $derived(
    challenge
      ? Math.floor(
          (Date.parse(localDay(now, challenge.timezone)) - Date.parse(challenge.start_date)) /
            86400000,
        ) + 1
      : 0,
  );
  const unread = $derived(
    !!challenge?.records.some((r) =>
      r.entries.some((e) => news.unread_keys.includes(challengeKey(news, r.day, e))),
    ),
  );
</script>

<section class="cp-challenge-preview" aria-label={$t('28 天挑战最新详情')}>
  <button class="cp-challenge-open" onclick={open} aria-label={$t('查看 28 天挑战完整记录')}>
    <span
      ><strong>{$t('Tibo 的 28 天挑战')}</strong><small
        >{challenge
          ? day < 1
            ? $t('尚未开始')
            : day > challenge.days
              ? $t('挑战已结束')
              : $t('第 {_0} / {_1} 天', { _0: day, _1: challenge.days })
          : $t('等待同步')}{news.challenge_status !== 'connected' && challenge
          ? $t(' · 离线缓存')
          : ''}</small
      ></span
    >
    <span
      >{#if unread}<span class="cp-new-label">{$t('新')}</span>{/if}<Icon name="right" /></span
    >
  </button>
  {#if entry && latest}<div class="cp-challenge-latest">
      <small>{$t('最新 ·')} {latest.date}</small><strong>{entry.title}</strong>
      <p class="cp-news-excerpt cp-news-clamped">{entry.text}</p>
    </div>{:else}<p class="cp-note">{$t('暂无挑战更新')}</p>{/if}
</section>
