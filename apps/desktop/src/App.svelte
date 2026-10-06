<script lang="ts">
  import { onMount } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { empty, native, onEvent, saveSettings, snapshot, windowAction } from './lib/ipc';
  import type { Settings } from './lib/types';
  let data = $state(empty);
  let mode = $state<'compact' | 'details'>(native ? 'compact' : 'details');
  let page = $state('overview');
  let selectedModel = $state<string | null>(null);
  let selectedSession = $state<string | null>(null);
  let error = $state('');
  let saving = $state(false);
  let settings = $state<Settings>({ ...empty.settings });
  let path = $state('');
  const tabs = [
    ['overview', '总览'],
    ['models', '模型'],
    ['sessions', 'Sessions'],
    ['news', '消息'],
    ['settings', '设置'],
  ];
  const number = (n: number) =>
    new Intl.NumberFormat('zh-CN', { notation: 'compact', maximumFractionDigits: 2 }).format(n);
  const money = (cost: number, unpriced: number) =>
    unpriced > 0
      ? cost > 0
        ? `$${(cost / 1e9).toFixed(2)} 已计价部分`
        : '待计价'
      : `$${(cost / 1e9).toFixed(2)}`;
  const time = (ts: string | null) =>
    ts
      ? new Intl.DateTimeFormat('zh-CN', {
          month: 'short',
          day: 'numeric',
          hour: '2-digit',
          minute: '2-digit',
        }).format(new Date(ts))
      : '尚未同步';
  const current = $derived(
    data.recent.find((s) => s.meta.id === selectedSession) ??
      data.recent.find((s) => s.meta.status === 'active') ??
      data.recent[0],
  );
  const model = $derived(data.usage.models.find((m) => m.model === selectedModel));
  const highest = $derived(Math.max(1, ...data.usage.days.map((d) => d.total)));
  async function refresh() {
    try {
      data = await snapshot();
      if (!saving) {
        settings = { ...data.settings };
        path = settings.windows_home ?? '';
      }
    } catch {
      error = '无法连接后台采集器';
    }
  }
  async function save() {
    saving = true;
    error = '';
    try {
      await saveSettings({ ...settings, windows_home: path.trim() || null });
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }
  function drag(e: PointerEvent) {
    if (native && e.button === 0) void getCurrentWindow().startDragging();
  }
  onMount(() => {
    let disposed = false;
    const cleanups: (() => void)[] = [];
    void refresh();
    const events = [
      onEvent('snapshot-changed', () => {
        if (!document.hidden) void refresh();
      }),
      onEvent<[string, string | null]>('window-mode', ([next, target]) => {
        mode = next as typeof mode;
        if (target) page = target;
        void refresh();
      }),
      onEvent<Settings>('settings-applied', (next) => {
        settings = { ...next };
        void refresh();
      }),
    ];
    void Promise.all(events).then((list) => {
      if (disposed) list.forEach((fn) => fn());
      else cleanups.push(...list);
    });
    const visible = () => {
      if (!document.hidden) void refresh();
    };
    document.addEventListener('visibilitychange', visible);
    const escape = (e: KeyboardEvent) => {
      if (e.key === 'Escape') void windowAction('compact');
    };
    window.addEventListener('keydown', escape);
    return () => {
      disposed = true;
      cleanups.forEach((fn) => fn());
      document.removeEventListener('visibilitychange', visible);
      window.removeEventListener('keydown', escape);
    };
  });
</script>

<main class:compact={mode === 'compact'} class:opaque={!settings.glass} data-theme={settings.theme}>
  <header>
    <button class="brand" onpointerdown={drag} aria-label="拖动 CodexPulse 窗口"
      ><svg viewBox="0 0 32 32" aria-hidden="true"><path d="M3 17h7l4-9 5 17 4-8h6" /></svg><span
        >Codex<span class="brand-pulse">Pulse</span></span
      ></button
    >
    <div class="head-actions">
      <span class="status-dot" class:busy={data.collecting}></span><button
        class="icon"
        aria-label="打开设置"
        onclick={() => {
          page = 'settings';
          void windowAction('settings');
        }}>⚙</button
      ><button class="icon" aria-label="收起窗口" onclick={() => void windowAction('compact')}
        >−</button
      >
    </div>
  </header>
  {#if mode === 'compact'}
    <button class="compact-content" onclick={() => void windowAction('expand')}>
      <div class="eyebrow">
        {current?.meta.status === 'active' ? '当前 session' : '最近 session'}<span
          >{data.collecting ? '正在索引' : '本地用量'}</span
        >
      </div>
      <div class="current-title">
        {current?.meta.title ?? current?.meta.project ?? '等待工作 session'}
      </div>
      <div class="compact-metrics">
        <div>
          <strong>{current ? number(current.total) : '—'}</strong><small>Session tokens</small>
        </div>
        <div><strong>—</strong><small>输出 tokens/s</small></div>
      </div>
      <div class="quota-placeholder"><span>额度</span><span>等待接口接入</span></div>
      <div class="compact-footer">
        <span>30 天 {number(data.usage.total)} tokens</span><span>展开详情 ↗</span>
      </div>
    </button>
  {:else}
    <nav aria-label="详情页面">
      {#each tabs as [key, label]}<button class:active={page === key} onclick={() => (page = key)}
          >{label}</button
        >{/each}
    </nav>
    <div class="content">
      {#if data.error || error}<div class="notice" role="status">{error || data.error}</div>{/if}
      {#if page === 'overview'}
        <div class="section-heading">
          <div>
            <p class="eyebrow">YOUR CODEX, AT A GLANCE</p>
            <h1>工作脉搏</h1>
          </div>
          <span class="badge">{data.collecting ? '正在索引…' : '已连接本地账本'}</span>
        </div>
        <section class="card quotas">
          <div class="card-heading">
            <h2>使用额度</h2>
            <span class="muted">待接入</span>
          </div>
          <p class="muted">额度接口正在开发，恢复时间与剩余额度暂不可用。</p>
        </section>
        <section class="card session-card">
          <div class="card-heading">
            <h2>{current?.meta.status === 'active' ? '当前工作 session' : '最近工作 session'}</h2>
            <span class="badge"
              >{current?.meta.status === 'active'
                ? '运行中'
                : current?.meta.status === 'completed'
                  ? '已完成'
                  : '状态未知'}</span
            >
          </div>
          <h3>{current?.meta.title ?? current?.meta.project ?? '暂无 session'}</h3>
          <p class="muted ellipsis">
            {current?.models.join(' · ') || '等待模型信息'} · {current?.sources.join(' / ') ||
              '等待数据源'}
          </p>
          <div class="metrics two">
            <div>
              <strong>{current ? number(current.total) : '—'}</strong><small>Session tokens</small>
            </div>
            <div>
              <strong>{current ? money(current.cost_nanousd, current.unpriced_tokens) : '—'}</strong
              ><small>API 等价估算</small>
            </div>
          </div>
          <div class="speed">
            <span>当前输出速度 <b>—</b> tokens/s</span><small>日志不提供连续输出 token 样本</small>
          </div>
        </section>
        <section class="card">
          <div class="card-heading">
            <h2>最近 30 天 · 所有模型</h2>
            <button class="text-button" onclick={() => (page = 'models')}>模型明细 →</button>
          </div>
          <div class="metrics three">
            <div><strong>{number(data.usage.total)}</strong><small>总 tokens</small></div>
            <div><strong>{data.usage.sessions}</strong><small>Sessions</small></div>
            <div>
              <strong>{money(data.usage.cost_nanousd, data.usage.unpriced_tokens)}</strong><small
                >美元估算</small
              >
            </div>
          </div>
          <div class="chart" role="img" aria-label="最近三十个自然日 token 用量">
            {#each data.usage.days as day}<div
                class="bar"
                style:height={`${Math.max(3, (day.total / highest) * 100)}%`}
                title={`${day.day}: ${day.total.toLocaleString()} tokens`}
              ></div>{/each}
          </div>
          <div class="chart-labels">
            <span>{data.usage.from_day || '30 天前'}</span><span
              >{data.usage.through_day || '今天'}</span
            >
          </div>
          <p class="footnote">统计时区：{data.usage.timezone} · 已连接来源的可归属记录</p>
        </section>
      {:else if page === 'models'}
        <div class="section-heading">
          <div>
            <p class="eyebrow">LAST 30 DAYS</p>
            <h1>模型用量</h1>
          </div>
          <span class="badge">{data.usage.models.length} 个模型</span>
        </div>
        <section class="card">
          <div class="metrics three">
            <div><strong>{number(data.usage.total)}</strong><small>所有模型总 tokens</small></div>
            <div><strong>{data.usage.sessions}</strong><small>独立 sessions</small></div>
            <div>
              <strong>{money(data.usage.cost_nanousd, data.usage.unpriced_tokens)}</strong><small
                >API 等价估算</small
              >
            </div>
          </div>
        </section>
        <div class="model-list">
          {#each data.usage.models as m}<button
              class="card model-row"
              class:selected={selectedModel === m.model}
              onclick={() => (selectedModel = selectedModel === m.model ? null : m.model)}
              ><div>
                <h3>{m.model}</h3>
                <p class="muted">
                  {m.sessions} sessions · {money(m.cost_nanousd, m.unpriced_tokens)}
                </p>
              </div>
              <strong>{number(m.total)}</strong>
              <div class="share-track">
                <span style:width={`${data.usage.total ? (m.total / data.usage.total) * 100 : 0}%`}
                ></span>
              </div></button
            >{:else}<div class="empty-state">
              尚无可归属的模型用量。连接数据源后自动更新。
            </div>{/each}
        </div>
        {#if model}<section class="card">
            <h2>{model.model} · token 拆分</h2>
            <dl>
              <dt>输入</dt>
              <dd>{number(model.input)}</dd>
              <dt>其中缓存输入</dt>
              <dd>{number(model.cached)}</dd>
              <dt>输出</dt>
              <dd>{number(model.output)}</dd>
              <dt>未计价 tokens</dt>
              <dd>{number(model.unpriced_tokens)}</dd>
            </dl>
            <p class="footnote">
              {model.incomplete_events > 0
                ? `${model.incomplete_events} 条记录缺少拆分字段；这里仅展示已知部分。`
                : '缓存为输入的子集，不重复加进总量。'}
            </p>
          </section>{/if}
      {:else if page === 'sessions'}
        <div class="section-heading">
          <div>
            <p class="eyebrow">RECENT WORK</p>
            <h1>最近 Sessions</h1>
          </div>
          <span class="badge">最近 10 个</span>
        </div>
        {#each data.recent as s}<button
            class="card session-row"
            class:selected={selectedSession === s.meta.id}
            onclick={() => (selectedSession = s.meta.id)}
            ><div class="card-heading">
              <h3>{s.meta.title ?? s.meta.project ?? s.meta.id.slice(0, 8)}</h3>
              <span class="badge"
                >{s.meta.status === 'active'
                  ? '运行中'
                  : s.meta.status === 'completed'
                    ? '已完成'
                    : s.meta.status === 'interrupted'
                      ? '已中断'
                      : '状态未知'}</span
              >
            </div>
            <p class="muted ellipsis">{s.models.join(' · ') || '模型未知'}</p>
            <div class="row-bottom">
              <span>{s.sources.join(' / ')}</span><strong>{number(s.total)} tokens</strong>
            </div>
            <div class="row-bottom">
              <small>{time(s.meta.last_activity)}</small><small
                >{money(s.cost_nanousd, s.unpriced_tokens)}</small
              >
            </div>
            {#if selectedSession === s.meta.id}<p class="footnote">
                Session ID：{s.meta.id}{s.meta.parent_id ? ` · 继承自 ${s.meta.parent_id}` : ''}
              </p>{/if}</button
          >{:else}<div class="empty-state">还没有采集到 session 元数据。</div>{/each}
      {:else if page === 'news'}
        <div class="section-heading">
          <div>
            <p class="eyebrow">RESET SIGNALS</p>
            <h1>重置消息</h1>
          </div>
          <span class="badge">待接入</span>
        </div>
        <section class="card">
          <h2>Tibo 的重置相关消息</h2>
          <p class="muted">
            Codex Resets
            公共接口接入正在开发。这里将分别显示公告、计划、预测与社区观察，并保留原始来源链接。
          </p>
          <p class="footnote">消息出现不代表当前账户已恢复；账户恢复需要额度快照确认。</p>
          <span class="muted">数据来源：codex-resets.com</span>
        </section>
      {:else if page === 'settings'}
        <div class="section-heading">
          <div>
            <p class="eyebrow">MAKE IT YOURS</p>
            <h1>设置</h1>
          </div>
          <span class="badge">Windows</span>
        </div>
        <section class="card">
          <h2>外观与浮窗</h2>
          <label class="setting-row"
            ><span>主题</span><select bind:value={settings.theme}
              ><option value="system">跟随系统</option><option value="light">浅色</option><option
                value="dark">深色</option
              ></select
            ></label
          ><label class="setting-row"
            ><span>毛玻璃效果<small>关闭后使用不透明背景</small></span><input
              type="checkbox"
              role="switch"
              bind:checked={settings.glass}
            /></label
          ><label class="setting-row"
            ><span>桌面浮窗<small>关闭后通过托盘打开同一个详情</small></span><input
              type="checkbox"
              role="switch"
              bind:checked={settings.floating}
            /></label
          ><label class="setting-row"
            ><span>窗口始终置顶</span><input
              type="checkbox"
              role="switch"
              bind:checked={settings.always_on_top}
            /></label
          >
        </section>
        <section class="card">
          <h2>数据采集</h2>
          <label class="setting-row"
            ><span>Windows App / CLI</span><input
              type="checkbox"
              role="switch"
              bind:checked={settings.windows_enabled}
            /></label
          ><label class="setting-row"
            ><span>已运行的 WSL<small>不主动启动已停止的发行版</small></span><input
              type="checkbox"
              role="switch"
              bind:checked={settings.wsl_enabled}
            /></label
          ><label class="path-label"
            >Windows Codex home<input
              bind:value={path}
              placeholder="自动探测 CODEX_HOME 或 .codex"
            /></label
          >
          <p class="footnote">统计时区：{settings.timezone} · 不保存对话正文，不上传本地用量。</p>
        </section>
        <button class="primary save-button" disabled={saving} onclick={() => void save()}
          >{saving ? '正在保存…' : '保存设置'}</button
        >
        <section class="card source-card">
          <h2>来源状态</h2>
          {#each data.sources as source}<div class="source-row">
              <div class="card-heading">
                <h3>{source.label}</h3>
                <span class="badge"
                  >{source.status === 'connected'
                    ? '已连接'
                    : source.status === 'no_logs'
                      ? '暂无日志'
                      : source.status === 'read_error'
                        ? '读取失败'
                        : '探测中'}</span
                >
              </div>
              <p class="muted path-value">{source.path}</p>
              <small
                >{source.files} 个日志 · {source.issues} 个解析问题 · {time(
                  source.last_read,
                )}</small
              >
            </div>{:else}<p class="muted">等待来源探测</p>{/each}
        </section>
      {/if}
    </div>
    <footer>
      <span>{data.collecting ? '正在建立本地索引…' : `同步于 ${time(data.updated_at)}`}</span><span
        >CodexPulse 0.1 · 开发中</span
      >
    </footer>
  {/if}
</main>
