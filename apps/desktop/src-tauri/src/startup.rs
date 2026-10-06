//! Login startup is an OS preference, never a cached checkbox in the ledger.
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[cfg(not(target_os = "macos"))]
    Unsupported,
    #[cfg(target_os = "macos")]
    Disabled,
    #[cfg(target_os = "macos")]
    Enabled,
    #[cfg(target_os = "macos")]
    RequiresApproval,
    #[cfg(target_os = "macos")]
    Unavailable,
}

pub fn update(app: &tauri::AppHandle, enabled: Option<bool>) -> Result<Status, String> {
    #[cfg(target_os = "macos")]
    {
        macos::update(app, enabled)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, enabled);
        Ok(Status::Unsupported)
    }
}

pub fn open_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    if macos::modern() {
        // Called on the AppKit main thread, after an explicit UI action.
        unsafe { objc2_service_management::SMAppService::openSystemSettingsLoginItems() };
        return Ok(());
    }
    Err("当前系统不支持此登录项入口".into())
}

#[cfg(target_os = "macos")]
mod macos {
    use super::Status;
    use objc2_service_management::{SMAppService, SMAppServiceStatus};
    use std::{fs, io::Write, path::Path};
    use tauri::Manager;

    const LABEL: &str = "com.codexpulse.desktop";
    const FAILURE: &str = "无法修改登录启动，请检查应用安装位置及系统登录项权限";

    pub fn modern() -> bool {
        objc2_foundation::NSProcessInfo::processInfo()
            .operatingSystemVersion()
            .majorVersion
            >= 13
    }

    fn service_status(service: &SMAppService) -> Status {
        // SMAppService is available only on macOS 13+. The caller checks that first.
        match unsafe { service.status() } {
            SMAppServiceStatus::NotRegistered => Status::Disabled,
            SMAppServiceStatus::Enabled => Status::Enabled,
            SMAppServiceStatus::RequiresApproval => Status::RequiresApproval,
            _ => Status::Unavailable,
        }
    }

    pub fn update(app: &tauri::AppHandle, enabled: Option<bool>) -> Result<Status, String> {
        let exe = std::env::current_exe().map_err(|_| FAILURE)?;
        let bundle = exe.ancestors().nth(3);
        if !bundle.is_some_and(|p| p.extension().is_some_and(|e| e == "app")) {
            return if enabled.is_some() {
                Err("请从完整的 CodexPulse.app 设置登录启动".into())
            } else {
                Ok(Status::Unavailable)
            };
        }
        if enabled == Some(true) && exe.starts_with("/Volumes") {
            return Err("请先将 CodexPulse 复制到应用程序目录".into());
        }
        if modern() {
            // The narrow IPC wrapper dispatches all ServiceManagement work to the main thread.
            let service = unsafe { SMAppService::mainAppService() };
            if let Some(on) = enabled {
                let before = service_status(&service);
                if on && !matches!(before, Status::Enabled | Status::RequiresApproval) {
                    if unsafe { service.registerAndReturnError() }.is_err()
                        && !matches!(
                            service_status(&service),
                            Status::Enabled | Status::RequiresApproval
                        )
                    {
                        return Err(FAILURE.into());
                    }
                } else if !on
                    && before != Status::Disabled
                    && unsafe { service.unregisterAndReturnError() }.is_err()
                    && service_status(&service) != Status::Disabled
                {
                    return Err(FAILURE.into());
                }
            }
            Ok(service_status(&service))
        } else {
            let file = app
                .path()
                .home_dir()
                .map_err(|_| FAILURE)?
                .join("Library/LaunchAgents/com.codexpulse.desktop.plist");
            legacy_update(&file, &exe, enabled)
        }
    }

    fn legacy_document(file: &Path) -> Result<Option<plist::Value>, String> {
        let metadata = match fs::symlink_metadata(file) {
            Ok(value) => value,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(FAILURE.into()),
        };
        // Do not overwrite symlinks, oversized input, or another application's entry.
        if !metadata.is_file() || metadata.len() > 64 * 1024 {
            return Err("登录启动配置无法识别，请检查系统登录项".into());
        }
        let value =
            plist::Value::from_file(file).map_err(|_| "登录启动配置无法识别，请检查系统登录项")?;
        if value
            .as_dictionary()
            .and_then(|d| d.get("Label"))
            .and_then(plist::Value::as_string)
            != Some(LABEL)
        {
            return Err("登录启动配置不属于 CodexPulse".into());
        }
        Ok(Some(value))
    }

