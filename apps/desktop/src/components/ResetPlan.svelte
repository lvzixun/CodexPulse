<script lang="ts">
  import { translator as t, locale } from '../lib/i18n';
  import { resetPlan, resetPlanCountdown } from '../lib/reset-plan';
  import type { Snapshot } from '../lib/types';
  import Icon from './Icon.svelte';

  let {
    news,
    now,
    timezone,
    showEmpty = false,
  }: {
    news: Snapshot['news'];
    now: number;
    timezone: string;
    showEmpty?: boolean;
  } = $props();
  const plan = $derived(resetPlan(news, now));
  const upcoming = $derived(plan?.state === 'upcoming');
  const at = $derived(
    upcoming && plan?.item.scheduled_for
      ? new Date(plan.item.scheduled_for).toLocaleString($locale === 'zh' ? 'zh-CN' : 'en-US', {
          timeZone: timezone,
          year: 'numeric',
          month: 'numeric',
          day: 'numeric',
          hour: '2-digit',
          minute: '2-digit',
        })
      : '',
  );
</script>

{#if upcoming && plan?.item.scheduled_for}
  <section class="cp-reset-plan" aria-label={$t('下一次重置提醒')}>
    <div class="cp-plan-heading">
      <span><Icon name="bell" />{$t('下一次已确认重置')}</span>
      <span class="cp-plan-badge">{$t('即将重置')}</span>
    </div>
    <strong class="cp-plan-countdown">
      {resetPlanCountdown(plan.item.scheduled_for, now, $locale)}
    </strong>
    <p class="cp-plan-time">{at} · {$t('本地时间')}</p>
    <p class="cp-note cp-plan-caveat">
      {$t('公开计划，实际执行以公告为准。')}{#if news.status !== 'connected'}
        {$t('离线缓存')}{/if}
    </p>
  </section>
{:else if plan || showEmpty}
  <div class="cp-reset-plan-status">
    <span>{$t('下一次已确认重置')}</span>
    <strong>
      {plan?.state === 'overdue'
        ? $t('计划时间已过，待确认执行')
        : plan
          ? $t('已公布计划，时间待定')
          : news.last_success
            ? $t('尚未公布')
            : $t('等待同步')}
    </strong>
  </div>
{/if}
