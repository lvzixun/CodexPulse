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
    value ? new Date(value).toLocaleString() : '尚未成功刷新';
  const stateNames: Record<string, string> = {
    refreshing: '正在刷新',
    queued: '等待刷新',
    backoff: '等待退避后重试',
    manual: '手动 · 无后台请求',
    idle: '等待自动刷新',
  };
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
<div class="cp-sectionhead"><span>网络刷新</span><small>修改后立即保存</small></div>
{#each groups as group}
  {@const config = group === 'quota' ? data.settings.quota_refresh : data.settings.news_refresh}
  {@const state = group === 'quota' ? data.quota : data.news}
  <div class="refresh-group">
    <div class="cp-sectionhead">
      <span>{group === 'quota' ? 'Codex 账号额度刷新' : 'Codex Resets 消息刷新'}</span>
    </div>
    <label class="cp-setting"
      >刷新方式
      <select
        oninput={(e) => e.stopPropagation()}
        value={config.mode}
        disabled={saving !== null}
        onchange={(e) => {
          e.stopPropagation();
          void change(group, { ...config, mode: e.currentTarget.value as RefreshConfig['mode'] });
        }}
      >
        <option value="auto">自动</option><option value="manual">手动</option>
      </select>
    </label>
    <label class="cp-setting"
      >自动刷新间隔
      <select
        oninput={(e) => e.stopPropagation()}
        value={custom[group] || !presets.includes(config.interval_seconds)
          ? 'custom'
          : String(config.interval_seconds)}
        disabled={saving !== null || config.mode === 'manual'}
        onchange={(e) => {
          e.stopPropagation();
          const value = e.currentTarget.value;
          custom[group] = value === 'custom';
          if (value !== 'custom')
            void change(group, { ...config, interval_seconds: Number(value) });
        }}
      >
        {#each presets as seconds}<option value={String(seconds)}
            >{seconds < 60 ? `${seconds} 秒` : `${seconds / 60} 分钟`}</option
          >{/each}
        <option value="custom">自定义</option>
      </select>
    </label>
    {#if custom[group] || !presets.includes(config.interval_seconds)}
      <label class="cp-setting"
        >自定义秒数
        <input
          aria-label={`${group === 'quota' ? '额度' : '消息'}自定义刷新秒数`}
          type="number"
          oninput={(e) => e.stopPropagation()}
          min="30"
          max="86400"
          value={config.interval_seconds}
          disabled={saving !== null || config.mode === 'manual'}
          onchange={(e) => {
            e.stopPropagation();
            if (e.currentTarget.checkValidity())
              void change(group, { ...config, interval_seconds: Number(e.currentTarget.value) });
          }}
        />
      </label>
    {/if}
    <div class="refresh-action">
      <span>{stateNames[state.request_status] ?? '等待初始化'}</span>
      <button
        class="cp-textbutton"
        disabled={state.request_status === 'refreshing' ||
          state.request_status === 'queued' ||
          saving !== null}
        onclick={() => void refresh(group)}>立即刷新</button
      >
    </div>
    <p class="cp-note">
      最近成功：{time(state.last_success)}
      {#if state.next_attempt && config.mode === 'auto'}<br />下次计划：{new Date(
          state.next_attempt * 1000,
        ).toLocaleString()}{/if}
    </p>
    {#if group === 'quota'}
      {#each Object.entries(data.quota.sources) as [id, source]}
        <p class="cp-note">
          {sourceNames([id], data.sources)} · {label(source.status)}
          {#if source.proxy_source === 'codex_env'}
            · Codex .env 代理{:else if source.proxy_source === 'process_env'}
            · 进程代理{:else if source.proxy_source === 'no_proxy'}
            · NO_PROXY 直连{:else if source.proxy_source === 'direct'}
            · 直连{/if}
        </p>
      {/each}
      <p class="cp-note">
        通过 HTTPS 读取当前登录账号，不启动 CLI。无法连接时保留同账号缓存并显示状态。
      </p>
    {:else}
      <p class="cp-note">
        重置公告 / Tibo：{label(data.news.status)} · 28 天挑战：{label(
          data.news.challenge_status,
        )}<br />遵守服务端缓存与退避；翻译仍由点击触发。
      </p>
    {/if}
  </div>
{/each}
{#if error}<p class="cp-note" role="alert">{error}</p>{/if}

<style>
  .refresh-group {
    border-bottom: 1px solid var(--cp-line);
    padding: 8px 0 12px;
  }
  .refresh-action {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 12px;
    margin-top: 6px;
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
