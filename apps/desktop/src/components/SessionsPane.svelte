<script lang="ts">
  import { sessionTokens, sessionLabel } from '../lib/format';
  import { sessionPage, sessionDetail, defaultSessionQuery, openSource } from '../lib/ipc';
  import type {
    SessionPage,
    SessionDetail,
    SessionPageRequest,
    TokenMeasure,
    UsageBreakdown,
  } from '../lib/types';
  import Icon from './Icon.svelte';
  let {
    query = $bindable(),
    selected = $bindable(),
    revision,
  }: { query: SessionPageRequest; selected: string | null; revision: string | null } = $props();
  let list = $state<SessionPage | null>(null),
    detail = $state<SessionDetail | null>(null);
  let loading = $state(false),
    detailLoading = $state(false),
    listError = $state(''),
    detailError = $state('');
  let modelsAfter = $state<string | null>(null),
    pricesAfter = $state<string | null>(null);
  let pageTicket = 0,
    detailTicket = 0,
    lastDetailId: string | null = null;
  const number = (n: number) =>
    new Intl.NumberFormat('en-US', { notation: 'compact', maximumFractionDigits: 1 }).format(n);
  const time = (ts: string | null) =>
    ts && Number.isFinite(Date.parse(ts))
      ? new Date(ts).toLocaleString('zh-CN', {
          month: 'short',
          day: 'numeric',
          hour: '2-digit',
          minute: '2-digit',
        })
      : '时间未知';
  const measure = (value: TokenMeasure, events: number) =>
    events === 0 || value.unknown_events === events
      ? '—'
      : `${number(value.known)}${value.unknown_events ? '*' : ''}`;
  const money = (usage: UsageBreakdown) =>
    usage.events === 0
      ? '—'
      : usage.unpriced_events === usage.events
        ? '待计价'
        : `$${(usage.cost_nanousd / 1e9).toFixed(2)}${usage.unpriced_events ? '*' : ''}`;
  const rate = (micros: number | null) =>
    micros === null
      ? '未知'
      : `$${(micros / 1e6).toLocaleString('en-US', { maximumFractionDigits: 6 })}`;
  $effect(() => {
    const request = { ...query, filter: { ...query.filter } };
    if (!request.cursor) void revision;
    const ticket = ++pageTicket;
    loading = true;
    listError = '';
    void sessionPage(request)
      .then((result) => {
        if (ticket === pageTicket) list = result;
      })
      .catch((e) => {
        if (ticket === pageTicket) listError = String(e);
      })
      .finally(() => {
        if (ticket === pageTicket) loading = false;
      });
    return () => {
      pageTicket++;
    };
  });
  $effect(() => {
    const id = selected;
    if (id !== lastDetailId) {
      modelsAfter = null;
      pricesAfter = null;
      lastDetailId = id;
      detail = null;
    }
    const request = { id: id ?? '', models_after: modelsAfter, prices_after: pricesAfter };
    void revision;
    const ticket = ++detailTicket;
    detailError = '';
    if (id) {
      detailLoading = true;
      void sessionDetail(request)
        .then((result) => {
          if (ticket === detailTicket) {
            detail = result;
            if (!result) detailError = '该 session 暂无可读取元数据';
          }
        })
        .catch((e) => {
          if (ticket === detailTicket) detailError = String(e);
        })
        .finally(() => {
          if (ticket === detailTicket) detailLoading = false;
        });
    } else detailLoading = false;
    return () => {
      detailTicket++;
    };
  });
  function navigate(direction: 'older' | 'newer') {
    const cursor = direction === 'older' ? list?.older : list?.newer;
    if (cursor) query = { ...query, cursor, direction };
  }
  async function source(url: string) {
    try {
      await openSource(url);
    } catch (e) {
      detailError = String(e);
    }
  }
</script>

<div class="cp-sectionhead">
  <span>{query.filter.model ? query.filter.model : '工作 sessions'}</span><small
    >{loading ? '读取中' : '每页 10 个'}</small
  >
