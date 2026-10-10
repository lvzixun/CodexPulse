<script lang="ts">
  import { translator as t, locale, localizeError } from '../lib/i18n';
  import { modelSpeed } from '../lib/ipc';
  import { untrack } from 'svelte';
  import { rateValue, speedGeometry, speedTime, tierKey } from '../lib/speed';
  import type { ModelSpeed, SpeedRange, SpeedPoint } from '../lib/types';
  let {
    model,
    from,
    through,
    revision,
    now,
    timezone,
  }: {
    model: string;
    from: string;
    through: string;
    revision: string;
    now: number;
    timezone: string;
  } = $props();
  let range = $state<SpeedRange>('recent50'),
    data = $state<ModelSpeed | null>(null),
    error = $state(''),
    retry = $state(0),
    loading = $state(false);
  let hover = $state<SpeedPoint | null>(null),
    liveHover = $state(false);
  let ticket = 0;
  let loadedRequest = '';
  const gradientId = $props.id();
  const displayLocale = $derived($locale === 'zh' ? 'zh-CN' : 'en-US');
  const geometry = $derived(speedGeometry(data?.points ?? []));
  const current = $derived(
    data?.current &&
      now - Date.parse(data.current.measured_at) >= 0 &&
      now - Date.parse(data.current.measured_at) <= 60000
      ? data.current
      : null,
  );
  const tip = $derived(liveHover ? current : hover);
  const rate = (sample: SpeedPoint) =>
    rateValue(sample)?.toLocaleString(displayLocale, {
      minimumFractionDigits: 1,
      maximumFractionDigits: 1,
    }) ?? '—';
  const time = (at: string | number, day = false) =>
    speedTime(typeof at === 'string' ? Date.parse(at) : at, displayLocale, timezone, day);
  const crossesDay = $derived(
    geometry.last - geometry.first >= 86400000 ||
      new Intl.DateTimeFormat(displayLocale, { timeZone: timezone }).format(geometry.first) !==
        new Intl.DateTimeFormat(displayLocale, { timeZone: timezone }).format(geometry.last),
  );
  $effect(() => {
    void revision;
    void retry;
    const request = { model, from_day: from, through_day: through, range };
    const id = ++ticket;
    const key = JSON.stringify(request);
    const foreground = untrack(() => !data || loadedRequest !== key);
    loading = foreground;
    if (foreground) error = '';
    void modelSpeed(request)
      .then((result) => {
        if (id === ticket) {
          const previous = hover;
          data = result;
          loadedRequest = key;
          error = '';
          hover = previous
            ? (result.points.find(
                (p) =>
                  p.measured_at === previous.measured_at && p.started_at === previous.started_at,
              ) ?? null)
            : null;
        }
      })
      .catch((e) => {
        if (id === ticket) error = String(e);
      })
      .finally(() => {
        if (id === ticket) loading = false;
      });
    return () => {
      ticket++;
    };
  });
</script>