    fn legacy_update(file: &Path, exe: &Path, enabled: Option<bool>) -> Result<Status, String> {
        let old = legacy_document(file)?;
        if let Some(on) = enabled {
            if !on {
                if old.is_some() {
                    fs::remove_file(file).map_err(|_| FAILURE)?;
                }
            } else {
                let mut dictionary = plist::Dictionary::new();
                dictionary.insert("Label".into(), LABEL.into());
                dictionary.insert(
                    "ProgramArguments".into(),
                    plist::Value::Array(vec![exe.to_str().ok_or(FAILURE)?.into()]),
                );
                dictionary.insert("RunAtLoad".into(), true.into());
                dictionary.insert(
                    "AssociatedBundleIdentifiers".into(),
                    plist::Value::Array(vec![LABEL.into()]),
                );
                let mut bytes = Vec::new();
                plist::Value::Dictionary(dictionary)
                    .to_writer_xml(&mut bytes)
                    .map_err(|_| FAILURE)?;
                let parent = file.parent().ok_or(FAILURE)?;
                fs::create_dir_all(parent).map_err(|_| FAILURE)?;
                let pending =
                    parent.join(format!(".codexpulse-login-{}.pending", std::process::id()));
                let result = (|| {
                    use std::os::unix::fs::OpenOptionsExt;
                    let mut output = fs::OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .mode(0o600)
                        .open(&pending)
                        .map_err(|_| FAILURE)?;
                    let write = (|| {
                        output.write_all(&bytes).map_err(|_| FAILURE)?;
                        output.sync_all().map_err(|_| FAILURE)?;
                        fs::rename(&pending, file).map_err(|_| FAILURE)
                    })();
                    if write.is_err() {
                        let _ = fs::remove_file(&pending);
                    }
                    write
                })();
                result?;
            }
        }
        let Some(value) = legacy_document(file)? else {
            return Ok(Status::Disabled);
        };
        let dictionary = value.as_dictionary().ok_or(FAILURE)?;
        let target = dictionary
            .get("ProgramArguments")
            .and_then(plist::Value::as_array)
            .and_then(|a| a.first())
            .and_then(plist::Value::as_string);
        if target == exe.to_str()
            && dictionary
                .get("RunAtLoad")
                .and_then(plist::Value::as_boolean)
                == Some(true)
        {
            Ok(Status::Enabled)
        } else {
            Ok(Status::Unavailable)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn legacy_roundtrip_escapes_paths_and_restores_disabled_without_touching_other_entries() {
            let directory = tempfile::tempdir().unwrap();
            let file = directory
                .path()
                .join("LaunchAgents/com.codexpulse.desktop.plist");
            let exe = directory
                .path()
                .join("App & <test>.app/Contents/MacOS/codexpulse");
            assert_eq!(legacy_update(&file, &exe, None).unwrap(), Status::Disabled);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            let other = file.parent().unwrap().join("other.app.plist");
            fs::write(&other, b"another login item").unwrap();
            assert_eq!(
                legacy_update(&file, &exe, Some(true)).unwrap(),
                Status::Enabled
            );
            let data = fs::read(&file).unwrap();
            assert!(std::str::from_utf8(&data).unwrap().contains("&amp;"));
            assert_eq!(legacy_update(&file, &exe, None).unwrap(), Status::Enabled);
            assert_eq!(fs::read(&file).unwrap(), data);
            assert_eq!(
                legacy_update(&file, &exe, Some(false)).unwrap(),
                Status::Disabled
            );
            assert_eq!(fs::read(other).unwrap(), b"another login item");
        }
        #[test]
        fn legacy_refuses_unowned_or_symlinked_configuration() {
            let directory = tempfile::tempdir().unwrap();
            let file = directory.path().join("login.plist");
            fs::write(
                &file,
                b"<plist><dict><key>Label</key><string>other.app</string></dict></plist>",
            )
            .unwrap();
            let original = fs::read(&file).unwrap();
            let exe = Path::new("/Applications/CodexPulse.app/Contents/MacOS/codexpulse");
            assert!(legacy_update(&file, exe, Some(true)).is_err());
            assert!(legacy_update(&file, exe, Some(false)).is_err());
            assert_eq!(fs::read(&file).unwrap(), original);
            let link = directory.path().join("linked.plist");
            std::os::unix::fs::symlink(&file, &link).unwrap();
            assert!(legacy_update(&link, exe, Some(true)).is_err());
            assert_eq!(fs::read(&file).unwrap(), original);
        }
        #[test]
        fn moved_application_is_not_repaired_until_user_enables_it() {
            let directory = tempfile::tempdir().unwrap();
            let file = directory.path().join("login.plist");
            let first = Path::new("/Applications/Old/CodexPulse.app/Contents/MacOS/codexpulse");
            let second = Path::new("/Applications/CodexPulse.app/Contents/MacOS/codexpulse");
            legacy_update(&file, first, Some(true)).unwrap();
            let old = fs::read(&file).unwrap();
            assert_eq!(
                legacy_update(&file, second, None).unwrap(),
                Status::Unavailable
            );
            assert_eq!(fs::read(&file).unwrap(), old);
            assert_eq!(
                legacy_update(&file, second, Some(true)).unwrap(),
                Status::Enabled
            );
        }
    }
}
