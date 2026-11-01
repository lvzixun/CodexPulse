use super::Status;
use crate::registry::{self, Value};
use std::path::Path;

const RUN: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str =
    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const NAME: &str = "CodexPulse";
const FAILURE: &str = "无法修改登录启动，请检查系统启动应用设置";
// Keep the old main-thread serialization after moving subprocess work off the UI thread.
static UPDATE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn read_string(path: &str) -> Result<Option<String>, String> {
    registry::read(path, NAME)?
        .map(|value| value.string())
        .transpose()
}

fn command(exe: &Path) -> Result<String, String> {
    let text = exe.to_str().ok_or(FAILURE)?;
    if !exe.is_absolute()
        || exe
            .file_name()
            .is_none_or(|n| !n.eq_ignore_ascii_case("codexpulse.exe"))
        || text.contains(['"', '\0'])
    {
        return Err(FAILURE.into());
    }
    Ok(format!("\"{text}\" --startup"))
}

fn owned(value: &str) -> bool {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix("\" --startup"))
        .is_some_and(|v| command(Path::new(v)).is_ok())
}

fn update_at(
    run: &str,
    approved: &str,
    exe: &Path,
    enabled: Option<bool>,
) -> Result<Status, String> {
    let expected = command(exe)?;
    let old = read_string(run)?;
    if old.as_deref().is_some_and(|v| !owned(v)) {
        return if enabled.is_some() {
            Err("登录启动配置不属于 CodexPulse".into())
        } else {
            Ok(Status::Unavailable)
        };
    }
    if let Some(on) = enabled {
        if on {
            registry::set(run, NAME, &Value::String(expected.clone()))?;
        } else {
            registry::delete(run, NAME)?;
        }
    }
    match read_string(run)?.as_deref() {
        None => return Ok(Status::Disabled),
        Some(value) if value != expected => return Ok(Status::Unavailable),
        _ => (),
    }
    // Windows owns approval; always read it and never overwrite it.
    match registry::read(approved, NAME)? {
        None => Ok(Status::Enabled),
        Some(Value::Binary(bytes)) if bytes.len() == 12 => {
            match u32::from_le_bytes(bytes[..4].try_into().unwrap()) {
                2 | 6 => Ok(Status::Enabled),
                3 | 7 => Ok(Status::RequiresApproval),
                _ => Ok(Status::Unavailable),
            }
        }
        Some(_) => Ok(Status::Unavailable),
    }
}

pub fn update(enabled: Option<bool>) -> Result<Status, String> {
    let _guard = UPDATE_LOCK.lock().map_err(|_| FAILURE.to_owned())?;
    update_at(
        RUN,
        APPROVED,
        &std::env::current_exe().map_err(|_| FAILURE)?,
        enabled,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    // Windows clock readings can repeat across parallel tests.
    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture(String);
    impl Fixture {
        fn new() -> Self {
            Self(format!(
                r"HKCU\Software\CodexPulseTests\{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
            ))
        }
        fn run(&self) -> String {
            format!(r"{}\Run", self.0)
        }
        fn approved(&self) -> String {
            format!(r"{}\Approval", self.0)
        }
        fn set(&self, path: &str, value: Value) {
            registry::set(path, NAME, &value).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = registry::delete_tree(&self.0);
        }
    }

    #[test]
    fn isolated_registry_roundtrip_unicode_spaces_and_moved_executable() {
        let f = Fixture::new();
        let exe = Path::new(r"C:\应用 空格 😀\codexpulse.exe");
        assert_eq!(
            update_at(&f.run(), &f.approved(), exe, None).unwrap(),
            Status::Disabled
        );
        assert_eq!(
            update_at(&f.run(), &f.approved(), exe, Some(true)).unwrap(),
            Status::Enabled
        );
        let original = read_string(&f.run()).unwrap();
        let moved = Path::new(r"C:\new\codexpulse.exe");
        assert_eq!(
            update_at(&f.run(), &f.approved(), moved, None).unwrap(),
            Status::Unavailable
        );
        assert_eq!(read_string(&f.run()).unwrap(), original);
        assert_eq!(
            update_at(&f.run(), &f.approved(), moved, Some(true)).unwrap(),
            Status::Enabled
        );
        assert_eq!(
            update_at(&f.run(), &f.approved(), moved, Some(false)).unwrap(),
            Status::Disabled
        );
        assert_eq!(
            update_at(&f.run(), &f.approved(), moved, Some(false)).unwrap(),
            Status::Disabled
        );
    }

    #[test]
    fn rejects_invalid_and_unowned_commands() {
        for value in [
            r"C:\other.exe",
            r"relative\codexpulse.exe",
            "C:\\bad\"\\codexpulse.exe",
        ] {
            assert!(command(Path::new(value)).is_err());
        }
        assert!(!owned(r#""C:\other.exe" --startup"#));
        assert!(!owned(r#""C:\codexpulse.exe" --other"#));
        let f = Fixture::new();
        f.set(&f.run(), Value::String("another application".into()));
        let exe = Path::new(r"C:\CodexPulse\codexpulse.exe");
        assert_eq!(
            update_at(&f.run(), &f.approved(), exe, None).unwrap(),
            Status::Unavailable
        );
        assert!(update_at(&f.run(), &f.approved(), exe, Some(true)).is_err());
        assert!(update_at(&f.run(), &f.approved(), exe, Some(false)).is_err());
        assert_eq!(
            read_string(&f.run()).unwrap().as_deref(),
            Some("another application")
        );
    }

    #[test]
    fn system_disabled_approval_is_read_only_and_unknown_records_are_unavailable() {
        let f = Fixture::new();
        let exe = Path::new(r"C:\CodexPulse\codexpulse.exe");
        update_at(&f.run(), &f.approved(), exe, Some(true)).unwrap();
        let mut bytes = vec![0; 12];
        bytes[0] = 3;
        f.set(&f.approved(), Value::Binary(bytes.clone()));
        assert_eq!(
            update_at(&f.run(), &f.approved(), exe, None).unwrap(),
            Status::RequiresApproval
        );
        assert_eq!(
            update_at(&f.run(), &f.approved(), exe, Some(true)).unwrap(),
            Status::RequiresApproval
        );
        assert_eq!(
            registry::read(&f.approved(), NAME).unwrap(),
            Some(Value::Binary(bytes))
        );
        f.set(&f.approved(), Value::Binary(vec![1, 2]));
        assert_eq!(
            update_at(&f.run(), &f.approved(), exe, None).unwrap(),
            Status::Unavailable
        );
        assert_eq!(
            update_at(&f.run(), &f.approved(), exe, Some(false)).unwrap(),
            Status::Disabled
        );
    }

    #[test]
    fn accepts_expand_string_run_values_from_other_launchers() {
        let f = Fixture::new();
        let exe = Path::new(r"C:\CodexPulse\codexpulse.exe");
        f.set(&f.run(), Value::ExpandString(command(exe).unwrap()));
        assert_eq!(
            update_at(&f.run(), &f.approved(), exe, None).unwrap(),
            Status::Enabled
        );
        assert_eq!(
            update_at(&f.run(), &f.approved(), exe, Some(false)).unwrap(),
            Status::Disabled
        );
    }
}