</div>
{#if query.filter.model}<p class="cp-note">
    {query.filter.from_day} — {query.filter.through_day} 的模型用量范围；详情展示 session 全部历史。
  </p>
  <button
    class="cp-textbutton"
    onclick={() => {
      query = defaultSessionQuery();
      selected = null;
    }}>返回所有模型 <Icon name="arrow" /></button
  >{/if}
{#if listError}<p class="cp-note" role="alert">{listError}</p>
  <button class="cp-textbutton" onclick={() => (query = { ...query })}>重试查询</button>{/if}
<div class="cp-session-list" aria-busy={loading}>
  {#each list?.items ?? [] as s}<button
      class="cp-sessionrow"
      aria-pressed={selected === s.meta.id}
      onclick={() => (selected = s.meta.id)}
    >
      <span
        ><span class="cp-truncate" title={sessionLabel(s.meta)}>{sessionLabel(s.meta)}</span><span
          >{sessionTokens(s, number)} tokens</span
        ></span
      >
      <span
        ><span class="cp-truncate" title={s.meta.project ?? undefined}
          >{s.meta.project ? `项目：${s.meta.project} · ` : ''}{s.models.join(' · ') || '模型未知'} ·
          {s.sources.join(' / ')}</span
        ><span>{time(s.meta.last_activity)}</span></span
      >
    </button>{:else}{#if !loading && !listError}<p class="cp-note">
        此范围还没有采集到 sessions。
      </p>{/if}{/each}
</div>
<div class="cp-pager">
  <button disabled={loading || !list?.newer} onclick={() => navigate('newer')}
    ><Icon name="arrow" /> 较新</button
  ><button disabled={loading || !list?.older} onclick={() => navigate('older')}
    >较早 <Icon name="right" /></button
  >
</div>
{#if selected}
  <section class="cp-detail" aria-busy={detailLoading}>
    {#if detailLoading && !detail}<p class="cp-note">正在读取 session 详情…</p>{/if}
    {#if detailError}<p class="cp-note" role="alert">{detailError}</p>{/if}
    {#if detail}
      {@const usage = detail.usage}
      <div class="cp-sectionhead">
        <span class="cp-truncate" title={sessionLabel(detail.session.meta)}
          >{sessionLabel(detail.session.meta)}</span
        ><small
          >{detail.session.meta.status === 'active'
            ? '运行中'
            : detail.session.meta.status === 'completed'
              ? '已完成'
              : '状态未知'}</small
        >
      </div>
      {#if detail.session.meta.project}<p class="cp-note">
          项目：{detail.session.meta.project}
        </p>{/if}
      <div class="cp-metrics">
        <div><span>Session tokens</span><strong>{measure(usage.total, usage.events)}</strong></div>
        <div><span>美元估算</span><strong>{money(usage)}</strong></div>
        <div><span>输出速度</span><strong>— <small>tok/s</small></strong></div>
      </div>
      <p class="cp-note">
        {usage.events} 条归属记录 · {time(usage.started_at)} — {time(usage.ended_at)}
      </p>
      <p class="cp-note cp-id">
        Session ID：{detail.session.meta.id}{detail.session.meta.parent_id
          ? ` · 继承自 ${detail.session.meta.parent_id}`
          : ''}
      </p>
      {#if usage.events === 0}<p class="cp-note">尚无可归属的用量记录。</p>{:else}
        <div class="cp-metrics cp-breakdown">
          <div><span>输入</span><strong>{measure(usage.input, usage.events)}</strong></div>
          <div><span>其中缓存读取</span><strong>{measure(usage.cached, usage.events)}</strong></div>
          <div><span>输出</span><strong>{measure(usage.output, usage.events)}</strong></div>
        </div>
        <p class="cp-note">
          缓存写入 {measure(usage.cache_write, usage.events)} · 输出中的推理 {measure(
            usage.reasoning,
            usage.events,
          )}。缓存属于输入，推理属于输出，不重复相加。* 表示已知部分，缺失字段保留未知。
        </p>
        <div class="cp-sectionhead cp-modelheading">
          <span>Session 的模型分类</span><small>每页最多 50 个</small>
        </div>
        {#each detail.models as m}<div class="cp-session-model">
            <div class="cp-sectionhead">
              <span>{m.model}</span><small
                >{measure(m.usage.total, m.usage.events)} tok · {money(m.usage)}</small
              >
            </div>
            <p class="cp-note">
              输入 {measure(m.usage.input, m.usage.events)} · 缓存读取 {measure(
                m.usage.cached,
                m.usage.events,
              )} · 输出 {measure(m.usage.output, m.usage.events)}
            </p>
          </div>{/each}
        <div class="cp-pager">
          <button
            disabled={modelsAfter === null || detailLoading}
            onclick={() => (modelsAfter = null)}>模型首页</button
          ><button
            disabled={!detail.models_next || detailLoading}
            onclick={() => (modelsAfter = detail!.models_next)}
            >下一页模型 <Icon name="right" /></button
          >
        </div>
        <div class="cp-sectionhead cp-modelheading">
          <span>计价依据与覆盖</span><small
            >{usage.events - usage.unpriced_events} / {usage.events} 条已计价</small
          >
        </div>
        <p class="cp-note">
          {number(usage.unpriced_tokens)} 个已知 tokens、{usage.unpriced_events} 条记录未计价。美元数是
          API 等价参考值；订阅实际账单以服务商为准。
        </p>
        {#each detail.prices as price}<details class="cp-price">
            <summary
              ><span>{price.version ?? '未计价 / 依据缺失'}</span><small
                >{price.events === price.unknown_totals
                  ? '—'
                  : number(price.tokens) + (price.unknown_totals ? '*' : '')} tok · {price.events} 条</small
              ></summary
            >
            {#if price.reference}<p class="cp-note">
                {price.reference.model} · {price.reference.service_tier} · 单次输入 {price.reference
                  .min_input ?? 0} — {price.reference.max_input ?? '不限'} tokens
              </p>
              <p class="cp-note">
                每百万 tokens：输入 {rate(price.reference.input_microusd)} · 缓存读取 {rate(
                  price.reference.cached_microusd,
                )} · 缓存写入 {rate(price.reference.cache_write_microusd)} · 输出 {rate(
                  price.reference.output_microusd,
                )}
              </p>
              <p class="cp-note">
                核查日期 {price.reference.checked_at} · 适用时间 {time(
                  price.reference.effective_from,
                )} — {price.reference.effective_to
                  ? time(price.reference.effective_to)
                  : '尚未设置结束时间'}
              </p>
              <button class="cp-textbutton" onclick={() => void source(price.reference!.source_url)}
                >查看价格来源 <Icon name="external" /></button
              >
            {:else}<p class="cp-note">
                {price.version
                  ? '该历史价格版本的来源条目当前未内置；保留已记录费用。'
                  : '缺少适用价格或计价所需元数据，不按零费用处理。'}
              </p>{/if}
          </details>{/each}
        <div class="cp-pager">
          <button
            disabled={pricesAfter === null || detailLoading}
            onclick={() => (pricesAfter = null)}>价格首页</button
          ><button
            disabled={!detail.prices_next || detailLoading}
            onclick={() => (pricesAfter = detail!.prices_next)}
            >下一页价格 <Icon name="right" /></button
          >
        </div>
      {/if}
      <p class="cp-note">日志不提供连续输出 token 样本。</p>
    {/if}
  </section>
{/if}
