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
        Arc, Condvar, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};

const LIVE_INTERVAL: Duration = Duration::from_millis(250);

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
            theme: "system".into(),
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
    FilesChanged,
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
    ModelSpeed(
        pulse_core::storage::ModelSpeedRequest,
        tokio::sync::oneshot::Sender<Result<pulse_core::storage::ModelSpeed, String>>,
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
    public_scopes: Arc<RwLock<Vec<crate::http_quota::Scope>>>,
    routing_ready: Arc<(Mutex<bool>, Condvar)>,
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
        let backend_public_scopes = public_scopes.clone();
        let routing_ready = Arc::new((Mutex::new(false), Condvar::new()));
        let collector_routing_ready = routing_ready.clone();
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
        let watcher_sender = sender.clone();
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
                        if let Ok(event) = event {
                            queue_watcher_event(&watcher_paths, &watcher_sender, event);
                        }
                    })
                    .ok();
                let mut speed_queue = VecDeque::<(String, PathBuf)>::new();
                let mut speed_queued = HashSet::<(String, PathBuf)>::new();
                let mut queue = VecDeque::<(String, PathBuf)>::new();
                let mut queued = HashSet::<(String, PathBuf)>::new();
                let mut last_repair = Instant::now() - Duration::from_secs(60);
                let mut last_wsl = Instant::now() - Duration::from_secs(60);
                let mut wsl_home_cache = BTreeMap::<String, PathBuf>::new();
                let mut last_publish = Instant::now() - Duration::from_secs(1);
                let mut last_live = Instant::now() - LIVE_INTERVAL;
                let mut live_dirty = false;
                #[cfg(target_os = "macos")]
                let mut last_status = Instant::now() - LIVE_INTERVAL;
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
                        Duration::from_millis(10)
                    } else if live_dirty {
                        LIVE_INTERVAL
                            .saturating_sub(last_live.elapsed())
                            .min(Duration::from_millis(300))
                    } else {
                        Duration::from_millis(300)
                    };
                    match receiver.recv_timeout(wait) {
                        Ok(Message::FilesChanged) => {}
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
                        Ok(Message::ModelSpeed(request, reply)) => {
                            let connected = sources
                                .iter()
                                .filter(|s| s.health.status == "connected")
                                .map(|s| s.health.id.clone())
                                .collect::<Vec<_>>();
                            let result = settings
                                .timezone
                                .parse::<Tz>()
                                .map_err(|_| "统计时区无效".to_string())
                                .and_then(|tz| {
                                    store
                                        .model_speed(&request, tz, Utc::now(), &connected)
                                        .map(|mut result| {
                                            result.history_pending =
                                                !queue.is_empty() || !speed_queue.is_empty();
                                            result
                                        })
                                        .map_err(|_| "无法读取运行速度，请重试".into())
                                });
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
                                        .and_then(|mut page| {
                                            let connected = sources
                                                .iter()
                                                .filter(|s| s.health.status == "connected")
                                                .map(|s| s.health.id.clone())
                                                .collect::<Vec<_>>();
                                            let now = Utc::now();
                                            for row in &mut page.items {
                                                row.speed = store
                                                    .model_speed(
                                                        &pulse_core::storage::ModelSpeedRequest {
                                                            model: row.usage.model.clone(),
                                                            from_day: request.from_day.clone(),
                                                            through_day: request
                                                                .through_day
                                                                .clone(),
                                                            range: Default::default(),
                                                        },
                                                        tz,
                                                        now,
                                                        &connected,
                                                    )?
                                                    .summary;
                                            }
                                            page.speed_pending =
                                                !queue.is_empty() || !speed_queue.is_empty();
                                            Ok(page)
                                        })
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
                                    probes_changed |= accept_probe(
                                        &mut quota,
                                        &mut stamps,
                                        &mut quota_schedule,
                                        &scope,
                                        stamp,
                                    );
                                    quota_epoch.store(quota_schedule.generation, Ordering::Relaxed);
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
                                    apply_profile_result(state, response.profile);
                                    apply_quota_result(
                                        state,
                                        response.result,
                                        now,
                                        &mut quota_schedule,
                                    );
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
                                    speed_queue.clear();
                                    speed_queued.clear();
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
                    let mut credential_scopes = BTreeMap::new();
                    if let Ok(mut paths) = changed.lock() {
                        for path in paths.drain(..) {
                            for source in sources.iter().filter(|s| {
                                s.wsl.is_none()
                                    && ["auth.json", "config.toml", ".env"]
                                        .iter()
                                        .any(|f| path == s.home.join(f))
                            }) {
                                credential_scopes.insert(source.health.id.clone(), scope(source));
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
                    // Local file notifications are hints, not proof of an account
                    // switch. Probe once per affected local source, outside the watcher
                    // lock, before publishing: same-account renewal keeps the cache;
                    // a different/unknown identity is still isolated immediately.
                    let mut credentials_changed = false;
                    for scope in credential_scopes.values() {
                        credentials_changed |= accept_probe(
                            &mut quota,
                            &mut stamps,
                            &mut quota_schedule,
                            scope,
                            crate::http_quota::probe(scope),
                        );
                    }
                    if credentials_changed {
                        quota_epoch.store(quota_schedule.generation, Ordering::Relaxed);
                        let _ = persist_quota(&store, &mut quota);
                        dirty = true;
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
                        if let Ok(mut ready) = collector_routing_ready.0.lock()
                            && !*ready
                        {
                            *ready = true;
                            collector_routing_ready.1.notify_all();
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
                        let titles_only = path
                            .file_name()
                            .is_some_and(|name| name == "session_index.jsonl");
                        let result = if titles_only {
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
                                    live_dirty |= !titles_only && report.records > 0;
                                }
                                if !titles_only {
                                    enqueue(
                                        &mut speed_queue,
                                        &mut speed_queued,
                                        (id.clone(), path.clone()),
                                    );
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
                    // An independent 8 ms / 64-record replay cursor leaves the usage ledger intact.
                    let speed_start = Instant::now();
                    while speed_start.elapsed() < Duration::from_millis(8) {
                        let Some((id, path)) = speed_queue.pop_front() else {
                            break;
                        };
                        speed_queued.remove(&(id.clone(), path.clone()));
                        if sources.iter().any(|source| {
                            source.health.id == id && source.available() && source.owns_path(&path)
                        }) && let Ok(report) =
                            pulse_core::collectors::jsonl::read_rates(&mut store, &id, &path)
                        {
                            dirty |= report.records > 0;
                            if report.more {
                                enqueue(&mut speed_queue, &mut speed_queued, (id, path));
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
                    let mut live_changed = false;
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
                        // Hidden windows receive no stream of state events; opening reads cache.
                        if app
                            .get_webview_window("pulse")
                            .is_some_and(|w| w.is_visible().unwrap_or(false))
                        {
                            let _ = app.emit("snapshot-changed", ());
                        }
                        last_publish = Instant::now();
                        dirty = false;
                        live_dirty = false;
                        last_live = Instant::now();
                    } else if live_dirty && last_live.elapsed() >= LIVE_INTERVAL {
                        let ids = shared
                            .read()
                            .map(|state| {
                                state
                                    .recent
                                    .iter()
                                    .map(|s| s.meta.id.clone())
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        if let Ok(activity) = store.session_activity(&ids)
                            && let Ok(mut state) = shared.write()
                        {
                            live_changed = update_live_sessions(&mut state, activity);
                            if live_changed {
                                state.sources = sources.iter().map(|s| s.health.clone()).collect();
                            }
                        }
                        if live_changed
                            && app
                                .get_webview_window("pulse")
                                .is_some_and(|w| w.is_visible().unwrap_or(false))
                        {
                            let _ = app.emit("snapshot-changed", ());
                        }
                        live_dirty = false;
                        last_live = Instant::now();
                    }
                    #[cfg(target_os = "macos")]
                    if live_changed || last_status.elapsed() >= LIVE_INTERVAL {
                        // Existing collector clock: activity/rate expiry still
                        // updates the native menu bar with no visible WebView.
                        crate::macos::update_status(&app);
                        last_status = Instant::now();
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
            public_scopes: backend_public_scopes,
            routing_ready,
        })
    }
    // Called only on blocking workers. Startup must not race first source discovery
    // and silently use a direct route before Codex's .env context is available.
    pub fn public_proxy_scopes(&self) -> Result<Vec<crate::http_quota::Scope>, String> {
        let ready = self.routing_ready.0.lock().map_err(|_| "代理来源不可用")?;
        let (ready, _) = self
            .routing_ready
            .1
            .wait_timeout_while(ready, Duration::from_secs(5), |ready| !*ready)
            .map_err(|_| "代理来源不可用")?;
        if !*ready {
            return Err("代理来源尚未就绪".into());
        }
        self.public_scopes
            .read()
            .map(|scopes| scopes.clone())
            .map_err(|_| "代理来源不可用".into())
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
fn apply_profile_result(
    state: &mut QuotaSource,
    result: Result<crate::account_profile::AccountProfile, crate::http_quota::Failure>,
) {
    match result {
        Ok(profile) => {
            state.profile_status = "connected".into();
            state.profile_last_success = Some(state.last_attempt.clone());
            state.profile = Some(profile);
        }
        Err(error) => {
            state.profile_status = error.code.clone();
            if [
                "not_signed_in",
                "account_mismatch",
                "credentials_unreadable",
                "credentials_invalid",
            ]
            .contains(&error.code.as_str())
            {
                state.profile = None;
                state.profile_last_success = None;
            }
        }
    }
}
fn apply_quota_result(
    state: &mut QuotaSource,
    result: Result<crate::http_quota::Readout, crate::http_quota::Failure>,
    now: i64,
    schedule: &mut crate::refresh::Schedule,
) {
    match result {
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
            if error.code == "credentials_changed" {
                // Retry with the new credentials, without waiving an existing wait.
                schedule.next = 0;
            } else {
                state.failures = state.failures.saturating_add(1);
                let backoff = (30_i64 * (1_i64 << state.failures.min(6))).min(1800);
                state.retry_at = state
                    .retry_at
                    .max(now.saturating_add(backoff.max(error.retry_after)));
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
// Used by periodic probes and coalesced local file notifications. No HTTP here.
fn accept_probe(
    quota: &mut QuotaState,
    stamps: &mut BTreeMap<String, crate::http_quota::Stamp>,
    schedule: &mut crate::refresh::Schedule,
    scope: &crate::http_quota::Scope,
    stamp: crate::http_quota::Stamp,
) -> bool {
    let previous = stamps.get(&scope.source_id);
    if previous == Some(&stamp) {
        return false;
    }
    let changed = previous.is_some();
    let recovered = reconcile_probe(quota, scope, &stamp, previous);
    stamps.insert(scope.source_id.clone(), stamp);
    if changed || recovered {
        schedule.invalidate(0);
    }
    true
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
        // Identity reconciliation above already clears switched/unknown accounts.
        // An expired token for this same account does not invalidate old values.
        // Keep the reason for a server/transport wait across local expiry;
        // otherwise renewal would misclassify it as authentication backoff.
        let keep_wait = stamp.status == "credentials_expired"
            && state.retry_at > 0
            && !["credentials_expired", "reauth_required"].contains(&state.status.as_str());
        if !keep_wait {
            state.status = stamp.status.clone();
        }
        state.profile_status = stamp.status.clone();
    }
    state.proxy_source = stamp.proxy_source.clone();
    quota.last_success = quota
        .sources
        .values()
        .filter_map(|q| q.last_success.clone())
        .max();
}
// A successful local probe can recover a stale authentication failure without
// issuing HTTP. Same-account renewal must release only authentication backoff;
// a new token does not waive a server's 429 / Retry-After or transport backoff.
fn reconcile_probe(
    quota: &mut QuotaState,
    scope: &crate::http_quota::Scope,
    stamp: &crate::http_quota::Stamp,
    previous: Option<&crate::http_quota::Stamp>,
) -> bool {
    let credentials_changed = previous.is_some_and(|old| old.revision != stamp.revision);
    reconcile_identity(quota, scope, stamp);
    if stamp.status != "ready" {
        return false;
    }
    let recovered = |status: &str| {
        status == "credentials_expired" || (status == "reauth_required" && credentials_changed)
    };
    let state = quota.sources.get_mut(&scope.source_id).unwrap();
    let quota_recovered = recovered(&state.status);
    let profile_recovered = recovered(&state.profile_status);
    if quota_recovered {
        state.status = "awaiting_refresh".into();
        state.failures = 0;
        state.retry_at = 0;
    }
    if profile_recovered {
        state.profile_status = "awaiting_refresh".into();
    }
    quota_recovered || profile_recovered
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

fn queue_watcher_event(
    paths: &Mutex<Vec<PathBuf>>,
    sender: &SyncSender<Message>,
    event: notify::Event,
) {
    // Reading credentials/logs must not cause our own watcher to invalidate them.
    let write_close = matches!(
        event.kind,
        notify::EventKind::Access(notify::event::AccessKind::Close(
            notify::event::AccessMode::Write
        ))
    );
    if write_close || !matches!(event.kind, notify::EventKind::Access(_)) {
        queue_file_events(paths, sender, event.paths);
    }
}
fn queue_file_events(
    paths: &Mutex<Vec<PathBuf>>,
    sender: &SyncSender<Message>,
    incoming: Vec<PathBuf>,
) {
    if let Ok(mut paths) = paths.lock() {
        let was_empty = paths.is_empty();
        let remaining = 4096usize.saturating_sub(paths.len());
        paths.extend(incoming.into_iter().take(remaining));
        if was_empty && !paths.is_empty() {
            // A full inbox already wakes the collector; paths remain buffered.
            // Only the first event in a burst needs a wakeup, never a new thread.
            let _ = sender.try_send(Message::FilesChanged);
        }
    }
}

fn update_live_sessions(
    state: &mut Snapshot,
    activity: Vec<pulse_core::domain::SessionMeta>,
) -> bool {
    let mut changed = false;
    for next in activity {
        if let Some(session) = state.recent.iter_mut().find(|s| s.meta.id == next.id) {
            let previous = &mut session.meta;
            if (
                &previous.status,
                &previous.last_activity,
                &previous.current_model,
                &previous.output_rate,
            ) != (
                &next.status,
                &next.last_activity,
                &next.current_model,
                &next.output_rate,
            ) {
                previous.status = next.status;
                previous.last_activity = next.last_activity;
                previous.current_model = next.current_model;
                previous.output_rate = next.output_rate;
                changed = true;
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_events_wake_once_per_burst_and_keep_paths_when_inbox_is_full() {
        let paths = Mutex::new(Vec::new());
        let (sender, receiver) = mpsc::sync_channel(1);
        queue_file_events(&paths, &sender, vec![]);
        assert!(receiver.try_recv().is_err());
        queue_file_events(&paths, &sender, vec!["a.jsonl".into(), "b.jsonl".into()]);
        queue_file_events(&paths, &sender, vec!["c.jsonl".into()]);
        assert!(matches!(receiver.try_recv(), Ok(Message::FilesChanged)));
        assert!(receiver.try_recv().is_err());
        assert_eq!(paths.lock().unwrap().len(), 3);
        paths.lock().unwrap().clear();
        sender.try_send(Message::FilesChanged).unwrap();
        queue_file_events(&paths, &sender, vec!["d.jsonl".into()]);
        assert_eq!(paths.lock().unwrap().as_slice(), [PathBuf::from("d.jsonl")]);
        assert!(matches!(receiver.try_recv(), Ok(Message::FilesChanged)));
        paths.lock().unwrap().clear();
        queue_file_events(
            &paths,
            &sender,
            (0..5000)
                .map(|n| PathBuf::from(format!("{n}.jsonl")))
                .collect(),
        );
        assert_eq!(paths.lock().unwrap().len(), 4096);
    }

    #[test]
    fn fast_activity_updates_preserve_titles_totals_and_network_state() {
        use pulse_core::domain::{OutputRate, SessionMeta};
        let mut state = Snapshot::default();
        state.usage.total = 800;
        state.quota.request_status = "paused".into();
        state.updated_at = Some("summary timestamp".into());
        state.recent.push(RecentSession {
            meta: SessionMeta {
                id: "main".into(),
                title: Some("Indexed title".into()),
                project: Some("Project".into()),
                status: "active".into(),
                ..Default::default()
            },
            sources: vec!["macos".into()],
            models: vec![],
            total: 120,
            events: 1,
            unknown_totals: 0,
            cost_nanousd: 12345,
            unpriced_tokens: 0,
            reference: Default::default(),
        });
        let next = SessionMeta {
            id: "main".into(),
            status: "active".into(),
            last_activity: "2026-10-07T10:00:01Z".into(),
            current_model: Some("gpt-6.1-sol".into()),
            output_rate: Some(OutputRate {
                output_tokens: 100,
                elapsed_ms: 2500,
                measured_at: "2026-10-07T10:00:01Z".into(),
                completed: false,
                service_tier: None,
            }),
            ..Default::default()
        };
        assert!(update_live_sessions(&mut state, vec![next.clone()]));
        assert!(!update_live_sessions(&mut state, vec![next]));
        assert_eq!(
            state.recent[0]
                .meta
                .output_rate
                .as_ref()
                .unwrap()
                .output_tokens,
            100
        );
        assert_eq!(state.recent[0].meta.title.as_deref(), Some("Indexed title"));
        assert_eq!(state.recent[0].meta.project.as_deref(), Some("Project"));
        assert_eq!(state.recent[0].total, 120);
        assert_eq!(state.recent[0].cost_nanousd, 12345);
        assert_eq!(state.usage.total, 800);
        assert_eq!(state.quota.request_status, "paused");
        assert_eq!(state.updated_at.as_deref(), Some("summary timestamp"));
        assert!(!update_live_sessions(
            &mut state,
            vec![SessionMeta {
                id: "not displayed".into(),
                ..Default::default()
            }]
        ));
        assert_eq!(state.recent.len(), 1);
        assert!(update_live_sessions(
            &mut state,
            vec![SessionMeta {
                id: "main".into(),
                status: "completed".into(),
                ..Default::default()
            }]
        ));
        assert!(state.recent[0].meta.output_rate.is_none());
    }

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
    fn cache_fixture(
        scope: &crate::http_quota::Scope,
        stamp: &crate::http_quota::Stamp,
    ) -> QuotaState {
        let mut quota = QuotaState::default();
        reconcile_identity(&mut quota, scope, stamp);
        let state = quota.sources.get_mut(&scope.source_id).unwrap();
        state.status = "connected".into();
        state.last_success = Some("2026-10-08T03:00:00Z".into());
        state.buckets.push(pulse_core::quota::QuotaBucket {
            source_id: scope.source_id.clone(),
            identity_key: stamp.identity.clone().unwrap(),
            identity_confirmed: true,
            limit_id: "codex".into(),
            name: "Codex".into(),
            plan: Some("pro".into()),
            primary: None,
            secondary: None,
            captured_at: "2026-10-08T03:00:00Z".into(),
        });
        state.allowance = Some(crate::account_allowance::AccountAllowance {
            balance: Some(12345.0),
            reset_cards: Some(2),
            ..Default::default()
        });
        state.profile = Some(crate::account_profile::AccountProfile {
            display_name: Some("Cached account".into()),
            ..Default::default()
        });
        state.profile_status = "connected".into();
        state.profile_last_success = state.last_success.clone();
        quota
    }
    fn write_test_auth(home: &std::path::Path, account: &str, revision: &str) {
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        let claims = serde_json::json!({"sub":"user", "exp":4102444800_i64, "jti":revision});
        let token = format!("header.{}.sig", URL_SAFE_NO_PAD.encode(claims.to_string()));
        std::fs::write(
            home.join("auth.json"),
            serde_json::json!({"tokens":{
                "access_token":token, "account_id":account
            }})
            .to_string(),
        )
        .unwrap();
    }
    #[test]
    fn watched_auth_renewal_preserves_cache_and_server_wait_until_verified_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let scope = crate::http_quota::Scope {
            source_id: "local".into(),
            home: directory.path().into(),
            wsl: None,
        };
        write_test_auth(directory.path(), "account-a", "old-token");
        let stamp = crate::http_quota::probe(&scope);
        assert_eq!(stamp.status, "ready");
        let mut quota = cache_fixture(&scope, &stamp);
        let state = quota.sources.get_mut(&scope.source_id).unwrap();
        state.status = "http_429".into();
        state.failures = 2;
        state.retry_at = 3600;
        let before = serde_json::to_value(&quota).unwrap();
        let mut stamps = BTreeMap::from([(scope.source_id.clone(), stamp)]);
        let mut schedule = crate::refresh::Schedule::new();
        let flight = schedule.begin();
        // A touch/duplicate notification must not invalidate a flight or any cache.
        assert!(!accept_probe(
            &mut quota,
            &mut stamps,
            &mut schedule,
            &scope,
            crate::http_quota::probe(&scope)
        ));
        assert_eq!(schedule.generation, flight);
        write_test_auth(directory.path(), "account-a", "renewed-token");
        assert!(accept_probe(
            &mut quota,
            &mut stamps,
            &mut schedule,
            &scope,
            crate::http_quota::probe(&scope)
        ));
        assert_eq!(
            serde_json::to_value(&quota).unwrap()["sources"],
            before["sources"]
        );
        assert_eq!(schedule.flight, Some(flight));
        assert!(!schedule.due_for_account(
            &crate::refresh::Config::default(),
            4000,
            3600,
            true,
            false
        ));
        assert!(!schedule.finish(flight, 4300));
        assert!(!schedule.due_for_account(
            &crate::refresh::Config::default(),
            3599,
            3600,
            true,
            false
        ));
        assert!(!schedule.due_for_account(
            &crate::refresh::Config::default(),
            4000,
            3600,
            false,
            false
        ));
        assert!(schedule.due_for_account(
            &crate::refresh::Config::default(),
            4000,
            3600,
            true,
            false
        ));
        // A real account change and logout must still discard every old value.
        write_test_auth(directory.path(), "account-b", "new-account");
        assert!(accept_probe(
            &mut quota,
            &mut stamps,
            &mut schedule,
            &scope,
            crate::http_quota::probe(&scope)
        ));
        let state = &quota.sources[&scope.source_id];
        assert!(
            state.buckets.is_empty()
                && state.allowance.is_none()
                && state.profile.is_none()
                && state.last_success.is_none()
        );
        quota = cache_fixture(&scope, &stamps[&scope.source_id]);
        std::fs::write(directory.path().join("auth.json"), "{}").unwrap();
        assert!(accept_probe(
            &mut quota,
            &mut stamps,
            &mut schedule,
            &scope,
            crate::http_quota::probe(&scope)
        ));
        let state = &quota.sources[&scope.source_id];
        assert!(
            state.identity.is_none()
                && state.buckets.is_empty()
                && state.allowance.is_none()
                && state.profile.is_none()
        );
    }
    #[test]
    fn same_account_expiry_and_failed_refresh_keep_last_success_until_new_success() {
        let scope = crate::http_quota::Scope {
            source_id: "local".into(),
            home: "test-home".into(),
            wsl: None,
        };
        let ready = crate::http_quota::Stamp {
            identity: Some("same-account".into()),
            status: "ready".into(),
            ..Default::default()
        };
        let mut quota = cache_fixture(&scope, &ready);
        let expired = crate::http_quota::Stamp {
            status: "credentials_expired".into(),
            ..ready.clone()
        };
        reconcile_identity(&mut quota, &scope, &expired);
        let state = quota.sources.get_mut(&scope.source_id).unwrap();
        assert!(state.profile.is_some() && state.allowance.is_some() && state.buckets.len() == 1);
        let saved = serde_json::to_value(&*state).unwrap();
        let mut schedule = crate::refresh::Schedule::new();
        for code in [
            "network_error",
            "http_429",
            "reauth_required",
            "credentials_changed",
        ] {
            apply_profile_result(state, Err(code.into()));
            apply_quota_result(
                state,
                Err(crate::http_quota::Failure {
                    code: code.into(),
                    retry_after: 3600,
                }),
                1000,
                &mut schedule,
            );
            let actual = serde_json::to_value(&*state).unwrap();
            for field in [
                "buckets",
                "allowance",
                "profile",
                "last_success",
                "profile_last_success",
            ] {
                assert_eq!(actual[field], saved[field], "{code}: {field}");
            }
        }
        assert!(state.retry_at >= 4600);
        state.last_attempt = "2026-10-08T04:00:00Z".into();
        apply_quota_result(
            state,
            Ok(crate::http_quota::Readout {
                buckets: Vec::new(),
                allowance: crate::account_allowance::AccountAllowance {
                    balance: Some(99.0),
                    ..Default::default()
                },
                allowance_retry_after: 0,
                proxy_source: "direct".into(),
            }),
            4600,
            &mut schedule,
        );
        assert_eq!(state.last_success.as_deref(), Some("2026-10-08T04:00:00Z"));
        assert_eq!(state.allowance.as_ref().unwrap().balance, Some(99.0));
        assert!(state.buckets.is_empty());
        assert_eq!((state.failures, state.retry_at), (0, 0));
    }
    #[test]
    fn watcher_ignores_reads_but_keeps_write_create_remove_and_rescan_hints() {
        use notify::{
            Event, EventKind,
            event::{AccessKind, AccessMode, ModifyKind},
        };
        let paths = Mutex::new(Vec::new());
        let (sender, receiver) = mpsc::sync_channel(1);
        queue_watcher_event(
            &paths,
            &sender,
            Event::new(EventKind::Access(AccessKind::Read)).add_path("auth.json".into()),
        );
        assert!(paths.lock().unwrap().is_empty() && receiver.try_recv().is_err());
        for kind in [
            EventKind::Access(AccessKind::Close(AccessMode::Write)),
            EventKind::Modify(ModifyKind::Any),
            EventKind::Create(notify::event::CreateKind::File),
            EventKind::Remove(notify::event::RemoveKind::File),
            EventKind::Any,
        ] {
            queue_watcher_event(
                &paths,
                &sender,
                Event::new(kind).add_path("auth.json".into()),
            );
        }
        assert_eq!(paths.lock().unwrap().len(), 5);
        assert!(matches!(receiver.try_recv(), Ok(Message::FilesChanged)));
    }
    #[test]
    fn same_account_renewal_releases_auth_backoff_without_changing_refresh_policy() {
        let scope = crate::http_quota::Scope {
            source_id: "wsl:Ubuntu".into(),
            home: PathBuf::from("C:/test-wsl"),
            wsl: Some(crate::source_config::WslTarget {
                distro: "Ubuntu".into(),
                user: String::new(),
            }),
        };
        let old = crate::http_quota::Stamp {
            identity: Some("same-account".into()),
            revision: "expired-token".into(),
            status: "credentials_expired".into(),
            ..Default::default()
        };
        let ready = crate::http_quota::Stamp {
            revision: "renewed-token".into(),
            status: "ready".into(),
            ..old.clone()
        };
        for previous in [Some(&old), None] {
            let mut quota = QuotaState::default();
            reconcile_identity(&mut quota, &scope, &old);
            let state = quota.sources.get_mut(&scope.source_id).unwrap();
            state.failures = 5;
            state.retry_at = 1060;
            assert!(reconcile_probe(&mut quota, &scope, &ready, previous));
            let state = &quota.sources[&scope.source_id];
            assert_eq!(state.identity, ready.identity);
            assert_eq!(state.status, "awaiting_refresh");
            assert_eq!(state.profile_status, "awaiting_refresh");
            assert_eq!((state.failures, state.retry_at), (0, 0));
            let mut schedule = crate::refresh::Schedule::new();
            schedule.invalidate(0);
            let auto = crate::refresh::Config::default();
            assert!(schedule.due_for_account(&auto, 1000, state.retry_at, true, false));
            assert!(!schedule.due_for_account(&auto, 1000, state.retry_at, false, false));
            let manual = crate::refresh::Config {
                mode: "manual".into(),
                ..auto
            };
            assert!(!schedule.due_for_account(&manual, 1000, state.retry_at, true, false));
            schedule.request();
            assert!(schedule.due_for_account(&manual, 1000, state.retry_at, true, false));
        }
    }
    #[test]
    fn local_probe_requires_new_credentials_for_401_and_preserves_other_backoffs() {
        let scope = crate::http_quota::Scope {
            source_id: "windows".into(),
            home: PathBuf::from("C:/test"),
            wsl: None,
        };
        let old = crate::http_quota::Stamp {
            identity: Some("same-account".into()),
            revision: "old-token".into(),
            status: "ready".into(),
            ..Default::default()
        };
        let renewed = crate::http_quota::Stamp {
            revision: "renewed-token".into(),
            ..old.clone()
        };
        for (status, stamp, expected) in [
            ("reauth_required", &old, false),
            ("reauth_required", &renewed, true),
            ("http_429", &renewed, false),
            ("http_403", &renewed, false),
            ("network_error", &renewed, false),
        ] {
            let mut quota = QuotaState::default();
            reconcile_identity(&mut quota, &scope, &old);
            let state = quota.sources.get_mut(&scope.source_id).unwrap();
            state.status = status.into();
            state.profile_status = status.into();
            state.failures = 5;
            state.retry_at = 1060;
            assert_eq!(
                reconcile_probe(&mut quota, &scope, stamp, Some(&old)),
                expected
            );
            let state = &quota.sources[&scope.source_id];
            if expected {
                assert_eq!((state.failures, state.retry_at), (0, 0));
            } else {
                assert_eq!(state.status, status);
                assert_eq!(state.profile_status, status);
                assert_eq!((state.failures, state.retry_at), (5, 1060));
            }
        }
        let expired = crate::http_quota::Stamp {
            status: "credentials_expired".into(),
            ..old.clone()
        };
        for status in ["http_429", "http_403", "network_error", "connected"] {
            let mut quota = cache_fixture(&scope, &old);
            let state = quota.sources.get_mut(&scope.source_id).unwrap();
            state.status = status.into();
            state.failures = 5;
            state.retry_at = 1060;
            reconcile_probe(&mut quota, &scope, &expired, Some(&old));
            reconcile_probe(&mut quota, &scope, &renewed, Some(&expired));
            let state = &quota.sources[&scope.source_id];
            assert_eq!(state.status, status);
            assert_eq!((state.failures, state.retry_at), (5, 1060));
            assert!(state.allowance.is_some() && state.profile.is_some());
        }
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
