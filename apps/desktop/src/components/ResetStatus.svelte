<script lang="ts">
  import { translator as t, locale } from '../lib/i18n';

  import type { Snapshot } from '../lib/types';
  import { resetAge } from '../lib/format';
  let { news, now }: { news: Snapshot['news']; now: number } = $props();
  const latest = $derived(news.latest_reset);
  const plan = $derived(
    news.scheduled_reset?.source_type === 'x_post' && news.scheduled_reset.author === 'thsottiaux'
      ? news.scheduled_reset
      : null,
  );
  const upcoming = $derived(
    plan?.scheduled_for && new Date(plan.scheduled_for).getTime() > now ? plan.scheduled_for : null,
  );
  const time = (at: string) =>
    new Date(at).toLocaleString($locale === 'zh' ? 'zh-CN' : 'en-US', {
      year: 'numeric',
      month: 'numeric',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
</script>

<div class="cp-reset-status" aria-label={$t('公开额度重置状态')}>
  <div>
    <span>{$t('最近一次重置')}{latest?.kind === 'observation' ? $t(' · 社区记录') : ''}</span
    ><strong title={latest ? time(latest.occurred_at) : undefined}
      >{latest ? resetAge(latest.occurred_at, now, $locale) : $t('等待同步')}</strong
    >
  </div>
  <div>
    <span>{$t('下一次已确认重置')}</span><strong
      >{upcoming
        ? time(upcoming)
        : plan?.scheduled_for
          ? $t('计划时间已过，待确认执行')
          : plan
            ? $t('已公布计划，时间待定')
            : news.last_success
              ? $t('尚未公布')
              : $t('等待同步')}</strong
    >
  </div>
  {#if news.status !== 'connected' && news.last_success}<small>{$t('离线缓存')}</small>{/if}
</div>
