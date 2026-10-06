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
    collections::{HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, RwLock,
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
    pub glass: bool,
    pub floating: bool,
    pub always_on_top: bool,
    pub windows_enabled: bool,
    pub wsl_enabled: bool,
    pub windows_home: Option<String>,
    pub timezone: String,
    pub compact_position: Option<(i32, i32)>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            glass: true,
            floating: true,
            always_on_top: true,
            windows_enabled: true,
            wsl_enabled: true,
            windows_home: None,
            timezone: iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".into()),
            compact_position: None,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !["system", "light", "dark"].contains(&self.theme.as_str()) {
            return Err("主题无效".into());
        }
        self.timezone
            .parse::<Tz>()
            .map_err(|_| "请填写有效的 IANA 时区".to_string())?;
        if self
            .windows_home
            .as_ref()
            .is_some_and(|p| !Path::new(p).is_absolute())
        {
            return Err("Codex home 必须是绝对路径".into());
        }
        Ok(())
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
}
pub enum Message {
    Settings(Settings, tokio::sync::oneshot::Sender<Result<(), String>>),
}
pub struct Backend {
    pub snapshot: Arc<RwLock<Snapshot>>,
    pub sender: SyncSender<Message>,
}
struct Source {
    health: SourceHealth,
    home: PathBuf,
}
impl Backend {
    pub fn start(app: tauri::AppHandle, db: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let store = Store::open(db)?;
        let settings = store.setting::<Settings>("app")?.unwrap_or_default();
        settings.validate().map_err(std::io::Error::other)?;
        let snapshot = Arc::new(RwLock::new(Snapshot {
            settings: settings.clone(),
            collecting: true,
            ..Default::default()
        }));
        let (sender, receiver) = mpsc::sync_channel(8);
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
                        if let Ok(event) = event {
                            if let Ok(mut paths) = watcher_paths.lock() {
                                for path in event.paths {
                                    if paths.len() < 4096 {
                                        paths.push(path);
                                    }
                                }
                            }
                        }
                    })
                    .ok();
                let mut queue = VecDeque::<(String, PathBuf)>::new();
                let mut queued = HashSet::<(String, PathBuf)>::new();
                let mut last_repair = Instant::now() - Duration::from_secs(60);
                let mut last_wsl = Instant::now() - Duration::from_secs(60);
                let mut last_publish = Instant::now() - Duration::from_secs(1);
                let mut dirty = true;
                let prices = PriceBook::default();
                loop {
                    match receiver.recv_timeout(Duration::from_millis(300)) {
                        Ok(Message::Settings(next, reply)) => {
                            // Changing timezone needs a bucket rebuild before enabling this setting.
                            // This restriction is removed by the migration implementation.
                            if next.timezone != settings.timezone {
                                let _ = reply.send(Err("当前版本暂不支持切换统计时区".into()));
                                continue;
                            }
                            let result = store
                                .set_setting("app", &next)
                                .map_err(|_| "无法保存设置".to_string());
                            if result.is_ok() {
                                settings = next;
                                last_repair = Instant::now() - Duration::from_secs(60);
                                dirty = true;
                            }
                            let _ = reply.send(result);
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    if last_repair.elapsed() >= Duration::from_secs(60) {
                        sources.retain(|s| s.health.id != "windows" || settings.windows_enabled);
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
                            if let Some(old) = sources
                                .iter()
                                .position(|s| s.health.id == "windows" && s.home != home)
                            {
                                sources.remove(old);
                            }
                            if !sources.iter().any(|s| s.health.id == "windows") {
                                if let Some(w) = watcher.as_mut() {
                                    let _ = w.watch(&home, RecursiveMode::Recursive);
                                }
                                sources.push(source(
                                    "windows".into(),
                                    "Windows App / CLI".into(),
                                    home,
                                ));
                            }
                        }
                        for source in &mut sources {
                            enqueue_source(source, &mut queue, &mut queued);
                        }
                        last_repair = Instant::now();
                        dirty = true;
                    }
                    if last_wsl.elapsed() >= Duration::from_secs(30) {
                        let discovered = if settings.wsl_enabled {
                            crate::platform::running_wsl_sources()
                        } else {
                            Vec::new()
                        };
                        sources.retain(|s| {
                            s.health.id == "windows"
                                || discovered.iter().any(|(id, _, _)| id == &s.health.id)
                        });
                        for (id, label, home) in discovered {
                            if !sources.iter().any(|s| s.health.id == id) {
                                sources.push(source(id, label, home));
                            }
                        }
                        // WSL paths are polled only while the distro was reported running.
                        for source in sources.iter_mut().filter(|s| s.health.id != "windows") {
                            enqueue_source(source, &mut queue, &mut queued);
                        }
                        last_wsl = Instant::now();
                        dirty = true;
                    }
                    if let Ok(mut paths) = changed.lock() {
                        for path in paths.drain(..) {
                            if path.extension().is_some_and(|e| e == "jsonl") {
                                if let Some(source) =
                                    sources.iter().find(|s| path.starts_with(&s.home))
                                {
                                    enqueue(
                                        &mut queue,
                                        &mut queued,
                                        (source.health.id.clone(), path),
                                    );
                                }
                            }
                        }
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
                        match read_batch(
                            &mut store,
                            &id,
                            &path,
                            settings.timezone.parse().unwrap_or(chrono_tz::UTC),
                            &prices,
                        ) {
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
                            state.collecting = !queue.is_empty();
                            state.sources = sources.iter().map(|s| s.health.clone()).collect();
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
        Ok(Self { snapshot, sender })
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
