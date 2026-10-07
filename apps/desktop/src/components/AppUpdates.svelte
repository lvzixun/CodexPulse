<script lang="ts">
  import { translator as t, locale } from '../lib/i18n';
  import type { AppUpdateInfo } from '../lib/types';
  let {
    info,
    busy,
    check,
    open,
    banner = false,
    dismiss = () => {},
  }: {
    info: AppUpdateInfo;
    busy: boolean;
    check: () => void;
    open: () => void;
    banner?: boolean;
    dismiss?: () => void;
  } = $props();
  const status = $derived(
    info.status === 'current'
      ? $t('已是最新版本')
      : info.status === 'available'
        ? $t('发现新版本 v{version}', { version: info.latest_version ?? '' })
        : info.status === 'rate_limited'
          ? $t('检查过于频繁，稍后重试')
          : info.status === 'idle'
            ? $t('尚未检查')
            : $t('暂时无法检查更新，请稍后重试'),
  );
</script>

{#if banner}
  <aside class="app-update-banner" role="status">
    <span>{$t('发现新版本 v{version}', { version: info.latest_version ?? '' })}</span>
    <button class="cp-textbutton" onclick={open}>{$t('升级')}</button>
    <button class="cp-textbutton" onclick={dismiss} aria-label={$t('稍后提醒')}>×</button>
  </aside>
{:else}
  <div class="cp-divider"></div>
  <div class="cp-sectionhead">
    <span>{$t('应用更新')}</span><small>v{info.current_version}</small>
  </div>
  <div class="cp-setting app-update-setting">
    <span
      >{busy ? $t('正在检查更新…') : status}
      {#if info.checked_at}<small class="cp-setting-hint">
          {$t('上次检查：{time}', {
            time: new Date(info.checked_at).toLocaleString($locale === 'zh' ? 'zh-CN' : 'en-US', {
              month: 'numeric',
              day: 'numeric',
              hour: '2-digit',
              minute: '2-digit',
            }),
          })}
        </small>{/if}
    </span>
    <button class="cp-textbutton" disabled={busy} onclick={check}>{$t('检查更新')}</button>
  </div>
  {#if info.update_available}<button class="cp-textbutton" onclick={open}
      >{$t('升级到 v{version}', { version: info.latest_version ?? '' })}</button
    >{/if}
{/if}

<style>
  .app-update-banner {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 16px;
    background: var(--accent-bg);
    flex-shrink: 0;
    font-size: 12px;
  }
  .app-update-banner span {
    flex: 1;
    min-width: 0;
  }
  .app-update-banner button {
    flex-shrink: 0;
  }
  .app-update-setting {
    gap: 12px;
  }
  .app-update-setting button {
    flex-shrink: 0;
  }
</style>
