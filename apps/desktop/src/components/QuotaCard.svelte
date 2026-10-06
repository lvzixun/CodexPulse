<script lang="ts">
  import { translator as t, locale } from '../lib/i18n';

  import type { QuotaBucket, AccountAllowance } from '../lib/types';
  import { untilReset, windowLabel } from '../lib/format';
  let {
    bucket,
    now,
    status,
    maxAgeSeconds,
    allowance,
  }: {
    bucket: QuotaBucket;
    now: number;
    status: string;
    maxAgeSeconds: number;
    allowance?: AccountAllowance | null;
  } = $props();
  const windows = $derived([bucket.primary, bucket.secondary].filter((w) => w !== null));
  const stale = $derived(
    now - new Date(bucket.captured_at).getTime() > maxAgeSeconds * 1000 || status !== 'connected',
  );
  const expiration = $derived(
    allowance?.next_expiration ? new Date(allowance.next_expiration) : null,
  );
  const expirationText = $derived(
    expiration
      ? new Intl.DateTimeFormat($locale === 'zh' ? 'zh-CN' : 'en-US', {
          year: 'numeric',
          month: '2-digit',
          day: '2-digit',
          hour: '2-digit',
          minute: '2-digit',
          hour12: false,
        }).format(expiration)
      : null,
  );
</script>

<div
  class="cp-quota"
  style:grid-template-columns={`repeat(${Math.max(1, windows.length)}, minmax(0, 1fr))`}
>
  {#each windows as window}<div>
      <span class="cp-label">{$t('{window}剩余', { window: windowLabel(window, $locale) })}</span
      ><strong
        >{window.remaining_percent === null
          ? '—'
          : window.remaining_percent.toFixed(0)}{#if window.remaining_percent !== null}<span
            class="cp-unit">%</span
          >{/if}</strong
      >{#if window.remaining_percent !== null}<meter
          min="0"
          max="100"
          value={window.remaining_percent}
          aria-label={$t('{_0}剩余额度', { _0: windowLabel(window, $locale) })}
          class:cp-low={window.remaining_percent < 25}
        ></meter>{/if}<small>{untilReset(window, now, $locale)}</small>
    </div>{/each}
</div>
{#if bucket.limit_id === 'codex'}
  <div class="allowance" aria-label={$t('重置卡与余额')}>
    <div>
      <span class="cp-label">{$t('重置卡')}</span>
      <strong
        >{allowance?.reset_cards == null
          ? '—'
          : allowance.reset_cards.toLocaleString()}{#if allowance?.reset_cards != null}<small>
            {$t('张', { count: allowance.reset_cards })}</small
          >{/if}</strong
      >
      {#if allowance?.applicable_reset_cards != null && allowance.applicable_reset_cards !== allowance.reset_cards}<small
          class="cp-note"
          >{$t('当前额度适用 {count} 张', { count: allowance.applicable_reset_cards })}</small
        >{/if}
    </div>
    <div>
      <span class="cp-label">{$t('当前余额')}</span>
      <strong
        >{allowance?.unlimited
          ? $t('不限')
          : allowance?.balance == null
            ? '—'
            : allowance.balance.toLocaleString($locale === 'zh' ? 'zh-CN' : 'en-US', {
                maximumFractionDigits: 2,
              })}{#if allowance?.balance != null && !allowance.unlimited}<small>
            credits</small
          >{/if}</strong
      >
    </div>
  </div>
  <p class="cp-note allowance-expiration">
    {#if expirationText && expiration}
      {expiration.getTime() > now ? $t('最近到期') : $t('已到期')}{$locale === 'zh'
        ? '：'
        : ': '}{expirationText}{#if allowance?.next_expiring_count}
        · {allowance.next_expiring_count}
        {$t('张', {
          count: allowance.next_expiring_count,
        })}{/if}{#if expiration.getTime() <= now}{$t('，等待刷新')}{/if}
    {:else if allowance?.reset_cards === 0}{$t(
        '暂无可用重置卡',
      )}{:else if allowance && allowance.expiration_status !== 'connected'}{$t(
        '重置卡到期时间暂不可用',
      )}{:else}{$t('重置卡到期时间未提供')}{/if}
  </p>
{/if}
{#if stale}<p class="cp-note">{$t('额度快照已过期 · 保留最近有效值')}</p>{/if}

<style>
  .allowance {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 12px;
    margin-top: 16px;
  }
  .allowance > div {
    min-width: 0;
  }
  .allowance strong {
    display: block;
    margin-top: 5px;
    font-size: 21px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .allowance strong small {
    margin-left: 3px;
    font-size: 11px;
    color: var(--cp-muted);
    font-weight: 400;
  }
  .allowance > div > small {
    display: block;
    font-size: 11px;
    margin-top: 4px;
  }
  .allowance-expiration {
    margin: 9px 0 0;
  }
</style>
