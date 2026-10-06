use crate::backend::{Backend, Settings};
use crate::geometry::{Anchor, Area, Placement};
use crate::window_lifecycle::Event;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{
    Emitter, Manager, Theme,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewState {
    pub mode: String,
    pub page: String,
    #[serde(default)]
    pub news_challenge: bool,
    #[serde(default = "default_news_limit")]
    pub news_limit: u32,
    pub selected_model: Option<String>,
    pub selected_session: Option<String>,
    #[serde(default)]
    pub session_query: pulse_core::storage::SessionPageRequest,
    #[serde(default)]
    pub model_query: pulse_core::storage::ModelPageRequest,
    pub scroll: std::collections::BTreeMap<String, f64>,
    pub glass_supported: bool,
    pub floating_supported: bool,
}
fn default_news_limit() -> u32 {
    5
}
impl Default for ViewState {
    fn default() -> Self {
        Self {
            mode: if cfg!(windows) { "compact" } else { "details" }.into(),
            page: "overview".into(),
            news_challenge: false,
            news_limit: default_news_limit(),
            selected_model: None,
            selected_session: None,
            session_query: Default::default(),
            model_query: Default::default(),
            scroll: Default::default(),
            glass_supported: false,
            floating_supported: cfg!(windows),
        }
    }
}
pub struct WindowHost {
    pub lifecycle: crate::window_lifecycle::Trace,
    generation: std::sync::atomic::AtomicU64,
    pub view: Mutex<ViewState>,
    anchor: Mutex<Option<Anchor>>,
    hidden_since: Mutex<Option<Instant>>,
    creating: Mutex<()>,
    material: Mutex<Option<(bool, String)>>,
    pub last_hidden: Mutex<Option<Instant>>,
    #[cfg(target_os = "macos")]
    pub badge: Mutex<bool>,
    #[cfg(target_os = "macos")]
    pub deactivate_observer: std::sync::atomic::AtomicUsize,
    maintenance: Mutex<Instant>,
    release_pending: std::sync::atomic::AtomicBool,
}
impl Default for WindowHost {
    fn default() -> Self {
        Self {
            lifecycle: Default::default(),
            generation: std::sync::atomic::AtomicU64::new(0),
            view: Mutex::new(ViewState::default()),
            anchor: Mutex::new(None),
            hidden_since: Mutex::new(None),
            creating: Mutex::new(()),
            material: Mutex::new(None),
            last_hidden: Mutex::new(None),
            #[cfg(target_os = "macos")]
            badge: Mutex::new(false),
            #[cfg(target_os = "macos")]
            deactivate_observer: std::sync::atomic::AtomicUsize::new(0),
            maintenance: Mutex::new(Instant::now()),
            release_pending: std::sync::atomic::AtomicBool::new(false),
        }
    }
}
pub fn trace_window(app: &tauri::AppHandle, event: Event, visible: Option<bool>) {
    if let Some(host) = app.try_state::<WindowHost>() {
        host.lifecycle.record(
            event,
            host.generation.load(std::sync::atomic::Ordering::Relaxed),
            visible,
        );
    }
}
impl WindowHost {
    pub fn diagnostics(&self) -> Result<crate::diagnostics::Window, String> {
        let view = self.view.lock().map_err(|_| "界面状态不可用")?;
        Ok(crate::diagnostics::Window {
            generation: self.generation.load(std::sync::atomic::Ordering::Relaxed),
            page: match view.page.as_str() {
                "overview" => "overview",
                "models" => "models",
                "sessions" => "sessions",
                "news" => "news",
                "settings" => "settings",
                _ => "unknown",
            },
            glass_supported: view.glass_supported,
            floating_supported: view.floating_supported,
            lifecycle: self.lifecycle.diagnostics(),
        })
    }
    pub fn remember_view(&self, next: ViewState) -> Result<(), String> {
        let pages = ["overview", "models", "sessions", "news", "settings"];
        let scroll_pages = [
            "overview",
            "models",
            "sessions",
            "news",
            "settings",
            "challenge",
        ];
        if !pages.contains(&next.page.as_str())
            || !(5..=105).contains(&next.news_limit)
            || next.selected_model.as_ref().is_some_and(|s| s.len() > 256)
            || next
                .selected_session
                .as_ref()
                .is_some_and(|s| s.len() > 256)
            || next.scroll.len() > scroll_pages.len()
            || next.scroll.iter().any(|(page, y)| {
                !scroll_pages.contains(&page.as_str())
                    || !y.is_finite()
                    || *y < 0.0
                    || *y > 10_000_000.0
            })
        {
            return Err("界面状态无效".into());
        }
        let query = &next.session_query;
        if (!next.model_query.from_day.is_empty()
            || !next.model_query.through_day.is_empty()
            || next.model_query.cursor.is_some()
            || matches!(
                next.model_query.direction,
                pulse_core::storage::ModelDirection::Previous
            ))
            && next.model_query.validate().is_err()
        {
            return Err("模型页面范围无效".into());
        }
        if query
            .filter
            .model
            .as_ref()
            .is_some_and(|s| s.is_empty() || s.len() > 256)
            || [&query.filter.from_day, &query.filter.through_day]
                .into_iter()
                .flatten()
                .any(|s| s.len() != 10 || chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_err())
            || query
                .cursor
                .as_ref()
                .is_some_and(|c| c.id.is_empty() || c.id.len() > 256 || c.activity.len() > 64)
        {
            return Err("Session 页面范围无效".into());
        }
        let mut view = self.view.lock().map_err(|_| "界面状态不可用")?;
        // The host owns mode/material/platform capabilities; the renderer owns navigation.
        view.page = next.page;
        view.news_challenge = next.news_challenge;
        view.news_limit = next.news_limit;
        view.selected_model = next.selected_model;
        view.selected_session = next.selected_session;
        view.session_query = next.session_query;
        view.model_query = next.model_query;
        view.scroll = next.scroll;
        Ok(())
    }
}

fn settings(app: &tauri::AppHandle) -> Settings {
    app.state::<Backend>()
        .snapshot
        .read()
        .map(|s| s.settings.clone())
        .unwrap_or_default()
}
fn areas(app: &tauri::AppHandle) -> Vec<Area> {
    let primary = app
        .primary_monitor()
        .ok()
        .flatten()
        .and_then(|m| m.name().cloned());
    let mut areas = app
        .available_monitors()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|m| {
            let work = m.work_area();
            (work.size.width > 0 && work.size.height > 0 && m.scale_factor() > 0.0).then(|| Area {
                monitor: m.name().cloned(),
                x: work.position.x,
                y: work.position.y,
                width: work.size.width,
                height: work.size.height,
                scale: m.scale_factor(),
            })
        })
        .collect::<Vec<_>>();
    areas.sort_by_key(|a| a.monitor != primary);
    areas
}
fn ensure_window(app: &tauri::AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    let host = app.state::<WindowHost>();
    let _creation = host.creating.lock().expect("window creation state");
    if let Some(window) = app.get_webview_window("pulse") {
        return Ok(window);
    }
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "pulse")
        .expect("pulse window configuration");
    let window = tauri::WebviewWindowBuilder::from_config(app, config)?
        .on_page_load(|window, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Finished {
                // The initial Windows frame settles after WebView creation. Reapply the
                // requested content size once, avoiding a title-bar-sized startup gap.
                scale_changed(window.app_handle());
            }
        })
        .build()?;
    host.generation
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    trace_window(app, Event::Created, window.is_visible().ok());
    #[cfg(target_os = "macos")]
    crate::macos::prepare_window(&window);
    apply_settings(app, &settings(app));
    Ok(window)
}
fn position(app: &tauri::AppHandle, w: &tauri::WebviewWindow, compact: bool) {
    #[cfg(target_os = "macos")]
    {
        crate::macos::position(app, w, &areas(app));
    }
    #[cfg(not(target_os = "macos"))]
    position_floating(app, w, compact);
    #[cfg(target_os = "macos")]
    let _ = compact;
}
#[cfg(not(target_os = "macos"))]
fn position_floating(app: &tauri::AppHandle, w: &tauri::WebviewWindow, compact: bool) {
    let host = app.state::<WindowHost>();
    let anchor = host
        .anchor
        .lock()
        .ok()
        .and_then(|a| a.clone())
        .or_else(|| settings(app).compact_anchor);
    let monitors = areas(app);
    if let Some(area) = crate::geometry::choose(&monitors, anchor.as_ref()) {
        let p = area.place(
            anchor.as_ref(),
            if compact {
                (184.0, 36.0)
            } else {
                (380.0, 800.0)
            },
        );
        // Restrict the configured minimum on small remote-desktop work areas.
        let _ = w.set_min_size(Some(tauri::PhysicalSize::new(
            p.width.min((320.0 * area.scale) as u32),
            p.height
                .min(((if compact { 36.0 } else { 240.0 }) * area.scale) as u32),
        )));
        let _ = w.set_size(tauri::PhysicalSize::new(p.width, p.height));
        let _ = w.set_position(tauri::PhysicalPosition::new(p.x, p.y));
    }
}
pub fn moved(app: &tauri::AppHandle, position: tauri::PhysicalPosition<i32>) {
    let host = app.state::<WindowHost>();
    if !host.view.lock().is_ok_and(|v| v.mode == "compact") {
        return;
    }
    let Some(window) = app.get_webview_window("pulse") else {
        return;
    };
    let Ok(size) = window.outer_size() else {
        return;
    };
    let monitors = areas(app);
    let center = (
        position.x as i64 + size.width as i64 / 2,
        position.y as i64 + size.height as i64 / 2,
    );
    let area = monitors
        .iter()
        .find(|a| {
            center.0 >= a.x as i64
                && center.1 >= a.y as i64
                && center.0 < a.x as i64 + a.width as i64
                && center.1 < a.y as i64 + a.height as i64
        })
        .or_else(|| monitors.first());
    if let Some(area) = area {
        let anchor = area.capture(Placement {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
        });
        if let Ok(mut old) = host.anchor.lock() {
            *old = Some(anchor.clone());
        }
        app.state::<Backend>().remember_anchor(anchor);
    }
}
pub fn hide(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("pulse")
        && w.is_visible().unwrap_or(false)
    {
        let _ = app.emit("window-visible", false);
        if w.hide().is_err() {
            trace_window(app, Event::HideFailed, w.is_visible().ok());
            return;
        }
        trace_window(app, Event::Hidden, w.is_visible().ok());
        if let Ok(mut last) = app.state::<WindowHost>().last_hidden.lock() {
            *last = Some(Instant::now());
        }
        if let Ok(mut hidden) = app.state::<WindowHost>().hidden_since.lock() {
            *hidden = Some(Instant::now());
        }
    }
}
/// Called by the existing collector scheduler. No extra maintenance thread/WebView.
pub fn release_hidden(app: &tauri::AppHandle) {
    let Some(host) = app.try_state::<WindowHost>() else {
        return;
    };
    let due = host
        .hidden_since
        .lock()
        .is_ok_and(|s| s.is_some_and(|t| t.elapsed() >= Duration::from_secs(300)));
    if !due {
        if let Ok(mut last) = host.maintenance.lock()
            && last.elapsed() >= Duration::from_secs(30)
        {
            *last = Instant::now();
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                if handle
                    .get_webview_window("pulse")
                    .is_some_and(|w| w.is_visible().unwrap_or(false))
                {
                    refresh_material(&handle);
                    correct_bounds(&handle);
                }
            });
        }
        return;
    }
    if host
        .release_pending
        .swap(true, std::sync::atomic::Ordering::Relaxed)
    {
        return;
    }
    let handle = app.clone();
    trace_window(app, Event::ReleaseRequested, None);
    if app
        .run_on_main_thread(move || {
            let host = handle.state::<WindowHost>();
            if let Ok(mut hidden) = host.hidden_since.lock()
                && hidden.is_some_and(|t| t.elapsed() >= Duration::from_secs(300))
                && let Some(w) = handle.get_webview_window("pulse")
                && !w.is_visible().unwrap_or(true)
            {
                // destroy bypasses CloseRequested; ExitRequested is guarded in main.
                if w.destroy().is_ok() {
                    trace_window(&handle, Event::ReleaseQueued, None);
                    *hidden = None;
                    if let Ok(mut material) = host.material.lock() {
                        *material = None;
                    }
                } else {
                    trace_window(&handle, Event::ReleaseFailed, w.is_visible().ok());
                }
            }
            host.release_pending
                .store(false, std::sync::atomic::Ordering::Relaxed);
        })
        .is_err()
    {
        host.release_pending
            .store(false, std::sync::atomic::Ordering::Relaxed);
        trace_window(app, Event::ReleaseFailed, None);
    }
}
pub fn setup(app: &tauri::App, settings: &Settings) -> Result<(), Box<dyn std::error::Error>> {
    let language = crate::language::effective(&settings.language);
    *app.state::<WindowHost>()
        .anchor
        .lock()
        .map_err(|_| "anchor state")? = settings.compact_anchor.clone();
    let settings_item = MenuItem::with_id(
        app,
        "settings",
        if language == "zh" {
            "设置"
        } else {
            "Settings"
        },
        true,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let exit = MenuItem::with_id(
        app,
        "exit",
        if language == "zh" { "退出" } else { "Quit" },
        true,
        None::<&str>,
    )?;
    let menu = Menu::with_items(app, &[&settings_item, &separator, &exit])?;
    #[cfg(target_os = "macos")]
    app.handle()
        .set_activation_policy(tauri::ActivationPolicy::Accessory)?;
    let icon = {
        #[cfg(target_os = "macos")]
        {
            crate::macos::icon()
        }
        #[cfg(not(target_os = "macos"))]
        {
            app.default_window_icon().ok_or("missing icon")?.clone()
        }
    };
    let tray = TrayIconBuilder::with_id("pulse-tray")
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("CodexPulse")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "settings" => show_details(app, Some("settings")),
            "exit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                #[cfg(target_os = "macos")]
                crate::macos::toggle(tray.app_handle());
                #[cfg(not(target_os = "macos"))]
                show_details(tray.app_handle(), None);
            }
        })
        .build(app)?;
    #[cfg(target_os = "macos")]
    crate::macos::set_status_symbol(&tray);
    #[cfg(not(target_os = "macos"))]
    let _ = tray;
    #[cfg(target_os = "macos")]
    crate::macos::observe_deactivation(app.handle());
    if cfg!(windows) && settings.floating {
        let w = ensure_window(app.handle())?;
        if settings.compact_anchor.is_none()
            && let Some((x, y)) = settings.compact_position
        {
            let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
            moved(app.handle(), tauri::PhysicalPosition::new(x, y));
        }
        position(app.handle(), &w, true);
        let _ = w.show();
    }
    Ok(())
}
pub fn apply_settings(app: &tauri::AppHandle, settings: &Settings) {
    crate::language::refresh_tray(app, crate::language::effective(&settings.language));
    if let Some(w) = app.get_webview_window("pulse") {
        let _ = w.set_always_on_top(cfg!(target_os = "macos") || settings.always_on_top);
        let _ = w.set_theme(match settings.theme.as_str() {
            "dark" => Some(Theme::Dark),
            "light" => Some(Theme::Light),
            _ => None,
        });
        #[cfg(target_os = "macos")]
        crate::macos::apply_appearance(&w, &settings.theme);
        refresh_material_with(app, settings);
        // A settings window remains open when the optional float is disabled.
        let _ = app.emit("settings-applied", settings);
    }
}
pub fn refresh_material(app: &tauri::AppHandle) {
    refresh_material_with(app, &settings(app));
}
fn refresh_material_with(app: &tauri::AppHandle, settings: &Settings) {
    if let Some(w) = app.get_webview_window("pulse") {
        let host = app.state::<WindowHost>();
        let allowed = settings.glass && crate::material::transparency_allowed();
        let Ok(mut previous) = host.material.lock() else {
            return;
        };
        let key = (allowed, settings.theme.clone());
        if previous.as_ref() == Some(&key) {
            return;
        }
        let effects = if allowed {
            Some(tauri::utils::config::WindowEffectsConfig {
                effects: if cfg!(target_os = "macos") {
                    vec![
                        tauri::utils::WindowEffect::LiquidGlassRegular,
                        tauri::utils::WindowEffect::Popover,
                    ]
                } else {
                    vec![tauri::utils::WindowEffect::Acrylic]
                },
                radius: cfg!(target_os = "macos").then_some(20.0),
                state: cfg!(target_os = "macos").then_some(tauri::utils::WindowEffectState::Active),
                ..Default::default()
            })
        } else {
            None
        };
        let supported = w.set_effects(effects).is_ok() && allowed;
        if !supported {
            let _ = w.set_effects(None);
        }
        *previous = Some(key);
        if let Ok(mut view) = host.view.lock() {
            view.glass_supported = supported;
        }
        let _ = app.emit("glass-supported", supported);
    }
}
pub fn show_details(app: &tauri::AppHandle, page: Option<&str>) {
    // Building a WebView in a synchronous native tray callback can deadlock on Windows.
    let handle = app.clone();
    let page = page.map(str::to_owned);
    tauri::async_runtime::spawn_blocking(move || show_details_now(&handle, page.as_deref()));
}
fn show_details_now(app: &tauri::AppHandle, page: Option<&str>) {
    trace_window(app, Event::ShowRequested, None);
    let host = app.state::<WindowHost>();
    if let Ok(mut view) = host.view.lock() {
        view.mode = "details".into();
        if let Some(page) = page {
            view.page = page.into();
        }
    }
    if let Ok(mut hidden) = host.hidden_since.lock() {
        *hidden = None;
    }
    if let Ok(w) = ensure_window(app) {
        position(app, &w, false);
        apply_settings(app, &settings(app));
        let _ = w.show();
        let _ = app.emit("window-visible", true);
        let _ = w.set_focus();
        trace_window(app, Event::Shown, w.is_visible().ok());
        let _ = app.emit("window-mode", ("details", page));
        let _ = app.emit("snapshot-changed", ());
    }
}
pub fn correct_bounds(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("pulse") {
        let compact = app
            .state::<WindowHost>()
            .view
            .lock()
            .is_ok_and(|v| v.mode == "compact");
        // Only relocate when current bounds are out of a work area. Detail drag position
        // remains valid while visible; compact restoration still uses its saved anchor.
        if let (Ok(p), Ok(size)) = (w.outer_position(), w.outer_size()) {
            let valid = areas(app).iter().any(|a| {
                p.x >= a.x
                    && p.y >= a.y
                    && p.x as i64 + size.width as i64 <= a.x as i64 + a.width as i64
                    && p.y as i64 + size.height as i64 <= a.y as i64 + a.height as i64
            });
            if !valid {
                position(app, &w, compact);
            }
        }
    }
}
pub fn scale_changed(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("pulse") {
        let compact = app
            .state::<WindowHost>()
            .view
            .lock()
            .is_ok_and(|v| v.mode == "compact");
        position(app, &w, compact);
    }
}
pub fn compact_or_hide(app: &tauri::AppHandle) {
    let settings = settings(app);
    let host = app.state::<WindowHost>();
    if let Some(w) = app.get_webview_window("pulse") {
        if cfg!(windows) && settings.floating {
            // Keep detail moves separate from the compact anchor. Capture resize-generated
            // move events only after both size and position have been restored.
            position(app, &w, true);
            if let Ok(mut view) = host.view.lock() {
                view.mode = "compact".into();
            }
            if let Ok(mut hidden) = host.hidden_since.lock() {
                *hidden = None;
            }
            let _ = app.emit("window-mode", ("compact", None::<String>));
            let _ = app.emit("window-visible", true);
            let _ = w.show();
        } else {
            hide(app);
        }
    }
}
#[cfg(windows)]
fn command_output(mut cmd: std::process::Command) -> Option<String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let mut child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        if child.try_wait().ok()?.is_some() {
            break;
        }
        if started.elapsed() > Duration::from_secs(3) {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    if output.stdout.contains(&0) {
        let chars = output
            .stdout
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect::<Vec<_>>();
        Some(
            String::from_utf16_lossy(&chars)
                .trim_start_matches('\u{feff}')
                .into(),
        )
    } else {
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}
pub struct WslDiscovery {
    pub id: String,
    pub label: String,
    pub home: PathBuf,
    pub target: crate::source_config::WslTarget,
    pub running: bool,
    pub status: &'static str,
}
pub fn running_wsl_sources(
    auto: bool,
    configured: &[crate::source_config::WslSource],
    cache: &mut std::collections::BTreeMap<String, PathBuf>,
) -> Vec<WslDiscovery> {
    #[cfg(not(windows))]
    {
        let _ = (auto, configured, cache);
        Vec::new()
    }
    #[cfg(windows)]
    {
        let mut list = std::process::Command::new("wsl.exe");
        list.args(["--list", "--running", "--quiet"]);
        let list = command_output(list);
        let names = list
            .as_deref()
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|name| !name.is_empty() && !name.contains(['\\', '/']))
            .collect::<Vec<_>>();
        let mut sources = configured
            .iter()
            .filter(|s| s.enabled)
            .map(|s| WslDiscovery {
                id: format!("wsl:{}:{}", s.distro, s.id),
                label: format!(
                    "WSL · {}{}",
                    s.distro,
                    if s.user.is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", s.user)
                    }
                ),
                home: s.path(),
                target: s.target(),
                running: names.contains(&s.distro.as_str()),
                status: if list.is_none() {
                    "wsl_unavailable"
                } else {
                    "wsl_stopped"
                },
            })
            .collect::<Vec<_>>();
        if !auto {
            return sources;
        }
        sources.extend(names.into_iter().take(8).filter_map(|name| {
            if let Some(home) = cache.get(name) {
                return Some(WslDiscovery {
                    id: format!("wsl:{name}"),
                    label: format!("WSL · {name}"),
                    home: home.clone(),
                    target: crate::source_config::WslTarget {
                        distro: name.into(),
                        user: String::new(),
                    },
                    running: true,
                    status: "discovering",
                });
            }
            let mut home = std::process::Command::new("wsl.exe");
            home.args([
                "-d",
                name,
                "--exec",
                "sh",
                "-c",
                "printf '%s' \"${CODEX_HOME:-$HOME/.codex}\"",
            ]);
            let home = command_output(home)?;
            let home = home.trim();
            if !crate::source_config::valid_linux_home(home) {
                return None;
            }
            let home = crate::source_config::unc_home(name, home);
            cache.insert(name.into(), home.clone());
            Some(WslDiscovery {
                id: format!("wsl:{name}"),
                label: format!("WSL · {name}"),
                home,
                target: crate::source_config::WslTarget {
                    distro: name.into(),
                    user: String::new(),
                },
                running: true,
                status: "discovering",
            })
        }));
        sources
    }
}

#[cfg(test)]
pub fn wsl_is_running(name: &str) -> bool {
    #[cfg(windows)]
    {
        let mut list = std::process::Command::new("wsl.exe");
        list.args(["--list", "--running", "--quiet"]);
        command_output(list).is_some_and(|output| output.lines().any(|line| line.trim() == name))
    }
    #[cfg(not(windows))]
    {
        let _ = name;
        false
    }
}
