<script lang="ts">
  import { translator as t, locale, localizeError } from '../lib/i18n';

  import { onMount } from 'svelte';
  import type { StartupStatus } from '../lib/types';
  import { getStartupStatus, setStartupEnabled, openStartupSettings, onEvent } from '../lib/ipc';
  import { subscriptionGroup } from '../lib/subscriptions';

  let status = $state<StartupStatus>('unavailable');
  let busy = $state(true);
  let error = $state('');
  let disposed = false;
  let revision = 0;
  const checked = $derived(status === 'enabled' || status === 'requires_approval');
  const hint = $derived(
    status === 'requires_approval'
      ? $t('待系统允许，请在登录项中确认')
      : status === 'unavailable'
        ? $t('系统暂无法识别此应用的登录项')
        : $t('登录后仅显示菜单栏图标 · 自动保存'),
  );

  async function refresh() {
    const ticket = ++revision;
    try {
      const next = await getStartupStatus();
      if (!disposed && ticket === revision) {
        status = next;
        error = '';
      }
    } catch (e) {
      if (!disposed && ticket === revision) error = String(e);
    } finally {
      if (!disposed && ticket === revision) busy = false;
    }
  }
  async function change(enabled: boolean) {
    const ticket = ++revision;
    busy = true;
    error = '';
    try {
      const next = await setStartupEnabled(enabled);
      if (!disposed && ticket === revision) status = next;
    } catch (e) {
      // Re-read OS state even after a failed mutation; never display a guessed checkbox.
      let next: StartupStatus | null = null;
      try {
        next = await getStartupStatus();
      } catch {}
      if (!disposed && ticket === revision) {
        if (next !== null) status = next;
        error = String(e);
      }
    } finally {
      if (!disposed && ticket === revision) busy = false;
    }
  }
  async function openSettings() {
    try {
      await openStartupSettings();
    } catch (e) {
      if (!disposed) error = String(e);
    }
  }
  onMount(() => {
    disposed = false;
    void refresh();
    const visible = () => {
      if (!busy && document.visibilityState === 'visible') void refresh();
    };
    document.addEventListener('visibilitychange', visible);
    const subscription = subscriptionGroup([
      () =>
        onEvent<boolean>('window-visible', (shown) => {
          if (!disposed && shown && !busy) void refresh();
        }),
    ]);
    void subscription.ready.catch(() => {
      if (!disposed) error = '无法自动更新登录启动状态，请重新打开设置';
    });
    return () => {
      disposed = true;
      revision++;
      document.removeEventListener('visibilitychange', visible);
      subscription.dispose();
    };
  });
</script>

{#if status !== 'unsupported'}
  <label class="cp-setting">
    <span
      >{$t('登录时启动')}<small class="cp-setting-hint"
        >{busy ? $t('正在读取系统状态…') : hint}</small
      ></span
    >
    <input
      type="checkbox"
      role="switch"
      switch={true}
      {checked}
      disabled={busy}
      onchange={(e) => {
        const enabled = e.currentTarget.checked;
        e.currentTarget.checked = checked;
        void change(enabled);
      }}
    />
  </label>
  {#if status === 'requires_approval'}
    <button class="cp-textbutton" onclick={() => void openSettings()}
      >{$t('打开系统登录项设置')}</button
    >
  {/if}
  {#if error}<p class="cp-note" role="alert">{localizeError(error, $locale)}</p>{/if}
{/if}
