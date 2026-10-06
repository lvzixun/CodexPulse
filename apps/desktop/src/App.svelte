<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import {
    empty,
    native,
    onEvent,
    saveSettings,
    snapshot,
    windowAction,
    getViewState,
    rememberView,
    defaultSessionQuery,
    readNews,
  } from './lib/ipc';
  import type { Settings, SessionPageRequest } from './lib/types';
  import QuotaCard from './components/QuotaCard.svelte';
  import NewsCard from './components/NewsCard.svelte';
  import Icon from './components/Icon.svelte';
  import ResetStatus from './components/ResetStatus.svelte';
  import ChallengePanel from './components/ChallengePanel.svelte';
  import SessionsPane from './components/SessionsPane.svelte';
  import SourceSettings from './components/SourceSettings.svelte';
  import {
    windowLabel,
    sessionTokens,
    sessionActivity,
    sessionLabel,
    sourceNames,
  } from './lib/format';
  import { openSource } from './lib/ipc';
  let data = $state(empty);
  let mode = $state<'compact' | 'details'>(native ? 'compact' : 'details');
  let page = $state('overview');
  let selectedModel = $state<string | null>(null);
  let selectedSession = $state<string | null>(null);
  let sessionQuery = $state<SessionPageRequest>(defaultSessionQuery());
  let error = $state('');
  let saving = $state(false);
  let settingsDirty = $state(false);
  let now = $state(Date.now());
  const copySettings = (value: Settings): Settings => ({
    ...value,
    windows_sources: value.windows_sources.map((s) => ({ ...s })),
    wsl_sources: value.wsl_sources.map((s) => ({ ...s })),
  });
  let settings = $state<Settings>(copySettings(empty.settings));
  let path = $state('');
  let viewReady = $state(false);
  let glassSupported = $state(!native);
  let floatingSupported = $state(true);
  let scroll = $state<Record<string, number>>({});
  let content: HTMLDivElement | undefined = $state();
  let restoreScroll = false;
  const tabs = [
    ['overview', '总览'],
    ['models', '模型'],
    ['sessions', 'Sessions'],
    ['news', '消息'],
    ['settings', '设置'],
  ];
  const accents: [Settings['accent'], string, string, string][] = [
    ['blue', '蓝色', '#8bbbff', '#2261d6'],
    ['violet', '紫色', '#b5b4ff', '#5148bf'],
    ['teal', '青绿', '#6cdbc7', '#0b7564'],
    ['amber', '琥珀', '#f2c675', '#915600'],
    ['rose', '玫红', '#ffa7c4', '#aa3462'],
  ];
  const number = (n: number) =>
    new Intl.NumberFormat('en-US', { notation: 'compact', maximumFractionDigits: 1 }).format(n);
  const compactNumber = (n: number) =>
    new Intl.NumberFormat('en-US', { notation: 'compact', maximumFractionDigits: 0 }).format(n);
  const magnitude = (n: number) => {
    const divisor = n >= 1e9 ? 1e9 : n >= 1e6 ? 1e6 : n >= 1e3 ? 1e3 : 1;
    return {
      value: (n / divisor).toLocaleString('en-US', { maximumFractionDigits: 1 }),
      unit: divisor === 1e9 ? 'B' : divisor === 1e6 ? 'M' : divisor === 1e3 ? 'K' : '',
    };
  };
  const shortDate = (day: string) => (day ? day.slice(5).replace('-', '/') : '—');
  const dateRange = $derived(
    `${shortDate(data.usage.from_day)} — ${shortDate(data.usage.through_day)}`,
  );
  const money = (cost: number, unpriced: number) =>
    unpriced > 0
      ? cost > 0
        ? `$${(cost / 1e9).toFixed(2)}*`
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
  const activityInfo = $derived(sessionActivity(data, now));
  const working = $derived(
    activityInfo.sessions.find((s) => s.meta.id === selectedSession) ?? activityInfo.working,
  );
  const activity = $derived(activityInfo.state);
  const activityLabel = $derived(
    activity === 'busy'
      ? `忙${activityInfo.sessions.length > 1 ? activityInfo.sessions.length : ''}`
      : activity === 'idle'
        ? '闲'
        : '?',
  );
  const activityTitle = $derived(
    activity === 'busy'
      ? `${activityInfo.sessions.length} 个 session 工作中（最近 5 分钟有日志事件）`
      : activity === 'idle'
        ? '未检测到工作中的 session'
        : '工作状态未知：日志过期或数据源未连接',
  );
  const current = $derived(working ?? data.recent[0]);
  const compactQuota = $derived(data.quota.buckets[0]);
  const compactWindows = $derived(
    [compactQuota?.secondary ?? compactQuota?.primary].filter((w) => w != null),
  );
  const compactStale = $derived(
    compactQuota != null &&
      (now - new Date(compactQuota.captured_at).getTime() > 6 * 60 * 1000 ||
        data.quota.sources[compactQuota.source_id]?.status !== 'connected'),
  );
  const highest = $derived(Math.max(1, ...data.usage.days.map((d) => d.total)));
  const unreadKeys = $derived(data.news.unread_keys ?? []);
  const importantUnread = $derived(data.news.important_unread ?? 0);
  async function acknowledgeNews() {
    try {
      await readNews([...unreadKeys]);
      await refresh();
    } catch (e) {
      error = String(e);
    }
  }
  async function togglePin() {
    try {
      await saveSettings({ ...data.settings, always_on_top: !data.settings.always_on_top });
      await refresh();
    } catch (e) {
      error = String(e);
    }
  }
  async function goPage(next: string) {
    if (content) scroll[page] = content.scrollTop;
    page = next;
    await restorePageScroll();
  }
  async function restorePageScroll() {
    restoreScroll = true;
    await tick();
    if (content) content.scrollTop = scroll[page] ?? 0;
    restoreScroll = false;
  }
  $effect(() => {
    const view = {
      mode,
      page,
      selected_model: selectedModel,
      selected_session: selectedSession,
      session_query: sessionQuery,
      scroll: { ...scroll },
      glass_supported: glassSupported,
      floating_supported: floatingSupported,
    };
    if (!viewReady) return;
    const timer = setTimeout(() => void rememberView(view).catch(() => {}), 200);
    return () => clearTimeout(timer);
  });
  async function refresh() {
    try {
      data = await snapshot();
      now = Date.now();
      if (!saving && !settingsDirty) {
        settings = copySettings(data.settings);
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
      settingsDirty = false;
      saving = false;
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
    let clock: ReturnType<typeof setInterval> | undefined;
    const clockVisible = (visible: boolean) => {
      clearInterval(clock);
      if (visible) {
        now = Date.now();
        clock = setInterval(() => (now = Date.now()), 60000);
      }
    };
    clockVisible(!document.hidden);
    const cleanups: (() => void)[] = [];
    const events = [
      onEvent('snapshot-changed', () => {
        if (!document.hidden) void refresh();
      }),
      onEvent<[string, string | null]>('window-mode', ([next, target]) => {
        if (content) scroll[page] = content.scrollTop;
        mode = next as typeof mode;
        if (target) void goPage(target);
        else void restorePageScroll();
        void refresh();
      }),
      onEvent<Settings>('settings-applied', (next) => {
        if (!settingsDirty || saving) {
          settings = { ...next };
          path = next.windows_home ?? '';
        }
        void refresh();
      }),
      onEvent<boolean>('window-visible', clockVisible),
      onEvent<boolean>('glass-supported', (supported) => (glassSupported = supported)),
    ];
    void Promise.all(events).then(async (list) => {
      if (disposed) list.forEach((fn) => fn());
      else {
        cleanups.push(...list);
        await refresh();
        const view = await getViewState();
        if (disposed) return;
        if (view) {
          mode = view.mode;
          page = view.page;
          selectedModel = view.selected_model;
          selectedSession = view.selected_session;
          sessionQuery = view.session_query ?? defaultSessionQuery();
          scroll = view.scroll;
          glassSupported = view.glass_supported;
          floatingSupported = view.floating_supported;
        }
        viewReady = true;
        await restorePageScroll();
      }
    });
    const visible = () => {
      clockVisible(!document.hidden);
      if (!document.hidden) void refresh();
    };
    document.addEventListener('visibilitychange', visible);
    const escape = (e: KeyboardEvent) => {
      if (e.key === 'Escape') void windowAction('compact');
      if (e.ctrlKey && e.key.toLowerCase() === 'q') void windowAction('exit');
    };
    window.addEventListener('keydown', escape);
    return () => {
      clearInterval(clock);
      disposed = true;
      cleanups.forEach((fn) => fn());
      document.removeEventListener('visibilitychange', visible);
      window.removeEventListener('keydown', escape);
    };
  });
</script>

<main
  class="cp-shell"
  class:native-shell={native}
  class:compact={mode === 'compact'}
  class:opaque={!settings.glass || !glassSupported}
  data-theme={settings.theme}
  data-accent={settings.accent ?? 'blue'}
>
  {#if mode === 'compact'}
    <div class="cp-compact-line">
      <button
        class="cp-compact-drag"
        class:cp-working={activity === 'busy'}
        class:cp-unknown={activity === 'unknown'}
        class:cp-indexing={data.collecting}
        onpointerdown={drag}
        aria-label={`${activityTitle} · 拖动 CodexPulse 浮窗`}
        title={`CodexPulse · ${activityTitle}${data.collecting ? ' · 索引中' : ''} · 拖动调整位置`}
        ><span class="cp-activity-dot"></span><b>{activityLabel}</b></button
      >
      <button
        class="cp-compact-info"
        onclick={() => void windowAction(unreadKeys.length ? 'news' : 'expand')}
        aria-label={importantUnread
          ? '查看新的重要重置消息'
          : unreadKeys.length
            ? '查看新消息'
            : '展开 CodexPulse 详情'}
      >
        {#each compactWindows as window}<span
            class:cp-stale={compactStale}
            title={`${windowLabel(window)}剩余额度${compactStale ? ' · 快照已过期' : ''}`}
            ><small
              >{window.duration_minutes && window.duration_minutes % 1440 === 0
                ? `${window.duration_minutes / 1440}d`
                : window.duration_minutes && window.duration_minutes % 60 === 0
                  ? `${window.duration_minutes / 60}h`
                  : window.duration_minutes === null
                    ? '额'
                    : `${window.duration_minutes}m`}</small
            ><b
              >{window.remaining_percent === null
                ? '—'
                : `${window.remaining_percent.toFixed(0)}%`}{compactStale ? '*' : ''}</b
            ></span
          >{:else}<small class="cp-compact-wait">{data.collecting ? '索引中' : '等待额度'}</small
          >{/each}
        {#if importantUnread}<span
            class="cp-reset-alert"
            title={`${importantUnread} 条新的重要重置消息 · 点击查看公告，实际额度以账户快照为准`}
            >重置<span class="cp-news-indicator"></span></span
          >{:else}<span
            class="cp-compact-tokens"
            title={`当前 session · ${current ? current.total.toLocaleString() + ' tokens' : '待采集'} · ${current?.models.join(' / ') || '模型未知'} · ${current ? money(current.cost_nanousd, current.unpriced_tokens) : '待计价'}`}
            ><b>{sessionTokens(current, compactNumber)}</b><small>tok</small></span
          >{#if unreadKeys.length}<span
              class="cp-news-indicator"
              title={`${unreadKeys.length} 条新动态`}
            ></span>{/if}{/if}
        <Icon name="right" />
      </button>
    </div>
  {:else}
    <header class="cp-header">
      <button class="cp-logo" onpointerdown={drag} aria-label="拖动 CodexPulse 窗口"
        ><Icon name="activity" /></button
      >
      <button class="cp-brand" onpointerdown={drag} aria-label="拖动 CodexPulse 窗口"
        ><strong>CodexPulse</strong><small
          >额度快照{data.quota?.buckets[0]?.plan ? ` · ${data.quota.buckets[0].plan}` : ''}</small
        ></button
      >
      {#if floatingSupported}<button
          class="cp-icon"
          aria-label="切换窗口置顶"
          aria-pressed={data.settings.always_on_top}
          onclick={() => void togglePin()}><Icon name="pin" /></button
        >{/if}
      <button class="cp-icon" aria-label="打开设置" onclick={() => void goPage('settings')}
        ><Icon name="settings" /></button
      ><button class="cp-icon" aria-label="收起详情" onclick={() => void windowAction('compact')}
        ><Icon name="up" /></button
      >
    </header>
    <button class="cp-source" onclick={() => void goPage('settings')}
      ><span
        ><span class="cp-dot"></span><span
          >{data.sources
            .filter((s) => s.status === 'connected')
            .map((s) => s.label)
            .join(' + ') || '等待连接本地来源'}</span
        ></span
      ><Icon name="right" /></button
    >
    <nav class="cp-tabs" aria-label="详情页面">
      {#each tabs.filter(([key]) => key !== 'settings') as [key, label]}<button
          aria-pressed={page === key}
          onclick={() => void goPage(key)}
          >{label}{#if key === 'news' && data.news?.items.length}<span class="cp-count"
              >{data.news.items.length}</span
            >{/if}</button
        >{/each}
    </nav>
    <div
      class="cp-body"
      bind:this={content}
      onscroll={() => {
        if (!restoreScroll && content) scroll[page] = content.scrollTop;
      }}
    >
      {#if data.error || error}<p class="cp-method" role="status">{error || data.error}</p>{/if}
      {#if page === 'overview'}
        {#each data.quota?.buckets ?? [] as bucket}{#if data.quota.buckets.length > 1}<div
              class="cp-sectionhead cp-bucket-label"
            >
              <span>{bucket.name} · {bucket.plan ?? '套餐未知'}</span><small
                >{bucket.source_id}</small
              >
            </div>{/if}<QuotaCard
            {bucket}
            {now}
            status={data.quota.sources[bucket.source_id]?.status ?? 'unknown'}
          />{:else}<p class="cp-note">
            尚未获得有效额度快照。请确认来源环境的 Codex 已登录。
          </p>{/each}
        <div class="cp-divider"></div>
        <div class="cp-sectionhead">
          <span
            ><span class="cp-dot"></span>
            {working ? '当前工作 session' : '最近工作 session'}</span
          ><span class="cp-label cp-truncate"
            >{sourceNames(current?.sources ?? [], data.sources)} · {current?.models.join(' / ') ||
              '模型未知'}</span
          >
        </div>
        <button
          class="cp-session-title"
          onclick={() => {
            selectedSession = current?.meta.id ?? null;
            sessionQuery = defaultSessionQuery();
            void goPage('sessions');
          }}
          ><span class="cp-truncate" title={sessionLabel(current?.meta, data.settings.hide_titles)}
            >{sessionLabel(current?.meta, data.settings.hide_titles)}</span
          ><Icon name="arrow" /></button
        >
        {#if current?.meta.project && !data.settings.hide_projects}<p
            class="cp-note cp-truncate"
            title={current.meta.project}
          >
            项目：{current.meta.project}
          </p>{/if}
        <div class="cp-metrics">
          <div>
            <span>本 session tokens</span><strong>{sessionTokens(current, number)}</strong>
          </div>
          <div>
            <span>美元估算</span><strong
              >{current?.events
                ? money(current.cost_nanousd, current.unpriced_tokens || current.unknown_totals)
                : '—'}</strong
            >
          </div>
          <div>
            <span>当前输出速度</span><strong title="日志不提供连续输出 token 样本"
              >— <small>tok/s</small></strong
            >
          </div>
        </div>
        <div class="cp-summary">
          <div class="cp-sectionhead">
            <span>最近 30 天 · 所有模型</span><small>{dateRange}</small>
          </div>
          <div class="cp-total">
            <strong
              >{magnitude(data.usage.total).value}<span
                >{magnitude(data.usage.total).unit} tokens</span
              ></strong
            >
            <div>
              <span>API 等价估算</span><b
                >{money(data.usage.cost_nanousd, data.usage.unpriced_tokens)}</b
              >
            </div>
          </div>
          <div class="cp-chart" role="img" aria-label="最近三十个自然日 token 用量">
            {#each data.usage.days as day}<span
                style:height={`${day.total ? Math.max(2, (day.total / highest) * 100) : 0}%`}
                title={`${day.day}: ${day.total.toLocaleString()} tokens`}
              ></span>{/each}
          </div>
          <div class="cp-chartaxis">
            <span>{shortDate(data.usage.from_day)}</span><span>每日 tokens</span><span
              >{shortDate(data.usage.through_day)}</span
            >
          </div>
          <button class="cp-textbutton" onclick={() => void goPage('models')}
            >{data.usage.sessions} 个 sessions · {data.usage.models.length} 个模型
            <span>查看分类 <Icon name="right" /></span></button
          >
        </div>
        <ResetStatus news={data.news} {now} />
        {#if data.news?.items.length || data.news?.challenge}<button
            class="cp-notice"
            onclick={() => void goPage('news')}
            ><Icon name="radio" /><span
              >Tibo 的 28 天挑战与消息<small>产品改进、额度重置与每日记录</small></span
            ><Icon name="right" /></button
          >{/if}
      {:else if page === 'models'}
        <div class="cp-sectionhead">
          <span>最近 30 天 · 所有模型</span><small>{dateRange}</small>
        </div>
        <div class="cp-total">
          <strong
            >{magnitude(data.usage.total).value}<span
              >{magnitude(data.usage.total).unit} tokens</span
            ></strong
          >
          <div>
            <span>美元估算</span><b>{money(data.usage.cost_nanousd, data.usage.unpriced_tokens)}</b>
          </div>
        </div>
        <div class="cp-metrics cp-breakdown">
          <div><span>输入</span><strong>{number(data.usage.input)}</strong></div>
          <div><span>其中缓存</span><strong>{number(data.usage.cached)}</strong></div>
          <div><span>输出</span><strong>{number(data.usage.output)}</strong></div>
        </div>
        <div class="cp-sectionhead cp-modelheading">
          <span>按模型分类</span><small>Tokens / 估算 USD</small>
        </div>
        {#each data.usage.models as m}<button
            class="cp-modelrow"
            aria-expanded={selectedModel === m.model}
            onclick={() => (selectedModel = selectedModel === m.model ? null : m.model)}
            ><span><strong>{m.model}</strong><small>{m.sessions} 个 sessions</small></span><span
              ><span
                ><b>{number(m.total)}</b><small>{money(m.cost_nanousd, m.unpriced_tokens)}</small
                ></span
              ><Icon name={selectedModel === m.model ? 'up' : 'right'} /></span
            ></button
          >{#if selectedModel === m.model}<div class="cp-model-detail">
              <div class="cp-metrics">
                <div><span>输入</span><strong>{number(m.input)}</strong></div>
                <div><span>其中缓存</span><strong>{number(m.cached)}</strong></div>
                <div><span>输出</span><strong>{number(m.output)}</strong></div>
              </div>
              <p class="cp-note">
                {number(m.unpriced_tokens)} tokens 未计价{m.incomplete_events
                  ? ` · ${m.incomplete_events} 条记录缺少拆分字段`
                  : ''}
              </p>
              <button
                class="cp-textbutton"
                onclick={() => {
                  sessionQuery = {
                    filter: {
                      model: m.model,
                      from_day: data.usage.from_day,
                      through_day: data.usage.through_day,
                    },
                    cursor: null,
                    direction: 'older',
                  };
                  selectedSession = null;
                  scroll.sessions = 0;
                  void goPage('sessions');
                }}>查看此模型最近 30 天的 sessions <Icon name="right" /></button
              >
            </div>{/if}{:else}<p class="cp-note">尚无可归属的模型用量。</p>{/each}
        <p class="cp-note">
          {data.usage.sessions} 个独立 sessions；一个 session 可使用多个模型。缓存属于输入，汇总不重复计算。
        </p>
      {:else if page === 'sessions'}
        <SessionsPane
          bind:query={sessionQuery}
          bind:selected={selectedSession}
          revision={data.updated_at}
          hideTitles={data.settings.hide_titles}
          hideProjects={data.settings.hide_projects}
          sources={data.sources}
        />
      {:else if page === 'news'}
        <div class="cp-sectionhead">
          <span>消息与额度恢复</span><small
            >{data.news?.status === 'connected' ? '已同步' : '等待同步'}</small
          >
        </div>
        <ResetStatus news={data.news} {now} />
        <ChallengePanel news={data.news} {now} />
        <div class="cp-sectionhead"><span>重置公告与历史</span><small>公开消息</small></div>
        {#if unreadKeys.length}<div class="cp-sectionhead">
            <span
              >{importantUnread
                ? `${importantUnread} 条重要重置消息`
                : `${unreadKeys.length} 条新动态`}</span
            ><button class="cp-textbutton" onclick={() => void acknowledgeNews()}
              >全部标为已读</button
            >
          </div>{/if}
        {#each data.news?.items ?? [] as item (`${item.kind}:${item.id}`)}<NewsCard
            {item}
            {now}
            unread={unreadKeys.includes(
              `${item.kind[0].toUpperCase()}${item.kind.slice(1)}:${item.id}`,
            )}
          />{:else}<p class="cp-note">
            暂时没有消息。公共接口每 5 分钟同步，遇到限流等待服务器指定时间。
          </p>{/each}
        <p class="cp-note">消息出现不代表当前账户已恢复；账户恢复需要额度快照确认。</p>
        <button
          class="cp-textbutton"
          onclick={() =>
            void openSource('https://codex-resets.com').catch((e) => (error = String(e)))}
          >数据来自 Codex Resets <Icon name="external" /></button
        >
        <p class="cp-note">最近成功同步：{time(data.news?.last_success ?? null)}</p>
      {:else if page === 'settings'}
        <div class="cp-sectionhead"><span>显示与数据源</span><small>本地设置</small></div>
        <section oninput={() => (settingsDirty = true)} onchange={() => (settingsDirty = true)}>
          <label class="cp-setting"
            >外观<select bind:value={settings.theme}
              ><option value="system">跟随系统</option><option value="light">浅色</option><option
                value="dark">深色</option
              ></select
            ></label
          ><label class="cp-setting"
            ><span>毛玻璃效果<small class="cp-setting-hint">关闭后使用不透明背景</small></span
            ><input type="checkbox" role="switch" bind:checked={settings.glass} /></label
          >
          <label class="cp-setting"
            ><span
              >隐藏会话标题<small class="cp-setting-hint">详情使用短 ID，悬停提示也隐藏名称</small
              ></span
            ><input type="checkbox" role="switch" bind:checked={settings.hide_titles} /></label
          >
          <label class="cp-setting"
            ><span>隐藏项目名称</span><input
              type="checkbox"
              role="switch"
              bind:checked={settings.hide_projects}
            /></label
          >
          {#if floatingSupported}<label class="cp-setting"
              ><span
                >显示桌面浮窗<small class="cp-setting-hint">关闭后通过系统托盘查看详情</small></span
              ><input type="checkbox" role="switch" bind:checked={settings.floating} /></label
            ><label class="cp-setting"
              ><span>浮窗始终置顶</span><input
                type="checkbox"
                role="switch"
                bind:checked={settings.always_on_top}
              /></label
            >{/if}
          <div class="cp-divider"></div>
          <div class="cp-sectionhead"><span>采集来源</span><small>本机 · 只读</small></div>
          <label class="cp-setting"
            ><span>Windows Codex App / CLI</span><input
              type="checkbox"
              role="switch"
              bind:checked={settings.windows_enabled}
            /></label
          ><label class="cp-setting"
            ><span>已运行的 WSL<small class="cp-setting-hint">不主动启动已停止的发行版</small></span
            ><input type="checkbox" role="switch" bind:checked={settings.wsl_enabled} /></label
          ><label class="cp-path"
            >Windows Codex home<input
              bind:value={path}
              placeholder="自动探测 CODEX_HOME 或 .codex"
            /></label
          >
          <SourceSettings bind:settings changed={() => (settingsDirty = true)} />
          <p class="cp-note">统计时区：{settings.timezone} · 不保存对话正文，不上传本地用量。</p>
          <div class="cp-divider"></div>
          <fieldset class="accent-picker">
            <legend>主题颜色</legend>
            <div class="accent-options">
              {#each accents as [key, label, dark, light]}<label
                  class:chosen={settings.accent === key}
                  style={`--swatch-dark:${dark};--swatch-light:${light}`}
                  ><input
                    type="radio"
                    name="accent"
                    value={key}
                    bind:group={settings.accent}
                    aria-label={`${label}${key === 'blue' ? '（默认）' : ''}`}
                  /><span class="accent-swatch" aria-hidden="true"></span><span>{label}</span
                  ></label
                >{/each}
            </div>
          </fieldset>
        </section>
        <button class="cp-save" disabled={saving} onclick={() => void save()}
          >{saving ? '正在保存…' : '保存设置'}</button
        >{#if settingsDirty}<button
            class="cp-textbutton"
            onclick={() => {
              settings = copySettings(data.settings);
              path = data.settings.windows_home ?? '';
              settingsDirty = false;
            }}>撤销未保存的修改</button
          >{/if}
        <div class="cp-divider"></div>
        <div class="cp-sectionhead"><span>来源状态</span><small>只读采集</small></div>
        {#each data.sources as source}<div class="cp-source-status">
            <div class="cp-sectionhead">
              <span>{source.label}</span><small
                >{source.status === 'connected'
                  ? '已连接'
                  : source.status === 'no_logs'
                    ? '暂无日志'
                    : source.status === 'read_error'
                      ? '读取失败'
                      : source.status === 'wsl_stopped'
                        ? '发行版已停止'
                        : source.status === 'wsl_unavailable'
                          ? '无法确认运行状态'
                          : '探测中'}</small
              >
            </div>
            <p class="cp-note">
              {source.path}<br />{source.files} 个日志 · {source.issues} 个解析问题 · {time(
                source.last_read,
              )}
            </p>
          </div>{:else}<p class="cp-note">等待来源探测</p>{/each}
      {/if}
    </div>
    <footer class="cp-footer">
      <span
        >{data.collecting ? '正在建立本地索引…' : `本地用量 · ${time(data.updated_at)} 更新`}</span
      >{#if data.usage.cost_nanousd > 0 && data.usage.unpriced_tokens > 0}<span
          class="cp-price-note"
          title={`另有 ${data.usage.unpriced_tokens.toLocaleString()} tokens 未计价`}
          >* 已计价部分</span
        >{/if}<button class="cp-textbutton" onclick={() => void windowAction('compact')}
        >收起详情 <Icon name="up" /></button
      >
    </footer>
  {/if}
</main>
