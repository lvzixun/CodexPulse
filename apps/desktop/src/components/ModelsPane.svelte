<script lang="ts">
  import { translator as t, locale, localizeError } from '../lib/i18n';

  import { tick } from 'svelte';
  import { modelPage } from '../lib/ipc';
  import { formatUsd } from '../lib/format';
  import type { ModelPage, ModelPageRequest } from '../lib/types';
  import Icon from './Icon.svelte';
  let {
    query = $bindable(),
    selected = $bindable(),
    revision,
    sessions,
    ready,
  }: {
    query: ModelPageRequest;
    selected: string | null;
    revision: string;
    sessions: (model: string) => void;
    ready: () => void;
  } = $props();
  let list = $state<ModelPage | null>(null);
  let loading = $state(false),
    error = $state('');
  let ticket = 0;
  const number = (n: number) =>
    new Intl.NumberFormat('en-US', { notation: 'compact', maximumFractionDigits: 1 }).format(n);
  const measure = (value: number, unknown: number, events: number) =>
    unknown === events ? '—' : `${number(value)}${unknown ? '*' : ''}`;
  const money = (cost: number, unpriced: number, events: number) =>
    unpriced === events ? $t('待计价') : `${formatUsd(cost)}${unpriced ? '*' : ''}`;
  $effect(() => {
    const request = { ...query, cursor: query.cursor ? { ...query.cursor } : null };
    if (!request.cursor) void revision;
    const current = ++ticket;
    loading = true;
    error = '';
    void modelPage(request)
      .then((result) => {
        if (ticket === current) list = result;
      })
      .catch((e) => {
        if (ticket === current) {
          error = String(e);
          list = null;
        }
      })
      .finally(async () => {
        if (ticket === current) {
          loading = false;
          await tick();
          if (ticket === current) ready();
        }
      });
    return () => {
      ticket++;
    };
  });
  function navigate(direction: 'next' | 'previous') {
    const cursor = list?.[direction];
    if (cursor) query = { ...query, cursor, direction };
  }
  function refreshList() {
    query = { ...query, cursor: null, direction: 'next' };
  }
</script>

{#if error}<p class="cp-note" role="alert">{localizeError(error, $locale)}</p>
  <button class="cp-textbutton" onclick={refreshList}>{$t('重试查询')}</button>{/if}
<div aria-busy={loading}>
  {#each list?.items ?? [] as m (m.model)}
    <button
      class="cp-modelrow"
      disabled={loading}
      aria-expanded={selected === m.model}
      onclick={() => (selected = selected === m.model ? null : m.model)}
    >
      <span class="cp-model-label"
        ><strong class="cp-truncate" title={m.model}>{m.model}</strong><small
          >{m.sessions}
          {$t('个 sessions')}
          {#if list && list.known_total > 0 && m.unknown_totals < m.events}
            · {((m.total / list.known_total) * 100).toFixed(1)}%{m.unknown_totals
              ? '*'
              : ''}{/if}</small
        >
        {#if list && list.known_total > 0 && m.unknown_totals < m.events}<span
            class="cp-model-bar"
            role="img"
            aria-label={$t('{_0} 占周期已知 token 用量的 {_1}%', {
              _0: m.model,
              _1: ((m.total / list.known_total) * 100).toFixed(1),
            })}
            ><span style:width={`${Math.min(100, (m.total / list.known_total) * 100)}%`}
            ></span></span
          >{/if}
      </span>
      <span
        ><span
          ><b>{measure(m.total, m.unknown_totals, m.events)}</b><small
            title={$t('按当前 API 价格估算')}
            >{m.reference.pending
              ? $t('计算中…')
              : money(m.reference.cost_nanousd, m.reference.unpriced_events, m.events)}</small
          ></span
        ><Icon name={selected === m.model ? 'up' : 'right'} /></span
      >
    </button>
    {#if selected === m.model}
      <div class="cp-model-detail">
        <div class="cp-metrics">
          <div>
            <span>{$t('输入')}</span><strong>{measure(m.input, m.unknown_input, m.events)}</strong>
          </div>
          <div>
            <span>{$t('其中缓存')}</span><strong
              >{measure(m.cached, m.unknown_cached, m.events)}</strong
            >
          </div>
          <div>
            <span>{$t('输出')}</span><strong>{measure(m.output, m.unknown_output, m.events)}</strong
            >
          </div>
        </div>
        {#if m.reference.unpriced_events || m.incomplete_events}
          <p class="cp-note">
            {m.reference.unpriced_events
              ? $t('{_0} 条信息不足{_1}', {
                  _0: m.reference.unpriced_events,
                  _1: m.reference.unpriced_tokens
                    ? ` · ${number(m.reference.unpriced_tokens)} tokens`
                    : '',
                })
              : ''}{m.incomplete_events
              ? $t('{_0}{_1} 条记录缺少拆分字段', {
                  _0: m.unpriced_events ? ' · ' : '',
                  _1: m.incomplete_events,
                })
              : ''}
          </p>
        {/if}
        <button class="cp-textbutton" disabled={loading} onclick={() => sessions(m.model)}
          >{$t('查看此模型最近 30 天的 sessions')} <Icon name="right" /></button
        >
      </div>
    {/if}
  {:else}
    {#if loading}<p class="cp-note">{$t('正在读取模型分类…')}</p>{:else if !error}<p
        class="cp-note"
      >
        {$t('尚无可归属的模型用量。')}
      </p>{/if}
  {/each}
</div>
{#if list?.unknown_total_events}<p class="cp-note">
    {$t('图表按已知 tokens 计算；缺少总量的记录不按零处理。')}
  </p>{/if}
{#if list?.next || list?.previous}
  <div class="cp-pager">
    <button disabled={loading || !list?.previous} onclick={() => navigate('previous')}
      ><Icon name="arrow" /> {$t('上一页')}</button
    >
    <button disabled={loading || !list?.next} onclick={() => navigate('next')}
      >{$t('下一页')} <Icon name="right" /></button
    >
  </div>
{/if}
{#if query.cursor && list && list.watermark !== revision}
  <button class="cp-textbutton" disabled={loading} onclick={refreshList}
    >{$t('分类有更新 · 刷新列表')} <Icon name="right" /></button
  >
{/if}
