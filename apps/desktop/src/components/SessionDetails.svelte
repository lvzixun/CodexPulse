<script lang="ts">
  import { translator as t, locale, localizeError } from '../lib/i18n';
  import { untrack } from 'svelte';

  import { formatUsd } from '../lib/format';
  import { sessionDetail } from '../lib/ipc';
  import type { SessionDetail, TokenMeasure } from '../lib/types';
  let {
    id,
    revision,
    hideProjects,
    ready,
  }: {
    id: string;
    revision: string | null;
    hideProjects: boolean;
    ready: () => void;
  } = $props();
  let detail = $state<SessionDetail | null>(null),
    detailLoading = $state(false),
    detailError = $state('');
  let detailTicket = 0,
    lastDetailId: string | null = null;
  let loadedRequest = '';
  const number = (n: number) =>
    new Intl.NumberFormat('en-US', { notation: 'compact', maximumFractionDigits: 1 }).format(n);
  const measure = (value: TokenMeasure, events: number) =>
    events === 0 || value.unknown_events === events
      ? '—'
      : `${number(value.known)}${value.unknown_events ? '*' : ''}`;
  $effect(() => {
    const selectedId = id;
    if (selectedId !== lastDetailId) {
      lastDetailId = selectedId;
      detail = null;
    }
    const request = { id: selectedId, models_after: null, prices_after: null };
    const key = JSON.stringify(request);
    const foreground = untrack(() => !detail || loadedRequest !== key);
    void revision;
    const ticket = ++detailTicket;
    detailError = '';
    if (selectedId) {
      detailLoading = foreground;
      void sessionDetail(request)
        .then((result) => {
          if (ticket === detailTicket) {
            detail = result;
            loadedRequest = key;
            if (!result) detailError = '该 session 暂无可读取元数据';
          }
        })
        .catch((e) => {
          if (ticket === detailTicket) detailError = String(e);
        })
        .finally(() => {
          if (ticket === detailTicket) {
            detailLoading = false;
            if (foreground) ready();
          }
        });
    } else detailLoading = false;
    return () => {
      detailTicket++;
    };
  });
</script>

<section class="cp-detail cp-session-detail" aria-busy={detailLoading}>
  {#if detailLoading && !detail}<p class="cp-note">{$t('正在读取 session 详情…')}</p>{/if}
  {#if detailError}<p class="cp-note" role="alert">{localizeError(detailError, $locale)}</p>{/if}
  {#if detail}
    {@const usage = detail.usage}
    <p class="cp-note cp-session-context">
      {#if detail.session.meta.project && !hideProjects}<span
          class="cp-truncate"
          title={detail.session.meta.project}>{detail.session.meta.project}</span
        ><span aria-hidden="true">·</span>{/if}
      <span class="cp-truncate">
        {detail.session.models.length === 1
          ? detail.session.models[0]
          : detail.session.models.length
            ? $t('{_0}{_1} 个模型', {
                _0: detail.session.models.length,
                _1: detail.session.models.length === 50 ? '+' : '',
              })
            : $t('模型未知')}</span
      >
    </p>
    <div class="cp-metrics">
      <div><span>Session tokens</span><strong>{measure(usage.total, usage.events)}</strong></div>
      <div>
        <span>{$t('API 等价估算')}</span><strong
          >{detail.session.reference.pending
            ? $t('计算中…')
            : detail.session.reference.unpriced_events === usage.events && usage.events > 0
              ? $t('待计价')
              : `${formatUsd(detail.session.reference.cost_nanousd)}${detail.session.reference.unpriced_events ? '*' : ''}`}</strong
        >
      </div>
      <div>
        <span
          >{detail.session.meta.output_rate?.completed
            ? $t('上轮平均速度')
            : $t('本轮平均速度')}</span
        ><strong title={$t('输出 tokens ÷ 轮次耗时，含推理、工具执行和等待')}
          >{detail.session.meta.output_rate
            ? (
                (detail.session.meta.output_rate.output_tokens * 1000) /
                detail.session.meta.output_rate.elapsed_ms
              ).toFixed(1)
            : '—'} <small>tok/s</small></strong
        >
      </div>
    </div>
    {#if usage.events > 0}
      <p class="cp-session-breakdown">
        <span>{$t('输入')} {measure(usage.input, usage.events)}</span>
        <span>{$t('其中缓存')} {measure(usage.cached, usage.events)}</span>
        <span>{$t('输出')} {measure(usage.output, usage.events)}</span>
      </p>
    {:else}<p class="cp-note">{$t('尚无可归属的用量记录。')}</p>{/if}
    {#if !detail.session.reference.pending && detail.session.reference.unpriced_events > 0 && detail.session.reference.unpriced_events < usage.events}
      <p class="cp-note cp-session-estimate-note">{$t('* 估算未覆盖全部用量')}</p>
    {/if}
  {/if}
</section>
