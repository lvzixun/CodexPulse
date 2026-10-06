<script lang="ts">
  import type { Settings } from '../lib/types';
  let { settings = $bindable(), changed }: { settings: Settings; changed: () => void } = $props();
  function addWindows() {
    settings.windows_sources = [
      ...settings.windows_sources,
      { id: crypto.randomUUID(), label: '额外 Windows 来源', home: '', enabled: true },
    ];
    changed();
  }
  function addWsl() {
    settings.wsl_sources = [
      ...settings.wsl_sources,
      { id: crypto.randomUUID(), distro: 'Ubuntu', user: '', home: '', enabled: true },
    ];
    changed();
  }
</script>

<p class="cp-note">
  如果 App 和 CLI 使用不同目录，可添加额外来源。停用来源后保留已采集的历史统计。
</p>
{#each settings.windows_sources as source (source.id)}
  <fieldset class="cp-source-config">
    <legend>额外 Windows 来源</legend>
    <label class="cp-path">显示名称<input bind:value={source.label} maxlength="64" /></label>
    <label class="cp-path"
      >Codex home<input
        bind:value={source.home}
        placeholder="D:\CodexData\.codex"
        maxlength="4096"
      /></label
    >
    <div class="cp-config-actions">
      <label>启用<input type="checkbox" bind:checked={source.enabled} /></label><button
        type="button"
        class="cp-textbutton"
        onclick={() => {
          settings.windows_sources = settings.windows_sources.filter((s) => s.id !== source.id);
          changed();
        }}>移除配置</button
      >
    </div>
  </fieldset>
{/each}
<button
  type="button"
  class="cp-textbutton"
  disabled={settings.windows_sources.length >= 8}
  onclick={addWindows}>添加 Windows 来源</button
>
<label class="cp-setting"
  ><span
    >自动探测 WSL 默认用户目录<small class="cp-setting-hint"
      >每 30 秒检查运行状态，已解析的路径保留缓存</small
    ></span
  ><input type="checkbox" role="switch" bind:checked={settings.wsl_auto_detect} /></label
>
{#each settings.wsl_sources as source (source.id)}
  <fieldset class="cp-source-config">
    <legend>指定 WSL 来源</legend>
    <label class="cp-path"
      >发行版<input bind:value={source.distro} maxlength="128" placeholder="Ubuntu" /></label
    >
    <label class="cp-path"
      >Linux 用户<input
        bind:value={source.user}
        maxlength="128"
        placeholder="留空使用发行版默认用户"
      /></label
    >
    <label class="cp-path"
      >Linux Codex home<input
        bind:value={source.home}
        maxlength="4096"
        placeholder="/home/user/.codex"
      /></label
    >
    <div class="cp-config-actions">
      <label>启用<input type="checkbox" bind:checked={source.enabled} /></label><button
        type="button"
        class="cp-textbutton"
        onclick={() => {
          settings.wsl_sources = settings.wsl_sources.filter((s) => s.id !== source.id);
          changed();
        }}>移除配置</button
      >
    </div>
  </fieldset>
{/each}
<button
  type="button"
  class="cp-textbutton"
  disabled={settings.wsl_sources.length >= 8}
  onclick={addWsl}>添加 WSL 来源</button
>
<p class="cp-note">
  各类最多 8 个额外来源。Windows 路径填写本地绝对路径；WSL 路径填写 Linux
  绝对路径，只在对应发行版已运行时读取。用户设置用于该来源的额度读取，不复制认证信息。
</p>
