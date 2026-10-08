#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod account_allowance;
mod account_profile;
mod backend;
mod diagnostics;
mod geometry;
mod http_quota;
mod inbox;
mod language;
#[cfg(target_os = "macos")]
mod macos;
mod material;
#[cfg(any(target_os = "macos", test))]
mod menubar_status;
mod news;
mod platform;
mod refresh;
#[cfg(test)]
mod rpc;
mod source_config;
mod startup;
mod updates;
mod window_lifecycle;

use backend::{Backend, Settings, Snapshot};
use tauri::{Manager, State};
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
async fn check_updates(app: tauri::AppHandle) -> Result<updates::Info, String> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || handle.state::<updates::Service>().check(&handle))
        .await
        .map_err(|_| "更新检查失败".to_string())??;
    app.state::<updates::Service>().prepare(&app)
}
#[tauri::command]
async fn get_app_update_info(app: tauri::AppHandle) -> Result<updates::Info, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<updates::Service>().info())
        .await
        .map_err(|_| "更新状态不可用".to_string())?
}
#[tauri::command]
async fn install_app_update(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<updates::Service>().install(&app))
        .await
        .map_err(|_| "安装更新失败".to_string())?
}
#[tauri::command]
fn open_app_release(app: tauri::AppHandle) -> Result<(), String> {
    app.opener()
        .open_url(updates::RELEASES_URL, None::<&str>)
        .map_err(|_| "无法打开下载页面".into())
}

