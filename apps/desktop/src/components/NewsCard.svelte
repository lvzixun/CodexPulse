<script lang="ts">
  import type { NewsItem } from '../lib/types';
  import { openSource, translateNews } from '../lib/ipc';
  import Icon from './Icon.svelte';
  let { item, now, unread = false }: { item: NewsItem; now: number; unread?: boolean } = $props();
  let error = $state('');
  let translated = $state('');
  let showTranslation = $state(false);
  let translating = $state(false);
  const translationKey = $derived(`${item.id}|${item.text}`);
  const important = $derived(
    (item.kind === 'announcement' || item.kind === 'scheduled') &&
      item.author === 'thsottiaux' &&
      item.source_type === 'x_post' &&
      (item.reset_type === 'regular' || item.reset_type === 'banked'),
  );
  $effect(() => {
    translationKey;
    translated = '';
    showTranslation = false;
    error = '';
  });
  async function translate() {
    if (translated) {
      showTranslation = !showTranslation;
      return;
    }
    const id = item.id;
    const original = item.text;
    translating = true;
    error = '';
    try {
      const text = await translateNews(id);
      if (item.id === id && item.text === original) {
        translated = text;
        showTranslation = true;
      }
    } catch (e) {
      if (item.id === id) error = String(e);
    } finally {
      translating = false;
    }
  }
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

<article class="cp-news-item" class:cp-important-news={important} class:cp-unread-news={unread}>
  <span class="cp-tag"
    >{important
      ? item.kind === 'scheduled'
        ? '重要 · 重置计划'
        : '重要 · 重置公告'
      : label[item.kind]}{unread ? ' · 新' : ''}{expired ? ' · 已过期' : ''}</span
  >
  <h3>
    {item.kind === 'forecast'
      ? '重置预测信号'
      : item.reset_type === 'banked'
        ? '备用重置额度'
        : important
          ? '额度重置消息'
          : 'Codex 动态'}
  </h3>
  <p>{showTranslation ? translated : item.text}</p>
  {#if showTranslation}<small class="cp-translation-source">中文译文 · Codex Resets</small>{/if}
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
  <button class="cp-textbutton" disabled={translating} onclick={() => void translate()}
    >{translating
      ? '翻译中…'
      : showTranslation
        ? '显示原文'
        : translated
          ? '显示译文'
          : '翻译'}</button
  >
  {#if error}<p class="cp-note" role="status">{error}</p>{/if}
</article>
