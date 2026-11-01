use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    String(String),
    ExpandString(String),
    Dword(u32),
    Binary(Vec<u8>),
    Other,
}

impl Value {
    pub fn dword(&self) -> Option<u32> {
        match self {
            Self::Dword(value) => Some(*value),
            _ => None,
        }
    }
}

pub struct Key(BTreeMap<String, Value>);
impl Key {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.0.get(&name.to_lowercase())
    }
}

fn quoted(text: &str) -> Result<(String, &str), String> {
    let mut chars = text
        .strip_prefix('"')
        .ok_or("invalid quoted registry string")?
        .char_indices();
    let mut value = String::new();
    while let Some((offset, ch)) = chars.next() {
        match ch {
            '"' => return Ok((value, &text[offset + 2..])),
            '\\' => match chars.next() {
                Some((_, next @ ('\\' | '"'))) => value.push(next),
                _ => return Err("invalid registry string escape".into()),
            },
            other => value.push(other),
        }
    }
    Err("unterminated registry string".into())
}

fn hex(text: &str) -> Result<Vec<u8>, String> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    text.split(',')
        .map(|byte| u8::from_str_radix(byte.trim(), 16).map_err(|e| e.to_string()))
        .collect()
}

fn utf16(bytes: &[u8]) -> Result<String, String> {
    if !bytes.len().is_multiple_of(2) {
        return Err("invalid UTF-16 registry data".into());
    }
    let units: Vec<_> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    let units = units.strip_suffix(&[0]).unwrap_or(&units);
    String::from_utf16(units).map_err(|error| error.to_string())
}

pub(super) fn parse_export(bytes: &[u8], key: &str) -> Result<Key, String> {
    let text = utf16(
        bytes
            .strip_prefix(&[0xff, 0xfe])
            .ok_or("registry export has no UTF-16LE BOM")?,
    )?;
    // reg.exe expands every LF to CRLF, including inside quoted REG_SZ values.
    // Undo that once, then split on LF without stripping the value's own CR.
    let text = text.replace("\r\n", "\n");
    let mut lines = text.split('\n');
    if lines.next() != Some("Windows Registry Editor Version 5.00") {
        return Err("unsupported registry export format".into());
    }
    let mut selected = false;
    let mut found = false;
    let mut values = BTreeMap::new();
    while let Some(line) = lines.next() {
        let line = line.trim_start();
        if let Some(section) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            if found {
                break;
            } // Descendant sections must never overwrite this key's values.
            selected = section.eq_ignore_ascii_case(key);
            found |= selected;
            continue;
        }
        if !selected || line.is_empty() || line.starts_with(';') {
            continue;
        }
        let (name, rest) = if let Some(rest) = line.strip_prefix('@') {
            (String::new(), rest)
        } else {
            quoted(line)?
        };
        let mut data = rest
            .strip_prefix('=')
            .ok_or("invalid registry value assignment")?
            .to_owned();
        let value = if data.starts_with('"') {
            let value = loop {
                match quoted(&data) {
                    Ok((value, rest)) => {
                        if !rest.is_empty() {
                            return Err("unexpected data after registry string".into());
                        }
                        break value;
                    }
                    Err(error) if error == "unterminated registry string" => {
                        data.push('\n');
                        data.push_str(lines.next().ok_or("unterminated registry string")?);
                    }
                    Err(error) => return Err(error),
                }
            };
            Value::String(value)
        } else {
            while data.ends_with('\\') {
                data.pop();
                data.push_str(
                    lines
                        .next()
                        .ok_or("incomplete registry hex continuation")?
                        .trim(),
                );
            }
            if let Some(data) = data.strip_prefix("dword:") {
                Value::Dword(u32::from_str_radix(data, 16).map_err(|e| e.to_string())?)
            } else if let Some(data) = data.strip_prefix("hex:") {
                Value::Binary(hex(data)?)
            } else if let Some(data) = data.strip_prefix("hex(1):") {
                Value::String(utf16(&hex(data)?)?)
            } else if let Some(data) = data.strip_prefix("hex(2):") {
                Value::ExpandString(utf16(&hex(data)?)?)
            } else {
                Value::Other
            }
        };
        values.insert(name.to_lowercase(), value);
    }
    if !found {
        return Err("requested key is absent from registry export".into());
    }
    Ok(Key(values))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_exports_and_truncated_data_are_rejected() {
        let key = r"HKEY_CURRENT_USER\Software\Test";
        assert!(parse_export(b"not UTF-16", key).is_err());
        for data in [
            "\"value\"=\"unterminated",
            "\"value\"=hex(2):00",
            "\"value\"=hex:01,\\",
        ] {
            let text = format!("Windows Registry Editor Version 5.00\r\n\r\n[{key}]\r\n{data}");
            let mut bytes = vec![0xff, 0xfe];
            bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
            assert!(parse_export(&bytes, key).is_err(), "{data}");
        }
    }
    #[test]
    fn actual_reg_exe_export_preserves_multiline_unicode_and_value_types() {
        let key = r"HKEY_CURRENT_USER\Software\CodexPulseRegistryTestFixture\目录😀";
        let snapshot = parse_export(include_bytes!("fixtures/values.reg"), key).unwrap();
        assert_eq!(
            snapshot.get("字符串😀"),
            Some(&Value::String(
                "  C:\\中文😀\\with \"quotes\"\\file\r\nnext line  ".into()
            ))
        );
        assert_eq!(
            snapshot.get("quoted\\\"name"),
            Some(&Value::String(String::new()))
        );
        assert_eq!(
            snapshot.get("NUMBER").and_then(Value::dword),
            Some(u32::MAX)
        );
        assert_eq!(
            snapshot.get("Binary"),
            Some(&Value::Binary((0..96).collect()))
        );
        assert_eq!(
            snapshot.get("EmptyBinary"),
            Some(&Value::Binary(Vec::new()))
        );
        assert_eq!(
            snapshot.get("Expanded"),
            Some(&Value::ExpandString(r"%SystemRoot%\应用😀".into()))
        );
        assert!(snapshot.get("missing").is_none());
    }
}
