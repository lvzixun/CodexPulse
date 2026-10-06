<script lang="ts">
  import type { QuotaBucket } from '../lib/types';
  import { untilReset, windowLabel } from '../lib/format';
  let {
    bucket,
    now,
    status,
    maxAgeSeconds,
  }: { bucket: QuotaBucket; now: number; status: string; maxAgeSeconds: number } = $props();
  const windows = $derived([bucket.primary, bucket.secondary].filter((w) => w !== null));
  const stale = $derived(
    now - new Date(bucket.captured_at).getTime() > maxAgeSeconds * 1000 || status !== 'connected',
  );
</script>

<div
  class="cp-quota"
  style:grid-template-columns={`repeat(${Math.max(1, windows.length)}, minmax(0, 1fr))`}
>
  {#each windows as window}<div>
      <span class="cp-label">{windowLabel(window)}剩余</span><strong
        >{window.remaining_percent === null
          ? '—'
          : window.remaining_percent.toFixed(0)}{#if window.remaining_percent !== null}<span
            class="cp-unit">%</span
          >{/if}</strong
      >{#if window.remaining_percent !== null}<meter
          min="0"
          max="100"
          value={window.remaining_percent}
          aria-label={`${windowLabel(window)}剩余额度`}
          class:cp-low={window.remaining_percent < 25}
        ></meter>{/if}<small>{untilReset(window, now)}</small>
    </div>{/each}
</div>
{#if stale}<p class="cp-note">额度快照已过期 · 保留最近有效值</p>{/if}
