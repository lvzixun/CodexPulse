<script lang="ts">
  import type { RefreshConfig, Snapshot } from '../lib/types';
  import { setRefresh, refreshNow } from '../lib/ipc';
  import { sourceNames } from '../lib/format';
  let { data, reload }: { data: Snapshot; reload: () => Promise<void> } = $props();
  const presets = [30, 60, 300, 900, 1800, 3600];
  const groups = ['quota', 'news'] as const;
  let saving = $state<string | null>(null);
  let error = $state('');
  let custom = $state<Record<string, boolean>>({});
  const time = (value: string | null) =>
    value ? `上次更新 ${new Date(value).toLocaleString()}` : '尚未更新';
  const failures: Record<string, string> = {
    connected: '已连接',
    ready: '已就绪',
    verifying_account: '正在确认账号',
    awaiting_refresh: '等待首次刷新',
    credentials_unreadable: '无法读取登录凭据',
    credentials_invalid: '登录文件格式不兼容',
    credential_config_invalid: '认证存储配置不兼容',
    credential_store_unsupported: '系统凭据库暂不支持直读',
    credentials_ephemeral: '凭据仅存于 Codex 进程内存',
    credentials_expired: '登录已过期，请在 Codex 中重新登录',
    not_signed_in: '未登录',
    reauth_required: '需要在 Codex 中重新登录',
    auth_mode_unsupported: '该登录方式不支持账号额度',
    custom_backend_unsupported: '自定义后端不支持此额度接口',
    account_identity_unavailable: '无法确认账号',
    network_error: '网络请求失败',
    unsupported_response: '额度接口响应不兼容',
    account_mismatch: '接口账号与凭据不符',
    credentials_changed: '账号或凭据已变化，旧结果已丢弃',
    proxy_config_invalid: 'Codex .env 代理配置无效',
    http_429: '服务端限流',
    http_403: '接口拒绝访问',
    body_too_large: '接口响应过大',
    body_read_error: '响应读取失败',
  };
  const label = (code: string) =>
    failures[code] ?? (code.startsWith('http_') ? `接口返回 ${code.slice(5)}` : code || '等待刷新');
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
    error = '';
    try {
      await refreshNow(group);
      await reload();
    } catch (e) {
      error = String(e);
    }
  }
</script>

<div class="cp-divider"></div>
<div class="cp-sectionhead"><span>网络刷新</span><small>自动保存</small></div>
{#each groups as group}
  {@const config = group === 'quota' ? data.settings.quota_refresh : data.settings.news_refresh}
  {@const state = group === 'quota' ? data.quota : data.news}
  {@const name = group === 'quota' ? '账号额度' : 'Resets 消息'}
  {@const busy = state.request_status === 'refreshing' || state.request_status === 'queued'}
  <div class="refresh-group">
    <div class="cp-setting refresh-row">
      <span class="refresh-name">{name}</span>
      <select
        aria-label={`${name}刷新频率`}
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
            >每 {seconds < 60 ? `${seconds} 秒` : `${seconds / 60} 分钟`}</option
          >{/each}
        <option value="custom">自定义间隔</option>
        <option value="manual">手动</option>
      </select>
      <button
        class="cp-textbutton"
        disabled={busy || saving !== null}
        onclick={() => void refresh(group)}>{busy ? '刷新中…' : '刷新'}</button
      >
    </div>
    {#if config.mode === 'auto' && (custom[group] || !presets.includes(config.interval_seconds))}
      <label class="custom-interval"
        >间隔（秒）
        <input
          aria-label={`${name}自定义刷新秒数`}
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
        · 稍后重试{/if}
    </p>
    {#if group === 'quota'}
      {#each Object.entries(data.quota.sources) as [id, source]}
        {#if isFailure(source.status)}<p class="cp-note" role="status">
            {sourceNames([id], data.sources)} · {label(source.status)}
          </p>{/if}
      {/each}
    {:else}
      {#if isFailure(data.news.status)}<p class="cp-note" role="status">
          重置公告 / Tibo · {label(data.news.status)}
        </p>{/if}
      {#if isFailure(data.news.challenge_status)}<p class="cp-note" role="status">
          28 天挑战 · {label(data.news.challenge_status)}
        </p>{/if}
    {/if}
  </div>
{/each}
{#if error}<p class="cp-note" role="alert">{error}</p>{/if}

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
