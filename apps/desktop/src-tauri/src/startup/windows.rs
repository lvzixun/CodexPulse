use super::Status;
use std::{os::windows::ffi::OsStrExt, path::Path};
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
    System::Registry::*,
};

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const NAME: &str = "CodexPulse";
const FAILURE: &str = "无法修改登录启动，请检查系统启动应用设置";
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
fn read(path: &str, kind: u32) -> Result<Option<Vec<u8>>, String> {
    let mut buffer = vec![0u8; 32768];
    let mut length = buffer.len() as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide(path).as_ptr(),
            wide(NAME).as_ptr(),
            kind,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut length,
        )
    };
    match result {
        ERROR_FILE_NOT_FOUND => Ok(None),
        ERROR_SUCCESS => {
            buffer.truncate(length as usize);
            Ok(Some(buffer))
        }
        _ => Err(FAILURE.into()),
    }
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
fn decode(bytes: &[u8]) -> Result<String, String> {
    if !bytes.len().is_multiple_of(2) {
        return Err(FAILURE.into());
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let units = units.strip_suffix(&[0]).ok_or(FAILURE)?;
    String::from_utf16(units).map_err(|_| FAILURE.into())
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
    let old = read(run, RRF_RT_REG_SZ)?.map(|v| decode(&v)).transpose()?;
    if old.as_deref().is_some_and(|v| !owned(v)) {
        return if enabled.is_some() {
            Err("登录启动配置不属于 CodexPulse".into())
        } else {
            Ok(Status::Unavailable)
        };
    }
    if let Some(on) = enabled {
        let mut handle = std::ptr::null_mut();
        let result = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                wide(run).as_ptr(),
                0,
                std::ptr::null(),
                0,
                KEY_SET_VALUE,
                std::ptr::null(),
                &mut handle,
                std::ptr::null_mut(),
            )
        };
        if result != ERROR_SUCCESS {
            return Err(FAILURE.into());
        }
        let key = Key(handle);
        let result = if on {
            let value: Vec<u16> = std::ffi::OsStr::new(&expected)
                .encode_wide()
                .chain(Some(0))
                .collect();
            unsafe {
                RegSetValueExW(
                    key.0,
                    wide(NAME).as_ptr(),
                    0,
                    REG_SZ,
                    value.as_ptr().cast(),
                    (value.len() * 2) as u32,
                )
            }
        } else {
            unsafe { RegDeleteValueW(key.0, wide(NAME).as_ptr()) }
        };
        if result != ERROR_SUCCESS && !(result == ERROR_FILE_NOT_FOUND && !on) {
            return Err(FAILURE.into());
        }
    }
    let current = read(run, RRF_RT_REG_SZ)?.map(|v| decode(&v)).transpose()?;
    match current.as_deref() {
        None => return Ok(Status::Disabled),
        Some(v) if v != expected => return Ok(Status::Unavailable),
        _ => (),
    }
    // Windows owns this compatibility record. Read it conservatively; never rewrite approval.
    match read(approved, RRF_RT_REG_BINARY)? {
        None => Ok(Status::Enabled),
        Some(v) if v.len() == 12 => match u32::from_le_bytes(v[..4].try_into().unwrap()) {
            2 | 6 => Ok(Status::Enabled),
            3 | 7 => Ok(Status::RequiresApproval),
            _ => Ok(Status::Unavailable),
        },
        Some(_) => Ok(Status::Unavailable),
    }
}
pub fn update(enabled: Option<bool>) -> Result<Status, String> {
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
    struct Fixture(String);
    impl Fixture {
        fn set(&self, path: &str, kind: u32, bytes: &[u8]) {
            let mut handle = std::ptr::null_mut();
            assert_eq!(
                unsafe {
                    RegCreateKeyExW(
                        HKEY_CURRENT_USER,
                        wide(path).as_ptr(),
                        0,
                        std::ptr::null(),
                        0,
                        KEY_SET_VALUE,
                        std::ptr::null(),
                        &mut handle,
                        std::ptr::null_mut(),
                    )
                },
                ERROR_SUCCESS
            );
            let key = Key(handle);
            assert_eq!(
                unsafe {
                    RegSetValueExW(
                        key.0,
                        wide(NAME).as_ptr(),
                        0,
                        kind,
                        bytes.as_ptr(),
                        bytes.len() as u32,
                    )
                },
                ERROR_SUCCESS
            );
        }
        fn new() -> Self {
            Self(format!(
                r"Software\CodexPulseTests\{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
        }
        fn run(&self) -> String {
            format!(r"{}\Run", self.0)
        }
        fn approved(&self) -> String {
            format!(r"{}\Approval", self.0)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            unsafe {
                RegDeleteTreeW(HKEY_CURRENT_USER, wide(&self.0).as_ptr());
            }
        }
    }
    #[test]
    fn isolated_registry_roundtrip_unicode_spaces_and_moved_executable() {
        let f = Fixture::new();
        let exe = Path::new(r"C:\应用 空格\codexpulse.exe");
        assert_eq!(
            update_at(&f.run(), &f.approved(), exe, None).unwrap(),
            Status::Disabled
        );
        assert_eq!(
            update_at(&f.run(), &f.approved(), exe, Some(true)).unwrap(),
            Status::Enabled
        );
        let original = read(&f.run(), RRF_RT_REG_SZ).unwrap();
        let moved = Path::new(r"C:\new\codexpulse.exe");
        assert_eq!(
            update_at(&f.run(), &f.approved(), moved, None).unwrap(),
            Status::Unavailable
        );
        assert_eq!(read(&f.run(), RRF_RT_REG_SZ).unwrap(), original);
        assert_eq!(
            update_at(&f.run(), &f.approved(), moved, Some(true)).unwrap(),
            Status::Enabled
        );
        assert_eq!(
            update_at(&f.run(), &f.approved(), moved, Some(false)).unwrap(),
            Status::Disabled
        );
    }
    #[test]
    fn rejects_invalid_and_unowned_commands() {
        for v in [
            r"C:\other.exe",
            r"relative\codexpulse.exe",
            "C:\\bad\"\\codexpulse.exe",
        ] {
            assert!(command(Path::new(v)).is_err());
        }
        assert!(!owned(r#""C:\other.exe" --startup"#));
        assert!(!owned(r#""C:\codexpulse.exe" --other"#));
        assert!(decode(&[0]).is_err());
        let f = Fixture::new();
        let bytes: Vec<u8> = wide("another application")
            .iter()
            .flat_map(|n| n.to_le_bytes())
            .collect();
        f.set(&f.run(), REG_SZ, &bytes);
        let exe = Path::new(r"C:\CodexPulse\codexpulse.exe");
        assert_eq!(
            update_at(&f.run(), &f.approved(), exe, None).unwrap(),
            Status::Unavailable
        );
        assert!(update_at(&f.run(), &f.approved(), exe, Some(true)).is_err());
        assert!(update_at(&f.run(), &f.approved(), exe, Some(false)).is_err());
        assert_eq!(read(&f.run(), RRF_RT_REG_SZ).unwrap(), Some(bytes));
    }
    #[test]
    fn system_disabled_approval_is_read_only_and_unknown_records_are_unavailable() {
        let f = Fixture::new();
        let exe = Path::new(r"C:\CodexPulse\codexpulse.exe");
        update_at(&f.run(), &f.approved(), exe, Some(true)).unwrap();
        let mut bytes = vec![0; 12];
        bytes[0] = 3;
        f.set(&f.approved(), REG_BINARY, &bytes);
        assert_eq!(
            update_at(&f.run(), &f.approved(), exe, None).unwrap(),
            Status::RequiresApproval
        );
        assert_eq!(
            update_at(&f.run(), &f.approved(), exe, Some(true)).unwrap(),
            Status::RequiresApproval
        );
        assert_eq!(read(&f.approved(), RRF_RT_REG_BINARY).unwrap(), Some(bytes));
        f.set(&f.approved(), REG_BINARY, &[1, 2]);
        assert_eq!(
            update_at(&f.run(), &f.approved(), exe, None).unwrap(),
            Status::Unavailable
        );
        assert_eq!(
            update_at(&f.run(), &f.approved(), exe, Some(false)).unwrap(),
            Status::Disabled
        );
    }
}
