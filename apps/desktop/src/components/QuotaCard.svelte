<script lang="ts">
  import type { QuotaBucket } from '../lib/types';
  import { untilReset, windowLabel } from '../lib/format';
  let { bucket, now, status }: { bucket: QuotaBucket; now: number; status: string } = $props();
  const stale = $derived(
    now - new Date(bucket.captured_at).getTime() > 6 * 60 * 1000 || status !== 'connected',
  );
</script>

<div class="quota-bucket">
  <div class="card-heading">
    <h3>
      {bucket.name === 'codex' ? '使用额度' : bucket.name}
      <span class="muted">{bucket.plan ?? ''}</span>
    </h3>
    <span class="badge">{bucket.identity_confirmed ? '身份已确认' : '身份未确认'}</span>
  </div>
  <div class="quota-windows">
    {#each [bucket.primary, bucket.secondary].filter((window) => window !== null) as window}
      <div class="quota-window">
        <span class="muted">{windowLabel(window)}剩余</span>
        <strong class="quota-value"
          >{window.remaining_percent === null
            ? '—'
            : window.remaining_percent.toFixed(0)}{#if window.remaining_percent !== null}<small
              >%</small
            >{/if}</strong
        >
        {#if window.remaining_percent !== null}<meter
            min="0"
            max="100"
            value={window.remaining_percent}
            aria-label={`${windowLabel(window)}剩余额度`}
          ></meter>{/if}
        <div class="row-bottom">
          <small>{untilReset(window, now)}</small><small>{stale ? '快照已过期' : '额度快照'}</small>
        </div>
      </div>
    {/each}
  </div>
  <p class="footnote">
    {bucket.source_id} · 更新于 {new Date(bucket.captured_at).toLocaleTimeString('zh-CN', {
      hour: '2-digit',
      minute: '2-digit',
    })}{stale ? ' · 保留最近有效值' : ''}
  </p>
</div>
