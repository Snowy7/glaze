//! Read-only Windows jump lists. Shell COM decodes and executes links; only container framing
//! and DestList pin metadata are parsed here. Never log destinations (they are private).
use std::path::{Path, PathBuf};
use windows::Win32::Storage::EnhancedStorage::{PKEY_AppUserModel_IsDestListSeparator, PKEY_Title};
use windows::Win32::System::Com::StructuredStorage::{
    CreateILockBytesOnHGlobal, IStorage, PropVariantToBoolean, PropVariantToStringAlloc,
    StgOpenStorageOnILockBytes,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree, IPersistStream, IStream,
    STATFLAG_NONAME, STATSTG, STGM_READ, STGM_SHARE_EXCLUSIVE,
};
use windows::Win32::UI::Shell::{Common::IObjectArray, PropertiesSystem::IPropertyStore, *};
use windows::core::{HSTRING, IUnknown, Interface, PWSTR};

const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_ITEMS: u32 = 256;
const LINK_CLSID: [u8; 16] = [1, 20, 2, 0, 0, 0, 0, 0, 192, 0, 0, 0, 0, 0, 0, 70];

#[derive(Debug, Clone, PartialEq)]
pub enum Launch {
    /// Preserve the entire shell link, including arguments, working directory and IDList.
    Link(Vec<u8>),
    Item(String),
}

