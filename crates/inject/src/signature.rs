//! Is this file signed by Microsoft? Fills [`Target::microsoft_signed`](crate::Target).
//!
//! Most Windows binaries carry no embedded signature: they are covered by a
//! signed *catalog* (`.cat`) that lists their hashes. So the check is
//! `WinVerifyTrust` on the file first, then a catalog lookup by file hash
//! (`CryptCATAdmin*`) followed by `WinVerifyTrust` on the catalog member.
//! A valid signature alone is not enough: the signer chain must also be
//! Microsoft's, see [`is_microsoft_chain`]. No network is used (no
//! revocation checks, cached intermediate certificates only).

use std::collections::HashMap;
use std::ffi::{OsStr, c_void};
use std::fs::File;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::ptr;
use std::slice;
use std::sync::{Mutex, OnceLock, PoisonError};

use windows::Win32::Foundation::{HANDLE, HWND, INVALID_HANDLE_VALUE};
use windows::Win32::Security::Cryptography::Catalog::{
    CATALOG_INFO, CryptCATAdminAcquireContext2, CryptCATAdminCalcHashFromFileHandle2,
    CryptCATAdminEnumCatalogFromHash, CryptCATAdminReleaseCatalogContext,
    CryptCATAdminReleaseContext, CryptCATCatalogInfoFromContext,
};
use windows::Win32::Security::Cryptography::{CERT_NAME_SIMPLE_DISPLAY_TYPE, CertGetNameStringW};
use windows::Win32::Security::WinTrust::{
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_CATALOG_INFO, WINTRUST_DATA, WINTRUST_DATA_0,
    WINTRUST_DATA_UNION_CHOICE, WINTRUST_FILE_INFO, WTD_CACHE_ONLY_URL_RETRIEVAL,
    WTD_CHOICE_CATALOG, WTD_CHOICE_FILE, WTD_REVOCATION_CHECK_NONE, WTD_REVOKE_NONE,
    WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE, WTHelperGetProvSignerFromChain,
    WTHelperProvDataFromStateData, WinVerifyTrust,
};
use windows::core::{HSTRING, PCWSTR};

/// Hash algorithms catalogs are keyed by: modern files first, then legacy.
const CATALOG_HASHES: [&str; 2] = ["SHA256", "SHA1"];

/// True if `path` has a valid embedded or catalog signature whose chain is
/// Microsoft's. Verdicts are cached per path for the life of the process, so
/// call it freely from event handlers; the first call per file costs a hash
/// and a signature check.
pub fn microsoft_signed(path: &Path) -> bool {
    static CACHE: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    // Windows paths are case-insensitive.
    let key = path.to_string_lossy().to_lowercase();
    let cached = cache
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key)
        .copied();
    if let Some(verdict) = cached {
        return verdict;
    }
    let verdict = verify_embedded(path) || verify_catalog(path);
    cache
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(key, verdict);
    verdict
}

/// The signer-name rule: the leaf certificate belongs to Microsoft and the
/// chain ends at a Microsoft root ("Microsoft Root Certificate Authority
/// 2010", ...). Third-party signers chain to public CAs instead.
fn is_microsoft_chain(leaf: &str, root: &str) -> bool {
    leaf.starts_with("Microsoft") && root.starts_with("Microsoft Root")
}

/// Verifies a signature embedded in the file itself.
fn verify_embedded(path: &Path) -> bool {
    let wide = wide(path.as_os_str());
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(wide.as_ptr()),
        ..Default::default()
    };
    let mut data = trust_data(
        WTD_CHOICE_FILE,
        WINTRUST_DATA_0 {
            pFile: &raw mut file,
        },
    );
    verify(&mut data)
}

/// Verifies the file through a signed catalog that lists its hash.
fn verify_catalog(path: &Path) -> bool {
    let Ok(file) = File::open(path) else {
        return false;
    };
    let handle = HANDLE(file.as_raw_handle());
    let wide_path = wide(path.as_os_str());
    CATALOG_HASHES
        .into_iter()
        .any(|algorithm| verify_in_catalogs(algorithm, handle, &wide_path))
    // `file` stays open until here, as `handle` borrows it.
}

