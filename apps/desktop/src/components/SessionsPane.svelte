<script lang="ts">
  import { translator as t, locale, localizeError } from '../lib/i18n';
  import { untrack } from 'svelte';
  import { slide } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import { reducedMotion } from '../lib/motion';

  import { sessionLabel, sessionRunState } from '../lib/format';
  import { sessionPage, defaultSessionQuery } from '../lib/ipc';
  import type { SessionPage, SessionPageRequest, SourceHealth } from '../lib/types';
  import Icon from './Icon.svelte';
  import SessionDetails from './SessionDetails.svelte';
  let {
    query = $bindable(),
    selected = $bindable(),
    revision,
    hideTitles,
    hideProjects,
    sources,
    now,
    ready,
  }: {
    query: SessionPageRequest;
    selected: string | null;
    revision: string | null;
    hideTitles: boolean;
    hideProjects: boolean;
    sources: SourceHealth[];
    now: number;
    ready: () => void;
  } = $props();
  let list = $state<SessionPage | null>(null),
    loading = $state(false),
    listError = $state('');
  let pageTicket = 0;
  let loadedRequest = '';
  const rows = $derived(list?.items ?? []);
  const time = (ts: string) =>
    Number.isFinite(Date.parse(ts))
      ? new Date(ts).toLocaleString($locale === 'zh' ? 'zh-CN' : 'en-US', {
          month: 'short',
          day: 'numeric',
          hour: '2-digit',
          minute: '2-digit',
        })
      : $t('时间未知');
  $effect(() => {
    const request = { ...query, filter: { ...query.filter } };
    if (!request.cursor) void revision;
    const ticket = ++pageTicket;
    const key = JSON.stringify(request);
    const foreground = untrack(() => !list || loadedRequest !== key);
    loading = foreground;
    if (foreground) listError = '';
    void sessionPage(request)
      .then((result) => {
        if (ticket === pageTicket) {
          list = result;
          loadedRequest = key;
          listError = '';
        }
      })
      .catch((e) => {
        if (ticket === pageTicket) listError = String(e);
      })
      .finally(() => {
        if (ticket === pageTicket) {
          loading = false;
          if (foreground) ready();
        }
      });
    return () => {
      pageTicket++;
    };
  });
  function navigate(direction: 'older' | 'newer') {
    const cursor = direction === 'older' ? list?.older : list?.newer;
    if (cursor) {
      query = { ...query, cursor, direction };
      selected = null;
    }
  }
</script>

{#if query.filter.model}<div class="cp-sectionhead">
    <span class="cp-truncate">{query.filter.model}</span><button
      class="cp-textbutton"
      onclick={() => {
        query = defaultSessionQuery();
        selected = null;
      }}>{$t('全部 Sessions')}</button
    >
  </div>{/if}
{#if listError}<p class="cp-note" role="alert">{localizeError(listError, $locale)}</p>
  <button class="cp-textbutton" onclick={() => (query = { ...query })}>{$t('重试查询')}</button
  >{/if}
<div class="cp-session-list" aria-busy={loading}>
  {#each rows as s (s.meta.id)}
    {@const state = sessionRunState(s, sources, now)}
    <div class="cp-session-entry">
      <button
        class="cp-sessionrow"
        aria-expanded={selected === s.meta.id}
        aria-controls={`session-detail-${s.meta.id}`}
        onclick={() => (selected = selected === s.meta.id ? null : s.meta.id)}
      >
        <span class="cp-session-heading"
          ><span class="cp-truncate" title={sessionLabel(s.meta, hideTitles, $locale)}
            >{sessionLabel(s.meta, hideTitles, $locale)}</span
          ><span
            class="cp-session-state"
            class:cp-session-running={state === 'running'}
            title={state === 'unknown'
              ? $t('日志过期或来源不可用，无法确认是否仍在运行')
              : state === 'running'
                ? $t('最近 5 分钟有活动且来源已连接')
                : undefined}
            >{state === 'running'
              ? $t('运行中')
              : state === 'completed'
                ? $t('已结束')
                : $t('状态未知')}</span
          ></span
        >
        <span class="cp-session-time">{time(s.meta.last_activity)}</span><span
          class="cp-session-chevron"
          class:cp-session-expanded={selected === s.meta.id}><Icon name="right" /></span
        >
      </button>
      {#if selected === s.meta.id}<div
          id={`session-detail-${s.meta.id}`}
          transition:slide={{ duration: reducedMotion.current ? 0 : 180, easing: cubicOut }}
        >
          <SessionDetails id={s.meta.id} {revision} {hideProjects} {ready} />
        </div>{/if}
    </div>
  {:else}{#if !loading && !listError}<p class="cp-note">
        {$t('还没有采集到 Sessions。')}
      </p>{/if}{/each}
</div>
{#if !rows.length && loading}<p class="cp-note">{$t('正在读取 Sessions…')}</p>{/if}
{#if list?.newer || list?.older}<div class="cp-pager">
    <button disabled={loading || !list?.newer} onclick={() => navigate('newer')}
      ><Icon name="arrow" /> {$t('较新')}</button
    ><button disabled={loading || !list?.older} onclick={() => navigate('older')}
      >{$t('较早')} <Icon name="right" /></button
    >
  </div>{/if}
