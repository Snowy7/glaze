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
    Ok(format!("mypaper/integrations/{key}\0")
        .encode_utf16()
        .collect())
}

/// Keys: `t3`, `obs`, `grok/<routine-id>`. Values are opaque UTF-8 bytes, never logged.
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

/// An explicit Connect click may collect a one-time code. Windows owns the masked dialog;
/// the result stays in memory until exchanged, and is never saved as a credential itself.
pub fn prompt_t3_pairing() -> Result<Option<Secret>> {
    use windows::Win32::Foundation::ERROR_CANCELLED;
    let title: Vec<u16> = "Connect T3 Code\0".encode_utf16().collect();
    let message: Vec<u16> = "Create a pairing code in T3 Code > Settings > Connections, then paste it below. Allow thread status, usage (diagnostics), and thread controls. Work starts only on your click in mypaper.\0".encode_utf16().collect();
    let target = target("t3-pairing")?;
    let info = CREDUI_INFOW {
        cbSize: std::mem::size_of::<CREDUI_INFOW>() as u32,
        pszCaptionText: PCWSTR(title.as_ptr()),
        pszMessageText: PCWSTR(message.as_ptr()),
        ..Default::default()
    };
    let mut username = [0_u16; 128];
    username[..7].copy_from_slice(&[109, 121, 112, 97, 112, 101, 114]);
    let mut password = [0_u16; 2561];
    // SAFETY: owned UTF-16 buffers remain live for this synchronous Windows-owned prompt.
    let status = unsafe {
        CredUIPromptForCredentialsW(
            Some(&info),
            PCWSTR(target.as_ptr()),
            None,
            0,
            &mut username,
            &mut password,
            None,
            CREDUI_FLAGS_GENERIC_CREDENTIALS
                | CREDUI_FLAGS_ALWAYS_SHOW_UI
                | CREDUI_FLAGS_DO_NOT_PERSIST
                | CREDUI_FLAGS_EXCLUDE_CERTIFICATES,
        )
    };
    let result = if status == ERROR_CANCELLED {
        Ok(None)
    } else {
        status.ok().map_err(anyhow::Error::from).and_then(|()| {
            let length = password
                .iter()
                .position(|ch| *ch == 0)
                .unwrap_or(password.len());
            String::from_utf16(&password[..length])
                .map(|text| Some(Secret(text.into_bytes())))
                .map_err(Into::into)
        })
    };
    password.fill(0);
    result
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