impl Launch {
    pub fn open(&self) {
        match self {
            Self::Item(path) => crate::system::open(path.clone()),
            Self::Link(bytes) => {
                // The shell consumes the link synchronously with NOASYNC, including DDE.
                // The private file is removed immediately afterwards, including on failure.
                use std::sync::atomic::{AtomicU64, Ordering};
                static NEXT: AtomicU64 = AtomicU64::new(0);
                let path = std::env::temp_dir().join(format!(
                    "glaze-jump-{}-{}.lnk",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                {
                    use std::io::Write;
                    let written = file.write_all(bytes).is_ok();
                    drop(file);
                    if written {
                        let name = HSTRING::from(path.as_os_str());
                        let mut info = SHELLEXECUTEINFOW {
                            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
                            fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
                            lpFile: windows::core::PCWSTR(name.as_ptr()),
                            nShow: 1,
                            ..Default::default()
                        };
                        // SAFETY: strings and struct remain alive through synchronous execution.
                        let _ = unsafe { ShellExecuteExW(&mut info) };
                    }
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub name: String,
    pub launch: Launch,
    identity: String,
}
#[derive(Debug, Clone, Default)]
pub struct Section {
    pub name: String,
    pub items: Vec<Option<Item>>,
}

#[derive(Debug, Clone, PartialEq)]
struct Destination {
    id: u64,
    pin: i32,
    path: String,
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let value = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(value)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn text(&mut self) -> Option<String> {
        let len = u16::from_le_bytes(self.take(2)?.try_into().ok()?) as usize;
        Some(String::from_utf16_lossy(
            &self
                .take(len.checked_mul(2)?)?
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        ))
    }
}

fn dest_list(bytes: &[u8]) -> Option<Vec<Destination>> {
    if bytes.is_empty() {
        return Some(vec![]);
    }
    let mut r = Reader { bytes, at: 0 };
    let version = r.u32()?;
    if !matches!(version, 1..=4 | 6) {
        return None;
    }
    let count = r.u32()?;
    if count > 100_000 {
        return None;
    }
    r.take(24)?;
    let mut out = Vec::new();
    for _ in 0..count {
        let fixed = r.take(if version == 1 { 112 } else { 128 })?;
        let id = u64::from_le_bytes(fixed[88..96].try_into().ok()?);
        let pin = i32::from_le_bytes(fixed[108..112].try_into().ok()?);
        let path = r.text()?;
        if version != 1 {
            let property_bytes = r.u32()? as usize;
            r.take(property_bytes)?;
        }
        out.push(Destination { id, pin, path });
    }
    Some(out)
}

/// Size of a complete MS-SHLLINK, not a signature scan: embedded headers/footers in paths
/// cannot split a task or turn malformed bytes into a different command.
fn link_size(bytes: &[u8]) -> Option<usize> {
    let mut r = Reader { bytes, at: 0 };
    if r.u32()? != 76 || r.take(16)? != LINK_CLSID {
        return None;
    }
    let flags = r.u32()?;
    r.take(52)?;
    if flags & 1 != 0 {
        let n = u16::from_le_bytes(r.take(2)?.try_into().ok()?);
        r.take(n as usize)?;
    }
    if flags & 2 != 0 {
        let n = r.u32()? as usize;
        if n < 4 {
            return None;
        }
        r.take(n - 4)?;
    }
    for flag in [4, 8, 16, 32, 64] {
        if flags & flag != 0 {
            let n = u16::from_le_bytes(r.take(2)?.try_into().ok()?) as usize;
            r.take(n.checked_mul(if flags & 128 != 0 { 2 } else { 1 })?)?;
        }
    }
    loop {
        let n = r.u32()? as usize;
        if n == 0 {
            return Some(r.at);
        }
        if n < 8 {
            return None;
        }
        r.take(n - 4)?;
    }
}

#[derive(Debug, PartialEq)]
struct Category {
    name: String,
    known: Option<u32>,
    links: Vec<Vec<u8>>,
}
fn custom_list(bytes: &[u8]) -> Option<Vec<Category>> {
    let mut r = Reader { bytes, at: 0 };
    if r.u32()? != 2 {
        return None;
    }
    let count = r.u32()?;
    if count > MAX_ITEMS {
        return None;
    }
    r.take(4)?;
    let mut out = Vec::new();
    for _ in 0..count {
        let kind = r.u32()?;
        let mut category = Category {
            name: String::new(),
            known: None,
            links: vec![],
        };
        if kind == 1 {
            let known = r.u32()?;
            if !matches!(known, 1 | 2) {
                return None;
            }
            category.known = Some(known);
        } else {
            category.name = match kind {
                0 => r.text()?,
                2 => "Tasks".into(),
                _ => return None,
            };
            let entries = r.u32()?;
            if entries > MAX_ITEMS {
                return None;
            }
            for _ in 0..entries {
                // OleSaveToStream prefixes the serialized link with its class ID.
                if r.bytes.get(r.at..r.at + 16)? == LINK_CLSID {
                    r.take(16)?;
                }
                let n = link_size(r.bytes.get(r.at..)?)?;
                category.links.push(r.take(n)?.to_vec());
            }
        }
        if r.u32()? != 0xbabffbab {
            return None;
        }
        out.push(category);
    }
    if r.at != bytes.len() {
        return None;
    }
    Some(out)
}

/// Windows' AppID CRC64: upper-case UTF-16LE, all-one seed, no final XOR.
pub fn app_hash(id: &str) -> String {
    let mut crc = u64::MAX;
    for byte in id.to_uppercase().encode_utf16().flat_map(u16::to_le_bytes) {
        crc ^= u64::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0x92c64265d32139a4 } else { 0 };
        }
    }
    format!("{crc:016x}")
}

fn read(path: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_BYTES as u64 {
        return None;
    }
    let mut bytes = vec![];
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= MAX_BYTES).then_some(bytes)
}

fn stream_bytes(stream: &IStream) -> Option<Vec<u8>> {
    // SAFETY: initialized struct and owned stream; bounded buffer lives through Read.
    unsafe {
        let mut stat = STATSTG::default();
        stream.Stat(&mut stat, STATFLAG_NONAME).ok()?;
        if stat.cbSize > MAX_BYTES as u64 {
            return None;
        }
        let mut bytes = vec![0; stat.cbSize as usize];
        let mut n = 0;
        stream
            .Read(bytes.as_mut_ptr().cast(), bytes.len() as u32, Some(&mut n))
            .ok()
            .ok()?;
        (n as usize == bytes.len()).then_some(bytes)
    }
}

fn storage(bytes: &[u8]) -> Option<IStorage> {
    // SAFETY: private memory-backed OLE storage; no writes to the user's jump list.
    unsafe {
        let memory = CreateILockBytesOnHGlobal(None, true).ok()?;
        memory
            .WriteAt(0, bytes.as_ptr().cast(), bytes.len() as u32, None)
            .ok()?;
        StgOpenStorageOnILockBytes(&memory, None, STGM_READ | STGM_SHARE_EXCLUSIVE, None, None).ok()
    }
}
fn storage_stream(storage: &IStorage, name: &str) -> Option<Vec<u8>> {
    // SAFETY: owned storage; OpenStream is read-only.
    let stream = unsafe {
        storage.OpenStream(
            &HSTRING::from(name),
            None,
            STGM_READ | STGM_SHARE_EXCLUSIVE,
            0,
        )
    }
    .ok()?;
    stream_bytes(&stream)
}
fn owned_string(value: PWSTR) -> Option<String> {
    // SAFETY: shell-owned allocated string, freed exactly once after copying.
    unsafe {
        let result = value.to_string().ok();
        CoTaskMemFree(Some(value.0.cast()));
        result
    }
}
fn property(
    store: &IPropertyStore,
    key: &windows::Win32::Foundation::PROPERTYKEY,
) -> Option<String> {
    // SAFETY: owned COM store and RAII PROPVARIANT.
    unsafe { owned_string(PropVariantToStringAlloc(&store.GetValue(key).ok()?).ok()?) }
        .filter(|v| !v.is_empty())
}
fn decode_link(bytes: Vec<u8>) -> Option<Option<Item>> {
    // SAFETY: only instantiate ShellLink, never a class ID supplied by a file.
    unsafe {
        let stream = SHCreateMemStream(Some(&bytes))?;
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        link.cast::<IPersistStream>().ok()?.Load(&stream).ok()?;
        let store = link.cast::<IPropertyStore>().ok()?;
        if store
            .GetValue(&PKEY_AppUserModel_IsDestListSeparator)
            .ok()
            .and_then(|v| PropVariantToBoolean(&v).ok())
            .is_some_and(|v| v.as_bool())
        {
            return Some(None);
        }
        let name = property(&store, &PKEY_Title)
            .or_else(|| {
                let mut value = [0u16; 32768];
                link.GetDescription(&mut value).ok()?;
                let n = value.iter().position(|v| *v == 0)?;
                (n > 0).then(|| String::from_utf16_lossy(&value[..n]))
            })
            .or_else(|| {
                let pidl = link.GetIDList().ok()?;
                let name = SHGetNameFromIDList(pidl, SIGDN_NORMALDISPLAY)
                    .ok()
                    .and_then(owned_string);
                CoTaskMemFree(Some(pidl.cast()));
                name
            })
            .or_else(|| {
                let mut path = [0u16; 32768];
                link.GetPath(&mut path, std::ptr::null_mut(), 0).ok()?;
                let n = path.iter().position(|v| *v == 0)?;
                (n > 0).then(|| crate::apps::file_stem(&String::from_utf16_lossy(&path[..n])))
            })?;
        let pidl = link.GetIDList().ok();
        let target = pidl
            .and_then(|pidl| {
                let path = SHGetNameFromIDList(pidl, SIGDN_DESKTOPABSOLUTEPARSING)
                    .ok()
                    .and_then(owned_string);
                CoTaskMemFree(Some(pidl.cast()));
                path
            })
            .unwrap_or_default();
        let mut args = [0u16; 32768];
        let _ = link.GetArguments(&mut args);
        let n = args.iter().position(|c| *c == 0).unwrap_or(args.len());
        let identity = format!(
            "{}\0{}",
            target.to_lowercase(),
            String::from_utf16_lossy(&args[..n])
        );
        Some(Some(Item {
            name,
            launch: Launch::Link(bytes),
            identity,
        }))
    }
}

fn documents(id: &str, kind: APPDOCLISTTYPE) -> Vec<Option<Item>> {
    let get = || -> Option<Vec<Option<Item>>> {
        // SAFETY: this function runs in the caller's initialized COM apartment.
        unsafe {
            let lists: IApplicationDocumentLists =
                CoCreateInstance(&ApplicationDocumentLists, None, CLSCTX_INPROC_SERVER).ok()?;
            lists.SetAppID(&HSTRING::from(id)).ok()?;
            let array: IObjectArray = lists.GetList(kind, MAX_ITEMS).ok()?;
            let mut out = vec![];
            for i in 0..array.GetCount().ok()?.min(MAX_ITEMS) {
                let Ok(object) = array.GetAt::<IUnknown>(i) else { continue; };
                if let Ok(link) = object
                    .cast::<IShellLinkW>()
                    .and_then(|link| link.cast::<IPersistStream>())
                {
                    if let Some(stream) = SHCreateMemStream(None) {
                        if link.Save(&stream, false).is_ok() {
                            let _ =
                                stream.Seek(0, windows::Win32::System::Com::STREAM_SEEK_SET, None);
                            if let Some(item) = stream_bytes(&stream).and_then(decode_link) {
                                out.push(item);
                            }
                        }
                    }
                } else if let Ok(item) = object.cast::<IShellItem>() {
                    if let (Some(name), Some(path)) = (
                        item.GetDisplayName(SIGDN_NORMALDISPLAY)
                            .ok()
                            .and_then(owned_string),
                        item.GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING)
                            .ok()
                            .and_then(owned_string),
                    ) {
                        let identity = format!("{}\0", path.to_lowercase());
                        out.push(Some(Item {
                            name,
                            launch: Launch::Item(path),
                            identity,
                        }));
                    }
                }
            }
            Some(out)
        }
    };
    get().unwrap_or_default()
}

/// Call on a worker STA when the menu opens. Missing, locked or unknown formats produce no
/// entries. An app without a list gets only its basic dock commands.
pub fn load(app: &crate::apps::Item) -> Vec<Section> {
    let _com = glaze_desktop::desktop_view::Com::init();
    let (target, shortcut_id) = crate::icons::resolve_link(&app.path);
    let exe = app.exe.clone().or(target).or_else(|| {
        app.path
            .to_lowercase()
            .ends_with(".exe")
            .then(|| app.path.clone())
    });
    let id = app.aumid.clone().or(shortcut_id).or_else(|| {
        exe.as_ref().map(|path| {
            if path.to_lowercase().ends_with("\\explorer.exe") {
                "Microsoft.Windows.Explorer".into()
            } else {
                implicit_id(path)
            }
        })
    });
    let Some(id) = id else {
        return vec![];
    };
    let Some(root) = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .map(|p| p.join(r"Microsoft\Windows\Recent"))
    else {
        return vec![];
    };
    let hash = app_hash(&id);
    let mut sections = vec![];
    let mut pinned = vec![];
    if let Some(storage) = read(
        &root
            .join("AutomaticDestinations")
            .join(format!("{hash}.automaticDestinations-ms")),
    )
    .as_deref()
    .and_then(storage)
    {
        if let Some(mut destinations) = storage_stream(&storage, "DestList")
            .as_deref()
            .and_then(dest_list)
        {
            destinations.retain(|d| d.pin >= 0);
            destinations.sort_by_key(|d| d.pin);
            for destination in destinations.into_iter().take(MAX_ITEMS as usize) {
                if let Some(Some(item)) =
                    storage_stream(&storage, &format!("{:x}", destination.id)).and_then(decode_link)
                {
                    pinned.push(Some(item));
                }
            }
        }
    }
    if !pinned.is_empty() {
        sections.push(Section {
            name: "Pinned".into(),
            items: pinned.clone(),
        });
    }
    let custom = read(
        &root
            .join("CustomDestinations")
            .join(format!("{hash}.customDestinations-ms")),
    )
    .as_deref()
    .and_then(custom_list)
    .unwrap_or_default();
    let has_known = custom.iter().any(|c| c.known.is_some());
    let mut known = vec![];
    if !has_known {
        known.push(2);
    }
    for category in custom {
        if let Some(kind) = category.known {
            known.push(kind);
        } else {
            let items = category
                .links
                .into_iter()
                .filter_map(decode_link)
                .collect::<Vec<_>>();
            if items.iter().any(Option::is_some) {
                sections.push(Section {
                    name: category.name,
                    items,
                });
            }
        }
    }
    for kind in known {
        let mut items = documents(
            &id,
            if kind == 1 {
                ADLT_FREQUENT
            } else {
                ADLT_RECENT
            },
        );
        items.retain(|item| {
            item.as_ref().is_some_and(|i| {
                !pinned.iter().flatten().any(|p| {
                    p.launch == i.launch
                        || (!p.identity.starts_with('\0') && p.identity == i.identity)
                })
            })
        });
        items.truncate(12);
        if !items.is_empty() {
            sections.push(Section {
                name: if kind == 1 { "Frequent" } else { "Recent" }.into(),
                items,
            });
        }
    }
    // Tasks belong below destinations, as on the Windows taskbar.
    sections.sort_by_key(|s| s.name == "Tasks");
    sections
}

fn implicit_id(path: &str) -> String {
    // Windows derives implicit AppIDs by replacing recognized install roots with their GUID.
    let upper = path.replace('/', "\\").to_uppercase();
    let roots = [
        ("LOCALAPPDATA", "{F1B32785-6FBA-4FCF-9D55-7B8E7F157091}"),
        ("APPDATA", "{3EB685DB-65F9-4CF6-A03A-E3EF65729F3D}"),
        (
            "ProgramFiles(x86)",
            "{7C5A40EF-A0FB-4BFC-874A-C0F2E0B9FA8E}",
        ),
        ("ProgramFiles", "{905E63B6-C1BF-494E-B29C-65B732D3D21A}"),
        ("SystemRoot", "{F38BF404-1D43-42F2-9305-67DE0B28FC23}"),
    ];
    for (variable, guid) in roots {
        if let Some(root) = std::env::var_os(variable) {
            let root = root.to_string_lossy().trim_end_matches('\\').to_uppercase();
            if let Some(rest) = upper.strip_prefix(&root).filter(|s| s.starts_with('\\')) {
                return format!("{guid}{rest}");
            }
        }
    }
    upper
}

#[cfg(test)]
mod tests {
    use super::*;
    fn link() -> Vec<u8> {
        let mut bytes = vec![0; 80];
        bytes[..4].copy_from_slice(&76u32.to_le_bytes());
        bytes[4..20].copy_from_slice(&LINK_CLSID);
        bytes
    }
    #[test]
    fn appid_hash_matches_windows_explorer() {
        assert_eq!(app_hash("Microsoft.Windows.Explorer"), "f01b4d95cf55d32a");
    }
    #[test]
    fn destlist_versions_keep_pin_rank_and_stream_id() {
        for version in [1u32, 3, 4, 6] {
            let mut bytes = vec![0; 32];
            bytes[..4].copy_from_slice(&version.to_le_bytes());
            bytes[4..8].copy_from_slice(&2u32.to_le_bytes());
            for (id, rank) in [(7u64, 2i32), (9, -1)] {
                let mut entry = vec![0; if version == 1 { 112 } else { 128 }];
                entry[88..96].copy_from_slice(&id.to_le_bytes());
                entry[108..112].copy_from_slice(&rank.to_le_bytes());
                bytes.extend(entry);
                bytes.extend(1u16.to_le_bytes());
                bytes.extend(65u16.to_le_bytes());
                if version != 1 {
                    bytes.extend([0; 4]);
                }
            }
            let parsed = dest_list(&bytes).unwrap();
            assert_eq!(
                parsed[0],
                Destination {
                    id: 7,
                    pin: 2,
                    path: "A".into()
                }
            );
            assert_eq!(parsed[1].pin, -1);
            for end in 1..bytes.len() {
                assert!(dest_list(&bytes[..end]).is_none());
            }
        }
    }
    #[test]
    fn custom_categories_tasks_and_known_lists_are_framed() {
        let mut bytes = vec![];
        for n in [2u32, 3, 0, 0] {
            bytes.extend(n.to_le_bytes());
        }
        bytes.extend(4u16.to_le_bytes());
        for c in "Work".encode_utf16() {
            bytes.extend(c.to_le_bytes());
        }
        bytes.extend(1u32.to_le_bytes());
        bytes.extend(LINK_CLSID);
        bytes.extend(link());
        bytes.extend(0xbabffbabu32.to_le_bytes());
        for n in [1u32, 1, 0xbabffbab, 2, 1] {
            bytes.extend(n.to_le_bytes());
        }
        bytes.extend(LINK_CLSID);
        bytes.extend(link());
        bytes.extend(0xbabffbabu32.to_le_bytes());
        let categories = custom_list(&bytes).unwrap();
        assert_eq!(categories[0].name, "Work");
        assert_eq!(categories[1].known, Some(1));
        assert_eq!(categories[2].name, "Tasks");
        for end in 0..bytes.len() {
            assert!(custom_list(&bytes[..end]).is_none());
        }
        *bytes.last_mut().unwrap() = 0;
        assert!(custom_list(&bytes).is_none());
    }
    #[test]
    fn link_lengths_reject_truncated_strings_and_extra_blocks() {
        let mut bytes = link();
        bytes[20..24].copy_from_slice(&(4u32 | 128).to_le_bytes());
        bytes.truncate(76);
        bytes.extend(3u16.to_le_bytes());
        bytes.extend([65, 0, 66, 0, 67, 0]);
        bytes.extend(0u32.to_le_bytes());
        assert_eq!(link_size(&bytes), Some(bytes.len()));
        for end in 0..bytes.len() {
            assert!(link_size(&bytes[..end]).is_none());
        }
    }

    #[test]
    fn version_six_skips_property_store_payloads() {
        let mut bytes = vec![0; 32];
        bytes[..4].copy_from_slice(&6u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&2u32.to_le_bytes());
        for id in [1u64, 2] {
            let mut fixed = vec![0; 128];
            fixed[88..96].copy_from_slice(&id.to_le_bytes());
            bytes.extend(fixed);
            bytes.extend(0u16.to_le_bytes());
            bytes.extend(8u32.to_le_bytes());
            bytes.extend([0xab; 8]);
        }
        let entries = dest_list(&bytes).unwrap();
        assert_eq!(entries.iter().map(|d| d.id).collect::<Vec<_>>(), [1, 2]);
        bytes.pop();
        assert!(dest_list(&bytes).is_none());
    }

    #[test]
    fn native_shell_links_keep_arguments_workdir_and_serialized_properties() {
        let _com = glaze_desktop::desktop_view::Com::init();
        // SAFETY: synthetic ShellLink in memory. This test never executes it.
        unsafe {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            link.SetPath(&HSTRING::from(r"C:\Windows\notepad.exe"))
                .unwrap();
            link.SetArguments(&HSTRING::from(r#""C:\Example\two words.txt" /example"#))
                .unwrap();
            link.SetWorkingDirectory(&HSTRING::from(r"C:\Example"))
                .unwrap();
            link.SetDescription(&HSTRING::from("Example task")).unwrap();
            let stream = SHCreateMemStream(None).unwrap();
            link.cast::<IPersistStream>()
                .unwrap()
                .Save(&stream, false)
                .unwrap();
            stream
                .Seek(0, windows::Win32::System::Com::STREAM_SEEK_SET, None)
                .unwrap();
            let bytes = stream_bytes(&stream).unwrap();
            assert_eq!(link_size(&bytes), Some(bytes.len()));
            let item = decode_link(bytes.clone()).unwrap().unwrap();
            assert_eq!(item.name, "Example task");
            assert_eq!(item.launch, Launch::Link(bytes));
        }
    }
}
