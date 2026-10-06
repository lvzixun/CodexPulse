//! UI language comes from the current user's OS display language, not region or shell locale.
use std::sync::Mutex;
use tauri::Manager;

pub struct Current(pub Mutex<&'static str>);

pub fn refresh_tray(app: &tauri::AppHandle, language: &'static str) {
    let Some(current) = app.try_state::<Current>() else {
        return;
    };
    let Ok(mut previous) = current.0.lock() else {
        return;
    };
    if *previous == language {
        return;
    }
    *previous = language;
    // Dispatch can execute immediately on the main thread. Never keep the lock across it.
    drop(previous);
    let handle = app.clone();
    if app
        .run_on_main_thread(move || {
            use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
            let result = (|| -> tauri::Result<()> {
                let settings = MenuItem::with_id(
                    &handle,
                    "settings",
                    if language == "zh" {
                        "设置"
                    } else {
                        "Settings"
                    },
                    true,
                    None::<&str>,
                )?;
                let exit = MenuItem::with_id(
                    &handle,
                    "exit",
                    if language == "zh" { "退出" } else { "Quit" },
                    true,
                    None::<&str>,
                )?;
                let divider = PredefinedMenuItem::separator(&handle)?;
                let menu = Menu::with_items(&handle, &[&settings, &divider, &exit])?;
                if let Some(tray) = handle.tray_by_id("pulse-tray") {
                    tray.set_menu(Some(menu))?;
                }
                Ok(())
            })();
            if result.is_err()
                && let Some(current) = handle.try_state::<Current>()
                && let Ok(mut value) = current.0.lock()
                && *value == language
            {
                *value = "";
            }
        })
        .is_err()
        && let Ok(mut value) = current.0.lock()
        && *value == language
    {
        *value = "";
    }
}

#[cfg(any(target_os = "macos", test))]
pub fn resolve(tag: &str) -> &'static str {
    match tag.split(['-', '_']).next() {
        Some(primary) if primary.eq_ignore_ascii_case("zh") => "zh",
        _ => "en",
    }
}

pub fn system_language() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        let preferred = objc2_foundation::NSLocale::preferredLanguages();
        preferred
            .firstObject()
            .map(|value| resolve(&value.to_string()))
            .unwrap_or("en")
    }
    #[cfg(target_os = "windows")]
    {
        // The primary language ID is the low ten bits; sublanguages include all Chinese regions.
        let id = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
        if id & 0x03ff == 0x04 { "zh" } else { "en" }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "en"
    }
}

pub fn text(zh: &'static str, en: &'static str) -> &'static str {
    if system_language() == "zh" { zh } else { en }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_variants_and_unsupported_languages() {
        for tag in ["zh", "zh-CN", "ZH_hant_TW", "zh-HK"] {
            assert_eq!(resolve(tag), "zh");
        }
        for tag in ["en", "en-GB", "fr-FR", "ja", "", "zhongwen"] {
            assert_eq!(resolve(tag), "en");
        }
    }
}
