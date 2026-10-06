<script lang="ts">
  import { tick } from 'svelte';
  import { modelPage } from '../lib/ipc';
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
    unpriced === events ? '待计价' : `$${(cost / 1e9).toFixed(2)}${unpriced ? '*' : ''}`;
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

{#if error}<p class="cp-note" role="alert">{error}</p>
  <button class="cp-textbutton" onclick={refreshList}>重试查询</button>{/if}
<div aria-busy={loading}>
  {#each list?.items ?? [] as m (m.model)}
    <button
      class="cp-modelrow"
      disabled={loading}
      aria-expanded={selected === m.model}
      onclick={() => (selected = selected === m.model ? null : m.model)}
    >
      <span><strong>{m.model}</strong><small>{m.sessions} 个 sessions</small></span>
      <span
        ><span
          ><b>{measure(m.total, m.unknown_totals, m.events)}</b><small
            >{money(m.cost_nanousd, m.unpriced_events, m.events)}</small
          ></span
        ><Icon name={selected === m.model ? 'up' : 'right'} /></span
      >
    </button>
    {#if selected === m.model}
      <div class="cp-model-detail">
        <div class="cp-metrics">
          <div><span>输入</span><strong>{measure(m.input, m.unknown_input, m.events)}</strong></div>
          <div>
            <span>其中缓存</span><strong>{measure(m.cached, m.unknown_cached, m.events)}</strong>
          </div>
          <div>
            <span>输出</span><strong>{measure(m.output, m.unknown_output, m.events)}</strong>
          </div>
        </div>
        {#if m.unpriced_events || m.incomplete_events}
          <p class="cp-note">
            {m.unpriced_events
              ? `${m.unpriced_events} 条未计价记录${m.unpriced_tokens ? ` · ${number(m.unpriced_tokens)} tokens` : ''}`
              : ''}{m.incomplete_events
              ? `${m.unpriced_events ? ' · ' : ''}${m.incomplete_events} 条记录缺少拆分字段`
              : ''}
          </p>
        {/if}
        <button class="cp-textbutton" disabled={loading} onclick={() => sessions(m.model)}
          >查看此模型最近 30 天的 sessions <Icon name="right" /></button
        >
      </div>
    {/if}
  {:else}
    {#if loading}<p class="cp-note">正在读取模型分类…</p>{:else if !error}<p class="cp-note">
        尚无可归属的模型用量。
      </p>{/if}
  {/each}
</div>
{#if list?.next || list?.previous}
  <div class="cp-pager">
    <button disabled={loading || !list?.previous} onclick={() => navigate('previous')}
      ><Icon name="arrow" /> 上一页</button
    >
    <button disabled={loading || !list?.next} onclick={() => navigate('next')}
      >下一页 <Icon name="right" /></button
    >
  </div>
{/if}
{#if query.cursor && list && list.watermark !== revision}
  <button class="cp-textbutton" disabled={loading} onclick={refreshList}
    >分类有更新 · 刷新列表 <Icon name="right" /></button
  >
{/if}