/// Hashes the file with `algorithm` and verifies it against each catalog
/// that contains that hash, until one validates.
fn verify_in_catalogs(algorithm: &str, file: HANDLE, wide_path: &[u16]) -> bool {
    let mut admin = 0isize;
    // SAFETY: `admin` is a valid out pointer; the context is released below.
    let acquired = unsafe {
        CryptCATAdminAcquireContext2(&mut admin, None, &HSTRING::from(algorithm), None, None)
    };
    if acquired.is_err() {
        return false;
    }
    let verified = verify_with_admin(admin, file, wide_path);
    // SAFETY: `admin` came from the successful acquire above and is released once.
    let _ = unsafe { CryptCATAdminReleaseContext(admin, 0) };
    verified
}

fn verify_with_admin(admin: isize, file: HANDLE, wide_path: &[u16]) -> bool {
    let mut hash = [0u8; 64];
    let mut hash_len = hash.len() as u32;
    // SAFETY: `file` is an open file handle owned by the caller; the buffer
    // and its length describe `hash`.
    let hashed = unsafe {
        CryptCATAdminCalcHashFromFileHandle2(
            admin,
            file,
            &mut hash_len,
            Some(hash.as_mut_ptr()),
            None,
        )
    };
    if hashed.is_err() {
        return false;
    }
    let hash = &mut hash[..hash_len as usize];
    // The catalog member tag is the hash as upper-case hex.
    let tag: Vec<u16> = hash
        .iter()
        .flat_map(|byte| format!("{byte:02X}").encode_utf16().collect::<Vec<_>>())
        .chain([0])
        .collect();

    // SAFETY: `admin` is a live context and `hash` holds the bytes just computed.
    let mut catalog = unsafe { CryptCATAdminEnumCatalogFromHash(admin, hash, None, None) };
    while catalog != 0 {
        if verify_member(admin, catalog, file, wide_path, &tag, hash) {
            // SAFETY: `catalog` is a live catalog context from the enumeration.
            let _ = unsafe { CryptCATAdminReleaseCatalogContext(admin, catalog, 0) };
            return true;
        }
        // SAFETY: passing the previous context lets the call release it and
        // return the next catalog (0 when there are no more).
        catalog =
            unsafe { CryptCATAdminEnumCatalogFromHash(admin, hash, None, Some(&mut catalog)) };
    }
    false
}

/// Verifies the file as a member of one catalog.
fn verify_member(
    admin: isize,
    catalog: isize,
    file: HANDLE,
    wide_path: &[u16],
    tag: &[u16],
    hash: &mut [u8],
) -> bool {
    let mut info = CATALOG_INFO {
        cbStruct: size_of::<CATALOG_INFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `catalog` is a live context and `info` is a valid out struct.
    if unsafe { CryptCATCatalogInfoFromContext(catalog, &mut info, 0) }.is_err() {
        return false;
    }
    let mut member = WINTRUST_CATALOG_INFO {
        cbStruct: size_of::<WINTRUST_CATALOG_INFO>() as u32,
        pcwszCatalogFilePath: PCWSTR(info.wszCatalogFile.as_ptr()),
        pcwszMemberTag: PCWSTR(tag.as_ptr()),
        pcwszMemberFilePath: PCWSTR(wide_path.as_ptr()),
        hMemberFile: file,
        pbCalculatedFileHash: hash.as_mut_ptr(),
        cbCalculatedFileHash: hash.len() as u32,
        hCatAdmin: admin,
        ..Default::default()
    };
    let mut data = trust_data(
        WTD_CHOICE_CATALOG,
        WINTRUST_DATA_0 {
            pCatalog: &raw mut member,
        },
    );
    verify(&mut data)
}

/// A `WINTRUST_DATA` that verifies offline with no UI.
fn trust_data(choice: WINTRUST_DATA_UNION_CHOICE, subject: WINTRUST_DATA_0) -> WINTRUST_DATA {
    WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: choice,
        Anonymous: subject,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL | WTD_REVOCATION_CHECK_NONE,
        ..Default::default()
    }
}

