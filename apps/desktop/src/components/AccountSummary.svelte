<script lang="ts">
  import { translator as t, locale, localizeError } from '../lib/i18n';

  import type { Snapshot } from '../lib/types';
  import { refreshNow } from '../lib/ipc';
  import { sourceNames } from '../lib/format';
  let { data }: { data: Snapshot } = $props();
  let selected = $state('');
  let pending = $state(false);
  let error = $state('');
  const accounts = $derived.by(() => {
    const seen = new Set<string>();
    return Object.entries(data.quota.sources)
      .sort(([, a], [, b]) => Number(!!b.profile) - Number(!!a.profile))
      .filter(([id, s]) => {
        const key = s.identity ?? id;
        if (seen.has(key)) return false;
        seen.add(key);
        return true;
      });
  });
  const id = $derived(accounts.some(([key]) => key === selected) ? selected : accounts[0]?.[0]);
  const source = $derived(id ? data.quota.sources[id] : undefined);
  const profile = $derived(source?.profile);
  const busy = $derived(pending || ['refreshing', 'queued'].includes(data.quota.request_status));
  const number = (n: number | null | undefined) =>
    n == null
      ? '—'
      : new Intl.NumberFormat($locale === 'zh' ? 'zh-CN' : 'en-US', {
          notation: 'compact',
          maximumFractionDigits: 1,
        }).format(n);
  const duration = (s: number | null | undefined) =>
    s == null
      ? '—'
      : s >= 3600
        ? $t('{_0} 小时 {_1} 分', { _0: Math.floor(s / 3600), _1: Math.floor((s % 3600) / 60) })
        : s >= 60
          ? $t('{_0} 分 {_1} 秒', { _0: Math.floor(s / 60), _1: s % 60 })
          : $t('{_0} 秒', { _0: s });
  async function refresh() {
    pending = true;
    error = '';
    try {
      await refreshNow('quota');
    } catch (e) {
      error = String(e);
    } finally {
      pending = false;
    }
  }
</script>

<section class="account-summary" aria-label={$t('账户累计统计')}>
  <div class="cp-sectionhead">
    <span>{$t('账户累计')}</span>
    <button class="cp-textbutton" disabled={busy} onclick={() => void refresh()}
      >{busy ? $t('刷新中…') : $t('刷新')}</button
    >
  </div>
  {#if accounts.length > 1}
    <select
      aria-label={$t('账户统计来源')}
      value={id}
      onchange={(e) => (selected = e.currentTarget.value)}
    >
      {#each accounts as [key]}<option value={key}>{sourceNames([key], data.sources)}</option
        >{/each}
    </select>
  {/if}
  {#if profile}
    <div class="account-name">
      <strong>{profile.display_name ?? $t('Codex 账户')}</strong>{#if profile.username}<span
          >@{profile.username.replace(/^@/, '')}</span
        >{/if}
    </div>
    <dl>
      <div class="major">
        <dt>{$t('累计 Token 数')}</dt>
        <dd title={profile.lifetime_tokens?.toLocaleString()}>{number(profile.lifetime_tokens)}</dd>
      </div>
      <div class="major">
        <dt>{$t('每日 Token 峰值')}</dt>
        <dd title={profile.peak_daily_tokens?.toLocaleString()}>
          {number(profile.peak_daily_tokens)}
        </dd>
      </div>
      <div>
        <dt>{$t('最长任务用时')}</dt>
        <dd>{duration(profile.longest_running_turn_sec)}</dd>
      </div>
      <div>
        <dt>{$t('最长连续天数')}</dt>
        <dd>
          {number(profile.longest_streak_days)}{#if profile.longest_streak_days !== null}<small>
              {$t('天')}</small
            >{/if}
        </dd>
      </div>
      <div>
        <dt>{$t('当前连续天数')}</dt>
        <dd>
          {number(profile.current_streak_days)}{#if profile.current_streak_days !== null}<small>
              {$t('天')}</small
            >{/if}
        </dd>
      </div>
    </dl>
    {#if profile.stats_unavailable}<p class="cp-note" role="status">
        {$t('账户统计暂不可用，请稍后刷新。')}
      </p>
    {:else if source?.profile_status !== 'connected'}<p class="cp-note" role="status">
        {$t('刷新失败，显示上次成功数据。')}
      </p>{/if}
    {#if profile.stats_as_of}<p class="account-date">{$t('统计至')} {profile.stats_as_of}</p>{/if}
  {:else}
    <p class="cp-note" role="status">
      {busy
        ? $t('正在获取账户资料…')
        : data.quota.request_status === 'backoff'
          ? $t('请求暂缓，将按等待时间重试。')
          : ['reauth_required', 'credentials_expired', 'not_signed_in'].includes(
                source?.profile_status ?? '',
              )
            ? $t('请在 Codex 中登录后刷新账户资料。')
            : source?.profile_status &&
                !['awaiting_refresh', 'verifying_account'].includes(source.profile_status)
              ? $t('账户资料暂不可用，可刷新重试。')
              : $t('刷新后显示姓名、用户名和账户累计指标。')}
    </p>
  {/if}
  {#if error}<p class="cp-note" role="alert">{localizeError(error, $locale)}</p>{/if}
</section>

<style>
  .account-summary {
    margin-top: 16px;
    padding-top: 14px;
    border-top: 1px solid var(--cp-line);
  }
  .cp-sectionhead {
    margin: 0 0 10px;
  }
  .account-name {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 10px;
    align-items: baseline;
    margin-bottom: 14px;
  }
  .account-name strong {
    font-size: 16px;
    font-weight: 600;
  }
  .account-name span {
    font-size: 11px;
    color: var(--cp-muted);
  }
  select {
    width: 100%;
    margin-bottom: 10px;
  }
  dl {
    display: grid;
    grid-template-columns: repeat(6, minmax(0, 1fr));
    gap: 14px 10px;
    margin: 0;
  }
  dl div {
    grid-column: span 2;
    min-width: 0;
  }
  dl .major {
    grid-column: span 3;
    padding-bottom: 10px;
    border-bottom: 1px solid var(--border);
  }
  dt {
    color: var(--cp-muted);
    font-size: 11px;
  }
  dd {
    margin: 4px 0 0;
    font-size: 12px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .major dd {
    font-size: 22px;
  }
  dd small {
    font-size: 11px;
    font-weight: 400;
  }
  .account-date {
    color: var(--cp-muted);
    font-size: 11px;
    margin-top: 12px;
  }
</style>
