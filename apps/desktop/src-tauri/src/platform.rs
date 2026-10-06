use crate::backend::{Backend, Settings};
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use tauri::{
    Emitter, LogicalSize, Manager, Theme,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
pub fn setup(app: &tauri::App, settings: &Settings) -> Result<(), Box<dyn std::error::Error>> {
    let settings_item = MenuItem::with_id(app, "settings", "设置", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let exit = MenuItem::with_id(app, "exit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&settings_item, &separator, &exit])?;
    TrayIconBuilder::new()
        .icon(app.default_window_icon().ok_or("missing icon")?.clone())
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
                show_details(tray.app_handle(), None);
            }
        })
        .build(app)?;
    apply_settings(app.handle(), settings);
    if let Some(w) = app.get_webview_window("pulse") {
        if let Some((x, y)) = settings.compact_position {
            let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
        } else if let Ok(Some(m)) = w.primary_monitor() {
            let scale = m.scale_factor();
            let size = m.size();
            let origin = m.position();
            let _ = w.set_position(tauri::PhysicalPosition::new(
                origin.x + size.width as i32 - (360.0 * scale) as i32,
                origin.y + (64.0 * scale) as i32,
            ));
        }
        if settings.floating {
            let _ = w.show();
        }
    }
    Ok(())
}
pub fn apply_settings(app: &tauri::AppHandle, settings: &Settings) {
    if let Some(w) = app.get_webview_window("pulse") {
        let _ = w.set_always_on_top(settings.always_on_top);
        let _ = w.set_theme(match settings.theme.as_str() {
            "dark" => Some(Theme::Dark),
            "light" => Some(Theme::Light),
            _ => None,
        });
        let effects = if settings.glass {
            Some(tauri::utils::config::WindowEffectsConfig {
                effects: vec![tauri::utils::WindowEffect::Acrylic],
                ..Default::default()
            })
        } else {
            None
        };
        let supported = w.set_effects(effects).is_ok();
        let _ = app.emit("glass-supported", supported);
        // A settings window remains open when the optional float is disabled.
        let _ = app.emit("settings-applied", settings);
    }
}
pub fn show_details(app: &tauri::AppHandle, page: Option<&str>) {
    if let Some(w) = app.get_webview_window("pulse") {
        let _ = w.set_size(LogicalSize::new(496.0, 700.0));
        let _ = w.center();
        let _ = w.show();
        let _ = w.set_focus();
        let _ = app.emit("window-mode", ("details", page));
        let _ = app.emit("snapshot-changed", ());
    }
}
pub fn compact_or_hide(app: &tauri::AppHandle) {
    let settings = app
        .state::<Backend>()
        .snapshot
        .read()
        .ok()
        .map(|s| s.settings.clone())
        .unwrap_or_default();
    if let Some(w) = app.get_webview_window("pulse") {
        if settings.floating {
            let _ = w.set_size(LogicalSize::new(336.0, 268.0));
            if let Some((x, y)) = settings.compact_position {
                let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
            }
            let _ = app.emit("window-mode", ("compact", None::<String>));
            let _ = w.show();
        } else {
            let _ = w.hide();
        }
    }
}
fn command_output(mut cmd: Command) -> Option<String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
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
pub fn running_wsl_sources() -> Vec<(String, String, PathBuf)> {
    #[cfg(not(windows))]
    {
        return Vec::new();
    }
    #[cfg(windows)]
    {
        let mut list = Command::new("wsl.exe");
        list.args(["--list", "--running", "--quiet"]);
        let Some(list) = command_output(list) else {
            return Vec::new();
        };
        list.lines()
            .map(str::trim)
            .filter(|name| !name.is_empty() && !name.contains(['\\', '/']))
            .filter_map(|name| {
                let mut home = Command::new("wsl.exe");
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
                if !home.starts_with('/') || home.contains(['\r', '\n']) {
                    return None;
                }
                Some((
                    format!("wsl:{name}"),
                    format!("WSL · {name}"),
                    PathBuf::from(format!(
                        "\\\\wsl.localhost\\{name}{}",
                        home.replace('/', "\\")
                    )),
                ))
            })
            .collect()
    }
}
