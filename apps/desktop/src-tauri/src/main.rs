#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod backend;
mod geometry;
mod http_quota;
mod inbox;
mod material;
mod news;
mod platform;
mod refresh;
#[cfg(test)]
mod rpc;
mod source_config;

use backend::{Backend, Settings, Snapshot};
use tauri::{Manager, State};
use tauri_plugin_opener::OpenerExt;

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
async fn refresh_now(group: String, state: State<'_, Backend>) -> Result<(), String> {
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
fn get_view_state(
    state: State<'_, platform::WindowsWindowHost>,
) -> Result<platform::ViewState, String> {
    state
        .view
        .lock()
        .map(|v| v.clone())
        .map_err(|_| "界面状态不可用".into())
}
#[tauri::command]
fn remember_view(
    view: platform::ViewState,
    state: State<'_, platform::WindowsWindowHost>,
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
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            platform::show_details(app, None)
        }))
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            refresh_now,
            set_refresh,
            translate_news,
            read_news,
            get_session_page,
            get_session_detail,
            set_settings,
            window_action,
            open_source,
            get_view_state,
            remember_view
        ])
        .setup(|app| {
            app.manage(platform::WindowsWindowHost::default());
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
            tauri::WindowEvent::ThemeChanged(_) => platform::refresh_material(window.app_handle()),
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("CodexPulse failed to start")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } = &event
            {
                // Destroying the last hidden WebView leaves the native tray and collectors alive.
                api.prevent_exit();
                return;
            }
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) && let Some(state) = app.try_state::<Backend>()
            {
                state.shutdown();
            }
        });
}
