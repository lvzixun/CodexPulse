<script lang="ts">
  import { translator as t, locale, localizeError } from '../lib/i18n';

  import { onDestroy } from 'svelte';
  import { getDiagnostics, copyDiagnostics } from '../lib/ipc';
  let report = $state('');
  let busy = $state(false);
  let copied = $state(false);
  let error = $state('');
  let disposed = false;
  onDestroy(() => (disposed = true));
  async function load(copy = false) {
    if (busy) return;
    busy = true;
    copied = false;
    error = '';
    try {
      const next = await (copy ? copyDiagnostics() : getDiagnostics());
      if (!disposed) {
        report = next;
        copied = copy;
      }
    } catch (e) {
      if (!disposed) error = String(e);
    } finally {
      if (!disposed) busy = false;
    }
  }
</script>

<div class="cp-divider"></div>
<div class="cp-sectionhead"><span>{$t('诊断信息')}</span><small>{$t('本机 · 脱敏')}</small></div>
<p class="cp-note">{$t('仅包含版本、状态和计数，不含账户、路径或会话内容。')}</p>
<div class="diagnostic-actions">
  <button class="cp-textbutton" disabled={busy} onclick={() => void load()}
    >{busy ? $t('正在读取…') : report ? $t('刷新诊断') : $t('查看诊断')}</button
  >
  {#if report}
    <button class="cp-textbutton" disabled={busy} onclick={() => void load(true)}
      >{$t('复制最新诊断')}</button
    >
    <button class="cp-textbutton" disabled={busy} onclick={() => (report = '')}>{$t('收起')}</button
    >
  {/if}
</div>
{#if report}
  <textarea aria-label={$t('脱敏诊断信息')} readonly value={report} spellcheck={false}></textarea>
{/if}
{#if copied}<p class="cp-note" role="status">{$t('诊断信息已复制。')}</p>{/if}
{#if error}<p class="cp-note" role="alert">{localizeError(error, $locale)}</p>{/if}

<style>
  .diagnostic-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 16px;
    margin-bottom: 10px;
  }
  textarea {
    display: block;
    width: 100%;
    height: 220px;
    box-sizing: border-box;
    resize: vertical;
    padding: 10px;
    color: var(--text);
    background: transparent;
    border: 1px solid var(--cp-line);
    border-radius: 8px;
    font:
      11px/1.5 ui-monospace,
      Menlo,
      monospace;
    white-space: pre;
  }
  textarea:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 3px;
  }
</style>
