//! All application registry access goes through the system reg.exe, including reads.
use std::{ffi::OsString, path::PathBuf, process::Output};

mod export;
use export::parse_export;
pub use export::{Key, Value};

impl Value {
    pub fn string(&self) -> Result<String, String> {
        match self {
            Self::String(value) => Ok(value.clone()),
            Self::ExpandString(value) => expand(value),
            _ => Err("registry value is not a string".into()),
        }
    }
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
    fn ExpandEnvironmentStringsW(source: *const u16, dest: *mut u16, size: u32) -> u32;
    fn GetOEMCP() -> u32;
    fn MultiByteToWideChar(
        page: u32,
        flags: u32,
        source: *const u8,
        length: i32,
        dest: *mut u16,
        size: i32,
    ) -> i32;
}

fn executable() -> Result<PathBuf, String> {
    use std::os::windows::ffi::OsStringExt;
    let mut path = vec![0u16; 260];
    loop {
        let size = unsafe { GetSystemDirectoryW(path.as_mut_ptr(), path.len() as u32) } as usize;
        if size == 0 {
            return Err(format!(
                "GetSystemDirectoryW: {}",
                std::io::Error::last_os_error()
            ));
        }
        if size < path.len() {
            return Ok(PathBuf::from(OsString::from_wide(&path[..size])).join("reg.exe"));
        }
        path.resize(size + 1, 0);
    }
}

fn run(args: &[&str]) -> Result<Output, String> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new(executable()?)
        .args(args)
        .creation_flags(0x08000000) // CREATE_NO_WINDOW, with no shell or PATH lookup.
        .output()
        .map_err(|error| format!("reg.exe could not start: {error}"))
}