#[tauri::command]
fn get_ui_language(app: tauri::AppHandle, state: State<'_, Backend>) -> &'static str {
    let preference = state
        .snapshot
        .read()
        .map(|s| s.settings.language.clone())
        .unwrap_or_default();
    let language = language::effective(&preference);
    language::refresh_tray(&app, language);
    language
}

#[tauri::command]
fn open_source(url: String, app: tauri::AppHandle) -> Result<(), String> {
    let parsed = url::Url::parse(&url).map_err(|_| "来源地址无效")?;
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || !matches!(
            parsed.host_str(),
            Some("codex-resets.com" | "x.com" | "twitter.com" | "developers.openai.com")
        )
        || parsed.host_str() == Some("developers.openai.com")
            && parsed.path() != "/api/docs/pricing"
    {
        return Err("来源地址不在允许的消息站点中".into());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|_| "无法打开来源链接".into())
}

#[tauri::command]
fn get_snapshot(state: State<'_, Backend>) -> Result<Snapshot, String> {
    state
        .snapshot
        .read()
        .map(|s| s.for_display())
        .map_err(|_| "snapshot unavailable".into())
}
#[tauri::command]
async fn refresh_now(group: String, state: State<'_, Backend>) -> Result<Option<i64>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    state
        .sender
        .try_send(backend::Message::Refresh(group, tx))
        .map_err(|_| "采集器繁忙，请稍后重试".to_string())?;
    rx.await.map_err(|_| "采集器已停止".to_string())?
}
#[tauri::command]
async fn set_refresh(
    group: String,
    config: refresh::Config,
    state: State<'_, Backend>,
) -> Result<(), String> {
    config.validate()?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    state
        .sender
        .try_send(backend::Message::RefreshSettings(group, config, tx))
        .map_err(|_| "采集器繁忙，请稍后重试".to_string())?;
    rx.await.map_err(|_| "采集器已停止".to_string())?
}
#[tauri::command]
async fn translate_news(id: String, state: State<'_, Backend>) -> Result<String, String> {
    state.translate(&id).await
}
#[tauri::command]
async fn read_news(keys: Vec<String>, state: State<'_, Backend>) -> Result<(), String> {
    if keys.len() > 256 || keys.iter().any(|key| key.len() > 512) {
        return Err("消息列表无效".into());
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    state
        .sender
        .try_send(backend::Message::ReadNews(keys, tx))
        .map_err(|_| "采集器繁忙，请稍后重试")?;
    tokio::time::timeout(std::time::Duration::from_secs(15), rx)
        .await
        .map_err(|_| "保存超时，请稍后重试")?
        .map_err(|_| "采集器已停止")?
}
#[tauri::command]
async fn get_session_page(
    request: pulse_core::storage::SessionPageRequest,
    state: State<'_, Backend>,
) -> Result<pulse_core::storage::SessionPage, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    state
        .sender
        .try_send(backend::Message::SessionPage(request, tx))
        .map_err(|_| "采集器繁忙，请稍后重试".to_string())?;
    tokio::time::timeout(std::time::Duration::from_secs(15), rx)
        .await
        .map_err(|_| "查询超时，请稍后重试".to_string())?
        .map_err(|_| "采集器已停止".to_string())?
}
#[tauri::command]
async fn get_model_page(
    request: pulse_core::storage::ModelPageRequest,
    state: State<'_, Backend>,
) -> Result<pulse_core::storage::ModelPage, String> {
    request
        .validate()
        .map_err(|_| "模型查询范围无效".to_string())?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    state
        .sender
        .try_send(backend::Message::ModelPage(request, tx))
        .map_err(|_| "采集器繁忙，请稍后重试".to_string())?;
    tokio::time::timeout(std::time::Duration::from_secs(15), rx)
        .await
        .map_err(|_| "查询超时，请稍后重试".to_string())?
        .map_err(|_| "采集器已停止".to_string())?
}
#[tauri::command]
async fn get_model_speed(
    request: pulse_core::storage::ModelSpeedRequest,
    state: State<'_, Backend>,
) -> Result<pulse_core::storage::ModelSpeed, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    state
        .sender
        .try_send(backend::Message::ModelSpeed(request, tx))
        .map_err(|_| "采集器繁忙，请稍后重试".to_string())?;
    tokio::time::timeout(std::time::Duration::from_secs(15), rx)
        .await
        .map_err(|_| "查询超时，请稍后重试".to_string())?
        .map_err(|_| "采集器已停止".to_string())?
}
#[tauri::command]
async fn get_session_detail(
    request: pulse_core::storage::SessionDetailRequest,
    state: State<'_, Backend>,
) -> Result<Option<pulse_core::storage::SessionDetail>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    state
        .sender
        .try_send(backend::Message::SessionDetail(request, tx))
        .map_err(|_| "采集器繁忙，请稍后重试".to_string())?;
    tokio::time::timeout(std::time::Duration::from_secs(15), rx)
        .await
        .map_err(|_| "查询超时，请稍后重试".to_string())?
        .map_err(|_| "采集器已停止".to_string())?
}
#[tauri::command]
fn get_view_state(state: State<'_, platform::WindowHost>) -> Result<platform::ViewState, String> {
    state
        .view
        .lock()
        .map(|v| v.clone())
        .map_err(|_| "界面状态不可用".into())
}
#[tauri::command]
fn remember_view(
    view: platform::ViewState,
    state: State<'_, platform::WindowHost>,
) -> Result<(), String> {
    state.remember_view(view)
}
#[tauri::command]
async fn set_settings(
    settings: Settings,
    app: tauri::AppHandle,
    state: State<'_, Backend>,
) -> Result<(), String> {
    settings.validate()?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    state
        .sender
        .try_send(backend::Message::Settings(settings.clone(), tx))
        .map_err(|_| "采集器繁忙，请稍后重试".to_string())?;
    rx.await.map_err(|_| "采集器已停止".to_string())??;
    platform::apply_settings(&app, &settings);
    Ok(())
}
#[tauri::command]
async fn set_ui_preferences(
    theme: Option<String>,
    language: Option<String>,
    app: tauri::AppHandle,
    state: State<'_, Backend>,
) -> Result<Settings, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    state
        .sender
        .try_send(backend::Message::UiPreferences(theme, language, tx))
        .map_err(|_| "采集器繁忙，请稍后重试".to_string())?;
    let settings = rx.await.map_err(|_| "采集器已停止".to_string())??;
    platform::apply_settings(&app, &settings);
    Ok(settings)
}
#[tauri::command]
async fn window_action(action: String, app: tauri::AppHandle) -> Result<(), String> {
    match action.as_str() {
        "expand" => platform::show_details(&app, None),
        "news" => platform::show_details(&app, Some("news")),
        "compact" => platform::compact_or_hide(&app),
        "settings" => platform::show_details(&app, Some("settings")),
        "exit" => app.exit(0),
        "hide" => platform::hide(&app),
        _ => return Err("unknown window action".into()),
    };
    Ok(())
}
async fn startup_status_on_main(
    app: tauri::AppHandle,
    enabled: Option<bool>,
) -> Result<startup::Status, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let _ = tx.send(startup::update(&handle, enabled));
    })
    .map_err(|_| "登录项服务不可用")?;
    rx.await.map_err(|_| "登录项服务已停止")?
}
#[tauri::command]
async fn get_startup_status(app: tauri::AppHandle) -> Result<startup::Status, String> {
    startup_status_on_main(app, None).await
}
#[tauri::command]
async fn set_startup_enabled(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<startup::Status, String> {
    startup_status_on_main(app, Some(enabled)).await
}
#[tauri::command]
async fn open_startup_settings(app: tauri::AppHandle) -> Result<(), String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(startup::open_settings());
    })
    .map_err(|_| "登录项服务不可用")?;
    rx.await.map_err(|_| "登录项服务已停止")?
}
fn diagnostic_text(backend: &Backend, host: &platform::WindowHost) -> Result<String, String> {
    let window = host.diagnostics()?;
    let snapshot = backend.snapshot.read().map_err(|_| "采集状态不可用")?;
    diagnostics::Report::from_snapshot(&snapshot, window).text()
}
#[tauri::command]
fn get_diagnostics(
    state: State<'_, Backend>,
    host: State<'_, platform::WindowHost>,
) -> Result<String, String> {
    diagnostic_text(&state, &host)
}
#[tauri::command]
async fn copy_diagnostics(
    app: tauri::AppHandle,
    state: State<'_, Backend>,
    host: State<'_, platform::WindowHost>,
) -> Result<String, String> {
    let text = diagnostic_text(&state, &host)?;
    let clipboard_text = text.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let clipboard_app = app.clone();
    app.run_on_main_thread(move || {
        let _ = tx.send(diagnostics::copy(&clipboard_app, &clipboard_text));
    })
    .map_err(|_| "剪贴板服务不可用")?;
    rx.await.map_err(|_| "剪贴板服务已停止")??;
    Ok(text)
}
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            if !args.iter().any(|arg| arg == "--startup") {
                platform::trace_window(app, window_lifecycle::Event::Reopen, None);
                platform::show_details(app, None);
            }
        }))
        .invoke_handler(tauri::generate_handler![
            get_ui_language,
            set_ui_preferences,
            get_snapshot,
            refresh_now,
            set_refresh,
            translate_news,
            read_news,
            get_session_page,
            get_model_page,
            get_model_speed,
            get_session_detail,
            set_settings,
            window_action,
            get_startup_status,
            set_startup_enabled,
            open_startup_settings,
            get_diagnostics,
            copy_diagnostics,
            open_source,
            get_view_state,
            remember_view,
            check_updates,
            get_app_update_info,
            install_app_update,
            open_app_release
        ])
        .setup(|app| {
            app.manage(updates::Service::default());
            app.manage(language::Current(std::sync::Mutex::new(
                language::system_language(),
            )));
            app.manage(platform::WindowHost::default());
            if let Ok(directory) = app.path().app_log_dir() {
                app.state::<platform::WindowHost>()
                    .lifecycle
                    .enable(directory);
                platform::trace_window(app.handle(), window_lifecycle::Event::Started, None);
            }
            let directory = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&directory)?;
            let backend = Backend::start(app.handle().clone(), directory.join("pulse.sqlite"))?;
            let settings = backend
                .snapshot
                .read()
                .map_err(|_| "snapshot lock")?
                .settings
                .clone();
            app.manage(backend);
            platform::setup(app, &settings)?;
            let handle = app.handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                use tauri::Emitter;
                let service = handle.state::<updates::Service>();
                if let Ok(info) = service.startup(&handle) {
                    let next = service.prepare(&handle).unwrap_or(info);
                    let _ = handle.emit("app-update", next);
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                platform::compact_or_hide(window.app_handle());
            }
            tauri::WindowEvent::Moved(position) => {
                platform::moved(window.app_handle(), *position);
            }
            tauri::WindowEvent::ScaleFactorChanged { .. } => {
                platform::scale_changed(window.app_handle())
            }
            #[cfg(target_os = "macos")]
            tauri::WindowEvent::Focused(false) => platform::hide(window.app_handle()),
            tauri::WindowEvent::ThemeChanged(_) => platform::refresh_material(window.app_handle()),
            tauri::WindowEvent::Destroyed => platform::trace_window(
                window.app_handle(),
                window_lifecycle::Event::Destroyed,
                Some(false),
            ),
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("CodexPulse failed to start")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if matches!(event, tauri::RunEvent::Reopen { .. }) {
                platform::trace_window(app, window_lifecycle::Event::Reopen, None);
                platform::show_details(app, None);
            }
            if let tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } = &event
            {
                platform::trace_window(app, window_lifecycle::Event::ExitRequested, None);
                // Destroying the last hidden WebView leaves the native tray and collectors alive.
                api.prevent_exit();
                return;
            }
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) && let Some(state) = app.try_state::<Backend>()
            {
                #[cfg(target_os = "macos")]
                macos::shutdown(app);
                state.shutdown();
            }
        });
}
