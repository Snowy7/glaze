//! Night light through its CloudStore state blob, which Settings writes and the display stack
//! follows (`HKCU\...\CloudStore\...\windows.data.bluelightreduction.bluelightreductionstate`).
//!
//! **Undocumented**: the blob is CloudStore's tagged binary format. As Settings writes it on
//! Windows 11 26H2: a `CB 01 00` header, `0A 02 01 00`, `2A 06` + the write time (Unix seconds,
//! LEB128), `2A 2B 0E` + the inner record's length and the inner record, then `00 00 00 00`. The
//! inner record is another header, `10 00` when night light is on, then `D0 0A 02 C6 14` + the
//! change time (FILETIME, LEB128) and `00`. A new write time is what makes Windows apply it.

use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_BINARY, RRF_RT_REG_BINARY, RegGetValueW, RegSetKeyValueW,
};
use windows::core::w;

const KEY: windows::core::PCWSTR = w!(
    r"Software\Microsoft\Windows\CurrentVersion\CloudStore\Store\DefaultAccount\Current\default$windows.data.bluelightreduction.bluelightreductionstate\windows.data.bluelightreduction.bluelightreductionstate"
);
const HEADER: [u8; 4] = [0x43, 0x42, 0x01, 0x00];
const ON: [u8; 2] = [0x10, 0x00];

/// Whether night light is on, or `None` when the blob is missing or unreadable.
pub fn read() -> Option<bool> {
    parse(&blob()?)
}

/// Turns night light on or off as Settings does, and reads it back.
pub fn set(on: bool) -> Result<(), String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    // FILETIME: 100 ns since 1601.
    let filetime = now.as_nanos() as u64 / 100 + 116_444_736_000_000_000;
    let data = encode(on, now.as_secs(), filetime);
    // SAFETY: the pointer and size describe `data`, alive for the call.
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            KEY,
            w!("Data"),
            REG_BINARY.0,
            Some(data.as_ptr().cast()),
            data.len() as u32,
        )
    }
    .ok()
    .map_err(|e| format!("Night light: {e}"))?;
    super::confirm("night light", on, read())
}

fn blob() -> Option<Vec<u8>> {
    // SAFETY: two-call pattern; the buffer is as large as the size the first call reported.
    unsafe {
        let mut bytes = 0u32;
        RegGetValueW(
            HKEY_CURRENT_USER,
            KEY,
            w!("Data"),
            RRF_RT_REG_BINARY,
            None,
            None,
            Some(&mut bytes),
        )
        .ok()
        .ok()?;
        let mut data = vec![0u8; bytes as usize];
        RegGetValueW(
            HKEY_CURRENT_USER,
            KEY,
            w!("Data"),
            RRF_RT_REG_BINARY,
            None,
            Some(data.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
        .ok()
        .ok()?;
        data.truncate(bytes as usize);
        Some(data)
    }
}

/// Whether `blob` says night light is on.
fn parse(blob: &[u8]) -> Option<bool> {
    let at = blob.windows(3).position(|w| w == [0x2A, 0x2B, 0x0E])? + 3;
    let len = *blob.get(at)?;
    // A record this small never needs a second length byte.
    if len & 0x80 != 0 {
        return None;
    }
    let inner = blob.get(at + 1..at + 1 + usize::from(len))?;
    (inner.get(..4)? == HEADER).then_some(())?;
    Some(inner.get(4..6) == Some(&ON[..]))
}

/// The blob Settings writes for `on` at `unix` seconds (and `filetime`).
fn encode(on: bool, unix: u64, filetime: u64) -> Vec<u8> {
    let mut inner = HEADER.to_vec();
    if on {
        inner.extend(ON);
    }
    inner.extend([0xD0, 0x0A, 0x02, 0xC6, 0x14]);
    leb128(&mut inner, filetime);
    inner.push(0);
    let mut blob = [HEADER, [0x0A, 0x02, 0x01, 0x00]].concat();
    blob.extend([0x2A, 0x06]);
    leb128(&mut blob, unix);
    blob.extend([0x2A, 0x2B, 0x0E, inner.len() as u8]);
    blob.extend(inner);
    blob.extend([0, 0, 0, 0]);
    blob
}

fn leb128(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bytes Settings wrote on this machine (build 26300) for "Turn on now" and "Turn off now".
    const SETTINGS_ON: &[u8] = &[
        0x43, 0x42, 0x01, 0x00, 0x0A, 0x02, 0x01, 0x00, 0x2A, 0x06, 0xAB, 0xC8, 0xB1, 0xD7, 0x06,
        0x2A, 0x2B, 0x0E, 0x15, 0x43, 0x42, 0x01, 0x00, 0x10, 0x00, 0xD0, 0x0A, 0x02, 0xC6, 0x14,
        0xFB, 0xF4, 0xFA, 0xBD, 0xE4, 0xA2, 0xDB, 0xEE, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    const SETTINGS_OFF: &[u8] = &[
        0x43, 0x42, 0x01, 0x00, 0x0A, 0x02, 0x01, 0x00, 0x2A, 0x06, 0xAD, 0xC8, 0xB1, 0xD7, 0x06,
        0x2A, 0x2B, 0x0E, 0x13, 0x43, 0x42, 0x01, 0x00, 0xD0, 0x0A, 0x02, 0xC6, 0x14, 0xB4, 0x89,
        0xFF, 0xC8, 0xE4, 0xA2, 0xDB, 0xEE, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn night_light_reads_and_writes_the_blob_settings_writes() {
        assert_eq!(parse(SETTINGS_ON), Some(true));
        assert_eq!(parse(SETTINGS_OFF), Some(false));
        // A fresh install's blob, before night light was ever used.
        let pristine = [
            0x43, 0x42, 0x01, 0x00, 0x0A, 0x02, 0x01, 0x00, 0x2A, 0x06, 0xC9, 0xAF, 0x8C, 0xCF,
            0x06, 0x2A, 0x2B, 0x0E, 0x05, 0x43, 0x42, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        assert_eq!(parse(&pristine), Some(false));
        // Settings' own bytes, again, from the times they carry.
        let time = |bytes: &[u8]| {
            bytes
                .iter()
                .enumerate()
                .fold(0u64, |v, (i, b)| v | (u64::from(b & 0x7F) << (7 * i)))
        };
        let on = encode(true, time(&SETTINGS_ON[10..15]), time(&SETTINGS_ON[30..39]));
        assert_eq!(on, SETTINGS_ON);
        let off = encode(false, time(&SETTINGS_OFF[10..15]), time(&SETTINGS_OFF[28..37]));
        assert_eq!(off, SETTINGS_OFF);
        assert_eq!(parse(&[0x43, 0x42]), None);
    }
}
