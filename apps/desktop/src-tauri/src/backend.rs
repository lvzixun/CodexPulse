use chrono::Utc;
use chrono_tz::Tz;
use notify::{RecursiveMode, Watcher};
use pulse_core::{
    collectors::jsonl::read_batch,
    pricing::PriceBook,
    storage::{RecentSession, Store, UsageSummary},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: String,
    pub language: String,
    pub accent: String,
    pub glass: bool,
    pub floating: bool,
    pub always_on_top: bool,
    // Retain the original serialized keys for existing settings; these fields
    // describe the native local source on both Windows and macOS.
    pub windows_enabled: bool,
    pub wsl_enabled: bool,
    pub windows_home: Option<String>,
    pub windows_sources: Vec<crate::source_config::LocalSource>,
    pub wsl_sources: Vec<crate::source_config::WslSource>,
    pub wsl_auto_detect: bool,
    pub hide_titles: bool,
    pub hide_projects: bool,
    pub quota_refresh: crate::refresh::Config,
    pub news_refresh: crate::refresh::Config,
    pub timezone: String,
    pub compact_position: Option<(i32, i32)>,
    pub compact_anchor: Option<crate::geometry::Anchor>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            language: "system".into(),
            accent: "blue".into(),
            glass: true,
            floating: cfg!(windows),
            always_on_top: true,
            windows_enabled: true,
            wsl_enabled: cfg!(windows),
            windows_home: None,
            windows_sources: Vec::new(),
            wsl_sources: Vec::new(),
            wsl_auto_detect: true,
            hide_titles: false,
            hide_projects: false,
            quota_refresh: crate::refresh::Config::default(),
            news_refresh: crate::refresh::Config::default(),
            timezone: iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".into()),
            compact_position: None,
            compact_anchor: None,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        self.quota_refresh.validate()?;
        self.news_refresh.validate()?;
        if self.compact_anchor.as_ref().is_some_and(|a| !a.valid()) {
            return Err("浮窗位置无效".into());
        }
        if !["system", "light", "dark"].contains(&self.theme.as_str()) {
            return Err("主题无效".into());
        }
        if !["system", "zh", "en"].contains(&self.language.as_str()) {
            return Err("语言无效".into());
        }
        if !["blue", "violet", "teal", "amber", "rose"].contains(&self.accent.as_str()) {
            return Err("主题颜色无效".into());
        }
        self.timezone
            .parse::<Tz>()
            .map_err(|_| "请填写有效的 IANA 时区".to_string())?;
        if self
            .windows_home
            .as_ref()
            .is_some_and(|p| !crate::source_config::valid_local_home(p))
        {
            return Err("Codex home 必须是绝对路径".into());
        }
        crate::source_config::validate(&self.windows_sources, &self.wsl_sources)
    }
    fn redact(&self, meta: &mut pulse_core::domain::SessionMeta) {
        if self.hide_titles {
            meta.title = None;
        }
        if self.hide_projects {
            meta.project = None;
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceHealth {
    pub id: String,
    pub label: String,
    pub path: String,
    pub status: String,
    pub last_read: Option<String>,
    pub files: usize,
    pub issues: usize,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub settings: Settings,
    pub usage: UsageSummary,
    pub recent: Vec<RecentSession>,
    pub sources: Vec<SourceHealth>,
    pub updated_at: Option<String>,
    pub collecting: bool,
    pub error: Option<String>,
    pub quota: QuotaState,
    pub news: crate::news::NewsSnapshot,
    pub timezone_rebuild: Option<pulse_core::storage::TimezoneProgress>,
    pub timezone_error: Option<String>,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct QuotaState {
    pub buckets: Vec<pulse_core::quota::QuotaBucket>,
    pub sources: BTreeMap<String, QuotaSource>,
    #[serde(default)]
    pub request_status: String,
    #[serde(default)]
    pub last_success: Option<String>,
    #[serde(default)]
    pub next_attempt: Option<i64>,
}
impl Snapshot {
    pub fn for_display(&self) -> Self {
        let mut snapshot = self.clone();
        for session in &mut snapshot.recent {
            snapshot.settings.redact(&mut session.meta);
        }
        snapshot
    }
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct QuotaSource {
    pub allowance: Option<crate::account_allowance::AccountAllowance>,
    pub profile: Option<crate::account_profile::AccountProfile>,
    pub profile_status: String,
    pub profile_last_success: Option<String>,
    pub home: String,
    pub status: String,
    pub last_attempt: String,
    pub failures: u32,
    pub buckets: Vec<pulse_core::quota::QuotaBucket>,
    pub identity: Option<String>,
    pub last_success: Option<String>,
    pub retry_at: i64,
    pub proxy_source: String,
}
pub enum Message {
    Settings(Settings, tokio::sync::oneshot::Sender<Result<(), String>>),
    UiPreferences(
        Option<String>,
        Option<String>,
        tokio::sync::oneshot::Sender<Result<Settings, String>>,
    ),
    Quotas(u64, Vec<crate::http_quota::ResultSet>),
    QuotaStarted(u64, crate::http_quota::Scope, crate::http_quota::Stamp),
    Probes(
        u64,
        Vec<(crate::http_quota::Scope, crate::http_quota::Stamp)>,
    ),
    News(u64, Box<crate::news::Feed>),
    Refresh(
        String,
        tokio::sync::oneshot::Sender<Result<Option<i64>, String>>,
    ),
    RefreshSettings(
        String,
        crate::refresh::Config,
        tokio::sync::oneshot::Sender<Result<(), String>>,
    ),
    ReadNews(
        Vec<String>,
        tokio::sync::oneshot::Sender<Result<(), String>>,
    ),
    SessionPage(
        pulse_core::storage::SessionPageRequest,
        tokio::sync::oneshot::Sender<Result<pulse_core::storage::SessionPage, String>>,
    ),
    ModelPage(
        pulse_core::storage::ModelPageRequest,
        tokio::sync::oneshot::Sender<Result<pulse_core::storage::ModelPage, String>>,
    ),
    SessionDetail(
        pulse_core::storage::SessionDetailRequest,
        tokio::sync::oneshot::Sender<Result<Option<pulse_core::storage::SessionDetail>, String>>,
    ),
}
enum NetworkRequest {
    Quotas(u64, Vec<crate::http_quota::Scope>),
    Probes(u64, Vec<crate::http_quota::Scope>),
    News(u64, Box<crate::news::Feed>, u64, bool),
    Translation(
        Vec<String>,
        tokio::sync::oneshot::Sender<Result<String, String>>,
    ),
}
pub struct Backend {
    pub snapshot: Arc<RwLock<Snapshot>>,
    pub sender: SyncSender<Message>,
    network_sender: SyncSender<NetworkRequest>,
    stopping: Arc<AtomicBool>,
    pending_anchor: Arc<Mutex<Option<(crate::geometry::Anchor, Instant)>>>,
    database: PathBuf,
}
struct Source {
    health: SourceHealth,
    home: PathBuf,
    wsl: Option<crate::source_config::WslTarget>,
}
impl Source {
    fn available(&self) -> bool {
        !matches!(
            self.health.status.as_str(),
            "wsl_stopped" | "wsl_unavailable"
        )
    }
    fn owns_path(&self, path: &std::path::Path) -> bool {
        path.extension().is_some_and(|e| e == "jsonl")
            && (path == self.home.join("session_index.jsonl")
                || path.starts_with(self.home.join("sessions"))
                || path.starts_with(self.home.join("archived_sessions")))
    }
    fn scope_matches(
        &self,
        id: &str,
        home: &std::path::Path,
        wsl: Option<&crate::source_config::WslTarget>,
    ) -> bool {
        self.health.id == id && self.home == home && self.wsl.as_ref() == wsl
    }
}
impl Backend {
    pub fn start(app: tauri::AppHandle, db: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let store = Store::open(&db)?;
        let settings = initial_settings(&store)?;
        let quota = store.setting::<QuotaState>("quota")?.unwrap_or_default();
        let mut news = store
            .setting::<crate::news::Feed>("news")?
            .unwrap_or_default();
        news.reconcile_cached_status();
        if news.challenge_status.is_empty() {
            // Refresh legacy caches once so the newly added challenge is available.
            news.next_attempt = 0;
        }
        let inbox = store
            .setting::<crate::inbox::Inbox>("news_inbox")?
            .unwrap_or_default();
        let snapshot = Arc::new(RwLock::new(Snapshot {
            settings: settings.clone(),
            quota: QuotaState::default(),
            news: inbox.view(news.view()),
            usage: store.summary(
                Utc::now()
                    .with_timezone(&settings.timezone.parse::<Tz>().unwrap_or(chrono_tz::UTC))
                    .date_naive(),
                settings.timezone.parse().unwrap_or(chrono_tz::UTC),
            )?,
            recent: store.recent_sessions(None, 10)?,
            collecting: true,
            ..Default::default()
        }));
        let (sender, receiver) = mpsc::sync_channel(8);
        let stopping = Arc::new(AtomicBool::new(false));
        let worker_stop = stopping.clone();
        let collector_stop = stopping.clone();
        let quota_epoch = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let news_epoch = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let worker_quota_epoch = quota_epoch.clone();
        let worker_news_epoch = news_epoch.clone();
        let pending_anchor = Arc::new(Mutex::new(None::<(crate::geometry::Anchor, Instant)>));
        let collector_anchor = pending_anchor.clone();
        let (rpc_sender, rpc_receiver) = mpsc::sync_channel::<NetworkRequest>(1);
        let network_sender = rpc_sender.clone();
        let replies = sender.clone();
        let network_app = app.clone();
        let public_scopes = Arc::new(RwLock::new(Vec::<crate::http_quota::Scope>::new()));
        let worker_public_scopes = public_scopes.clone();
        thread::Builder::new()
            .name("pulse-network".into())
            .spawn(move || {
                let mut translator = crate::news::Translator::default();
                while !worker_stop.load(Ordering::Relaxed) {
                    match rpc_receiver.recv_timeout(Duration::from_millis(300)) {
                        Ok(NetworkRequest::Translation(keys, reply)) => {
                            let scopes = worker_public_scopes.read().map(|s| s.clone());
                            let result = match scopes {
                                Ok(scopes) => translator.translate(&keys, &scopes),
                                Err(_) => Err("消息网络配置暂不可用".into()),
                            };
                            let _ = reply.send(result);
                        }
                        Ok(NetworkRequest::News(generation, feed, interval, manual)) => {
                            let cancelled = || {
                                worker_stop.load(Ordering::Relaxed)
                                    || worker_news_epoch.load(Ordering::Relaxed) != generation
                            };
                            let feed = if cancelled() {
                                *feed
                            } else {
                                let scopes = worker_public_scopes
                                    .read()
                                    .unwrap_or_else(|error| error.into_inner())
                                    .clone();
                                crate::news::fetch(*feed, interval, manual, &cancelled, &scopes)
                            };
                            if !worker_stop.load(Ordering::Relaxed) {
                                let _ = replies.send(Message::News(generation, Box::new(feed)));
                            }
                        }
                        Ok(NetworkRequest::Probes(generation, scopes)) => {
                            let mut results = Vec::new();
                            for scope in scopes {
                                if worker_stop.load(Ordering::Relaxed)
                                    || worker_quota_epoch.load(Ordering::Relaxed) != generation
                                {
                                    break;
                                }
                                let stamp = crate::http_quota::probe(&scope);
                                results.push((scope, stamp));
                            }
                            if !worker_stop.load(Ordering::Relaxed) {
                                let _ = replies.send(Message::Probes(generation, results));
                            }
                        }
                        Ok(NetworkRequest::Quotas(generation, scopes)) => {
                            let cancelled = || {
                                worker_stop.load(Ordering::Relaxed)
                                    || worker_quota_epoch.load(Ordering::Relaxed) != generation
                                    || (cfg!(windows)
                                        && !crate::platform::account_panel_visible(&network_app))
                            };
                            let mut results = Vec::new();
                            for scope in scopes {
                                if cancelled() {
                                    break;
                                }
                                let (stamp, result) = crate::http_quota::checked_fetch(
                                    &scope,
                                    |stamp| {
                                        let _ = replies.send(Message::QuotaStarted(
                                            generation,
                                            scope.clone(),
                                            stamp.clone(),
                                        ));
                                    },
                                    |credentials| {
                                        if cancelled() {
                                            Err("cancelled".into())
                                        } else {
                                            crate::http_quota::fetch_cancellable(
                                                &scope,
                                                credentials,
                                                &cancelled,
                                            )
                                        }
                                    },
                                );
                                let (profile_stamp, mut profile) = crate::http_quota::checked_fetch(
                                    &scope,
                                    |_| {},
                                    |credentials| {
                                        if cancelled() {
                                            Err("cancelled".into())
                                        } else {
                                            crate::http_quota::fetch_profile(credentials)
                                        }
                                    },
                                );
                                let (stamp, result) = if stamp != profile_stamp {
                                    profile = Err("credentials_changed".into());
                                    (profile_stamp, Err("credentials_changed".into()))
                                } else {
                                    (stamp, result)
                                };
                                results.push(crate::http_quota::ResultSet {
                                    scope,
                                    stamp,
                                    result,
                                    profile,
                                });
                            }
                            if !worker_stop.load(Ordering::Relaxed) {
                                let _ = replies.send(Message::Quotas(generation, results));
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                }
            })?;
        let shared = snapshot.clone();
        thread::Builder::new()
            .name("pulse-collector".into())
            .spawn(move || {
                let mut store = store;
                let mut settings = settings;
                let mut sources = Vec::<Source>::new();
                let changed = Arc::new(Mutex::new(Vec::<PathBuf>::new()));
                let watcher_paths = changed.clone();
                let mut watcher =
                    notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                        if let Ok(event) = event
                            && let Ok(mut paths) = watcher_paths.lock()
                        {
                            for path in event.paths {
                                if paths.len() < 4096 {
                                    paths.push(path);
                                }
                            }
                        }
                    })
                    .ok();
                let mut queue = VecDeque::<(String, PathBuf)>::new();
                let mut queued = HashSet::<(String, PathBuf)>::new();
                let mut last_repair = Instant::now() - Duration::from_secs(60);
                let mut last_wsl = Instant::now() - Duration::from_secs(60);
                let mut wsl_home_cache = BTreeMap::<String, PathBuf>::new();
                let mut last_publish = Instant::now() - Duration::from_secs(1);
                let mut dirty = true;
                let mut quota = quota;
                let mut news = news;
                let mut inbox = inbox;
                let mut news_schedule = crate::refresh::Schedule::new();
                let mut quota_schedule = crate::refresh::Schedule::new();
                let mut stamps = BTreeMap::<String, crate::http_quota::Stamp>::new();
                let mut probe_in_flight = false;
                let mut last_probe = Instant::now() - Duration::from_secs(60);
                news_schedule.next = news.next_attempt.min(news.challenge_next_attempt);
                let prices = PriceBook::bundled().unwrap_or_default();
                let mut timezone_error = None;
                let mut last_timezone_check = Instant::now() - Duration::from_secs(60);
                let mut summary_day = Utc::now()
                    .with_timezone(&settings.timezone.parse::<Tz>().unwrap_or(chrono_tz::UTC))
                    .date_naive();
                loop {
                    let anchor = collector_anchor.lock().ok().and_then(|mut pending| {
                        if pending
                            .as_ref()
                            .is_some_and(|(_, moved)| moved.elapsed() >= Duration::from_millis(500))
                        {
                            pending.take().map(|(anchor, _)| anchor)
                        } else {
                            None
                        }
                    });
                    if let Some(anchor) = anchor {
                        settings.compact_anchor = Some(anchor.clone());
                        settings.compact_position = None;
                        if store.set_setting("app", &settings).is_ok() {
                            if let Ok(mut state) = shared.write() {
                                state.settings = settings.clone();
                            }
                        } else if let Ok(mut pending) = collector_anchor.lock() {
                            *pending = Some((anchor, Instant::now()));
                        }
                    }
                    if collector_stop.load(Ordering::Relaxed) {
                        break;
                    }
                    crate::platform::release_hidden(&app);
                    if last_timezone_check.elapsed() >= Duration::from_secs(60) {
                        last_timezone_check = Instant::now();
                        let detected = iana_time_zone::get_timezone()
                            .ok()
                            .and_then(|zone| zone.parse::<Tz>().ok());
                        let result = detected
                            .ok_or_else(|| {
                                "无法读取系统时区，继续使用已保存的统计时区；稍后自动重试。"
                                    .to_string()
                            })
                            .and_then(|zone| {
                                sync_system_timezone(&mut store, &settings.timezone, zone).map_err(
                                    |_| {
                                        "无法开始时区重建，已保留有效统计；稍后自动重试。"
                                            .to_string()
                                    },
                                )
                            });
                        match result {
                            Ok(changed) => {
                                dirty |= changed || timezone_error.is_some();
                                timezone_error = None;
                            }
                            Err(error) => {
                                // Do not publish an unfinished stage when the current OS zone
                                // cannot be confirmed. Keep the last committed calendar.
                                let _ = store.cancel_timezone_rebuild();
                                dirty |= timezone_error.as_ref() != Some(&error);
                                timezone_error = Some(error);
                            }
                        }
                    }
                    let wait = if store.timezone_rebuild_status().is_some() {
                        10
                    } else {
                        300
                    };
                    match receiver.recv_timeout(Duration::from_millis(wait)) {
                        Ok(Message::ReadNews(keys, reply)) => {
                            let mut next = inbox.clone();
                            next.acknowledge(&keys);
                            let result = store
                                .set_setting("news_inbox", &next)
                                .map_err(|_| "无法保存已读状态".into());
                            if result.is_ok() {
                                inbox = next;
                                dirty = true;
                            }
                            let _ = reply.send(result);
                        }
                        Ok(Message::ModelPage(request, reply)) => {
                            let result = settings
                                .timezone
                                .parse::<Tz>()
                                .map_err(|_| "统计时区无效".to_string())
                                .and_then(|tz| {
                                    store
                                        .model_page(&request, tz)
                                        .map_err(|_| "无法读取模型分类，请刷新列表".into())
                                });
                            let _ = reply.send(result);
                        }
                        Ok(Message::SessionPage(request, reply)) => {
                            let result = settings
                                .timezone
                                .parse::<chrono_tz::Tz>()
                                .map_err(|_| "统计时区无效".into())
                                .and_then(|tz| {
                                    store
                                        .session_page_with_activity(
                                            &request,
                                            tz,
                                            Utc::now(),
                                            &sources
                                                .iter()
                                                .filter(|s| s.health.status == "connected")
                                                .map(|s| s.health.id.clone())
                                                .collect::<Vec<_>>(),
                                        )
                                        .map(|mut page| {
                                            for session in &mut page.items {
                                                settings.redact(&mut session.meta);
                                            }
                                            page
                                        })
                                        .map_err(|_| "无法读取 session 历史".into())
                                });
                            let _ = reply.send(result);
                        }
                        Ok(Message::SessionDetail(request, reply)) => {
                            let _ = reply.send(
                                store
                                    .session_detail(&request)
                                    .map(|mut detail| {
                                        if let Some(detail) = &mut detail {
                                            settings.redact(&mut detail.session.meta);
                                        }
                                        detail
                                    })
                                    .map_err(|_| "无法读取 session 详情".into()),
                            );
                        }
                        Ok(Message::Refresh(group, reply)) => {
                            let result = match group.as_str() {
                                "quota" => {
                                    request_quota_refresh(&mut quota_schedule, &mut quota);

                                    Ok(None)
                                }
                                "news" => {
                                    let waiting = if news_schedule.flight.is_none() {
                                        news.prepare_manual_refresh(Utc::now().timestamp())
                                    } else {
                                        None
                                    };
                                    news_schedule.request();
                                    Ok(waiting)
                                }
                                _ => Err("刷新组无效".into()),
                            };
                            dirty = true;
                            let _ = reply.send(result);
                        }
                        Ok(Message::RefreshSettings(group, config, reply)) => {
                            let result = config.validate().and_then(|()| {
                                let mut next = settings.clone();
                                let old = match group.as_str() {
                                    "quota" => {
                                        let old = next.quota_refresh.clone();
                                        next.quota_refresh = config.clone();
                                        old
                                    }
                                    "news" => {
                                        let old = next.news_refresh.clone();
                                        next.news_refresh = config.clone();
                                        old
                                    }
                                    _ => return Err("刷新组无效".into()),
                                };
                                store
                                    .set_setting("app", &next)
                                    .map_err(|_| "无法保存刷新设置".to_string())?;
                                if old != config {
                                    let now = Utc::now().timestamp();
                                    if group == "quota" {
                                        let next_due =
                                            if old.mode == "manual" && config.mode == "auto" {
                                                now
                                            } else {
                                                last_attempt(&quota)
                                                    .saturating_add(config.interval_seconds as i64)
                                            };
                                        quota_schedule.invalidate(next_due);
                                        quota_epoch
                                            .store(quota_schedule.generation, Ordering::Relaxed);
                                    } else {
                                        let attempt = news
                                            .last_attempt
                                            .as_deref()
                                            .and_then(|s| {
                                                chrono::DateTime::parse_from_rfc3339(s).ok()
                                            })
                                            .map_or(0, |d| d.timestamp());
                                        news.next_attempt =
                                            attempt.saturating_add(config.interval_seconds as i64);
                                        let challenge_attempt = news
                                            .challenge_cache
                                            .fetched_at
                                            .as_deref()
                                            .and_then(|s| {
                                                chrono::DateTime::parse_from_rfc3339(s).ok()
                                            })
                                            .map_or(0, |d| d.timestamp());
                                        news.challenge_next_attempt = challenge_attempt
                                            .saturating_add(config.interval_seconds as i64);
                                        news_schedule.invalidate(
                                            if old.mode == "manual" && config.mode == "auto" {
                                                now
                                            } else {
                                                attempt
                                                    .saturating_add(config.interval_seconds as i64)
                                            },
                                        );
                                        news_epoch
                                            .store(news_schedule.generation, Ordering::Relaxed);
                                    }
                                }
                                settings = next;
                                dirty = true;
                                // IPC acknowledgement must observe the persisted config immediately.
                                if let Ok(mut state) = shared.write() {
                                    state.settings = settings.clone();
                                }
                                Ok(())
                            });
                            let _ = reply.send(result);
                        }
                        Ok(Message::Probes(generation, results)) => {
                            probe_in_flight = false;
                            if generation == quota_schedule.generation {
                                let mut probes_changed = false;
                                for (scope, stamp) in results {
                                    if !sources.iter().any(|s| {
                                        s.scope_matches(
                                            &scope.source_id,
                                            &scope.home,
                                            scope.wsl.as_ref(),
                                        ) && s.available()
                                    }) {
                                        continue;
                                    }
                                    let changed = stamps
                                        .get(&scope.source_id)
                                        .is_some_and(|old| old != &stamp);
                                    if stamps.get(&scope.source_id) == Some(&stamp) {
                                        continue;
                                    }
                                    probes_changed = true;
                                    reconcile_identity(&mut quota, &scope, &stamp);
                                    stamps.insert(scope.source_id.clone(), stamp);
                                    if changed {
                                        quota_schedule.invalidate(0);
                                        quota_epoch
                                            .store(quota_schedule.generation, Ordering::Relaxed);
                                    }
                                }
                                if probes_changed {
                                    let _ = persist_quota(&store, &mut quota);
                                    dirty = true;
                                }
                            }
                        }
                        Ok(Message::QuotaStarted(generation, scope, stamp)) => {
                            if generation == quota_schedule.generation
                                && sources.iter().any(|s| {
                                    s.scope_matches(
                                        &scope.source_id,
                                        &scope.home,
                                        scope.wsl.as_ref(),
                                    )
                                })
                            {
                                reconcile_identity(&mut quota, &scope, &stamp);
                                stamps.insert(scope.source_id.clone(), stamp);
                                dirty = true;
                            }
                        }
                        Ok(Message::News(generation, next)) => {
                            let next_due = next
                                .next_attempt
                                .max(next.retry_until)
                                .min(next.challenge_next_attempt.max(next.challenge_retry_until));
                            if news_schedule.finish(generation, next_due) {
                                news = *next;
                                inbox.update(&news);
                                let _ = store.set_setting("news_inbox", &inbox);
                                let _ = store.set_setting("news", &news.for_storage());
                            }
                            dirty = true;
                        }
                        Ok(Message::Quotas(generation, results)) => {
                            let now = Utc::now().timestamp();
                            if quota_schedule.finish(
                                generation,
                                now.saturating_add(settings.quota_refresh.interval_seconds as i64),
                            ) {
                                for response in results {
                                    let scope = response.scope;
                                    if !sources.iter().any(|s| {
                                        s.scope_matches(
                                            &scope.source_id,
                                            &scope.home,
                                            scope.wsl.as_ref(),
                                        ) && s.available()
                                    }) {
                                        continue;
                                    }
                                    reconcile_identity(&mut quota, &scope, &response.stamp);
                                    stamps.insert(scope.source_id.clone(), response.stamp);
                                    let state = quota
                                        .sources
                                        .get_mut(&scope.source_id)
                                        .expect("reconciled source");
                                    state.last_attempt = Utc::now().to_rfc3339();
                                    match response.profile {
                                        Ok(profile) => {
                                            state.profile_status = "connected".into();
                                            state.profile_last_success =
                                                Some(state.last_attempt.clone());
                                            state.profile = Some(profile);
                                        }
                                        Err(error) => {
                                            state.profile_status = error.code.clone();
                                            if [
                                                "not_signed_in",
                                                "reauth_required",
                                                "account_mismatch",
                                                "credentials_unreadable",
                                                "credentials_invalid",
                                                "credentials_changed",
                                            ]
                                            .contains(&error.code.as_str())
                                            {
                                                state.profile = None;
                                                state.profile_last_success = None;
                                            }
                                        }
                                    }
                                    match response.result {
                                        Ok(readout) => {
                                            state.status = "connected".into();
                                            state.failures = 0;
                                            state.retry_at = if readout.allowance_retry_after > 0 {
                                                now.saturating_add(readout.allowance_retry_after)
                                            } else {
                                                0
                                            };
                                            state.allowance = Some(readout.allowance);
                                            state.last_success = Some(state.last_attempt.clone());
                                            state.buckets = readout.buckets;
                                            state.proxy_source = readout.proxy_source;
                                        }
                                        Err(error) => {
                                            state.status = error.code.clone();
                                            if [
                                                "reauth_required",
                                                "credentials_expired",
                                                "credentials_changed",
                                            ]
                                            .contains(&error.code.as_str())
                                            {
                                                state.allowance = None;
                                            }
                                            if error.code == "credentials_changed" {
                                                state.retry_at = 0;
                                                quota_schedule.next = 0;
                                            } else {
                                                state.failures = state.failures.saturating_add(1);
                                                let backoff = (30_i64
                                                    * (1_i64 << state.failures.min(6)))
                                                .min(1800);
                                                state.retry_at = now
                                                    .saturating_add(backoff.max(error.retry_after));
                                            }
                                            if [
                                                "not_signed_in",
                                                "account_mismatch",
                                                "credentials_unreadable",
                                                "credentials_invalid",
                                            ]
                                            .contains(&error.code.as_str())
                                            {
                                                state.buckets.clear();
                                                state.allowance = None;
                                            }
                                        }
                                    }
                                }
                                quota.last_success = quota
                                    .sources
                                    .values()
                                    .filter_map(|s| s.last_success.clone())
                                    .max();
                                let _ = persist_quota(&store, &mut quota);
                            }
                            dirty = true;
                        }
                        Ok(Message::UiPreferences(theme, language, reply)) => {
                            let result =
                                update_ui_preferences(&mut store, &settings, theme, language);
                            let result = result.map(|next| {
                                settings = next;
                                if let Ok(mut state) = shared.write() {
                                    state.settings = settings.clone();
                                }
                                dirty = true;
                                settings.clone()
                            });
                            let _ = reply.send(result);
                        }
                        Ok(Message::Settings(mut next, reply)) => {
                            // Location is host-owned and can change while a UI settings draft is open.
                            next.compact_anchor = settings.compact_anchor.clone();
                            next.compact_position = settings.compact_position;
                            // Refresh controls are saved independently, even when this appearance draft is old.
                            next.quota_refresh = settings.quota_refresh.clone();
                            next.news_refresh = settings.news_refresh.clone();
                            // Language and appearance save independently of source/privacy drafts.
                            next.language = settings.language.clone();
                            next.theme = settings.theme.clone();
                            // Timezone has its own atomic rebuild path. An older appearance
                            // draft must never restore the pre-rebuild timezone.
                            next.timezone = settings.timezone.clone();
                            let sources_changed = settings.windows_enabled != next.windows_enabled
                                || settings.wsl_enabled != next.wsl_enabled
                                || settings.windows_home != next.windows_home
                                || settings.windows_sources != next.windows_sources
                                || settings.wsl_sources != next.wsl_sources
                                || settings.wsl_auto_detect != next.wsl_auto_detect;
                            // Save the source switch and invalidation in the same transaction.
                            let result = persist_settings(&mut store, &next, sources_changed)
                                .map_err(|_| "无法保存设置".to_string());
                            if result.is_ok() {
                                settings = next;
                                if let Ok(mut state) = shared.write() {
                                    state.settings = settings.clone();
                                }
                                if sources_changed {
                                    wsl_home_cache.clear();
                                    quota = QuotaState::default();
                                    // Cancel queued reads for old roots before a changed source ID is reused.
                                    queue.clear();
                                    queued.clear();
                                    last_repair = Instant::now() - Duration::from_secs(60);
                                    last_wsl = Instant::now() - Duration::from_secs(60);
                                    stamps.clear();
                                    quota_schedule.invalidate(0);
                                    quota_epoch.store(quota_schedule.generation, Ordering::Relaxed);
                                    last_probe = Instant::now() - Duration::from_secs(60);
                                }
                                dirty = true;
                            }
                            let _ = reply.send(result);
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    if last_repair.elapsed() >= Duration::from_secs(60) {
                        let mut windows = Vec::new();
                        if settings.windows_enabled {
                            let home = settings
                                .windows_home
                                .as_ref()
                                .map(PathBuf::from)
                                .or_else(|| std::env::var_os("CODEX_HOME").map(PathBuf::from))
                                .unwrap_or_else(|| {
                                    PathBuf::from(
                                        std::env::var_os(if cfg!(windows) {
                                            "USERPROFILE"
                                        } else {
                                            "HOME"
                                        })
                                        .unwrap_or_default(),
                                    )
                                    .join(".codex")
                                });
                            windows.push((
                                if cfg!(target_os = "macos") {
                                    "macos"
                                } else {
                                    "windows"
                                }
                                .to_owned(),
                                if cfg!(target_os = "macos") {
                                    "macOS App / CLI"
                                } else {
                                    "Windows App / CLI"
                                }
                                .to_owned(),
                                home,
                            ));
                            windows.extend(
                                settings
                                    .windows_sources
                                    .iter()
                                    .filter(|s| s.enabled)
                                    .map(|s| {
                                        (
                                            format!(
                                                "{}:{}",
                                                if cfg!(target_os = "macos") {
                                                    "macos"
                                                } else {
                                                    "windows"
                                                },
                                                s.id
                                            ),
                                            s.label.clone(),
                                            PathBuf::from(&s.home),
                                        )
                                    }),
                            );
                        }
                        let removed = sources
                            .iter()
                            .filter(|s| {
                                s.wsl.is_none()
                                    && !windows
                                        .iter()
                                        .any(|(id, _, home)| id == &s.health.id && home == &s.home)
                            })
                            .map(|s| s.home.clone())
                            .collect::<Vec<_>>();
                        sources.retain(|s| {
                            s.wsl.is_some()
                                || windows
                                    .iter()
                                    .any(|(id, _, home)| id == &s.health.id && home == &s.home)
                        });
                        if let Some(w) = watcher.as_mut() {
                            for home in removed {
                                if !sources.iter().any(|s| s.wsl.is_none() && s.home == home) {
                                    let _ = w.unwatch(&home);
                                }
                            }
                        }
                        for (id, label, home) in windows {
                            if let Some(existing) = sources.iter_mut().find(|s| s.health.id == id) {
                                existing.health.label = label;
                            } else {
                                if let Some(w) = watcher.as_mut()
                                    && !sources.iter().any(|s| s.wsl.is_none() && s.home == home)
                                {
                                    let _ = w.watch(&home, RecursiveMode::Recursive);
                                }
                                sources.push(source(id, label, home));
                            }
                        }
                        for source in sources.iter_mut().filter(|s| s.wsl.is_none()) {
                            enqueue_source(source, &mut queue, &mut queued);
                        }
                        last_repair = Instant::now();
                        dirty = true;
                    }
                    if last_wsl.elapsed() >= Duration::from_secs(30) {
                        let discovered = if settings.wsl_enabled {
                            crate::platform::running_wsl_sources(
                                settings.wsl_auto_detect,
                                &settings.wsl_sources,
                                &mut wsl_home_cache,
                            )
                        } else {
                            Vec::new()
                        };
                        sources.retain(|s| {
                            s.wsl.is_none()
                                || discovered.iter().any(|item| {
                                    s.scope_matches(&item.id, &item.home, Some(&item.target))
                                })
                        });
                        for item in discovered {
                            if let Some(old) = sources.iter_mut().find(|s| s.health.id == item.id) {
                                if !item.running {
                                    old.health.status = item.status.into();
                                } else if !old.available() {
                                    old.health.status = "discovering".into();
                                }
                            } else {
                                let mut next = source(item.id, item.label, item.home);
                                next.wsl = Some(item.target);
                                if !item.running {
                                    next.health.status = item.status.into();
                                }
                                sources.push(next);
                            }
                        }
                        // WSL paths are polled only while the distro was reported running.
                        for source in sources
                            .iter_mut()
                            .filter(|s| s.wsl.is_some() && s.available())
                        {
                            enqueue_source(source, &mut queue, &mut queued);
                        }
                        last_wsl = Instant::now();
                        dirty = true;
                    }
                    if let Ok(mut paths) = changed.lock() {
                        for path in paths.drain(..) {
                            for source in sources.iter().filter(|s| {
                                s.wsl.is_none()
                                    && ["auth.json", "config.toml", ".env"]
                                        .iter()
                                        .any(|f| path == s.home.join(f))
                            }) {
                                if let Some(q) = quota.sources.get_mut(&source.health.id) {
                                    q.buckets.clear();
                                    q.allowance = None;
                                    q.profile = None;
                                    q.profile_last_success = None;
                                    q.profile_status = "verifying_account".into();
                                    q.identity = None;
                                    q.status = "verifying_account".into();
                                }
                                stamps.remove(&source.health.id);
                                quota_schedule.invalidate(0);
                                quota_epoch.store(quota_schedule.generation, Ordering::Relaxed);
                                last_probe = Instant::now() - Duration::from_secs(60);
                                dirty = true;
                            }
                            for source in sources
                                .iter()
                                .filter(|s| s.wsl.is_none() && s.owns_path(&path))
                            {
                                enqueue(
                                    &mut queue,
                                    &mut queued,
                                    (source.health.id.clone(), path.clone()),
                                );
                            }
                        }
                    }
                    let now = Utc::now().timestamp();
                    // Only enabled, available sources enter the public routing
                    // context. In particular, never read a stopped WSL home.
                    if dirty {
                        let next = sources
                            .iter()
                            .filter(|s| s.available())
                            .map(scope)
                            .collect();
                        if let Ok(mut current) = public_scopes.write()
                            && *current != next
                        {
                            *current = next;
                        }
                    }
                    let quota_block = sources
                        .iter()
                        .filter(|s| s.available())
                        .map(|s| quota.sources.get(&s.health.id).map_or(0, |q| q.retry_at))
                        .min()
                        .unwrap_or(0);
                    // Re-query the current native window so tray-only startup,
                    // hiding and WebView destruction/rebuilding use the same policy.
                    let panel_visible = crate::platform::account_panel_visible(&app);
                    if (!cfg!(windows) || panel_visible)
                        && quota_schedule.due_for_account(
                            &settings.quota_refresh,
                            now,
                            0,
                            panel_visible,
                            !cfg!(windows),
                        )
                    {
                        let scopes = sources
                            .iter()
                            .filter(|s| {
                                s.available()
                                    && quota
                                        .sources
                                        .get(&s.health.id)
                                        .is_none_or(|q| q.retry_at <= now)
                            })
                            .map(scope)
                            .collect::<Vec<_>>();
                        if !scopes.is_empty()
                            && rpc_sender
                                .try_send(NetworkRequest::Quotas(quota_schedule.generation, scopes))
                                .is_ok()
                        {
                            quota_schedule.begin();
                            dirty = true;
                        }
                    }
                    let news_block = news.retry_until.min(news.challenge_retry_until);
                    if news_schedule.due(&settings.news_refresh, now, news_block) {
                        let manual = news_schedule.requested;
                        if rpc_sender
                            .try_send(NetworkRequest::News(
                                news_schedule.generation,
                                Box::new(news.clone()),
                                settings.news_refresh.interval_seconds,
                                manual,
                            ))
                            .is_ok()
                        {
                            news_schedule.begin();
                            dirty = true;
                        }
                    }
                    // Local-only account validation also runs in manual mode: no HTTP or CLI.
                    if !probe_in_flight && last_probe.elapsed() >= Duration::from_secs(5) {
                        let scopes = sources
                            .iter()
                            .filter(|s| s.available())
                            .map(scope)
                            .collect::<Vec<_>>();
                        if !scopes.is_empty()
                            && rpc_sender
                                .try_send(NetworkRequest::Probes(quota_schedule.generation, scopes))
                                .is_ok()
                        {
                            probe_in_flight = true;
                            last_probe = Instant::now();
                        }
                    }
                    let quota_status =
                        quota_schedule.status(&settings.quota_refresh, now, quota_block);
                    let news_status = news_schedule.status(&settings.news_refresh, now, news_block);
                    if quota.request_status != quota_status || news.request_status != news_status {
                        dirty = true;
                    }
                    quota.request_status = quota_status.into();
                    quota.next_attempt = if settings.quota_refresh.mode == "auto" {
                        Some(quota_schedule.next.max(quota_block))
                    } else {
                        None
                    };
                    news.request_status = news_status.into();
                    news.display_next_attempt = if news_schedule.requested {
                        Some(news_block.max(now))
                    } else if settings.news_refresh.mode == "auto" {
                        Some(news_schedule.next.max(news_block))
                    } else if news_block > now {
                        Some(news_block)
                    } else {
                        None
                    };
                    let batch_start = Instant::now();
                    while batch_start.elapsed() < Duration::from_millis(250) {
                        let Some((id, path)) = queue.pop_front() else {
                            break;
                        };
                        queued.remove(&(id.clone(), path.clone()));
                        let Some(source) = sources.iter_mut().find(|s| s.health.id == id) else {
                            continue;
                        };
                        if !source.owns_path(&path) || !source.available() {
                            continue;
                        }
                        let result = if path
                            .file_name()
                            .is_some_and(|name| name == "session_index.jsonl")
                        {
                            pulse_core::collectors::jsonl::read_titles(&mut store, &id, &path)
                        } else {
                            read_batch(
                                &mut store,
                                &id,
                                &path,
                                settings.timezone.parse().unwrap_or(chrono_tz::UTC),
                                &prices,
                            )
                        };
                        match result {
                            Ok(report) => {
                                source.health.status = "connected".into();
                                if !report.unchanged {
                                    source.health.last_read = Some(Utc::now().to_rfc3339());
                                    source.health.issues += report.issues;
                                    dirty = true;
                                }
                                if report.more {
                                    enqueue(&mut queue, &mut queued, (id, path));
                                }
                            }
                            Err(_) => {
                                source.health.status = "read_error".into();
                                dirty = true;
                            }
                        }
                    }
                    // Small transactions on the existing single writer keep local ingestion,
                    // settings and queries responsive throughout historical rebuilding.
                    match store.step_reference_prices(&prices, 1024) {
                        Ok(n) if n > 0 => dirty = true,
                        Err(_) => dirty = true,
                        _ => {}
                    }
                    let rebuild_start = Instant::now();
                    while store.timezone_rebuild_status().is_some()
                        && !collector_stop.load(Ordering::Relaxed)
                        && rebuild_start.elapsed() < Duration::from_millis(8)
                    {
                        let result = store.step_timezone_rebuild(256).and_then(|progress| {
                            if progress.ready && !collector_stop.load(Ordering::Relaxed) {
                                let mut next = settings.clone();
                                next.timezone = progress.target_timezone;
                                store.commit_timezone_rebuild(&[(
                                    "app",
                                    serde_json::to_value(&next)?,
                                )])?;
                                settings = next;
                                last_publish = Instant::now() - Duration::from_secs(1);
                            }
                            Ok(())
                        });
                        dirty = true;
                        if result.is_err() {
                            let _ = store.cancel_timezone_rebuild();
                            timezone_error =
                                Some("时区重建失败，已保留有效统计；稍后自动重试。".into());
                            break;
                        }
                    }
                    let day = Utc::now()
                        .with_timezone(&settings.timezone.parse::<Tz>().unwrap_or(chrono_tz::UTC))
                        .date_naive();
                    if day != summary_day {
                        summary_day = day;
                        dirty = true;
                    }
                    if dirty && last_publish.elapsed() >= Duration::from_secs(1) {
                        let timezone = settings.timezone.parse::<Tz>().unwrap_or(chrono_tz::UTC);
                        let usage = store
                            .summary(Utc::now().with_timezone(&timezone).date_naive(), timezone);
                        let recent = store.recent_sessions(None, 10);
                        if let Ok(mut state) = shared.write() {
                            state.settings = settings.clone();
                            state.timezone_rebuild = store.timezone_rebuild_status();
                            state.timezone_error = timezone_error.clone();
                            state.news = inbox.view(news.view());
                            state.collecting = !queue.is_empty();
                            state.sources = sources.iter().map(|s| s.health.clone()).collect();
                            let live = quota
                                .sources
                                .iter()
                                .filter(|(id, q)| {
                                    sources.iter().any(|s| {
                                        &s.health.id == *id
                                            && s.health.path == q.home
                                            && s.available()
                                    })
                                })
                                .map(|(id, q)| {
                                    let mut q = q.clone();
                                    if !stamps.get(id).is_some_and(|stamp| {
                                        stamp.identity.is_some() && stamp.identity == q.identity
                                    }) {
                                        q.buckets.clear();
                                        q.allowance = None;
                                        q.profile = None;
                                        q.profile_last_success = None;
                                        q.profile_status = "verifying_account".into();
                                        if !stamps.contains_key(id) {
                                            q.status = "verifying_account".into();
                                        }
                                    }
                                    (id.clone(), q)
                                })
                                .collect::<BTreeMap<_, _>>();
                            state.quota = QuotaState {
                                buckets: pulse_core::quota::merged(
                                    live.values().flat_map(|q| q.buckets.clone()).collect(),
                                ),
                                sources: live,
                                request_status: quota.request_status.clone(),
                                last_success: quota.last_success.clone(),
                                next_attempt: quota.next_attempt,
                            };
                            match (usage, recent) {
                                (Ok(usage), Ok(recent)) => {
                                    state.usage = usage;
                                    state.recent = recent;
                                    state.error = None;
                                    state.updated_at = Some(Utc::now().to_rfc3339());
                                }
                                _ => state.error = Some("账本查询失败；请查看数据源状态".into()),
                            }
                        }
                        #[cfg(target_os = "macos")]
                        crate::macos::update_status(&app);
                        // Hidden windows receive no stream of state events; opening reads cache.
                        if app
                            .get_webview_window("pulse")
                            .is_some_and(|w| w.is_visible().unwrap_or(false))
                        {
                            let _ = app.emit("snapshot-changed", ());
                        }
                        last_publish = Instant::now();
                        dirty = false;
                    }
                }
            })?;
        Ok(Self {
            snapshot,
            sender,
            network_sender,
            stopping,
            pending_anchor,
            database: db,
        })
    }
    pub async fn translate(&self, id: &str) -> Result<String, String> {
        let keys = {
            let state = self.snapshot.read().map_err(|_| "消息暂不可用")?;
            let item = state
                .news
                .items
                .iter()
                .find(|item| item.id == id)
                .or_else(|| {
                    state
                        .news
                        .active_watch
                        .as_ref()
                        .map(|watch| &watch.item)
                        .filter(|item| item.id == id)
                })
                .ok_or("消息已更新，请刷新后重试")?;
            let mut keys = vec![item.id.clone()];
            if let Some(url) = item
                .source_url
                .as_deref()
                .and_then(|u| url::Url::parse(u).ok())
                && matches!(url.host_str(), Some("x.com" | "twitter.com"))
                && let Some(post) = url.path().strip_prefix("/thsottiaux/status/")
                && !post.is_empty()
                && post.len() <= 32
                && post.bytes().all(|b| b.is_ascii_digit())
            {
                keys.push(post.into());
            }
            keys
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.network_sender
            .try_send(NetworkRequest::Translation(keys, tx))
            .map_err(|_| "同步器繁忙，请稍后重试")?;
        tokio::time::timeout(Duration::from_secs(45), rx)
            .await
            .map_err(|_| "翻译查询超时，请重试")?
            .map_err(|_| "同步器已停止")?
    }
    pub fn remember_anchor(&self, anchor: crate::geometry::Anchor) {
        if let Ok(mut pending) = self.pending_anchor.lock() {
            *pending = Some((anchor, Instant::now()));
        }
    }
    pub fn shutdown(&self) {
        self.stopping.store(true, Ordering::Relaxed);
        let anchor = self
            .pending_anchor
            .lock()
            .ok()
            .and_then(|mut p| p.take().map(|(anchor, _)| anchor));
        if let Some(anchor) = anchor
            && let Ok(mut settings) = self.snapshot.read().map(|s| s.settings.clone())
        {
            settings.compact_anchor = Some(anchor);
            settings.compact_position = None;
            // A bounded SQLite write flushes the last drag even if the collector is exiting.
            if let Ok(store) = Store::open(&self.database) {
                let _ = store.set_setting("app", &settings);
            }
        }
    }
}
fn sync_system_timezone(
    store: &mut Store,
    published: &str,
    detected: Tz,
) -> Result<bool, pulse_core::storage::StoreError> {
    let stage = store.timezone_rebuild_status();
    if stage
        .as_ref()
        .is_some_and(|stage| stage.target_timezone == detected.name())
    {
        return Ok(false);
    }
    let changed = stage.is_some();
    if changed {
        store.cancel_timezone_rebuild()?;
    }
    if detected.name() == published {
        return Ok(changed);
    }
    store.begin_timezone_rebuild(detected)?;
    Ok(true)
}

fn initial_settings(store: &Store) -> Result<Settings, Box<dyn std::error::Error>> {
    let stored = store.setting::<serde_json::Value>("app")?;
    let has_timezone = stored
        .as_ref()
        .is_some_and(|value| value.get("timezone").is_some());
    let settings: Settings = stored
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    settings.validate().map_err(std::io::Error::other)?;
    // Persist the published calendar before collecting facts. Later OS changes
    // rebuild the calendar instead of mixing old and new date buckets.
    if !has_timezone {
        store.set_setting("app", &settings)?;
    }
    Ok(settings)
}

fn request_quota_refresh(schedule: &mut crate::refresh::Schedule, quota: &mut QuotaState) {
    // Explicit retries can recover client transport failures immediately. Preserve
    // server backoff (including Retry-After), authentication failures and single flight.
    for state in quota.sources.values_mut() {
        if matches!(state.status.as_str(), "network_error" | "body_read_error") {
            state.retry_at = 0;
        }
    }
    schedule.request();
}

fn persist_quota(
    store: &Store,
    quota: &mut QuotaState,
) -> Result<(), pulse_core::storage::StoreError> {
    quota.buckets = pulse_core::quota::merged(
        quota
            .sources
            .values()
            .flat_map(|s| s.buckets.clone())
            .collect(),
    );
    // Account profile remains in memory; never persist personal names/handles.
    let mut persisted = quota.clone();
    for source in persisted.sources.values_mut() {
        source.profile = None;
        source.profile_status.clear();
        source.profile_last_success = None;
    }
    store.set_setting("quota", &persisted)
}
fn scope(source: &Source) -> crate::http_quota::Scope {
    crate::http_quota::Scope {
        source_id: source.health.id.clone(),
        home: source.home.clone(),
        wsl: source.wsl.clone(),
    }
}
fn last_attempt(quota: &QuotaState) -> i64 {
    quota
        .sources
        .values()
        .filter_map(|s| {
            chrono::DateTime::parse_from_rfc3339(&s.last_attempt)
                .ok()
                .map(|d| d.timestamp())
        })
        .max()
        .unwrap_or(0)
}
fn reconcile_identity(
    quota: &mut QuotaState,
    scope: &crate::http_quota::Scope,
    stamp: &crate::http_quota::Stamp,
) {
    let state = quota.sources.entry(scope.source_id.clone()).or_default();
    let home = scope.home.to_string_lossy().into_owned();
    if state.home != home || state.identity != stamp.identity || stamp.identity.is_none() {
        state.home = home;
        state.identity = stamp.identity.clone();
        state.buckets.clear();
        state.allowance = None;
        state.profile = None;
        state.profile_status = "awaiting_refresh".into();
        state.profile_last_success = None;
        state.last_success = None;
        state.failures = 0;
        state.retry_at = 0;
        state.status = "awaiting_refresh".into();
    }
    if stamp.status != "ready" {
        state.allowance = None;
        state.status = stamp.status.clone();
        state.profile_status = stamp.status.clone();
        state.profile = None;
        state.profile_last_success = None;
    }
    state.proxy_source = stamp.proxy_source.clone();
    quota.last_success = quota
        .sources
        .values()
        .filter_map(|q| q.last_success.clone())
        .max();
}
fn source(id: String, label: String, home: PathBuf) -> Source {
    Source {
        health: SourceHealth {
            id,
            label,
            path: home.to_string_lossy().into_owned(),
            status: "discovering".into(),
            last_read: None,
            files: 0,
            issues: 0,
        },
        home,
        wsl: None,
    }
}
fn update_ui_preferences(
    store: &mut Store,
    current: &Settings,
    theme: Option<String>,
    language: Option<String>,
) -> Result<Settings, String> {
    let mut next = current.clone();
    if let Some(theme) = theme {
        next.theme = theme;
    }
    if let Some(language) = language {
        next.language = language;
    }
    next.validate()?;
    persist_settings(store, &next, false).map_err(|_| "无法保存设置".to_string())?;
    Ok(next)
}

fn persist_settings(
    store: &mut Store,
    next: &Settings,
    sources_changed: bool,
) -> Result<(), pulse_core::storage::StoreError> {
    if sources_changed {
        store.set_settings(&[
            ("quota", serde_json::to_value(QuotaState::default())?),
            ("app", serde_json::to_value(next)?),
        ])
    } else {
        store.set_setting("app", next)
    }
}
fn enqueue(
    queue: &mut VecDeque<(String, PathBuf)>,
    known: &mut HashSet<(String, PathBuf)>,
    item: (String, PathBuf),
) {
    if known.insert(item.clone()) {
        queue.push_back(item);
    }
}
fn enqueue_source(
    source: &mut Source,
    queue: &mut VecDeque<(String, PathBuf)>,
    known: &mut HashSet<(String, PathBuf)>,
) {
    let index = source.home.join("session_index.jsonl");
    if index.is_file() {
        enqueue(queue, known, (source.health.id.clone(), index));
    }
    source.health.files = 0;
    for folder in ["sessions", "archived_sessions"] {
        for entry in walkdir::WalkDir::new(source.home.join(folder))
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            if entry.file_type().is_file() && entry.path().extension().is_some_and(|e| e == "jsonl")
            {
                source.health.files += 1;
                enqueue(queue, known, (source.health.id.clone(), entry.into_path()));
            }
        }
    }
    if source.health.files == 0 {
        source.health.status = "no_logs".into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_timezone_changes_replace_stale_stages_and_publish_atomically() {
        let mut store = Store::in_memory().unwrap();
        let mut settings = Settings {
            timezone: "Asia/Shanghai".into(),
            quota_refresh: crate::refresh::Config {
                mode: "manual".into(),
                interval_seconds: 61,
            },
            news_refresh: crate::refresh::Config {
                mode: "auto".into(),
                interval_seconds: 907,
            },
            ..Settings::default()
        };
        store.set_setting("app", &settings).unwrap();
        assert!(
            !sync_system_timezone(&mut store, &settings.timezone, chrono_tz::Asia::Shanghai)
                .unwrap()
        );
        assert!(sync_system_timezone(&mut store, &settings.timezone, chrono_tz::UTC).unwrap());
        assert!(!sync_system_timezone(&mut store, &settings.timezone, chrono_tz::UTC).unwrap());
        assert!(
            sync_system_timezone(&mut store, &settings.timezone, chrono_tz::Asia::Tokyo).unwrap()
        );
        assert_eq!(
            store.timezone_rebuild_status().unwrap().target_timezone,
            "Asia/Tokyo"
        );
        assert_eq!(initial_settings(&store).unwrap().timezone, "Asia/Shanghai");
        assert!(
            sync_system_timezone(&mut store, &settings.timezone, chrono_tz::Asia::Shanghai)
                .unwrap()
        );
        assert!(store.timezone_rebuild_status().is_none());
        assert!(sync_system_timezone(&mut store, &settings.timezone, chrono_tz::UTC).unwrap());
        let progress = store.step_timezone_rebuild(256).unwrap();
        assert!(progress.ready);
        settings.timezone = progress.target_timezone;
        store
            .commit_timezone_rebuild(&[("app", serde_json::to_value(&settings).unwrap())])
            .unwrap();
        let published = initial_settings(&store).unwrap();
        assert_eq!(published.timezone, "UTC");
        assert_eq!(published.quota_refresh, settings.quota_refresh);
        assert_eq!(published.news_refresh, settings.news_refresh);
        assert!(!sync_system_timezone(&mut store, &published.timezone, chrono_tz::UTC).unwrap());
    }
    #[test]
    fn bootstrap_persists_calendar_and_restart_keeps_published_zone_until_rebuild() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("pulse.sqlite");
        {
            let store = Store::open(&database).unwrap();
            store
                .set_setting("app", &serde_json::json!({"theme":"light"}))
                .unwrap();
            let settings = initial_settings(&store).unwrap();
            let saved: Settings = store.setting("app").unwrap().unwrap();
            assert_eq!(saved.timezone, settings.timezone);
            assert_eq!(saved.theme, "light");
            let explicit = Settings {
                timezone: "UTC".into(),
                ..settings
            };
            store.set_setting("app", &explicit).unwrap();
        }
        let store = Store::open(&database).unwrap();
        assert_eq!(initial_settings(&store).unwrap().timezone, "UTC");
    }
    #[test]
    fn refresh_configs_survive_restart_and_account_switch_clears_every_cache_view() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("pulse.sqlite");
        let settings = Settings {
            quota_refresh: crate::refresh::Config {
                mode: "manual".into(),
                interval_seconds: 61,
            },
            news_refresh: crate::refresh::Config {
                mode: "auto".into(),
                interval_seconds: 907,
            },
            ..Settings::default()
        };
        {
            let store = Store::open(&database).unwrap();
            store.set_setting("app", &settings).unwrap();
        }
        let store = Store::open(&database).unwrap();
        let reloaded: Settings = store.setting("app").unwrap().unwrap();
        assert_eq!(reloaded.quota_refresh, settings.quota_refresh);
        assert_eq!(reloaded.news_refresh, settings.news_refresh);
        let scope = crate::http_quota::Scope {
            source_id: "windows".into(),
            home: PathBuf::from("C:/test"),
            wsl: None,
        };
        let stamp = crate::http_quota::Stamp {
            identity: Some("account-a-hash".into()),
            status: "ready".into(),
            ..Default::default()
        };
        let mut quota = QuotaState::default();
        reconcile_identity(&mut quota, &scope, &stamp);
        let bucket = pulse_core::quota::QuotaBucket {
            source_id: "windows".into(),
            identity_key: "account-a-hash".into(),
            identity_confirmed: true,
            limit_id: "codex".into(),
            name: "Codex".into(),
            plan: None,
            primary: None,
            secondary: None,
            captured_at: "2026-10-06T00:00:00Z".into(),
        };
        quota
            .sources
            .get_mut("windows")
            .unwrap()
            .buckets
            .push(bucket);
        quota.sources.get_mut("windows").unwrap().last_success = Some("old success".into());
        let state = quota.sources.get_mut("windows").unwrap();
        state.profile = Some(crate::account_profile::AccountProfile {
            display_name: Some("Private test name".into()),
            ..Default::default()
        });
        state.profile_last_success = Some("old profile success".into());
        state.allowance = Some(crate::account_allowance::AccountAllowance {
            balance: Some(42.75),
            reset_cards: Some(2),
            ..Default::default()
        });

        persist_quota(&store, &mut quota).unwrap();
        assert_eq!(quota.buckets.len(), 1);
        assert!(quota.sources["windows"].profile.is_some());
        let persisted: serde_json::Value = store.setting("quota").unwrap().unwrap();
        assert!(!persisted.to_string().contains("Private test name"));
        let cached: QuotaState = store.setting("quota").unwrap().unwrap();
        assert!(cached.sources["windows"].profile.is_none());
        assert_eq!(
            cached.sources["windows"]
                .allowance
                .as_ref()
                .unwrap()
                .balance,
            Some(42.75)
        );

        let switched = crate::http_quota::Stamp {
            identity: Some("account-b-hash".into()),
            status: "ready".into(),
            ..Default::default()
        };
        reconcile_identity(&mut quota, &scope, &switched);
        assert!(quota.sources["windows"].profile.is_none());
        assert!(quota.sources["windows"].profile_last_success.is_none());
        assert!(quota.sources["windows"].allowance.is_none());
        persist_quota(&store, &mut quota).unwrap();
        let saved: QuotaState = store.setting("quota").unwrap().unwrap();
        assert!(saved.buckets.is_empty());
        assert!(saved.sources["windows"].buckets.is_empty());
        assert!(saved.sources["windows"].last_success.is_none());
        assert_eq!(
            saved.sources["windows"].identity.as_deref(),
            Some("account-b-hash")
        );
    }
    #[test]
    fn explicit_retry_recovers_transport_but_keeps_server_backoff_and_single_flight() {
        let mut quota = QuotaState::default();
        for (id, status) in [
            ("transport", "network_error"),
            ("limited", "http_429"),
            ("auth", "reauth_required"),
        ] {
            quota.sources.insert(
                id.into(),
                QuotaSource {
                    status: status.into(),
                    retry_at: 900,
                    ..Default::default()
                },
            );
        }
        let mut schedule = crate::refresh::Schedule::new();
        request_quota_refresh(&mut schedule, &mut quota);
        assert_eq!(quota.sources["transport"].retry_at, 0);
        assert_eq!(quota.sources["limited"].retry_at, 900);
        assert_eq!(quota.sources["auth"].retry_at, 900);
        assert!(schedule.requested);
        schedule.begin();
        request_quota_refresh(&mut schedule, &mut quota);
        assert!(!schedule.requested);
    }
    #[test]
    fn legacy_settings_keep_defaults_and_source_drafts_roundtrip() {
        let mut settings: Settings =
            serde_json::from_str(r#"{"theme":"light","windows_home":null}"#).unwrap();
        assert!(settings.wsl_auto_detect);
        assert_eq!(settings.language, "system");
        assert!(!settings.hide_titles);
        settings.wsl_sources.push(crate::source_config::WslSource {
            id: "manual".into(),
            distro: "Ubuntu".into(),
            user: "dev".into(),
            home: "/home/dev/.codex".into(),
            enabled: false,
        });
        assert!(settings.validate().is_ok());
        let decoded: Settings =
            serde_json::from_value(serde_json::to_value(&settings).unwrap()).unwrap();
        assert_eq!(decoded.wsl_sources, settings.wsl_sources);
        settings.language = "en".into();
        let decoded: Settings =
            serde_json::from_value(serde_json::to_value(&settings).unwrap()).unwrap();
        assert_eq!(decoded.language, "en");
        assert!(decoded.validate().is_ok());
        settings.language = "fr".into();
        assert!(settings.validate().is_err());
    }
    #[test]
    fn hiding_names_removes_ipc_values_without_changing_statistics_or_internal_titles() {
        for (titles, projects) in [(true, false), (false, true), (true, true), (false, false)] {
            let mut snapshot = Snapshot::default();
            snapshot.settings.hide_titles = titles;
            snapshot.settings.hide_projects = projects;
            snapshot.recent.push(RecentSession {
                meta: pulse_core::domain::SessionMeta {
                    id: "s".into(),
                    title: Some("Secret title".into()),
                    project: Some("Secret project".into()),
                    ..Default::default()
                },
                sources: vec!["windows".into()],
                models: vec!["model".into()],
                total: 123,
                events: 1,
                unknown_totals: 0,
                cost_nanousd: 789,
                unpriced_tokens: 0,
                reference: Default::default(),
            });
            let display = snapshot.for_display();
            let json = serde_json::to_string(&display).unwrap();
            assert_eq!(json.contains("Secret title"), !titles);
            assert_eq!(json.contains("Secret project"), !projects);
            assert_eq!(display.recent[0].total, 123);
            assert_eq!(display.recent[0].cost_nanousd, 789);
            assert_eq!(
                snapshot.recent[0].meta.title.as_deref(),
                Some("Secret title")
            );
            assert_eq!(
                snapshot.recent[0].meta.project.as_deref(),
                Some("Secret project")
            );
        }
    }
    #[test]
    fn source_changes_reject_stale_roots_users_and_non_rollout_notifications() {
        let home = PathBuf::from(r"C:\test\.codex");
        let mut source = source("test".into(), "Test".into(), home.clone());
        assert!(source.owns_path(&home.join("session_index.jsonl")));
        assert!(source.owns_path(&home.join("sessions/2026/test.jsonl")));
        assert!(!source.owns_path(&home.join("other.jsonl")));
        assert!(!source.owns_path(&home.join("auth.json")));
        assert!(!source.owns_path(&PathBuf::from(r"C:\old\sessions\test.jsonl")));
        let target = crate::source_config::WslTarget {
            distro: "Ubuntu".into(),
            user: "dev".into(),
        };
        source.wsl = Some(target.clone());
        assert!(source.scope_matches("test", &home, Some(&target)));
        let mut changed = target;
        changed.user = "other".into();
        assert!(!source.scope_matches("test", &home, Some(&changed)));
        for status in ["wsl_stopped", "wsl_unavailable"] {
            source.health.status = status.into();
            assert!(!source.available());
        }
        source.health.status = "connected".into();
        assert!(source.available());
    }
    #[test]
    fn ui_preferences_persist_without_restoring_source_drafts_or_clearing_quota() {
        let mut store = Store::in_memory().unwrap();
        store
            .set_setting("quota", &serde_json::json!({"test":"cached"}))
            .unwrap();
        let settings = Settings {
            hide_titles: true,
            windows_enabled: false,
            ..Default::default()
        };
        let next = update_ui_preferences(
            &mut store,
            &settings,
            Some("light".into()),
            Some("en".into()),
        )
        .unwrap();
        let saved = store.setting::<Settings>("app").unwrap().unwrap();
        assert_eq!(saved.theme, "light");
        assert_eq!(saved.language, "en");
        assert!(saved.hide_titles);
        assert!(!saved.windows_enabled);
        assert_eq!(
            store
                .setting::<serde_json::Value>("quota")
                .unwrap()
                .unwrap()["test"],
            "cached"
        );
        assert!(
            update_ui_preferences(&mut store, &next, Some("invalid".into()), Some("zh".into()))
                .is_err()
        );
        assert_eq!(
            store.setting::<Settings>("app").unwrap().unwrap().language,
            "en"
        );
        let next = update_ui_preferences(&mut store, &next, None, Some("system".into())).unwrap();
        assert_eq!(next.theme, "light");
        assert_eq!(next.language, "system");
    }

    #[test]
    fn appearance_changes_keep_quota_while_source_switches_invalidate_the_persisted_cache() {
        let mut store = Store::in_memory().unwrap();
        store
            .set_setting("quota", &serde_json::json!({"test":"old account"}))
            .unwrap();
        let mut settings = Settings {
            accent: "violet".into(),
            ..Default::default()
        };
        persist_settings(&mut store, &settings, false).unwrap();
        assert_eq!(
            store
                .setting::<serde_json::Value>("quota")
                .unwrap()
                .unwrap()["test"],
            "old account"
        );
        settings.wsl_auto_detect = false;
        persist_settings(&mut store, &settings, true).unwrap();
        assert!(
            store
                .setting::<QuotaState>("quota")
                .unwrap()
                .unwrap()
                .sources
                .is_empty()
        );
        assert!(
            !store
                .setting::<Settings>("app")
                .unwrap()
                .unwrap()
                .wsl_auto_detect
        );
    }
}
