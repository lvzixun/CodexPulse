<script lang="ts">
  import { translator as t, locale, localizeError } from './lib/i18n';

  import { onMount, tick } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { Menu, MenuItem } from '@tauri-apps/api/menu';
  import { LogicalPosition } from '@tauri-apps/api/dpi';
  import {
    empty,
    native,
    onEvent,
    saveSettings,
    saveUiPreferences,
    snapshot,
    windowAction,
    getViewState,
    rememberView,
    defaultSessionQuery,
    readNews,
    checkAppUpdates,
    getAppUpdateInfo,
    openAppRelease,
    installAppUpdate,
  } from './lib/ipc';
  import type {
    Settings,
    SessionPageRequest,
    ModelPageRequest,
    Snapshot,
    AppUpdateInfo,
  } from './lib/types';
  import { version as appVersion } from '../package.json';
  import AppUpdates from './components/AppUpdates.svelte';
  import QuotaCard from './components/QuotaCard.svelte';
  import NewsPane from './components/NewsPane.svelte';
  import Icon from './components/Icon.svelte';
  import ResetStatus from './components/ResetStatus.svelte';
  import AccountSummary from './components/AccountSummary.svelte';
  import ModelsPane from './components/ModelsPane.svelte';
  import SessionsPane from './components/SessionsPane.svelte';
  import SourceSettings from './components/SourceSettings.svelte';
  import RefreshSettings from './components/RefreshSettings.svelte';
  import StartupSettings from './components/StartupSettings.svelte';
  import DiagnosticsPanel from './components/DiagnosticsPanel.svelte';
  import { subscriptionGroup } from './lib/subscriptions';
  import { refreshLanguage } from './lib/system-language';
  import {
    windowLabel,
    sessionTokens,
    sessionActivity,
    sessionLabel,
    sourceNames,
    formatUsd,
  } from './lib/format';
  import { openSource } from './lib/ipc';
  let data = $state(empty);
  let mode = $state<'compact' | 'details'>(native ? 'compact' : 'details');
  let page = $state('overview');
  let newsChallenge = $state(false);
  let newsLimit = $state(5);
  const scrollKey = () => (page === 'news' && newsChallenge ? 'challenge' : page);
  let selectedModel = $state<string | null>(null);
  let selectedSession = $state<string | null>(null);
  let modelQuery = $state<ModelPageRequest>({
    from_day: '',
    through_day: '',
    cursor: null,
    direction: 'next',
  });
  let sessionQuery = $state<SessionPageRequest>(defaultSessionQuery());
  let error = $state('');
  let saving = $state(false);
  let preferencesBusy = $state(false);
  let settingsDirty = $state(false);
  let now = $state(Date.now());
  const copySettings = (value: Settings): Settings => ({
    ...value,
    windows_sources: value.windows_sources.map((s) => ({ ...s })),
    wsl_sources: value.wsl_sources.map((s) => ({ ...s })),
    quota_refresh: { ...value.quota_refresh },
    news_refresh: { ...value.news_refresh },
  });
  let settings = $state<Settings>(copySettings(empty.settings));
  let path = $state('');
  let viewReady = $state(false);
  let windowVisible = $state(!native);
  let documentVisible = $state(!document.hidden);
  let newsReadPending = $state(false);
  let newsReadAttempt = '';
  let appUpdate = $state<AppUpdateInfo>({
    current_version: appVersion,
    latest_version: null,
    update_available: false,
    checked_at: null,
    next_check_at: 0,
    status: 'idle',
    downloaded_bytes: 0,
    total_bytes: null,
    revision: 0,
  });
  let updateBusy = $state(false);
  let dismissedUpdate = $state<string | null>(null);
  let compactMenu: Menu | null = null;
  let compactMenuItems: MenuItem[] = [];
  let compactMenuLocale = '';
  let compactMenuBusy = false;
  let compactMenuDisposed = false;
  let glassSupported = $state(!native);
  let floatingSupported = $state(true);
  let scroll = $state<Record<string, number>>({});
  let content: HTMLDivElement | undefined = $state();
  let restoreScroll = false;
  const tabs = $derived([
    ['overview', $t('总览')],
    ['models', $t('模型')],
    ['sessions', 'Sessions'],
    ['news', 'Tibo'],
    ['settings', $t('设置')],
  ]);
  const accents: [Settings['accent'], string, string, string][] = $derived([
    ['blue', $t('蓝色'), '#8bbbff', '#2261d6'],
    ['violet', $t('紫色'), '#b5b4ff', '#5148bf'],
    ['teal', $t('青绿'), '#6cdbc7', '#0b7564'],
    ['amber', $t('琥珀'), '#f2c675', '#915600'],
    ['rose', $t('玫红'), '#ffa7c4', '#aa3462'],
  ]);
  const number = (n: number) =>
    new Intl.NumberFormat('en-US', { notation: 'compact', maximumFractionDigits: 1 }).format(n);
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
    unpriced > 0 ? (cost > 0 ? `${formatUsd(cost)}*` : $t('待计价')) : formatUsd(cost);
  const time = (ts: string | null) =>
    ts
      ? new Intl.DateTimeFormat($locale === 'zh' ? 'zh-CN' : 'en-US', {
          month: $locale === 'zh' ? 'short' : 'numeric',
          day: 'numeric',
          hour: '2-digit',
          minute: '2-digit',
          hour12: false,
        }).format(new Date(ts))
      : $t('尚未同步');
  const activityInfo = $derived(sessionActivity(data, now));
  const referenceMoney = (value: Snapshot['usage']['reference']) =>
    value.pending ? $t('计算中…') : money(value.cost_nanousd, value.unpriced_events);
  const working = $derived(
    activityInfo.sessions.find((s) => s.meta.id === selectedSession) ?? activityInfo.working,
  );
  const activity = $derived(activityInfo.state);
  const activityLabel = $derived(
    activity === 'busy'
      ? $t('忙{_0}', { _0: activityInfo.sessions.length > 1 ? activityInfo.sessions.length : '' })
      : activity === 'idle'
        ? $t('闲')
        : '?',
  );
  const activityTitle = $derived(
    activity === 'busy'
      ? $t('{_0} 个 session 工作中（最近 5 分钟有日志事件）', { _0: activityInfo.sessions.length })
      : activity === 'idle'
        ? $t('未检测到工作中的 session')
        : $t('工作状态未知：日志过期或数据源未连接'),
  );
  const current = $derived(working ?? data.recent[0]);
  const outputRate = $derived(current?.meta.output_rate);
  const rateFresh = $derived(
    outputRate && (outputRate.completed || now - Date.parse(outputRate.measured_at) <= 60000),
  );
  const rateText = $derived(
    rateFresh && outputRate
      ? ((outputRate.output_tokens * 1000) / outputRate.elapsed_ms).toFixed(1)
      : '—',
  );
  const compactModel = $derived(working?.meta.current_model ?? null);
  const compactRate = $derived(working ? rateText : '—');
  const highest = $derived(Math.max(1, ...data.usage.days.map((d) => d.total)));
  const unreadKeys = $derived(data.news.unread_keys ?? []);
  const accountBucket = $derived(
    data.quota.buckets.find((bucket) => bucket.limit_id === 'codex' && bucket.identity_confirmed),
  );
  const accountSource = $derived(
    accountBucket
      ? data.quota.sources[accountBucket.source_id]
      : Object.values(data.quota.sources).find((source) => source.identity && source.profile),
  );
  const accountName = $derived(
    accountSource?.profile?.display_name ||
      (accountSource?.profile?.username
        ? `@${accountSource.profile.username.replace(/^@/, '')}`
        : accountSource?.identity
          ? $t('账户资料同步中')
          : $t('账户待连接')),
  );
  const accountPlan = $derived(
    accountBucket?.plan
      ? ((
          {
            free: 'Free',
            plus: 'Plus',
            pro: 'Pro',
            team: 'Team',
            business: 'Business',
            enterprise: 'Enterprise',
            edu: 'Edu',
          } as Record<string, string>
        )[accountBucket.plan.toLowerCase()] ?? accountBucket.plan)
      : null,
  );
  const importantUnread = $derived(data.news.important_unread ?? 0);
  async function acknowledgeNews(keys: string[]) {
    try {
      await readNews(keys);
      await refresh();
    } catch (e) {
      error = String(e);
    }
  }
  async function loadCachedAppUpdate() {
    if (!native) return;
    try {
      const next = await getAppUpdateInfo();
      if (next.revision >= appUpdate.revision) appUpdate = next;
    } catch {
      // Reading process-local state must not overwrite a completed network check.
    }
  }
  async function loadAppUpdate() {
    if (!native || updateBusy) return;
    updateBusy = true;
    try {
      const next = await checkAppUpdates();
      if (next.revision >= appUpdate.revision) appUpdate = next;
    } catch {
      appUpdate = { ...appUpdate, status: 'network_error', next_check_at: Date.now() / 1000 + 900 };
    } finally {
      updateBusy = false;
    }
  }
  function openUpgrade() {
    void openAppRelease().catch((e) => (error = String(e)));
  }
  async function applyUpgrade() {
    if (updateBusy || appUpdate.status !== 'ready') return;
    updateBusy = true;
    try {
      await installAppUpdate();
    } catch (e) {
      error = String(e);
    } finally {
      updateBusy = false;
    }
  }
  $effect(() => {
    if (!viewReady || mode !== 'details' || page !== 'news' || !windowVisible || !documentVisible) {
      newsReadAttempt = '';
      return;
    }
    const keys = [...unreadKeys].slice(0, 256);
    const revision = JSON.stringify([data.news.last_success, data.news.last_attempt, keys]);
    if (!keys.length || newsReadPending || newsReadAttempt === revision) return;
    // Wait until this snapshot is rendered. Only acknowledge its keys, so messages
    // arriving while the write is pending remain eligible for the next pass.
    const timer = setTimeout(() => {
      newsReadAttempt = revision;
      newsReadPending = true;
      void acknowledgeNews(keys).finally(() => (newsReadPending = false));
    }, 0);
    return () => clearTimeout(timer);
  });
  async function togglePin() {
    try {
      await saveSettings({ ...data.settings, always_on_top: !data.settings.always_on_top });
      await refresh();
    } catch (e) {
      error = String(e);
    }
  }
  async function goPage(next: string) {
    if (next === page) return;
    if (content) scroll[scrollKey()] = content.scrollTop;
    page = next;
    await restorePageScroll();
  }
  async function goNewsDetail(detail: boolean) {
    if (content) scroll[scrollKey()] = content.scrollTop;
    newsChallenge = detail;
    await restorePageScroll();
  }
  async function restorePageScroll(modelsReady = false) {
    restoreScroll = true;
    await tick();
    // A paged model list has no height until its asynchronous read completes.
    if (mode === 'details' && page === 'models' && !modelsReady) return;
    if (content) content.scrollTop = scroll[scrollKey()] ?? 0;
    await tick();
    restoreScroll = false;
  }
  $effect(() => {
    const view = {
      mode,
      page,
      news_challenge: newsChallenge,
      news_limit: newsLimit,
      selected_model: selectedModel,
      selected_session: selectedSession,
      session_query: sessionQuery,
      model_query: modelQuery,
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
      const next = await snapshot();
      if (data.settings.timezone !== next.settings.timezone) {
        sessionQuery = {
          ...sessionQuery,
          cursor: null,
          direction: 'older',
          filter: sessionQuery.filter.model
            ? {
                ...sessionQuery.filter,
                from_day: next.usage.from_day,
                through_day: next.usage.through_day,
              }
            : { ...sessionQuery.filter },
        };
      }
      if (
        modelQuery.from_day !== next.usage.from_day ||
        modelQuery.through_day !== next.usage.through_day ||
        data.settings.timezone !== next.settings.timezone
      ) {
        modelQuery = {
          from_day: next.usage.from_day,
          through_day: next.usage.through_day,
          cursor: null,
          direction: 'next',
        };
      }
      data = next;
      now = Date.now();
      if (!saving && !settingsDirty) {
        settings = copySettings(data.settings);
        path = settings.windows_home ?? '';
      }
    } catch {
      error = '无法连接后台采集器';
    }
  }
  async function savePreference(preferences: Partial<Pick<Settings, 'theme' | 'language'>>) {
    preferencesBusy = true;
    error = '';
    try {
      const next = await saveUiPreferences(preferences);
      data.settings = next;
      settings.theme = next.theme;
      settings.language = next.language;
      await refreshLanguage();
    } catch (e) {
      settings.theme = data.settings.theme;
      settings.language = data.settings.language;
      error = String(e);
    } finally {
      preferencesBusy = false;
    }
  }
  async function save() {
    saving = true;
    error = '';
    try {
      await saveSettings({
        ...settings,
        windows_home: path.trim() || null,
        quota_refresh: data.settings.quota_refresh,
        news_refresh: data.settings.news_refresh,
        timezone: data.settings.timezone,
      });
      settingsDirty = false;
      saving = false;
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }
  let compactGesture: { pointer: number; x: number; y: number; target: Element } | null = null;
  let compactDragged = false;
  function startWindowDrag() {
    void getCurrentWindow()
      .startDragging()
      .catch((e) => (error = String(e)));
  }
  function drag(e: PointerEvent) {
    if (!native || e.button !== 0 || !e.isPrimary || !(e.target instanceof Element)) return;
    compactGesture = null;
    compactDragged = false;
    if (mode === 'compact' && e.target.closest('.cp-compact-line')) {
      const target = e.target.closest('button') ?? e.target;
      compactGesture = { pointer: e.pointerId, x: e.clientX, y: e.clientY, target };
      // Keep receiving movement even when the pointer leaves the tiny window.
      try {
        target.setPointerCapture(e.pointerId);
      } catch {}
    } else if (e.target.closest('.cp-header')) {
      const button = e.target.closest('button');
      if (button && !button.matches('.cp-logo, .cp-brand')) return;
      e.preventDefault();
      startWindowDrag();
    }
  }
  function moveCompact(e: PointerEvent) {
    const gesture = compactGesture;
    if (!gesture || e.pointerId !== gesture.pointer) return;
    if (!(e.buttons & 1)) {
      compactGesture = null;
      return;
    }
    if (Math.hypot(e.clientX - gesture.x, e.clientY - gesture.y) < 4) return;
    compactGesture = null;
    compactDragged = true;
    try {
      gesture.target.releasePointerCapture(e.pointerId);
    } catch {}
    startWindowDrag();
  }
  function openCompact(e: MouseEvent) {
    // Native dragging can deliver a final click; it must not expand the window.
    // A fresh pointer-down clears this flag, so the next deliberate click works.
    if (compactDragged && e.detail !== 0) {
      e.preventDefault();
      return;
    }
    void windowAction(unreadKeys.length ? 'news' : 'expand');
  }
  async function closeCompactMenu() {
    const resources = [...compactMenuItems, ...(compactMenu ? [compactMenu] : [])];
    compactMenuItems = [];
    compactMenu = null;
    await Promise.all(resources.map((resource) => resource.close().catch(() => {})));
  }
  async function compactContextMenu(event: MouseEvent) {
    if (mode !== 'compact' || !floatingSupported) return;
    event.preventDefault();
    if (!native || compactMenuBusy || compactMenuDisposed) return;
    compactMenuBusy = true;
    try {
      if (!compactMenu || compactMenuLocale !== $locale) {
        await closeCompactMenu();
        for (const [action, label] of [
          ['expand', '展开'],
          ['exit', '退出'],
          ['hide', '隐藏'],
        ]) {
          compactMenuItems.push(
            await MenuItem.new({
              id: `compact-${action}`,
              text: $t(label),
              action: () => void windowAction(action).catch((e) => (error = String(e))),
            }),
          );
        }
        compactMenu = await Menu.new({ items: compactMenuItems });
        compactMenuLocale = $locale;
      }
      if (!compactMenuDisposed && mode === 'compact' && windowVisible)
        await compactMenu.popup(new LogicalPosition(event.clientX, event.clientY));
    } catch (e) {
      error = String(e);
      await closeCompactMenu();
    } finally {
      compactMenuBusy = false;
      if (compactMenuDisposed) await closeCompactMenu();
    }
  }
  onMount(() => {
    let disposed = false;
    let clock: ReturnType<typeof setInterval> | undefined;
    let languageClock: ReturnType<typeof setInterval> | undefined;
    const clockVisible = (visible: boolean) => {
      if (disposed) return;
      clearInterval(clock);
      clearInterval(languageClock);
      if (visible) {
        now = Date.now();
        void refreshLanguage();
        clock = setInterval(() => {
          now = Date.now();
        }, 1000);
        languageClock = setInterval(() => void refreshLanguage(), 60000);
      }
    };
    clockVisible(windowVisible && documentVisible);
    const subscriptions = subscriptionGroup([
      () =>
        onEvent<AppUpdateInfo>('app-update', (next) => {
          if (!disposed && next.revision >= appUpdate.revision) appUpdate = next;
        }),
      () =>
        onEvent('snapshot-changed', () => {
          if (!disposed && !document.hidden) void refresh();
        }),
      () =>
        onEvent<[string, string | null]>('window-mode', ([next, target]) => {
          if (disposed) return;
          if (content) scroll[scrollKey()] = content.scrollTop;
          const changed = mode !== next;
          mode = next as typeof mode;
          if (target && target !== page) void goPage(target);
          else if (changed) void restorePageScroll();
          void refresh();
        }),
      () =>
        onEvent<Settings>('settings-applied', (next) => {
          if (disposed) return;
          data.settings = next;
          settings.theme = next.theme;
          settings.language = next.language;
          void refreshLanguage();
          if (!settingsDirty || saving) {
            settings = { ...next };
            path = next.windows_home ?? '';
          }
          void refresh();
        }),
      () =>
        onEvent<boolean>('window-visible', (visible) => {
          windowVisible = visible;
          clockVisible(visible && documentVisible);
          if (visible && !disposed) void loadCachedAppUpdate();
        }),
      () =>
        onEvent<boolean>('glass-supported', (supported) => {
          if (!disposed) glassSupported = supported;
        }),
    ]);
    void subscriptions.ready
      .catch(() => {
        if (!disposed) error = '无法订阅自动更新，请重新打开面板';
      })
      .then(async () => {
        if (!disposed) {
          void loadCachedAppUpdate();
          windowVisible = native ? await getCurrentWindow().isVisible() : true;
          clockVisible(windowVisible && documentVisible);
          await refresh();
          if (disposed) return;
          const view = await getViewState();
          if (disposed) return;
          if (view) {
            mode = view.mode;
            page = tabs.some(([key]) => key === view.page) ? view.page : 'overview';
            newsChallenge = view.news_challenge ?? false;
            newsLimit = view.news_limit ?? 5;
            selectedModel = view.selected_model;
            selectedSession = view.selected_session;
            sessionQuery = view.session_query ?? defaultSessionQuery();
            if (
              view.model_query?.from_day === data.usage.from_day &&
              view.model_query?.through_day === data.usage.through_day &&
              (!view.model_query.cursor ||
                view.model_query.cursor.timezone === data.settings.timezone)
            )
              modelQuery = view.model_query;
            scroll = view.scroll;
            glassSupported = view.glass_supported;
            floatingSupported = view.floating_supported;
          }
          viewReady = true;
          await restorePageScroll();
        }
      })
      .catch(() => {
        if (!disposed) error = '无法加载界面，请重新打开面板';
      });
    const visible = () => {
      documentVisible = !document.hidden;
      clockVisible(windowVisible && documentVisible);
      if (!document.hidden) void refresh();
    };
    document.addEventListener('visibilitychange', visible);
    const escape = (e: KeyboardEvent) => {
      if (e.key === 'Escape') void windowAction('compact');
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'q') void windowAction('exit');
    };
    window.addEventListener('keydown', escape);
    window.addEventListener('languagechange', refreshLanguage);
    return () => {
      compactMenuDisposed = true;
      if (!compactMenuBusy) void closeCompactMenu();
      clearInterval(clock);
      clearInterval(languageClock);
      disposed = true;
      subscriptions.dispose();
      document.removeEventListener('visibilitychange', visible);
      window.removeEventListener('keydown', escape);
      window.removeEventListener('languagechange', refreshLanguage);
    };
  });
</script>

<svelte:window
  oncontextmenu={compactContextMenu}
  onpointerdown={drag}
  onpointermove={moveCompact}
  onpointerup={() => (compactGesture = null)}
  onpointercancel={() => (compactGesture = null)}
  onblur={() => (compactGesture = null)}
/>

<main
  class="cp-shell"
  class:native-shell={native}
  class:compact={mode === 'compact'}
  class:menubar={!floatingSupported}
  class:opaque={!settings.glass || !glassSupported}
  data-theme={data.settings.theme}
  data-accent={settings.accent ?? 'blue'}
>
  {#if mode === 'compact'}
    <div class="cp-compact-line">
      <button
        class="cp-compact-drag"
        class:cp-working={activity === 'busy'}
        class:cp-unknown={activity === 'unknown'}
        class:cp-indexing={data.collecting}
        aria-label={$t('{_0} · 拖动 CodexPulse 浮窗', { _0: activityTitle })}
        title={$t('CodexPulse · {_0}{_1} · 拖动调整位置', {
          _0: activityTitle,
          _1: data.collecting ? $t(' · 索引中') : '',
        })}><span class="cp-activity-dot"></span><b>{activityLabel}</b></button
      >
      <button
        class="cp-compact-info"
        onclick={openCompact}
        aria-label={importantUnread
          ? $t('查看新的重要重置消息')
          : unreadKeys.length
            ? $t('查看新消息')
            : $t('展开 CodexPulse 详情')}
      >
        <span class="cp-compact-model" title={compactModel ?? $t('模型未知')}>
          <b>{compactModel ? compactModel.replace(/^gpt-/, '') : '—'}</b>
        </span>
        <span class="cp-compact-rate" title={$t('本轮平均输出速度 · 含推理、工具和等待')}>
          <b>{compactRate}</b><small>t/s</small>
        </span>
        {#if importantUnread}
          <span
            class="cp-reset-alert"
            title={$t('{_0} 条新的重要重置消息 · 点击查看公告，实际额度以账户快照为准', {
              _0: importantUnread,
            })}
          >
            {$t('重置')}
          </span>
        {:else if unreadKeys.length}
          <span class="cp-news-indicator" title={$t('{_0} 条新动态', { _0: unreadKeys.length })}
          ></span>
        {/if}
        <Icon name="right" />
      </button>
    </div>
  {:else}
    <header class="cp-header">
      <button class="cp-logo" aria-label={$t('拖动 CodexPulse 窗口')}
        ><Icon name="activity" /></button
      >
      <button class="cp-brand" aria-label={$t('拖动 CodexPulse 窗口')}
        ><strong>CodexPulse</strong><small
          >{$t('额度快照')}{data.quota?.buckets[0]?.plan
            ? ` · ${data.quota.buckets[0].plan}`
            : ''}</small
        ></button
      >
      {#if floatingSupported}<button
          class="cp-icon"
          aria-label={$t('切换窗口置顶')}
          aria-pressed={data.settings.always_on_top}
          onclick={() => void togglePin()}><Icon name="pin" /></button
        >{/if}
      <button class="cp-icon" aria-label={$t('打开设置')} onclick={() => void goPage('settings')}
        ><Icon name="settings" /></button
      ><button
        class="cp-icon"
        aria-label={$t('收起详情')}
        onclick={() => void windowAction('compact')}><Icon name="up" /></button
      >
      <button
        class="cp-icon cp-exit"
        aria-label={$t('退出 CodexPulse')}
        title={$t('退出 CodexPulse')}
        onclick={() => void windowAction('exit')}>{$t('退出')}</button
      >
    </header>
    <button
      class="cp-source"
      aria-label={$t('账户与数据源设置')}
      onclick={() => void goPage('settings')}
      ><span
        ><span class="cp-dot"></span><span class="cp-truncate"
          >{accountName}{accountPlan ? ` · ${accountPlan}` : ''}</span
        ></span
      ><Icon name="right" /></button
    >
    <nav class="cp-tabs" aria-label={$t('详情页面')}>
      {#each tabs.filter(([key]) => key !== 'settings') as [key, label]}<button
          aria-pressed={page === key}
          onclick={() => void goPage(key)}
          >{label}{#if key === 'news' && unreadKeys.length}<span
              class="cp-count"
              aria-label={$t('{_0} 条未读新消息', { _0: unreadKeys.length })}
              >{unreadKeys.length}</span
            >{/if}</button
        >{/each}
    </nav>
    {#if appUpdate.update_available && dismissedUpdate !== appUpdate.latest_version}
      <AppUpdates
        info={appUpdate}
        busy={updateBusy}
        check={() => void loadAppUpdate()}
        open={openUpgrade}
        install={() => void applyUpgrade()}
        banner
        dismiss={() => (dismissedUpdate = appUpdate.latest_version)}
      />
    {/if}
    <div
      class="cp-body"
      bind:this={content}
      onscroll={() => {
        if (!restoreScroll && content) scroll[scrollKey()] = content.scrollTop;
      }}
    >
      {#if data.error || error || data.timezone_error}<p class="cp-method" role="status">
          {localizeError(error || data.error || data.timezone_error, $locale)}
        </p>{/if}
      {#if page === 'overview'}
        {#each data.quota?.buckets ?? [] as bucket}{#if data.quota.buckets.length > 1}<div
              class="cp-sectionhead cp-bucket-label"
            >
              <span>{bucket.name} · {bucket.plan ?? $t('套餐未知')}</span><small
                >{bucket.source_id}</small
              >
            </div>{/if}<QuotaCard
            {bucket}
            {now}
            allowance={bucket.limit_id === 'codex'
              ? data.quota.sources[bucket.source_id]?.allowance
              : undefined}
            status={data.quota.sources[bucket.source_id]?.status ?? 'unknown'}
            requestStatus={data.quota.request_status}
            maxAgeSeconds={data.settings.quota_refresh.interval_seconds + 60}
          />{:else}<p class="cp-note">
            {data.settings.quota_refresh.mode === 'manual'
              ? $t('暂无已验证额度缓存。手动模式可在设置中点击立即刷新。')
              : $t('尚未获得有效额度快照，可在设置中查看来源状态。')}
          </p>{/each}
        <ResetStatus news={data.news} {now} timezone={data.settings.timezone} />
        <div class="cp-divider"></div>
        <section class="cp-session-overview" aria-label={working ? $t('当前会话') : $t('最近会话')}>
          <div class="cp-sectionhead">
            <span>{working ? $t('当前会话') : $t('最近会话')}</span>
            <small class:cp-session-active={!!working}>
              {#if working}<span class="cp-dot"></span>{/if}{working
                ? $t('工作中')
                : $t('最近记录')}
            </small>
          </div>
          <button
            class="cp-session-title"
            onclick={() => {
              selectedSession = current?.meta.id ?? null;
              sessionQuery = defaultSessionQuery();
              void goPage('sessions');
            }}
            ><span
              class="cp-truncate"
              title={sessionLabel(current?.meta, data.settings.hide_titles, $locale)}
              >{sessionLabel(current?.meta, data.settings.hide_titles, $locale)}</span
            ><Icon name="arrow" /></button
          >
          <div class="cp-session-meta">
            <span class="cp-truncate"
              >{sourceNames(current?.sources ?? [], data.sources)} · {current?.models.join(' / ') ||
                $t('模型未知')}</span
            >
            {#if current?.meta.project && !data.settings.hide_projects}<span
                class="cp-truncate"
                title={current.meta.project}>{$t('项目：')}{current.meta.project}</span
              >{/if}
          </div>
          <div class="cp-metrics">
            <div>
              <span>Tokens</span><strong>{sessionTokens(current, number)}</strong>
            </div>
            <div>
              <span>{$t('API 等价估算')}</span><strong
                >{current?.events ? referenceMoney(current.reference) : '—'}</strong
              >
            </div>
            <div>
              <span>{outputRate?.completed ? $t('上轮平均速度') : $t('本轮平均速度')}</span><strong
                title={$t('本轮输出 tokens ÷ 轮次耗时（含推理、工具执行和等待）；有日志样本时刷新')}
                >{rateText} <small>tok/s</small></strong
              >
            </div>
          </div>
        </section>
        <section class="cp-summary" aria-label={$t('用量统计：最近 30 天与账户累计')}>
          <div class="cp-sectionhead">
            <span>{$t('最近 30 天累计')}</span><small>{dateRange}</small>
          </div>
          <div class="cp-total">
            <strong
              >{magnitude(data.usage.total).value}<span
                >{magnitude(data.usage.total).unit} tokens</span
              ></strong
            >
            <div>
              <span>{$t('当前 API 等价估算')}</span><b>{referenceMoney(data.usage.reference)}</b>
            </div>
          </div>
          <p class="cp-note cp-pricing-coverage">
            {data.usage.reference.price_date}
            {$t('价格 · 区分缓存 / Fast / 长上下文')}
          </p>
          {#if data.usage.reference.unpriced_tokens > 0 && !data.usage.reference.pending}
            <p class="cp-note cp-pricing-coverage">
              {$t('覆盖')}
              {number(Math.max(0, data.usage.total - data.usage.reference.unpriced_tokens))} tokens （{data
                .usage.total
                ? (
                    (Math.max(0, data.usage.total - data.usage.reference.unpriced_tokens) /
                      data.usage.total) *
                    100
                  ).toFixed(1)
                : '0.0'}%） · {number(data.usage.reference.unpriced_tokens)}
              {$t('信息不足')}
            </p>
          {/if}
          <div class="cp-chart" role="img" aria-label={$t('最近三十个自然日 token 用量')}>
            {#each data.usage.days as day}<span
                style:height={`${day.total ? Math.max(2, (day.total / highest) * 100) : 0}%`}
                title={`${day.day}: ${day.total.toLocaleString()} tokens`}
              ></span>{/each}
          </div>
          <div class="cp-chartaxis">
            <span>{shortDate(data.usage.from_day)}</span><span>{$t('每日 tokens')}</span><span
              >{shortDate(data.usage.through_day)}</span
            >
          </div>
          <button class="cp-textbutton" onclick={() => void goPage('models')}
            >{data.usage.sessions}
            {$t('个 sessions ·')}
            {data.usage.model_count}
            {$t('个模型')}
            <span>{$t('查看分类')} <Icon name="right" /></span></button
          >
          <AccountSummary {data} />
        </section>
      {:else if page === 'models'}
        <div class="cp-sectionhead">
          <span>{$t('最近 30 天 · 所有模型')}</span><small>{dateRange}</small>
        </div>
        <div class="cp-total">
          <strong
            >{magnitude(data.usage.total).value}<span
              >{magnitude(data.usage.total).unit} tokens</span
            ></strong
          >
          <div>
            <span>{$t('当前 API 等价估算')}</span><b>{referenceMoney(data.usage.reference)}</b>
          </div>
        </div>
        <div class="cp-metrics cp-breakdown">
          <div><span>{$t('输入')}</span><strong>{number(data.usage.input)}</strong></div>
          <div><span>{$t('其中缓存')}</span><strong>{number(data.usage.cached)}</strong></div>
          <div><span>{$t('输出')}</span><strong>{number(data.usage.output)}</strong></div>
        </div>
        <div class="cp-sectionhead cp-modelheading">
          <span>{$t('按模型分类')}</span><small>{$t('Tokens 占比 / 估算 USD')}</small>
        </div>
        <ModelsPane
          bind:query={modelQuery}
          bind:selected={selectedModel}
          revision={`${data.usage.fact_revision}:${data.usage.sessions}:${data.usage.reference.cost_nanousd}:${data.usage.reference.pending}`}
          ready={() => void restorePageScroll(true)}
          sessions={(model) => {
            sessionQuery = {
              filter: { model, from_day: data.usage.from_day, through_day: data.usage.through_day },
              cursor: null,
              direction: 'older',
            };
            selectedSession = null;
            scroll.sessions = 0;
            void goPage('sessions');
          }}
        />
        <p class="cp-note">
          {data.usage.sessions}
          {$t('个独立 sessions；一个 session 可使用多个模型。缓存属于输入，汇总不重复计算。')}
        </p>
      {:else if page === 'sessions'}
        <SessionsPane
          bind:query={sessionQuery}
          bind:selected={selectedSession}
          revision={data.updated_at}
          hideTitles={data.settings.hide_titles}
          hideProjects={data.settings.hide_projects}
          sources={data.sources}
          {now}
          ready={() => void restorePageScroll()}
        />
      {:else if page === 'news'}
        <NewsPane
          news={data.news}
          {now}
          timezone={data.settings.timezone}
          bind:limit={newsLimit}
          detail={newsChallenge}
          navigate={(detail) => void goNewsDetail(detail)}
        />
      {:else if page === 'settings'}
        <div class="cp-sectionhead">
          <span>{$t('显示与数据源')}</span><small>{$t('本地设置')}</small>
        </div>
        <StartupSettings windows={floatingSupported} />
        <AppUpdates
          info={appUpdate}
          busy={updateBusy}
          check={() => void loadAppUpdate()}
          open={openUpgrade}
          install={() => void applyUpgrade()}
        />
        <section oninput={() => (settingsDirty = true)} onchange={() => (settingsDirty = true)}>
          <label class="cp-setting"
            ><span>{$t('语言')}<small class="cp-setting-hint">{$t('自动保存')}</small></span>
            <select
              bind:value={settings.language}
              disabled={preferencesBusy}
              oninput={(e) => e.stopPropagation()}
              onchange={(e) => {
                e.stopPropagation();
                void savePreference({ language: e.currentTarget.value as Settings['language'] });
              }}
            >
              <option value="system">{$t('跟随系统')}</option>
              <option value="zh">中文</option><option value="en">English</option>
            </select>
          </label>
          <label class="cp-setting"
            ><span>{$t('外观')}<small class="cp-setting-hint">{$t('自动保存')}</small></span><select
              bind:value={settings.theme}
              disabled={preferencesBusy}
              oninput={(e) => e.stopPropagation()}
              onchange={(e) => {
                e.stopPropagation();
                void savePreference({ theme: e.currentTarget.value as Settings['theme'] });
              }}
              ><option value="system">{$t('跟随系统')}</option><option value="light"
                >{$t('浅色')}</option
              ><option value="dark">{$t('深色')}</option></select
            ></label
          ><label class="cp-setting"
            ><span
              >{$t('毛玻璃效果')}<small class="cp-setting-hint">{$t('关闭后使用不透明背景')}</small
              ></span
            ><input
              type="checkbox"
              role="switch"
              switch={floatingSupported ? undefined : true}
              bind:checked={settings.glass}
            /></label
          >
          <label class="cp-setting"
            ><span
              >{$t('隐藏会话标题')}<small class="cp-setting-hint"
                >{$t('详情使用短 ID，悬停提示也隐藏名称')}</small
              ></span
            ><input
              type="checkbox"
              role="switch"
              switch={floatingSupported ? undefined : true}
              bind:checked={settings.hide_titles}
            /></label
          >
          <label class="cp-setting"
            ><span>{$t('隐藏项目名称')}</span><input
              type="checkbox"
              role="switch"
              switch={floatingSupported ? undefined : true}
              bind:checked={settings.hide_projects}
            /></label
          >
          {#if floatingSupported}<label class="cp-setting"
              ><span
                >{$t('显示桌面浮窗')}<small class="cp-setting-hint"
                  >{$t('关闭后通过系统托盘查看详情')}</small
                ></span
              ><input
                type="checkbox"
                role="switch"
                switch={floatingSupported ? undefined : true}
                bind:checked={settings.floating}
              /></label
            ><label class="cp-setting"
              ><span>{$t('浮窗始终置顶')}</span><input
                type="checkbox"
                role="switch"
                switch={floatingSupported ? undefined : true}
                bind:checked={settings.always_on_top}
              /></label
            >{/if}
          <div class="cp-divider"></div>
          <div class="cp-sectionhead">
            <span>{$t('采集来源')}</span><small>{$t('本机 · 只读')}</small>
          </div>
          <label class="cp-setting"
            ><span>{floatingSupported ? 'Windows' : 'macOS'} Codex App / CLI</span><input
              type="checkbox"
              role="switch"
              switch={floatingSupported ? undefined : true}
              bind:checked={settings.windows_enabled}
            /></label
          >{#if floatingSupported}<label class="cp-setting"
              ><span
                >{$t('已运行的 WSL')}<small class="cp-setting-hint"
                  >{$t('不主动启动已停止的发行版')}</small
                ></span
              ><input
                type="checkbox"
                role="switch"
                switch={floatingSupported ? undefined : true}
                bind:checked={settings.wsl_enabled}
              /></label
            >{/if}<label class="cp-path"
            >{floatingSupported ? 'Windows' : 'macOS'} Codex home<input
              bind:value={path}
              placeholder={$t('自动探测 CODEX_HOME 或 .codex')}
            /></label
          >
          <SourceSettings
            bind:settings
            windows={floatingSupported}
            changed={() => (settingsDirty = true)}
          />
          <RefreshSettings {data} reload={refresh} />
          <div class="cp-divider"></div>
          <fieldset class="accent-picker">
            <legend>{$t('主题颜色')}</legend>
            <div class="accent-options">
              {#each accents as [key, label, dark, light]}<label
                  class:chosen={settings.accent === key}
                  style={`--swatch-dark:${dark};--swatch-light:${light}`}
                  ><input
                    type="radio"
                    name="accent"
                    value={key}
                    bind:group={settings.accent}
                    aria-label={`${label}${key === 'blue' ? $t('（默认）') : ''}`}
                  /><span class="accent-swatch" aria-hidden="true"></span><span>{label}</span
                  ></label
                >{/each}
            </div>
          </fieldset>
        </section>
        <button class="cp-save" disabled={saving} onclick={() => void save()}
          >{saving ? $t('正在保存…') : $t('保存设置')}</button
        >{#if settingsDirty}<button
            class="cp-textbutton"
            onclick={() => {
              settings = copySettings(data.settings);
              path = data.settings.windows_home ?? '';
              settingsDirty = false;
            }}>{$t('撤销未保存的修改')}</button
          >{/if}
        <div class="cp-divider"></div>
        <div class="cp-sectionhead">
          <span>{$t('来源状态')}</span><small>{$t('只读采集')}</small>
        </div>
        {#each data.sources as source}<div class="cp-source-status">
            <div class="cp-sectionhead">
              <span>{source.label}</span><small
                >{source.status === 'connected'
                  ? $t('已连接')
                  : source.status === 'no_logs'
                    ? $t('暂无日志')
                    : source.status === 'read_error'
                      ? $t('读取失败')
                      : source.status === 'wsl_stopped'
                        ? $t('发行版已停止')
                        : source.status === 'wsl_unavailable'
                          ? $t('无法确认运行状态')
                          : $t('探测中')}</small
              >
            </div>
            <p class="cp-note">
              {source.path}<br />{source.files}
              {$t('个日志 ·')}
              {source.issues}
              {$t('个解析问题 ·')}
              {time(source.last_read)}
            </p>
          </div>{:else}<p class="cp-note">{$t('等待来源探测')}</p>{/each}
        <DiagnosticsPanel />
      {/if}
    </div>
    <footer class="cp-footer">
      <span
        >{data.collecting
          ? $t('正在建立本地索引…')
          : $t('本地用量 · {_0} 更新', { _0: time(data.updated_at) })}</span
      >{#if data.usage.reference.unpriced_events > 0 && !data.usage.reference.pending}<span
          class="cp-price-note"
          title={$t('另有 {_0} tokens 缺少模型价格、服务层级或完整计数', {
            _0: data.usage.reference.unpriced_tokens.toLocaleString(),
          })}>{$t('* 估算未覆盖全部用量')}</span
        >{/if}<button class="cp-textbutton" onclick={() => void windowAction('compact')}
        >{$t('收起详情')} <Icon name="up" /></button
      >
    </footer>
  {/if}
</main>
