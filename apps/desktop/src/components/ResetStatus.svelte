<script lang="ts">
  import { translator as t, locale } from '../lib/i18n';

  import ResetWatch from './ResetWatch.svelte';
  import ResetPlan from './ResetPlan.svelte';
  import type { Snapshot } from '../lib/types';
  import { resetAge } from '../lib/format';
  import { resetPlan } from '../lib/reset-plan';
  let { news, now, timezone }: { news: Snapshot['news']; now: number; timezone: string } = $props();
  const latest = $derived(news.latest_reset);
  const upcoming = $derived(resetPlan(news, now)?.state === 'upcoming');
  const time = (at: string) =>
    new Date(at).toLocaleString($locale === 'zh' ? 'zh-CN' : 'en-US', {
      timeZone: timezone,
      year: 'numeric',
      month: 'numeric',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
</script>

<div
  class="cp-reset-status"
  class:cp-reset-status-upcoming={upcoming}
  aria-label={$t('公开额度重置状态')}
>
  <div>
    <span>{$t('最近一次重置')}{latest?.kind === 'observation' ? $t(' · 社区记录') : ''}</span
    ><strong title={latest ? time(latest.occurred_at) : undefined}
      >{latest ? resetAge(latest.occurred_at, now, $locale) : $t('等待同步')}</strong
    >
  </div>
  <ResetPlan {news} {now} {timezone} showEmpty />
  <ResetWatch {news} {now} compact />
  {#if !upcoming && news.status !== 'connected' && news.last_success}<small>{$t('离线缓存')}</small
    >{/if}
</div>