<div class="cp-speed" aria-busy={loading}>
  <div class="cp-speed-head">
    <span>{$t('运行速度')}</span><select bind:value={range} aria-label={$t('速度曲线范围')}
      ><option value="recent50">{$t('最近 50 轮')}</option><option value="recent100"
        >{$t('最近 100 轮')}</option
      ><option value="month">{$t('最近 30 天')}</option></select
    >
  </div>
  <div class="cp-speed-info">
    <span>tok/s · {$t('本地时间')}</span>
    {#if current}<button
        class="cp-speed-current"
        onmouseenter={() => (liveHover = true)}
        onmouseleave={() => (liveHover = false)}
        onfocus={() => (liveHover = true)}
        onblur={() => (liveHover = false)}
        aria-label={`${$t('当前')} ${rate(current)} tok/s · ${$t(tierKey(current.service_tier))}`}
        ><span class="cp-speed-dot"></span>{$t('当前')}
        <strong>{rate(current)} <small>tok/s</small></strong><span class="cp-speed-tier"
          >{$t(tierKey(current.service_tier))}</span
        ></button
      >{:else}<span>{$t('当前无运行样本')}</span>{/if}
  </div>
  {#if geometry.points.length}
    <svg
      class="cp-speed-chart"
      viewBox="0 0 316 125"
      role="group"
      aria-label={$t('运行速度曲线，横轴为本地时间')}
    >
      <defs
        ><linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1"
          ><stop stop-color="var(--cp-accent)" stop-opacity=".18" /><stop
            offset="1"
            stop-color="var(--cp-accent)"
            stop-opacity=".01"
          /></linearGradient
        ></defs
      >
      {#each [0, geometry.ceiling / 2, geometry.ceiling] as value}<line
          class="grid"
          x1="30"
          x2="311"
          y1={96 - (value / geometry.ceiling) * 80}
          y2={96 - (value / geometry.ceiling) * 80}
        /><text x="23" y={100 - (value / geometry.ceiling) * 80} text-anchor="end"
          >{value.toLocaleString(displayLocale, {
            notation: 'compact',
            maximumFractionDigits: 1,
          })}</text
        >{/each}
      <path
        d={`${geometry.path} L${geometry.points.at(-1)!.x},96 L${geometry.points[0].x},96 Z`}
        fill={`url(#${gradientId})`}
      /><path class="curve" d={geometry.path} />
      {#each geometry.points as p}<circle
          cx={p.x}
          cy={p.y}
          r={hover === p.sample ? 4 : 3}
          class:visible={hover === p.sample || p === geometry.points.at(-1)}
          tabindex="0"
          role="button"
          onclick={() => (hover = p.sample)}
          onkeydown={(event) => {
            if (event.key === 'Enter' || event.key === ' ') {
              event.preventDefault();
              hover = p.sample;
            }
          }}
          aria-label={`${time(p.time, true)} · ${rate(p.sample)} tok/s · ${$t(tierKey(p.sample.service_tier))}`}
          onmouseenter={() => (hover = p.sample)}
          onmouseleave={() => (hover = null)}
          onfocus={() => (hover = p.sample)}
          onblur={() => (hover = null)}
        />{/each}
      {#each geometry.first === geometry.last ? [0] : [0, 0.5, 1] as fraction}<text
          x={geometry.first === geometry.last ? 173 : 30 + fraction * 281}
          y="118"
          text-anchor={fraction === 0 && geometry.first !== geometry.last
            ? 'start'
            : fraction === 1
              ? 'end'
              : 'middle'}
          >{time(geometry.first + (geometry.last - geometry.first) * fraction, crossesDay)}</text
        >{/each}
    </svg>
  {:else}<p class="cp-note">
      {data?.history_pending ? $t('正在补采运行样本…') : $t('尚无有效运行样本')}
    </p>{/if}
  <div class="cp-speed-readout">
    {#if tip}<div class="cp-speed-tooltip" role="tooltip">
        <span>{time(tip.measured_at, true)}</span>
        <span class="cp-speed-sample-value"
          ><strong>{rate(tip)}</strong> tok/s
          <span class="cp-speed-tier">{$t(tierKey(tip.service_tier))}</span></span
        >
      </div>
    {:else}<p class="cp-speed-note">
        {$t('排除会话空闲 · 轮内工具等待可能包含在内')}{#if data?.history_pending}
          · {$t('历史补采中')}{/if}{#if range === 'month'}
          · {$t('按时间段加权汇总')}{/if}
      </p>{/if}
  </div>
  {#if error}<p class="cp-note" role="alert">
      {localizeError(error, $locale)}
      <button class="cp-textbutton" onclick={() => retry++}>{$t('重试查询')}</button>
    </p>{/if}
</div>

<style>
  .cp-speed {
    position: relative;
    margin-top: 12px;
    padding-top: 11px;
    border-top: 1px solid var(--cp-line);
    min-width: 0;
  }
  .cp-speed-head,
  .cp-speed-info {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    font-size: 12px;
  }
  .cp-speed-head {
    font-weight: 600;
  }
  .cp-speed-info {
    font-size: 11px;
    color: var(--cp-muted);
    margin: 6px 0 3px;
  }
  select {
    font: inherit;
    font-size: 11px;
    color: var(--cp-muted);
    background: transparent;
    border: 0;
    max-width: 50%;
    padding: 2px;
    border-radius: 4px;
  }
  option {
    background: var(--cp-solid);
    color: var(--cp-text);
  }
  .cp-speed-current {
    display: flex;
    align-items: baseline;
    gap: 3px;
    white-space: nowrap;
    background: transparent;
    border: 0;
    padding: 0;
    font: inherit;
    color: var(--cp-muted);
    cursor: help;
  }
  .cp-speed-current strong {
    font-size: 16px;
    color: var(--cp-text);
  }
  .cp-speed-current small {
    font-size: 10px;
    font-weight: 400;
    color: var(--cp-muted);
  }
  .cp-speed-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--cp-accent);
    align-self: center;
  }
  .cp-speed-chart {
    display: block;
    width: 100% !important;
    height: 125px !important;
    overflow: visible;
  }
  .cp-speed-chart text {
    font: 10px system-ui;
    fill: var(--cp-muted);
  }
  .grid {
    stroke: var(--cp-line);
    stroke-width: 1;
  }
  .curve {
    stroke: var(--cp-accent);
    stroke-width: 2;
    fill: none;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  circle {
    fill: var(--cp-accent);
    opacity: 0;
    cursor: help;
    stroke: transparent;
    stroke-width: 5;
  }
  circle.visible,
  circle:focus-visible {
    opacity: 1;
    outline: none;
  }
  circle:focus-visible {
    stroke: var(--cp-text);
    stroke-width: 1;
  }
  .cp-speed-readout {
    min-height: 30px;
    margin: 3px 0 8px;
  }
  .cp-speed-note {
    font-size: 10px;
    color: var(--cp-muted);
    line-height: 1.5;
  }
  .cp-speed-tooltip {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 3px 8px;
    pointer-events: none;
    color: var(--cp-muted);
    font-size: 11px;
    line-height: 1.5;
  }
  .cp-speed-tooltip strong {
    font-size: 12px;
    color: var(--cp-accent);
  }
  .cp-speed-sample-value {
    display: inline-flex;
    align-items: baseline;
    gap: 4px;
    white-space: nowrap;
    color: var(--cp-text);
  }
</style>
