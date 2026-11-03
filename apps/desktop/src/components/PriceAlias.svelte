<script lang="ts">
  import { translator as t, localizeError } from '../lib/i18n';
  import { setPriceAlias } from '../lib/ipc';
  let {
    model,
    current = '',
    models = [],
  }: { model: string; current?: string; models?: string[] } = $props();
  let target = $state(''),
    busy = $state(false),
    error = $state('');
  $effect(() => {
    target = current;
  });
  async function save(value: string) {
    busy = true;
    error = '';
    try {
      await setPriceAlias(model, value.trim());
    } catch (e) {
      error = localizeError(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="cp-price-alias">
  <label for={`price-alias-${model}`}>{$t('关联官方参考模型')}</label>
  <p class="cp-note">{$t('仅影响参考费用估算，保留原模型名称和用量记录。')}</p>
  <input
    id={`price-alias-${model}`}
    list={`price-options-${model}`}
    bind:value={target}
    placeholder="gpt-6.1-sol / deepseek-flash"
    disabled={busy}
  />
  <datalist id={`price-options-${model}`}>
    {#each models as name}<option value={name}></option>{/each}
  </datalist>
  <button class="cp-button" disabled={busy || !target.trim()} onclick={() => void save(target)}
    >{$t('保存关联')}</button
  >
  {#if current}<button class="cp-button" disabled={busy} onclick={() => void save('')}
      >{$t('取消关联')}</button
    >{/if}
  {#if error}<p class="cp-note" role="alert">{error}</p>{/if}
</div>

<style>
  .cp-price-alias {
    margin-top: 12px;
  }
  label {
    font-size: 12px;
  }
  input {
    width: 100%;
    min-width: 0;
    box-sizing: border-box;
    margin: 6px 0;
    padding: 8px;
    color: inherit;
    background: var(--cp-surface, transparent);
    border: 1px solid var(--cp-border, #666);
    border-radius: 7px;
  }
  button {
    margin-right: 6px;
  }
</style>
