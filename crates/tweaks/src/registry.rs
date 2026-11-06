//! The real [`Store`]: values under `HKEY_CURRENT_USER`.

use anyhow::{Result, bail};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_BINARY, REG_DWORD, REG_EXPAND_SZ,
    REG_OPTION_NON_VOLATILE, REG_SZ, REG_VALUE_TYPE, RegCloseKey, RegCreateKeyExW, RegDeleteKeyW,
    RegDeleteValueW, RegOpenKeyExW, RegQueryInfoKeyW, RegQueryValueExW, RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR};

use crate::undo::{Store, Value};

pub struct Registry;

/// An open key, closed on drop.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by us and is closed exactly once.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

impl Key {
    /// Opens an existing key; `None` if it does not exist.
    fn open(
        path: &str,
        access: windows::Win32::System::Registry::REG_SAM_FLAGS,
    ) -> Result<Option<Self>> {
        let mut handle = HKEY::default();
        // SAFETY: `handle` is a valid out pointer; the path string outlives the call.
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                &HSTRING::from(path),
                None,
                access,
                &mut handle,
            )
        };
        if status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND {
            return Ok(None);
        }
        check(status)?;
        Ok(Some(Self(handle)))
    }

    /// Opens the key for writing, creating it if needed.
    fn create(path: &str) -> Result<Self> {
        let mut handle = HKEY::default();
        // SAFETY: as in `open`; no class, security attributes or disposition are used.
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                &HSTRING::from(path),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut handle,
                None,
            )
        };
        check(status)?;
        Ok(Self(handle))
    }
}

fn check(status: WIN32_ERROR) -> Result<()> {
    if status.is_err() {
        bail!("registry error {}", status.0);
    }
    Ok(())
}

impl Registry {
    /// The untouched type and bytes, for settings whose undo must be byte-for-byte exact.
    pub fn read_exact(&self, key: &str, name: &str) -> Result<Option<Value>> {
        Ok(self.read_bytes(key, name)?.map(|(kind, bytes)| Value::Raw {
            kind: kind.0,
            bytes,
        }))
    }

    fn read_bytes(&self, key: &str, name: &str) -> Result<Option<(REG_VALUE_TYPE, Vec<u8>)>> {
        let Some(key) = Key::open(key, KEY_QUERY_VALUE)? else {
            return Ok(None);
        };
        let name = HSTRING::from(name);
        let mut kind = REG_VALUE_TYPE::default();
        let mut size = 0u32;
        // SAFETY: size query; the out pointers are valid locals.
        let status =
            unsafe { RegQueryValueExW(key.0, &name, None, Some(&mut kind), None, Some(&mut size)) };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        check(status)?;

        let mut data = vec![0u8; size as usize];
        // SAFETY: `data` holds `size` bytes, as passed in `size`.
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                &name,
                None,
                Some(&mut kind),
                Some(data.as_mut_ptr()),
                Some(&mut size),
            )
        };
        check(status)?;
        data.truncate(size as usize);
        Ok(Some((kind, data)))
    }
}

impl Store for Registry {
    fn read(&self, key: &str, name: &str) -> Result<Option<Value>> {
        let Some((kind, data)) = self.read_bytes(key, name)? else {
            return Ok(None);
        };
        match kind {
            REG_DWORD => Ok(Some(Value::Dword(u32::from_le_bytes(
                data.try_into()
                    .or_else(|_| bail!("malformed DWORD value"))?,
            )))),
            REG_BINARY => Ok(Some(Value::Binary(data))),
            REG_SZ => Ok(Some(Value::String(decode_text(&data)))),
            REG_EXPAND_SZ => Ok(Some(Value::ExpandString(decode_text(&data)))),
            other => bail!("unsupported registry value type {}", other.0),
        }
    }

    fn write(&mut self, key: &str, name: &str, value: &Value) -> Result<()> {
        let key = Key::create(key)?;
        let (kind, bytes) = match value {
            Value::Raw { kind, bytes } => (REG_VALUE_TYPE(*kind), bytes.clone()),
            Value::Dword(v) => (REG_DWORD, v.to_le_bytes().to_vec()),
            Value::Binary(b) => (REG_BINARY, b.clone()),
            Value::String(s) => (REG_SZ, encode_text(s)),
            Value::ExpandString(s) => (REG_EXPAND_SZ, encode_text(s)),
        };
        // SAFETY: the key is open for writing; `bytes` is the value's data.
        check(unsafe { RegSetValueExW(key.0, &HSTRING::from(name), None, kind, Some(&bytes)) })
    }

    fn delete(&mut self, key: &str, name: &str) -> Result<()> {
        let Some(key) = Key::open(key, KEY_SET_VALUE)? else {
            return Ok(());
        };
        // SAFETY: the key is open for writing.
        let status = unsafe { RegDeleteValueW(key.0, &HSTRING::from(name)) };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        check(status)
    }

    fn first_missing(&self, key: &str) -> Result<Option<String>> {
        // Walk down from the root: the first prefix that does not open is the one a write creates.
        let mut path = String::new();
        for part in key.split('\\') {
            if !path.is_empty() {
                path.push('\\');
            }
            path.push_str(part);
            if Key::open(&path, KEY_QUERY_VALUE)?.is_none() {
                return Ok(Some(path));
            }
        }
        Ok(None)
    }

    fn delete_if_empty(&mut self, key: &str) -> Result<()> {
        let Some(open) = Key::open(key, KEY_QUERY_VALUE)? else {
            return Ok(());
        };
        let (mut subkeys, mut values) = (0u32, 0u32);
        // SAFETY: only the two count out pointers are given, both valid locals.
        let status = unsafe {
            RegQueryInfoKeyW(
                open.0,
                None,
                None,
                None,
                Some(&mut subkeys),
                None,
                None,
                Some(&mut values),
                None,
                None,
                None,
                None,
            )
        };
        check(status)?;
        drop(open);
        if subkeys != 0 || values != 0 {
            return Ok(());
        }
        // SAFETY: the path string outlives the call; the key is closed (above).
        let status = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, &HSTRING::from(key)) };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        check(status)
    }
}

/// `REG_SZ` / `REG_EXPAND_SZ` bytes (UTF-16, usually NUL-terminated) as text.
fn decode_text(bytes: &[u8]) -> String {
    let (pairs, _) = bytes.as_chunks::<2>();
    let units: Vec<u16> = pairs.iter().copied().map(u16::from_le_bytes).collect();
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}

/// UTF-16 with the terminating NUL the registry expects.
fn encode_text(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_values_round_trip_with_their_terminator() {
        let bytes = encode_text(r"C:\Cursors\a.cur");
        assert_eq!(&bytes[bytes.len() - 2..], [0, 0]);
        assert_eq!(decode_text(&bytes), r"C:\Cursors\a.cur");
        // An empty string is just the terminator (what the CLSID toggle writes).
        assert_eq!(encode_text(""), [0, 0]);
        assert_eq!(decode_text(&[0, 0]), "");
        assert_eq!(decode_text(&[]), "");
        // Non-ASCII and a missing terminator both decode.
        assert_eq!(decode_text(&encode_text("é")[..2]), "é");
    }
}
