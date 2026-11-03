//! The real [`Store`]: values under `HKEY_CURRENT_USER`.

use anyhow::{Result, bail};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_BINARY, REG_DWORD,
    REG_OPTION_NON_VOLATILE, REG_VALUE_TYPE, RegCloseKey, RegCreateKeyExW, RegDeleteValueW,
    RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
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

impl Store for Registry {
    fn read(&self, key: &str, name: &str) -> Result<Option<Value>> {
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
        match kind {
            REG_DWORD => Ok(Some(Value::Dword(u32::from_le_bytes(
                data.try_into()
                    .or_else(|_| bail!("malformed DWORD value"))?,
            )))),
            REG_BINARY => Ok(Some(Value::Binary(data))),
            other => bail!("unsupported registry value type {}", other.0),
        }
    }

    fn write(&mut self, key: &str, name: &str, value: &Value) -> Result<()> {
        let key = Key::create(key)?;
        let (kind, bytes) = match value {
            Value::Dword(v) => (REG_DWORD, v.to_le_bytes().to_vec()),
            Value::Binary(b) => (REG_BINARY, b.clone()),
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
}
