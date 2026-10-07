<script lang="ts">
  import { translator as t, locale, localizeError } from '../lib/i18n';

  import type { NewsEntry } from '../lib/news';
  import { openSource, translateNews } from '../lib/ipc';
  import Icon from './Icon.svelte';
  let {
    entry,
    now,
    unread = false,
  }: { entry: NewsEntry; now: number; unread?: boolean } = $props();
  let error = $state('');
  let translated = $state('');
  let showTranslation = $state(false);
  let translating = $state(false);
  let expanded = $state(false);
  const translationKey = $derived(`${entry.key}|${entry.text}`);
  // The original-post button retains the link; omit trailing shortened links from the excerpt.
  const original = $derived(
    entry.sourceUrl ? entry.text.replace(/(?:\s+https:\/\/t\.co\/\S+)+\s*$/, '') : entry.text,
  );
  const body = $derived(showTranslation ? translated : original);
  const long = $derived(body.length > 180 || body.split('\n').length > 4);
  const item = $derived(entry.item);
  const expired = $derived(!!item?.expires_at && Date.parse(item.expires_at) <= now);
  const schedulePassed = $derived(!!item?.scheduled_for && Date.parse(item.scheduled_for) <= now);
  $effect(() => {
    translationKey;
    translated = '';
    showTranslation = false;
    expanded = false;
    error = '';
  });
  const time = (ts: string) =>
    new Date(ts).toLocaleString($locale === 'zh' ? 'zh-CN' : 'en-US', {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  const dateLabel = $derived(
    entry.dateOnly
      ? new Intl.DateTimeFormat($locale === 'zh' ? 'zh-CN' : 'en-US', {
          month: 'short',
          day: 'numeric',
          timeZone: 'UTC',
        }).format(new Date(entry.date))
      : time(entry.date),
  );
  async function translate() {
    if (!item) return;
    if (translated) {
      showTranslation = !showTranslation;
      return;
    }
    const id = item.id;
    const key = translationKey;
    translating = true;
    error = '';
    try {
      const text = await translateNews(id);
      if (translationKey === key) {
        translated = text;
        showTranslation = true;
      }
    } catch (e) {
      if (translationKey === key) error = String(e);
    } finally {
      translating = false;
    }
  }
  async function source() {
    if (!entry.sourceUrl) return;
    try {
      await openSource(entry.sourceUrl);
    } catch (e) {
      error = String(e);
    }
  }
</script>

<article class="cp-news-row" class:cp-unread-news={unread}>
  <div class="cp-news-row-heading">
    <strong>{entry.title}</strong>
    {#if unread}<span class="cp-new-label">{$t('新')}</span>{/if}
  </div>
  <div class="cp-news-meta">
    <span class:cp-reset-label={entry.latestReset}>{entry.label}</span>
    <span>{dateLabel}{expired ? $t(' · 已过期') : ''}</span>
  </div>
  <p class="cp-news-excerpt" class:cp-news-clamped={long && !expanded}>{body}</p>
  {#if showTranslation}<small class="cp-translation-source">{$t('中文译文 · Codex Resets')}</small
    >{/if}
  {#if item?.kind === 'scheduled'}<p class="cp-note">
      {item.scheduled_for
        ? $t('计划时间：{_0}', { _0: time(item.scheduled_for) })
        : $t('计划时间尚未公布')}{schedulePassed ? $t(' · 等待执行确认') : ''}
    </p>{/if}
  {#if item?.kind === 'forecast'}<p class="cp-note">
      {$t('预测概率：')}{item.probability === null ? $t('未知') : `${item.probability}%`} · {item.forecast_window ??
        $t('时间范围未知')}
      {$t('· 预测不代表已执行')}
    </p>{/if}
  <div class="cp-news-actions">
    {#if long}<button
        class="cp-textbutton"
        onclick={() => {
          expanded = !expanded;
        }}>{expanded ? $t('收起全文') : $t('展开全文')}</button
      >{/if}
    {#if entry.sourceUrl}<button class="cp-textbutton" onclick={() => void source()}
        >{$t('原文')} <Icon name="external" /></button
      >{/if}
    {#if item}<button class="cp-textbutton" disabled={translating} onclick={() => void translate()}
        >{translating
          ? $t('翻译中…')
          : showTranslation
            ? $t('显示原文')
            : translated
              ? $t('显示译文')
              : $t('翻译')}</button
      >{/if}
  </div>
  {#if error}<p class="cp-note" role="status">{localizeError(error, $locale)}</p>{/if}
</article>
