<script lang="ts">
  import { translator as t, locale, localizeError } from '../lib/i18n';

  import type { RefreshConfig, Snapshot } from '../lib/types';
  import { setRefresh, refreshNow } from '../lib/ipc';
  import { sourceNames } from '../lib/format';
  let { data, reload }: { data: Snapshot; reload: () => Promise<void> } = $props();
  const presets = [30, 60, 300, 900, 1800, 3600];
  const groups = ['quota', 'news'] as const;
  let saving = $state<string | null>(null);
  let requesting = $state<string | null>(null);
  let notices = $state<Record<string, number | null | undefined>>({});
  let error = $state('');
  let custom = $state<Record<string, boolean>>({});
  const time = (value: string | null) =>
    value
      ? $t('上次更新 {_0}', {
          _0: new Date(value).toLocaleString($locale === 'zh' ? 'zh-CN' : 'en-US'),
        })
      : $t('尚未更新');
  const retryTime = (value: number) =>
    new Date(value * 1000).toLocaleTimeString($locale === 'zh' ? 'zh-CN' : 'en-US');
  $effect(() => {
    for (const group of groups) {
      const state = group === 'quota' ? data.quota : data.news;
      if (state.request_status === 'refreshing') delete notices[group];
    }
  });
  const failures: Record<string, string> = $derived({
    connected: $t('已连接'),
    ready: $t('已就绪'),
    verifying_account: $t('正在确认账号'),
    awaiting_refresh: $t('等待首次刷新'),
    credentials_unreadable: $t('无法读取登录凭据'),
    credentials_invalid: $t('登录文件格式不兼容'),
    credential_config_invalid: $t('认证存储配置不兼容'),
    credential_store_unsupported: $t('系统凭据库暂不支持直读'),
    credentials_ephemeral: $t('凭据仅存于 Codex 进程内存'),
    credentials_expired: $t('登录已过期，请在 Codex 中重新登录'),
    not_signed_in: $t('未登录'),
    reauth_required: $t('需要在 Codex 中重新登录'),
    auth_mode_unsupported: $t('该登录方式不支持账号额度'),
    custom_backend_unsupported: $t('自定义后端不支持此额度接口'),
    account_identity_unavailable: $t('无法确认账号'),
    network_error: $t('网络请求失败'),
    unsupported_response: $t('额度接口响应不兼容'),
    account_mismatch: $t('接口账号与凭据不符'),
    credentials_changed: $t('账号或凭据已变化，旧结果已丢弃'),
    proxy_config_invalid: $t('Codex .env 代理配置无效'),
    proxy_config_unreadable: $t('无法读取 Codex .env 代理配置'),
    http_429: $t('服务端限流'),
    http_403: $t('接口拒绝访问'),
    site_verification_required: $t('站点要求访问验证'),
    body_too_large: $t('接口响应过大'),
    body_read_error: $t('响应读取失败'),
    history_timeout: $t('重置历史同步超时'),
    history_too_large: $t('重置历史超出读取上限'),
    history_page_too_large: $t('重置历史超出读取上限'),
    history_limit_exceeded: $t('重置历史超出读取上限'),
    invalid_history: $t('重置历史格式不兼容'),
    invalid_history_pagination: $t('重置历史格式不兼容'),
    invalid_history_cursor: $t('重置历史分页暂不可用'),
    history_cursor_did_not_advance: $t('重置历史分页暂不可用'),
    missing_history_cache: $t('等待重置历史同步'),
  });
  const label = (code: string) =>
    failures[code] ??
    (code.startsWith('http_')
      ? $t('接口返回 {_0}', { _0: code.slice(5) })
      : code || $t('等待刷新'));
  const isFailure = (code: string) =>
    !['', 'connected', 'ready', 'verifying_account', 'awaiting_refresh'].includes(code);
  async function change(group: 'quota' | 'news', config: RefreshConfig) {
    saving = group;
    error = '';
    try {
      await setRefresh(group, config);
      await reload();
    } catch (e) {
      error = String(e);
    } finally {
      saving = null;
    }
  }
  async function refresh(group: 'quota' | 'news') {
    requesting = group;
    delete notices[group];
    error = '';
    try {
      notices[group] = await refreshNow(group);
      await reload();
    } catch (e) {
      error = String(e);
    } finally {
      requesting = null;
    }
  }
</script>

