<script lang="ts">
  import type { NewsItem } from '../lib/types';
  import { openSource } from '../lib/ipc';
  import Icon from './Icon.svelte';
  let { item, now }: { item: NewsItem; now: number } = $props();
  let error = $state('');
  const label = {
    announcement: '公告',
    scheduled: '计划',
    forecast: '预测',
    observation: '社区观察',
  };
  const expired = $derived(item.expires_at !== null && new Date(item.expires_at).getTime() <= now);
  const schedulePassed = $derived(
    item.scheduled_for !== null && new Date(item.scheduled_for).getTime() <= now,
  );
  const time = (ts: string) =>
    new Date(ts).toLocaleString('zh-CN', {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  async function source() {
    if (item.source_url) {
      try {
        await openSource(item.source_url);
      } catch (e) {
        error = String(e);
      }
    }
  }
</script>

<article class="cp-news-item">
  <span class="cp-tag">{label[item.kind]}{expired ? ' · 已过期' : ''}</span>
  <h3>
    {item.kind === 'forecast'
      ? '重置预测信号'
      : item.reset_type === 'banked'
        ? '备用重置额度'
        : '额度重置消息'}
  </h3>
  <p>{item.text}</p>
  {#if item.kind === 'scheduled'}<p class="cp-note">
      {item.scheduled_for
        ? `计划时间：${time(item.scheduled_for)}`
        : '计划时间尚未公布'}{schedulePassed ? ' · 时间已过，等待执行证据' : ''}
    </p>{/if}
  {#if item.kind === 'forecast'}<p class="cp-note">
      预测概率：{item.probability === null ? '未知' : `${item.probability}%`} · {item.forecast_window ??
        '时间范围未知'} · 预测不代表已执行
    </p>{/if}
  <small
    >{item.author === 'thsottiaux'
      ? 'Tibo · X'
      : item.source_type === 'observed'
        ? '社区观察'
        : '来源待确认'} · {time(item.occurred_at)}</small
  >{#if item.source_url}<button class="cp-textbutton" onclick={() => void source()}
      >查看原文 <Icon name="external" /></button
    >{/if}
  {#if error}<p class="cp-note" role="status">{error}</p>{/if}
</article>
