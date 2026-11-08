//! Per-user Windows Credential Manager storage, shared with Studio.
use anyhow::{Result, ensure};
use windows::{Win32::Security::Credentials::*, core::PCWSTR};

fn target(key: &str) -> Result<Vec<u16>> {
    ensure!(
        !key.is_empty()
            && key.len() <= 128
            && key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_/".contains(c)),
        "invalid credential key"
    );
    Ok(format!("glaze/integrations/{key}\0")
        .encode_utf16()
        .collect())
}

/// Keys: `obs`, `grok/<routine-id>`. Values are opaque UTF-8 bytes, never logged.
pub fn write(key: &str, value: &str) -> Result<()> {
    ensure!(
        value.len() <= 2560 && !value.is_empty(),
        "credential is empty or too long"
    );
    let mut name = target(key)?;
    let mut bytes = value.as_bytes().to_vec();
    let cred = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: windows::core::PWSTR(name.as_mut_ptr()),
        CredentialBlobSize: bytes.len() as u32,
        CredentialBlob: bytes.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        ..Default::default()
    };
    // SAFETY: all credential buffers are owned and remain alive for this synchronous call.
    let result = unsafe { CredWriteW(&cred, 0) };
    bytes.fill(0);
    result?;
    Ok(())
}

pub struct Secret(Vec<u8>);
impl Secret {
    pub fn text(&self) -> Result<&str> {
        Ok(std::str::from_utf8(&self.0)?)
    }
}
impl Drop for Secret {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

pub fn read(key: &str) -> Result<Secret> {
    let name = target(key)?;
    let mut credential = std::ptr::null_mut();
    // SAFETY: Win32 allocates the result; it is copied then freed exactly once below.
    unsafe {
        CredReadW(
            PCWSTR(name.as_ptr()),
            CRED_TYPE_GENERIC,
            None,
            &mut credential,
        )?;
        let cred = &*credential;
        let bytes = if cred.CredentialBlobSize == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(cred.CredentialBlob, cred.CredentialBlobSize as usize)
                .to_vec()
        };
        CredFree(credential.cast());
        Ok(Secret(bytes))
    }
}

pub fn remove(key: &str) -> Result<()> {
    let name = target(key)?;
    // SAFETY: the null-terminated target is alive during the call.
    unsafe {
        CredDeleteW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, None)?;
    }
    Ok(())
}