<div class="cp-divider"></div>
<div class="cp-sectionhead"><span>{$t('网络刷新')}</span><small>{$t('自动保存')}</small></div>
{#each groups as group}
  {@const config = group === 'quota' ? data.settings.quota_refresh : data.settings.news_refresh}
  {@const state = group === 'quota' ? data.quota : data.news}
  {@const name = group === 'quota' ? $t('额度与账户统计') : $t('Resets 消息')}
  {@const busy = state.request_status === 'refreshing' || state.request_status === 'queued'}
  <div class="refresh-group">
    <div class="cp-setting refresh-row">
      <span class="refresh-name">{name}</span>
      <select
        aria-label={$t('{_0}刷新频率', { _0: name })}
        oninput={(e) => e.stopPropagation()}
        value={config.mode === 'manual'
          ? 'manual'
          : custom[group] || !presets.includes(config.interval_seconds)
            ? 'custom'
            : String(config.interval_seconds)}
        disabled={saving !== null}
        onchange={(e) => {
          e.stopPropagation();
          const value = e.currentTarget.value;
          custom[group] = value === 'custom';
          if (value === 'manual') void change(group, { ...config, mode: 'manual' });
          else if (value !== 'custom')
            void change(group, { mode: 'auto', interval_seconds: Number(value) });
          else if (config.mode === 'manual') void change(group, { ...config, mode: 'auto' });
        }}
      >
        {#each presets as seconds}<option value={String(seconds)}
            >{$t(seconds < 60 ? '每 {value} 秒' : '每 {value} 分钟', {
              value: seconds < 60 ? seconds : seconds / 60,
            })}</option
          >{/each}
        <option value="custom">{$t('自定义间隔')}</option>
        <option value="manual">{$t('手动')}</option>
      </select>
      <button
        class="cp-textbutton"
        disabled={busy || requesting !== null || saving !== null}
        onclick={() => void refresh(group)}
        >{busy || requesting === group
          ? $t('刷新中…')
          : state.request_status === 'backoff'
            ? $t('重试')
            : $t('刷新')}</button
      >
    </div>
    {#if config.mode === 'auto' && (custom[group] || !presets.includes(config.interval_seconds))}
      <label class="custom-interval"
        >{$t('间隔（秒）')}
        <input
          aria-label={$t('{_0}自定义刷新秒数', { _0: name })}
          type="number"
          min="30"
          max="86400"
          value={config.interval_seconds}
          disabled={saving !== null}
          oninput={(e) => e.stopPropagation()}
          onchange={(e) => {
            e.stopPropagation();
            if (e.currentTarget.checkValidity())
              void change(group, { mode: 'auto', interval_seconds: Number(e.currentTarget.value) });
          }}
        />
      </label>
    {/if}
    <p class="cp-note refresh-meta" role="status">
      {time(state.last_success)}
      {#if state.request_status === 'backoff'}
        {#if state.next_attempt}{$t('· 下次尝试 {_0}', {
            _0: retryTime(state.next_attempt),
          })}{:else}{$t('· 稍后重试')}{/if}
      {/if}
    </p>
    {#if notices[group] !== undefined && !busy}
      <p class="cp-note refresh-meta" role="status">
        {notices[group]
          ? $t('已安排重试，预计 {_0} 再次尝试', { _0: retryTime(notices[group]!) })
          : $t('已请求刷新')}
      </p>
    {/if}
    {#if group === 'quota'}
      {#each Object.entries(data.quota.sources) as [id, source]}
        {#if isFailure(source.status)}<p class="cp-note" role="status">
            {sourceNames([id], data.sources)} · {label(source.status)}
          </p>{/if}
        {#if isFailure(source.profile_status)}<p class="cp-note" role="status">
            {sourceNames([id], data.sources)}
            {$t('· 账户统计：')}{label(source.profile_status)}
          </p>{/if}
      {/each}
    {:else}
      {#if isFailure(data.news.status)}<p class="cp-note" role="status">
          {$t('重置公告 / Tibo ·')}
          {label(data.news.status)}
        </p>{/if}
      {#if isFailure(data.news.history_status ?? '')}<p class="cp-note" role="status">
          {$t('重置历史 ·')}{label(data.news.history_status ?? '')}
        </p>{/if}
      {#if isFailure(data.news.challenge_status)}<p class="cp-note" role="status">
          {$t('28 天挑战 ·')}
          {label(data.news.challenge_status)}
        </p>{/if}
    {/if}
  </div>
{/each}
{#if error}<p class="cp-note" role="alert">{localizeError(error, $locale)}</p>{/if}

<style>
  .refresh-group {
    border-bottom: 1px solid var(--cp-line);
    padding: 8px 0;
  }
  .refresh-row {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 13px;
    padding: 0;
  }
  .refresh-name {
    flex: 1;
  }
  .refresh-row select {
    max-width: 150px;
  }
  .refresh-row button {
    flex-shrink: 0;
    min-width: 42px;
    min-height: 28px;
    padding: 4px 8px !important;
    justify-content: center;
  }
  .refresh-meta {
    margin: 5px 0 0;
  }
  .custom-interval {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    margin-top: 8px;
  }
  input {
    width: 110px;
    padding: 4px 7px;
    color: inherit;
    background: transparent;
    border: 1px solid var(--cp-line);
    border-radius: 4px;
  }
</style>
