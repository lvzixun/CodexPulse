<script lang="ts">
  import { translator as t, locale } from '../lib/i18n';

  import type { Snapshot } from '../lib/types';
  import { resetCalendar } from '../lib/reset-calendar';
  import { resetAge } from '../lib/format';
  let { news, now, timezone }: { news: Snapshot['news']; now: number; timezone: string } = $props();
  let selected = $state('');
  const stats = $derived(news.reset_stats);
  const since = $derived(
    news.latest_reset ? resetAge(news.latest_reset.occurred_at, now, $locale) : '—',
  );
  const cells = $derived(
    resetCalendar(news.reset_history, now, timezone, stats?.history_complete ?? false, $locale),
  );
  const selectedCell = $derived(cells.find((cell) => cell.date === selected));
  const descriptions: Record<string, string> = $derived({
    regular: $t('常规重置'),
    banked: $t('备用重置'),
    both: $t('常规及备用重置'),
    none: $t('无公开重置记录'),
    unknown: $t('未载入历史'),
  });
  const dateTime = (ts: string) =>
    new Date(ts).toLocaleString($locale === 'zh' ? 'zh-CN' : 'en-US', {
      timeZone: timezone,
      month: 'numeric',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
</script>

<section class="cp-reset-overview" aria-label={$t('公开重置统计')}>
  <div class="cp-reset-recent">
    <div><span class="cp-label">{$t('最近一次额度重置')}</span><strong>{since}</strong></div>
    <span class="cp-note"
      >{news.latest_reset ? dateTime(news.latest_reset.occurred_at) : $t('等待同步')}</span
    >
  </div>
  <dl class="cp-reset-metrics">
    <div>
      <dt>{$t('重置次数')}</dt>
      <dd>{stats?.total ?? '—'}</dd>
    </div>
    <div>
      <dt>{$t('平均间隔')}</dt>
      <dd>
        {stats?.average_interval_days == null
          ? '—'
          : $t('{_0} 天', { _0: stats.average_interval_days.toFixed(1) })}
      </dd>
    </div>
    <div>
      <dt>{$t('最长等待')}</dt>
      <dd>
        {stats?.longest_wait_days == null
          ? '—'
          : $t('{_0} 天', { _0: stats.longest_wait_days.toFixed(1) })}
      </dd>
    </div>
  </dl>
  <div class="cp-reset-calendar-heading">
    <strong>{$t('重置历史')}</strong><span>{$t('近半年 · 本地时间')}</span>
  </div>
  <div class="cp-reset-legend">
    <span><i class="regular"></i>{$t('常规')}</span><span><i class="banked"></i>{$t('备用')}</span
    ><span><i class="none"></i>{stats?.history_complete ? $t('未重置') : $t('无记录')}</span>
  </div>
  <div class="cp-reset-calendar-scroll">
    <div
      class="cp-reset-calendar"
      style:grid-template-columns={`repeat(${cells.at(-1)?.column ?? 1}, minmax(0, 1fr))`}
      aria-label={$t('最近半年公开重置日期')}
    >
      {#each cells as cell (cell.date)}
        {#if cell.month}<span
            class="cp-reset-month"
            style:grid-column={cell.column}
            style:grid-row="1">{cell.month}</span
          >{/if}
        <button
          class="cp-reset-day"
          class:regular={cell.type === 'regular' || cell.type === 'both'}
          class:banked={cell.type === 'banked'}
          class:unknown={cell.type === 'unknown'}
          class:selected={selected === cell.date}
          style:grid-column={cell.column}
          style:grid-row={cell.row}
          title={`${cell.date} · ${descriptions[cell.type]}${cell.entries.length ? $t(' · {_0} 次', { _0: cell.entries.length }) : ''}`}
          aria-label={`${cell.date} · ${descriptions[cell.type]}`}
          aria-pressed={selected === cell.date}
          onclick={() => (selected = selected === cell.date ? '' : cell.date)}
        ></button>
      {/each}
    </div>
  </div>
  {#if selectedCell}<p class="cp-note cp-reset-day-info">
      {selectedCell.date} · {descriptions[selectedCell.type]}{#if selectedCell.entries.length}
        · {selectedCell.entries.length} {$t('次')}{/if}
    </p>{/if}
  <p class="cp-note cp-reset-scope">
    {$t('公开重置记录，不代表当前账户的额度变动。')}{#if !stats?.history_complete}{$t(
        '历史尚未全部载入。',
      )}{/if}
  </p>
</section>