fn error(operation: &str, output: &Output) -> String {
    // Only error output is included; query/export data may contain proxy credentials.
    let bytes = &output.stderr;
    let mut text = vec![0u16; bytes.len()];
    let size = unsafe {
        MultiByteToWideChar(
            GetOEMCP(),
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            text.as_mut_ptr(),
            text.len() as i32,
        )
    };
    let message = if size > 0 {
        String::from_utf16_lossy(&text[..size as usize])
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    format!(
        "reg.exe {operation} failed (exit {:?}): {}",
        output.status.code(),
        message.trim()
    )
}

fn canonical(key: &str) -> Result<String, String> {
    let (root, rest) = key.split_once('\\').unwrap_or((key, ""));
    let root = match root.to_ascii_uppercase().as_str() {
        "HKCU" | "HKEY_CURRENT_USER" => "HKEY_CURRENT_USER",
        "HKLM" | "HKEY_LOCAL_MACHINE" => "HKEY_LOCAL_MACHINE",
        "HKCR" | "HKEY_CLASSES_ROOT" => "HKEY_CLASSES_ROOT",
        "HKU" | "HKEY_USERS" => "HKEY_USERS",
        "HKCC" | "HKEY_CURRENT_CONFIG" => "HKEY_CURRENT_CONFIG",
        _ => return Err("invalid registry root".into()),
    };
    Ok(if rest.is_empty() {
        root.into()
    } else {
        format!("{root}\\{rest}")
    })
}

fn missing_key(key: &str) -> Result<bool, String> {
    let Some((parent, name)) = key.rsplit_once('\\') else {
        return Ok(false); // A predefined hive is never classified as absent.
    };
    let output = run(&["query", parent, "/f", name, "/k", "/e"])?;
    if output.status.success() {
        return Ok(false);
    }
    // reg query's exact key search reports no matches with exit 1, a summary on
    // stdout, and no stderr. Do not match translated error/summary text.
    if output.status.code() == Some(1)
        && output.stderr.iter().all(u8::is_ascii_whitespace)
        && !output.stdout.iter().all(u8::is_ascii_whitespace)
    {
        return Ok(true);
    }
    if missing_key(parent)? {
        Ok(true)
    } else {
        Err(error("query", &output))
    }
}

pub fn read_key(key: &str) -> Result<Option<Key>, String> {
    let key = canonical(key)?;
    let dir = tempfile::tempdir().map_err(|error| error.to_string())?;
    let path = dir.path().join("values.reg");
    let file = path
        .to_str()
        .ok_or("registry snapshot path is not Unicode")?;
    let output = run(&["export", &key, file, "/y"])?;
    if !output.status.success() {
        return if missing_key(&key)? {
            Ok(None)
        } else {
            Err(error("export", &output))
        };
    }
    // reg query uses a console code page, which can lose characters in paths.
    // Export is UTF-16LE and preserves strings independently of the Windows locale.
    let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
    parse_export(&bytes, &key).map(Some)
}

pub fn read(key: &str, name: &str) -> Result<Option<Value>, String> {
    Ok(read_key(key)?.and_then(|key| key.get(name).cloned()))
}

pub fn set(key: &str, name: &str, value: &Value) -> Result<(), String> {
    let key = canonical(key)?;
    let (kind, data) = match value {
        Value::String(value) => ("REG_SZ", value.clone()),
        Value::ExpandString(value) => ("REG_EXPAND_SZ", value.clone()),
        Value::Dword(value) => ("REG_DWORD", format!("0x{value:x}")),
        Value::Binary(value) => (
            "REG_BINARY",
            value.iter().map(|b| format!("{b:02x}")).collect(),
        ),
        Value::Other => return Err("unsupported registry value type".into()),
    };
    let output = run(&["add", &key, "/v", name, "/t", kind, "/d", &data, "/f"])?;
    if output.status.success() {
        Ok(())
    } else {
        Err(error("add", &output))
    }
}

pub fn delete(key: &str, name: &str) -> Result<(), String> {
    let key = canonical(key)?;
    let output = run(&["delete", &key, "/v", name, "/f"])?;
    if output.status.success() || read(&key, name)?.is_none() {
        Ok(())
    } else {
        Err(error("delete", &output))
    }
}

#[cfg(test)]
pub fn delete_tree(key: &str) -> Result<(), String> {
    let output = run(&["delete", &canonical(key)?, "/f"])?;
    if output.status.success() {
        Ok(())
    } else {
        Err(error("delete", &output))
    }
}

fn expand(value: &str) -> Result<String, String> {
    let source: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
    let mut dest = vec![0u16; 256];
    loop {
        let size = unsafe {
            ExpandEnvironmentStringsW(source.as_ptr(), dest.as_mut_ptr(), dest.len() as u32)
        } as usize;
        if size == 0 {
            return Err(format!(
                "ExpandEnvironmentStringsW: {}",
                std::io::Error::last_os_error()
            ));
        }
        if size <= dest.len() {
            return String::from_utf16(&dest[..size - 1]).map_err(|error| error.to_string());
        }
        dest.resize(size, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(String);
    impl Fixture {
        fn new() -> Self {
            Self(format!(
                r"HKCU\Software\CodexPulseRegistryTests\{}-{}\目录😀",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let parent = self.0.rsplit_once('\\').unwrap().0;
            let _ = delete_tree(parent);
        }
    }

    #[test]
    fn real_reg_exe_preserves_unicode_types_and_exact_key_scope() {
        let f = Fixture::new();
        assert!(read_key(&f.0).unwrap().is_none());
        let text = "  C:\\中文😀\\with \"quotes\"\\file\r\nnext line  ";
        let binary: Vec<u8> = (0..96).collect(); // Forces wrapped hex in the export.
        let expanded = r"%SystemRoot%\应用😀";
        for (name, value) in [
            ("字符串😀", Value::String(text.into())),
            ("quoted\\\"name", Value::String(String::new())),
            ("Number", Value::Dword(u32::MAX)),
            ("Binary", Value::Binary(binary.clone())),
            ("EmptyBinary", Value::Binary(Vec::new())),
            ("Expanded", Value::ExpandString(expanded.into())),
        ] {
            set(&f.0, name, &value).unwrap();
        }
        set(&format!(r"{}\child", f.0), "Number", &Value::Dword(1)).unwrap();
        let snapshot = read_key(&f.0).unwrap().unwrap();
        assert_eq!(snapshot.get("字符串😀").unwrap().string().unwrap(), text);
        assert_eq!(
            snapshot.get("quoted\\\"name"),
            Some(&Value::String(String::new()))
        );
        assert_eq!(
            snapshot.get("NUMBER").and_then(Value::dword),
            Some(u32::MAX)
        );
        assert_eq!(snapshot.get("Binary"), Some(&Value::Binary(binary)));
        assert_eq!(
            snapshot.get("EmptyBinary"),
            Some(&Value::Binary(Vec::new()))
        );
        assert_eq!(
            snapshot.get("Expanded"),
            Some(&Value::ExpandString(expanded.into()))
        );
        assert_eq!(
            snapshot.get("Expanded").unwrap().string().unwrap(),
            format!("{}\\应用😀", std::env::var("SystemRoot").unwrap())
        );
        assert!(snapshot.get("missing").is_none());
        delete(&f.0, "Number").unwrap();
        delete(&f.0, "Number").unwrap();
        assert!(read(&f.0, "Number").unwrap().is_none());
        assert_eq!(
            read(&format!(r"{}\child", f.0), "Number").unwrap(),
            Some(Value::Dword(1))
        );
    }

    #[test]
    fn empty_keys_are_distinct_from_missing_keys_and_values() {
        let f = Fixture::new();
        let result = run(&["add", &f.0, "/f"]).unwrap();
        assert!(result.status.success());
        assert!(read_key(&f.0).unwrap().is_some());
        assert!(read(&f.0, "absent").unwrap().is_none());
        assert!(
            read_key(&format!(r"{}\missing\nested", f.0))
                .unwrap()
                .is_none()
        );
        assert!(read_key("NOT_A_HIVE\\test").is_err());
    }
}