/// Runs `WinVerifyTrust`, then checks the signer chain while the provider
/// state is still open, and closes it again.
fn verify(data: &mut WINTRUST_DATA) -> bool {
    // A window handle of -1 means "no UI".
    let no_ui = HWND(INVALID_HANDLE_VALUE.0);
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    data.dwStateAction = WTD_STATEACTION_VERIFY;
    // SAFETY: `data` is fully initialised and the file/catalog structs it
    // points to (and their strings and hash) outlive the call.
    let status =
        unsafe { WinVerifyTrust(no_ui, &mut action, ptr::from_mut(data).cast::<c_void>()) };
    let microsoft = status == 0 && signer_is_microsoft(data.hWVTStateData);

    data.dwStateAction = WTD_STATEACTION_CLOSE;
    // SAFETY: same `data`; CLOSE frees the state the VERIFY call allocated.
    let _ = unsafe { WinVerifyTrust(no_ui, &mut action, ptr::from_mut(data).cast::<c_void>()) };
    microsoft
}

/// Reads the first signer's certificate chain out of an open verification
/// state and applies [`is_microsoft_chain`].
fn signer_is_microsoft(state: HANDLE) -> bool {
    // SAFETY: `state` comes from a successful VERIFY that is not closed yet,
    // so the provider data, signer and certificate contexts stay valid for
    // this whole function. Null results are checked before use.
    unsafe {
        let provider = WTHelperProvDataFromStateData(state);
        if provider.is_null() {
            return false;
        }
        let signer = WTHelperGetProvSignerFromChain(provider, 0, false, 0);
        if signer.is_null() || (*signer).pasCertChain.is_null() || (*signer).csCertChain == 0 {
            return false;
        }
        let chain = slice::from_raw_parts((*signer).pasCertChain, (*signer).csCertChain as usize);
        let name = |cert: &windows::Win32::Security::WinTrust::CRYPT_PROVIDER_CERT| {
            let mut buffer = [0u16; 256];
            let len = CertGetNameStringW(
                cert.pCert,
                CERT_NAME_SIMPLE_DISPLAY_TYPE,
                0,
                None,
                Some(&mut buffer),
            );
            // `len` counts the terminating NUL.
            String::from_utf16_lossy(&buffer[..(len as usize).saturating_sub(1)])
        };
        match (chain.first(), chain.last()) {
            (Some(leaf), Some(root)) => is_microsoft_chain(&name(leaf), &name(root)),
            _ => false,
        }
    }
}

/// UTF-16, NUL-terminated.
fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain([0]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_rule_wants_microsoft_leaf_and_root() {
        let root = "Microsoft Root Certificate Authority 2010";
        assert!(is_microsoft_chain("Microsoft Windows", root));
        assert!(is_microsoft_chain("Microsoft Corporation", root));
        // A public-CA signer, and a Microsoft-looking leaf under another root.
        assert!(!is_microsoft_chain(
            "Valve Corporation",
            "DigiCert Global Root G2"
        ));
        assert!(!is_microsoft_chain("Microsoft Windows", "Evil Root CA"));
        assert!(!is_microsoft_chain(
            "Microsoft Windows",
            "Microsoft Code Verification Root"
        ));
    }

    #[test]
    fn windows_binaries_verify_through_the_catalog_and_other_files_do_not() {
        // explorer.exe is catalog-signed on current Windows builds.
        let windir = std::env::var_os("SystemRoot").expect("SystemRoot");
        let explorer = Path::new(&windir).join("explorer.exe");
        assert!(microsoft_signed(&explorer));
        // The test binary itself is unsigned; a missing file is not signed either.
        assert!(!microsoft_signed(&std::env::current_exe().unwrap()));
        assert!(!microsoft_signed(Path::new(r"C:\no\such\file.exe")));
    }
}
