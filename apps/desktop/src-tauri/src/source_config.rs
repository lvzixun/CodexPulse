use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowsSource {
    pub id: String,
    pub label: String,
    pub home: String,
    pub enabled: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WslSource {
    pub id: String,
    pub distro: String,
    pub user: String,
    pub home: String,
    pub enabled: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslTarget {
    pub distro: String,
    pub user: String,
}

pub fn valid_windows_home(home: &str) -> bool {
    home.len() <= 4096 && !home.chars().any(char::is_control) && Path::new(home).is_absolute()
        // WSL roots must use the WSL configuration so running-state checks cannot be bypassed.
        && !home.starts_with(['\\', '/'])
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn valid_segment(value: &str) -> bool {
    !value.trim().is_empty()
        && value == value.trim()
        && value.len() <= 128
        && value
            .chars()
            .all(|c| c.is_alphanumeric() || "-_. ".contains(c))
        && !matches!(value, "." | "..")
}
pub fn valid_linux_home(home: &str) -> bool {
    home.len() <= 4096
        && home.starts_with('/')
        && !home.starts_with("//")
        && !home.contains('\\')
        && !home.chars().any(char::is_control)
        && !home.split('/').any(|p| matches!(p, "." | ".."))
}
impl WslSource {
    pub fn target(&self) -> WslTarget {
        WslTarget {
            distro: self.distro.clone(),
            user: self.user.clone(),
        }
    }
    pub fn path(&self) -> PathBuf {
        unc_home(&self.distro, &self.home)
    }
}
pub fn unc_home(distro: &str, home: &str) -> PathBuf {
    PathBuf::from(format!(
        "\\\\wsl.localhost\\{distro}{}",
        home.replace('/', "\\")
    ))
}
pub fn validate(windows: &[WindowsSource], wsl: &[WslSource]) -> Result<(), String> {
    if windows.len() > 8 || wsl.len() > 8 {
        return Err("每类最多配置 8 个额外来源".into());
    }
    let mut ids = HashSet::new();
    for source in windows {
        if !valid_id(&source.id)
            || !ids.insert(&source.id)
            || source.label.trim().is_empty()
            || source.label.chars().count() > 64
            || source.label.chars().any(char::is_control)
            || !valid_windows_home(&source.home)
        {
            return Err("Windows 来源名称或本地绝对路径无效".into());
        }
    }
    ids.clear();
    for source in wsl {
        if !valid_id(&source.id)
            || !ids.insert(&source.id)
            || !valid_segment(&source.distro)
            || (!source.user.is_empty()
                && (!valid_segment(&source.user) || source.user.contains(' ')))
            || !valid_linux_home(&source.home)
        {
            return Err("请填写有效的 WSL 发行版、用户和 Linux 绝对路径".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_stay_inside_named_wsl_distribution_and_configuration_is_bounded() {
        let wsl = WslSource {
            id: "a".into(),
            distro: "Ubuntu".into(),
            user: "dev".into(),
            home: "/home/dev/.codex".into(),
            enabled: true,
        };
        assert!(validate(&[], std::slice::from_ref(&wsl)).is_ok());
        assert_eq!(
            wsl.path(),
            PathBuf::from(r"\\wsl.localhost\Ubuntu\home\dev\.codex")
        );
        for path in [
            "relative",
            "//other/home",
            "/home/../other",
            "/home/./dev",
            "/home\\other",
            "/home\nother",
        ] {
            assert!(!valid_linux_home(path));
        }
        let mut wrong = wsl.clone();
        wrong.distro = "../Ubuntu".into();
        assert!(validate(&[], &[wrong]).is_err());
        assert!(validate(&[], &[wsl.clone(), wsl.clone()]).is_err());
        assert!(validate(&[], &vec![wsl; 9]).is_err());
        assert!(!valid_windows_home(
            r"\\wsl.localhost\Ubuntu\home\dev\.codex"
        ));
    }
}
