#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod backend;
mod platform;
mod rpc;

use backend::{Backend, Settings, Snapshot};
use tauri::{Emitter, Manager, State};

#[tauri::command]
fn get_snapshot(state: State<'_, Backend>) -> Result<Snapshot, String> {
    state
        .snapshot
        .read()
        .map(|s| s.clone())
        .map_err(|_| "snapshot unavailable".into())
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
fn window_action(action: String, app: tauri::AppHandle) -> Result<(), String> {
    match action.as_str() {
        "expand" => platform::show_details(&app, None),
        "compact" => platform::compact_or_hide(&app),
        "settings" => platform::show_details(&app, Some("settings")),
        "exit" => app.exit(0),
        "hide" => {
            if let Some(w) = app.get_webview_window("pulse") {
                w.hide().map_err(|e| e.to_string())?;
            }
        }
        _ => return Err("unknown window action".into()),
    };
    Ok(())
}
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            platform::show_details(app, None)
        }))
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            set_settings,
            window_action
        ])
        .setup(|app| {
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
                let _ = window.app_handle().emit("window-position", position);
            }
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("CodexPulse failed to start")
        .run(|app, event| {
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                if let Some(state) = app.try_state::<Backend>() {
                    state.shutdown();
                }
            }
        });
}
