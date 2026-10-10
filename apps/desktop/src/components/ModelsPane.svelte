<script lang="ts">
  import { translator as t, locale, localizeError } from '../lib/i18n';

  import { tick, untrack } from 'svelte';
  import { slide } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import { modelColor } from '../lib/model-color';
  import { reducedMotion } from '../lib/motion';
  import { modelPage } from '../lib/ipc';
  import { formatUsd } from '../lib/format';
  import type { ModelPage, ModelPageRequest } from '../lib/types';
  import Icon from './Icon.svelte';
  import ModelSpeedChart from './ModelSpeedChart.svelte';
  import { rateValue, tierKey } from '../lib/speed';
  let {
    query = $bindable(),
    selected = $bindable(),
    revision,
    sessions,
    ready,
    now,
    timezone,
  }: {
    query: ModelPageRequest;
    selected: string | null;
    revision: string;
    sessions: (model: string) => void;
    ready: () => void;
    now: number;
    timezone: string;
  } = $props();
  let list = $state<ModelPage | null>(null);
  let loading = $state(false),
    error = $state('');
  let ticket = 0;
  let loadedRequest = '';
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
    const key = JSON.stringify(request);
    // Snapshot updates must not disable/dim the already displayed model rows.
    const foreground = untrack(() => !list || loadedRequest !== key);
    loading = foreground;
    if (foreground) error = '';
    void modelPage(request)
      .then((result) => {
        if (ticket === current) {
          list = result;
          loadedRequest = key;
          error = '';
        }
      })
      .catch((e) => {
        if (ticket === current) {
          error = String(e);
          if (foreground) list = null;
        }
      })
      .finally(async () => {
        if (ticket === current) {
          loading = false;
          await tick();
          if (ticket === current && foreground) ready();
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
    <div class="cp-model-entry" style={`--model-color:${modelColor(m.model)}`}>
      <button
        class="cp-modelrow"
        disabled={loading}
        aria-expanded={selected === m.model}
        aria-controls={`model-detail-${m.model}`}
        onclick={() => (selected = selected === m.model ? null : m.model)}
      >
        <span class="cp-model-label"
          ><strong class="cp-truncate cp-model-name" title={m.model}
            ><i aria-hidden="true"></i>{m.model}</strong
          ><small
            >{m.sessions}
            {$t('个 sessions')}
            {#if list && list.known_total > 0 && m.unknown_totals < m.events}
              · {((m.total / list.known_total) * 100).toFixed(1)}%{m.unknown_totals
                ? '*'
                : ''}{/if}</small
          >
          <span
            class="cp-model-speed"
            title={$t('有效轮次的总输出除以总运行耗时，不包含会话空闲。')}
          >
            {$t('近 50 轮均速')}
            <b
              >{rateValue(m.speed)?.toLocaleString($locale === 'zh' ? 'zh-CN' : 'en-US', {
                maximumFractionDigits: 1,
                minimumFractionDigits: 1,
              }) ?? '—'}</b
            >
            {#if rateValue(m.speed) !== null}
              tok/s{/if}
            {#if m.speed?.service_tier === 'mixed'}<span class="cp-speed-tier mixed"
                >{$t(tierKey('mixed'))}</span
              >{/if}
          </span>
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
          ><span class="cp-model-chevron" class:cp-model-expanded={selected === m.model}
            ><Icon name="right" /></span
          ></span
        >
      </button>
      {#if selected === m.model}
        <div
          id={`model-detail-${m.model}`}
          transition:slide={{ duration: reducedMotion.current ? 0 : 180, easing: cubicOut }}
        >
          <div class="cp-model-detail">
            <div class="cp-metrics">
              <div>
                <span>{$t('输入')}</span><strong
                  >{measure(m.input, m.unknown_input, m.events)}</strong
                >
              </div>
              <div>
                <span>{$t('其中缓存')}</span><strong
                  >{measure(m.cached, m.unknown_cached, m.events)}</strong
                >
              </div>
              <div>
                <span>{$t('输出')}</span><strong
                  >{measure(m.output, m.unknown_output, m.events)}</strong
                >
              </div>
            </div>
            <ModelSpeedChart
              model={m.model}
              from={query.from_day}
              through={query.through_day}
              {revision}
              {now}
              {timezone}
            />
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
        </div>
      {/if}
    </div>
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
