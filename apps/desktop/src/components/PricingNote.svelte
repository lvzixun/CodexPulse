<script lang="ts">
  import { translator as t, locale } from '../lib/i18n';
  import { referenceExplanation } from '../lib/pricing';
  import type { ReferenceEstimate, Snapshot } from '../lib/types';
  let { reference, status }: { reference: ReferenceEstimate; status?: Snapshot['pricing'] } =
    $props();
</script>

<p class="cp-note cp-pricing-coverage">
  {status?.dates
    ? Object.entries(status.dates)
        .map(([name, date]) => `${name === 'openai' ? 'OpenAI' : 'DeepSeek'} ${date}`)
        .join(' · ')
    : reference.price_date}
  {$t('官方 API 参考价 · 缓存 / 档位 / 上下文')}
  {#if status?.syncing}
    · {$t('正在更新价格')}{/if}
</p>
{#if status?.error}<p class="cp-note">{$t('官方价格更新失败，继续使用已缓存价格')}</p>{/if}
{#if referenceExplanation(reference, $locale)}<p class="cp-note cp-pricing-coverage">
    {referenceExplanation(reference, $locale)}
  </p>{/if}
