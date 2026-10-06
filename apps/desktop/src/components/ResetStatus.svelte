<script lang="ts">
  import type { Snapshot } from '../lib/types';
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
    new Date(at).toLocaleString('zh-CN', {
      year: 'numeric',
      month: 'numeric',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
</script>

<div class="cp-reset-status" aria-label="公开额度重置状态">
  <div>
    <span>最近一次重置{latest?.kind === 'observation' ? ' · 社区记录' : ''}</span><strong
      >{latest ? time(latest.occurred_at) : '等待同步'}</strong
    >
  </div>
  <div>
    <span>下一次已确认重置</span><strong
      >{upcoming
        ? time(upcoming)
        : plan?.scheduled_for
          ? '计划时间已过，待确认执行'
          : plan
            ? '已公布计划，时间待定'
            : news.last_success
              ? '尚未公布'
              : '等待同步'}</strong
    >
  </div>
  {#if news.status !== 'connected' && news.last_success}<small>离线缓存</small>{/if}
</div>
