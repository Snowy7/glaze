//! Windows-owned registration input, shown only after a user's Connect click. Secrets go straight to Credential Manager.
use anyhow::{Result, ensure};
use windows::{
    Win32::{Foundation::ERROR_CANCELLED, Security::Credentials::*},
    core::PCWSTR,
};

pub(crate) fn prompt(
    caption: &str,
    message: &str,
    initial_id: &str,
    secret_key: Option<&str>,
) -> Result<Option<String>> {
    let caption = caption.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let message = message.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let target = "mypaper/integrations/app-registration\0"
        .encode_utf16()
        .collect::<Vec<_>>();
    let info = CREDUI_INFOW {
        cbSize: std::mem::size_of::<CREDUI_INFOW>() as u32,
        pszCaptionText: PCWSTR(caption.as_ptr()),
        pszMessageText: PCWSTR(message.as_ptr()),
        ..Default::default()
    };
    let mut username = [0_u16; 512];
    for (slot, ch) in username.iter_mut().take(511).zip(initial_id.encode_utf16()) {
        *slot = ch;
    }
    let mut password = [0_u16; 2561];
    // SAFETY: native Windows owns the modal. All writable buffers outlive this synchronous call.
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
    let result = (|| -> Result<Option<String>> {
        if status == ERROR_CANCELLED {
            return Ok(None);
        }
        status.ok()?;
        let id = String::from_utf16(
            &username[..username
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(username.len())],
        )?
        .trim()
        .to_owned();
        ensure!(
            !id.is_empty()
                && id.len() <= 256
                && id.bytes().all(|b| b.is_ascii_graphic())
                && !id.contains(['\r', '\n', '\0']),
            "Enter the registered application's client ID in User name"
        );
        let mut secret = String::from_utf16(
            &password[..password
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(password.len())],
        )?;
        let stored = if let Some(key) = secret_key {
            if !secret.trim().is_empty() {
                super::credentials::write(key, secret.trim())
            } else {
                Ok(())
            }
        } else {
            Ok(())
        };
        // SAFETY: replacing UTF-8 bytes with zero preserves UTF-8.
        unsafe {
            secret.as_bytes_mut().fill(0);
        }
        stored?;
        Ok(Some(id))
    })();
    password.fill(0);
    result
}

pub fn configure(
    service: super::calendar_oauth::Service,
    current_id: &str,
) -> Result<Option<String>> {
    if !current_id.is_empty() {
        return Ok(Some(current_id.into()));
    }
    match service {
        super::calendar_oauth::Service::Google => prompt(
            "Connect Google Calendar",
            "Register a Desktop app in Google Cloud and enable the Calendar API. Paste its client ID in User name, and its optional client secret in Password. Your Google sign-in opens next.",
            "",
            Some("calendar/google/client-secret"),
        ),
        super::calendar_oauth::Service::Microsoft => prompt(
            "Connect Microsoft / Outlook",
            "Register a public desktop app in Microsoft Entra with redirect http://localhost:53683/oauth/callback. Paste the Application (client) ID in User name. Leave Password empty. Your Microsoft sign-in opens next.",
            "",
            None,
        ),
    }
}
