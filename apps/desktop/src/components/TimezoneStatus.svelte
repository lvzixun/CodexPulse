<script lang="ts">
  import type { Snapshot } from '../lib/types';
  let { data }: { data: Snapshot } = $props();
</script>

<div class="cp-divider"></div>
<div class="cp-sectionhead"><span>统计时区</span><small>跟随系统</small></div>
<p class="cp-note" role="status">
  {#if data.timezone_rebuild}
    系统时区已变化，正在按 {data.timezone_rebuild.target_timezone} 重建。 已处理 {data.timezone_rebuild.processed.toLocaleString()}
    条用量记录；完成前继续显示 {data.settings.timezone} 的有效统计。
  {:else}
    按系统时区 {data.settings.timezone} 划分自然日。
  {/if}
</p>
{#if data.timezone_error}<p class="cp-note" role="alert">{data.timezone_error}</p>{/if}
