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
    pub accent: String,
    pub glass: bool,
    pub floating: bool,
    pub always_on_top: bool,
    pub windows_enabled: bool,
    pub wsl_enabled: bool,
    pub windows_home: Option<String>,
    pub windows_sources: Vec<crate::source_config::WindowsSource>,
    pub wsl_sources: Vec<crate::source_config::WslSource>,
    pub wsl_auto_detect: bool,
    pub hide_titles: bool,
    pub hide_projects: bool,
    pub timezone: String,
    pub compact_position: Option<(i32, i32)>,
    pub compact_anchor: Option<crate::geometry::Anchor>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            accent: "blue".into(),
            glass: true,
            floating: true,
            always_on_top: true,
            windows_enabled: true,
            wsl_enabled: true,
            windows_home: None,
            windows_sources: Vec::new(),
            wsl_sources: Vec::new(),
            wsl_auto_detect: true,
            hide_titles: false,
            hide_projects: false,
            timezone: iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".into()),
            compact_position: None,
            compact_anchor: None,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if self.compact_anchor.as_ref().is_some_and(|a| !a.valid()) {
            return Err("浮窗位置无效".into());
        }
        if !["system", "light", "dark"].contains(&self.theme.as_str()) {
            return Err("主题无效".into());
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
            .is_some_and(|p| !crate::source_config::valid_windows_home(p))
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
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct QuotaState {
    pub buckets: Vec<pulse_core::quota::QuotaBucket>,
    pub sources: BTreeMap<String, QuotaSource>,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaSource {
    pub home: String,
    pub status: String,
    pub last_attempt: String,
    pub failures: u32,
    pub buckets: Vec<pulse_core::quota::QuotaBucket>,
}
pub enum Message {
    Settings(Settings, tokio::sync::oneshot::Sender<Result<(), String>>),
    Quotas(Vec<crate::rpc::ResultSet>),
    News(Box<crate::news::Feed>),
    ReadNews(
        Vec<String>,
        tokio::sync::oneshot::Sender<Result<(), String>>,
    ),
    SessionPage(
        pulse_core::storage::SessionPageRequest,
        tokio::sync::oneshot::Sender<Result<pulse_core::storage::SessionPage, String>>,
    ),
    SessionDetail(
        pulse_core::storage::SessionDetailRequest,
        tokio::sync::oneshot::Sender<Result<Option<pulse_core::storage::SessionDetail>, String>>,
    ),
}
enum NetworkRequest {
    Quotas(crate::rpc::Request),
    News(Box<crate::news::Feed>),
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
    active_child: crate::rpc::ActiveChild,
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
        let settings = store.setting::<Settings>("app")?.unwrap_or_default();
        settings.validate().map_err(std::io::Error::other)?;
        let quota = store.setting::<QuotaState>("quota")?.unwrap_or_default();
        let mut news = store
            .setting::<crate::news::Feed>("news")?
            .unwrap_or_default();
        if news.challenge_status.is_empty() {
            // Refresh legacy caches once so the newly added challenge is available.
            news.next_attempt = 0;
        }
        let inbox = store
            .setting::<crate::inbox::Inbox>("news_inbox")?
            .unwrap_or_default();
        let snapshot = Arc::new(RwLock::new(Snapshot {
            settings: settings.clone(),
            quota: quota.clone(),
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
        let active_child = Arc::new(Mutex::new(None));
        let pending_anchor = Arc::new(Mutex::new(None::<(crate::geometry::Anchor, Instant)>));
        let collector_anchor = pending_anchor.clone();
        let worker_child = active_child.clone();
        let (rpc_sender, rpc_receiver) = mpsc::sync_channel::<NetworkRequest>(1);
        let network_sender = rpc_sender.clone();
        let replies = sender.clone();
        thread::Builder::new()
            .name("pulse-network".into())
            .spawn(move || {
                let mut translator = crate::news::Translator::default();
                while !worker_stop.load(Ordering::Relaxed) {
                    match rpc_receiver.recv_timeout(Duration::from_millis(300)) {
                        Ok(NetworkRequest::Translation(keys, reply)) => {
                            let _ = reply.send(translator.translate(&keys));
                        }
                        Ok(NetworkRequest::News(feed)) => {
                            let feed = crate::news::fetch(*feed);
                            if !worker_stop.load(Ordering::Relaxed) {
                                let _ = replies.send(Message::News(Box::new(feed)));
                            }
                        }
                        Ok(NetworkRequest::Quotas(request)) => {
                            let mut results = Vec::new();
                            for scope in request.scopes {
                                if worker_stop.load(Ordering::Relaxed) {
                                    break;
                                }
                                results.push(crate::rpc::ResultSet {
                                    source_id: scope.source_id.clone(),
                                    home: scope.home.clone(),
                                    wsl: scope.wsl.clone(),
                                    result: crate::rpc::read(&scope, &worker_stop, &worker_child),
                                });
                            }
                            if !worker_stop.load(Ordering::Relaxed) {
                                let _ = replies.send(Message::Quotas(results));
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
                let mut news_in_flight = false;
                let mut quota_in_flight = false;
                let mut last_quota = Instant::now() - Duration::from_secs(3600);
                let prices = PriceBook::bundled().unwrap_or_default();
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
                    match receiver.recv_timeout(Duration::from_millis(300)) {
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
                        Ok(Message::SessionPage(request, reply)) => {
                            let result = settings
                                .timezone
                                .parse::<chrono_tz::Tz>()
                                .map_err(|_| "统计时区无效".into())
                                .and_then(|tz| {
                                    store
                                        .session_page(&request, tz)
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
                        Ok(Message::News(next)) => {
                            news_in_flight = false;
                            news = *next;
                            inbox.update(&news);
                            let _ = store.set_setting("news_inbox", &inbox);
                            let _ = store.set_setting("news", &news.for_storage());
                            dirty = true;
                        }
                        Ok(Message::Quotas(results)) => {
                            quota_in_flight = false;
                            for response in results {
                                let Some(source) = sources.iter().find(|s| {
                                    s.scope_matches(
                                        &response.source_id,
                                        &response.home,
                                        response.wsl.as_ref(),
                                    )
                                }) else {
                                    continue;
                                };
                                let home = source.home.to_string_lossy().into_owned();
                                let state =
                                    quota.sources.entry(response.source_id).or_insert_with(|| {
                                        QuotaSource {
                                            home: home.clone(),
                                            status: "unknown".into(),
                                            last_attempt: String::new(),
                                            failures: 0,
                                            buckets: Vec::new(),
                                        }
                                    });
                                if state.home != home {
                                    state.home = home;
                                    state.buckets.clear();
                                }
                                state.last_attempt = Utc::now().to_rfc3339();
                                match response.result {
                                    Ok(buckets) => {
                                        state.status = "connected".into();
                                        state.failures = 0;
                                        state.buckets = buckets;
                                    }
                                    Err(code) => {
                                        state.status = code;
                                        state.failures = state.failures.saturating_add(1);
                                    }
                                }
                            }
                            quota.buckets = pulse_core::quota::merged(
                                quota
                                    .sources
                                    .values()
                                    .flat_map(|q| q.buckets.clone())
                                    .collect(),
                            );
                            let _ = store.set_setting("quota", &quota);
                            dirty = true;
                        }
                        Ok(Message::Settings(mut next, reply)) => {
                            // Location is host-owned and can change while a UI settings draft is open.
                            next.compact_anchor = settings.compact_anchor.clone();
                            next.compact_position = settings.compact_position;
                            // Changing timezone needs a bucket rebuild before enabling this setting.
                            // This restriction is removed by the migration implementation.
                            if next.timezone != settings.timezone {
                                let _ = reply.send(Err("当前版本暂不支持切换统计时区".into()));
                                continue;
                            }
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
                                    last_quota = Instant::now() - Duration::from_secs(3600);
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
                                        std::env::var_os("USERPROFILE").unwrap_or_default(),
                                    )
                                    .join(".codex")
                                });
                            windows.push((
                                "windows".to_owned(),
                                "Windows App / CLI".to_owned(),
                                home,
                            ));
                            windows.extend(
                                settings
                                    .windows_sources
                                    .iter()
                                    .filter(|s| s.enabled)
                                    .map(|s| {
                                        (
                                            format!("windows:{}", s.id),
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
                    let quota_interval = if app
                        .get_webview_window("pulse")
                        .is_some_and(|w| w.is_visible().unwrap_or(false))
                    {
                        60
                    } else {
                        300
                    };
                    if !quota_in_flight
                        && !sources.is_empty()
                        && last_quota.elapsed() >= Duration::from_secs(quota_interval)
                    {
                        let scopes = sources
                            .iter()
                            .filter(|source| {
                                if !source.available() {
                                    return false;
                                }
                                let old = quota.sources.get(&source.health.id);
                                old.is_none_or(|old| {
                                    old.home != source.health.path
                                        || old.failures == 0
                                        || chrono::DateTime::parse_from_rfc3339(&old.last_attempt)
                                            .is_ok_and(|last| {
                                                (Utc::now() - last.with_timezone(&Utc))
                                                    .num_seconds()
                                                    >= ((60u64.saturating_mul(
                                                        1u64 << old.failures.min(5),
                                                    ))
                                                    .min(1800)
                                                        as i64)
                                            })
                                })
                            })
                            .map(|s| crate::rpc::Scope {
                                source_id: s.health.id.clone(),
                                home: s.home.clone(),
                                wsl: s.wsl.clone(),
                            })
                            .collect::<Vec<_>>();
                        if !scopes.is_empty()
                            && rpc_sender
                                .try_send(NetworkRequest::Quotas(crate::rpc::Request { scopes }))
                                .is_ok()
                        {
                            quota_in_flight = true;
                        }
                        last_quota = Instant::now();
                    }
                    if !news_in_flight
                        && Utc::now().timestamp() >= news.next_attempt
                        && rpc_sender
                            .try_send(NetworkRequest::News(Box::new(news.clone())))
                            .is_ok()
                    {
                        news_in_flight = true;
                    }
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
                    if dirty && last_publish.elapsed() >= Duration::from_secs(1) {
                        let timezone = settings.timezone.parse::<Tz>().unwrap_or(chrono_tz::UTC);
                        let usage = store
                            .summary(Utc::now().with_timezone(&timezone).date_naive(), timezone);
                        let recent = store.recent_sessions(None, 10);
                        if let Ok(mut state) = shared.write() {
                            state.settings = settings.clone();
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
                                .map(|(id, q)| (id.clone(), q.clone()))
                                .collect::<BTreeMap<_, _>>();
                            state.quota = QuotaState {
                                buckets: pulse_core::quota::merged(
                                    live.values().flat_map(|q| q.buckets.clone()).collect(),
                                ),
                                sources: live,
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
                    }
                }
            })?;
        Ok(Self {
            snapshot,
            sender,
            network_sender,
            stopping,
            active_child,
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
        crate::rpc::stop_child(&self.active_child);
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
    fn legacy_settings_keep_defaults_and_source_drafts_roundtrip() {
        let mut settings: Settings =
            serde_json::from_str(r#"{"theme":"light","windows_home":null}"#).unwrap();
        assert!(settings.wsl_auto_detect);
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
