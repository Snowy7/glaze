//! The desktop's own folder view, through the documented shell interfaces:
//! `IShellWindows::FindWindowSW(CSIDL_DESKTOP)` → `IShellBrowser` →
//! `IShellView` → `IFolderView2` (the same route Windows itself offers to
//! automation clients). Everything Explorer's own "View", "Sort by" and
//! "Refresh" do, without touching Explorer's process:
//!
//! - icon size ([`IconSize`]) and sort order ([`SortKey`]) of the desktop,
//! - show and hide the icons (`FWF_NOICONS`),
//! - refresh, new folder / text document, paste,
//! - where the icons are ([`DesktopView::icon_rects`]), for the right-click hook.
//!
//! Every method talks to Explorer over COM, so call it from a thread that can
//! block for a few milliseconds (never from a hook callback).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use windows::Win32::Foundation::{HWND, POINT, PROPERTYKEY};
use windows::Win32::Storage::EnhancedStorage::{
    PKEY_DateModified, PKEY_ItemNameDisplay, PKEY_ItemTypeText, PKEY_Size,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize, IServiceProvider,
};
use windows::Win32::System::Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_I4};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    CMF_NORMAL, CMINVOKECOMMANDINFO, FOLDERID_Desktop, FVM_ICON, FWF_NOICONS, IContextMenu,
    IFolderView2, ILFindLastID, IShellBrowser, IShellView, IShellWindows, KF_FLAG_DEFAULT,
    SHCNE_CREATE, SHCNE_MKDIR, SHCNF_FLUSH, SHCNF_PATHW, SHChangeNotify, SHGetKnownFolderPath,
    SHParseDisplayName, SID_STopLevelBrowser, SORT_ASCENDING, SORT_DESCENDING, SORTCOLUMN,
    SVGIO_ALLVIEW, SVGIO_BACKGROUND, SVSI_DESELECTOTHERS, SVSI_EDIT, SVSI_ENSUREVISIBLE,
    SVSI_FOCUSED, SVSI_SELECT, SWC_DESKTOP, SWFO_NEEDDISPATCH, ShellWindows,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyMenu, GetDesktopWindow, SW_SHOWNORMAL,
};
use windows::core::{HSTRING, Interface, PCSTR, PCWSTR};

use crate::Rect;

/// `CSIDL_DESKTOP`: "the desktop" for `FindWindowSW`.
const CSIDL_DESKTOP: i32 = 0;

/// A `VT_I4` variant.
fn variant_i4(value: i32) -> VARIANT {
    let inner = VARIANT_0_0 {
        vt: VT_I4,
        wReserved1: 0,
        wReserved2: 0,
        wReserved3: 0,
        Anonymous: VARIANT_0_0_0 { lVal: value },
    };
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(inner),
        },
    }
}

/// Keeps COM alive on the calling thread (balanced on drop). Fine to nest,
/// and fine on a thread that already chose its apartment.
pub struct Com(bool);

impl Com {
    pub fn init() -> Self {
        // SAFETY: plain per-thread COM init; balanced in `drop` when it succeeded.
        Self(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok())
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balances the successful `CoInitializeEx` of `init`.
            unsafe { CoUninitialize() };
        }
    }
}

/// The desktop's icon size, as in Explorer's View menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconSize {
    Small,
    Medium,
    Large,
}

impl IconSize {
    /// The edge, in pixels, Explorer uses for this size.
    pub fn pixels(self) -> i32 {
        match self {
            Self::Small => 32,
            Self::Medium => 48,
            Self::Large => 96,
        }
    }

    /// The size a view reports in pixels belongs to (whatever the user dragged it to).
    pub fn nearest(pixels: i32) -> Self {
        match pixels {
            ..=39 => Self::Small,
            40..=71 => Self::Medium,
            _ => Self::Large,
        }
    }
}

/// What the desktop can be sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Size,
    Type,
    Modified,
}

impl SortKey {
    pub const ALL: [Self; 4] = [Self::Name, Self::Size, Self::Type, Self::Modified];

    fn property(self) -> PROPERTYKEY {
        match self {
            Self::Name => PKEY_ItemNameDisplay,
            Self::Size => PKEY_Size,
            Self::Type => PKEY_ItemTypeText,
            Self::Modified => PKEY_DateModified,
        }
    }

    fn of(property: &PROPERTYKEY) -> Option<Self> {
        Self::ALL.into_iter().find(|key| {
            let known = key.property();
            known.fmtid == property.fmtid && known.pid == property.pid
        })
    }
}

/// What "New" can create on the desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewItem {
    Folder,
    TextDocument,
}

impl NewItem {
    /// The default name and extension (as Explorer names them), to be made unique.
    fn name(self) -> (&'static str, &'static str) {
        match self {
            Self::Folder => ("New folder", ""),
            Self::TextDocument => ("New Text Document", ".txt"),
        }
    }
}

/// `New folder`, `New folder (2)`, ... : the first name `taken` does not know.
fn unique_name(base: &str, extension: &str, taken: impl Fn(&str) -> bool) -> String {
    let first = format!("{base}{extension}");
    if !taken(&first) {
        return first;
    }
    (2..)
        .map(|n| format!("{base} ({n}){extension}"))
        .find(|name| !taken(name))
        .expect("an unbounded range has a free name")
}

/// A handle on the desktop's folder view. Cheap to make (one COM lookup); make
/// a fresh one per operation rather than keeping it, since it dies with Explorer.
pub struct DesktopView {
    view: IShellView,
    folder: IFolderView2,
    _com: Com, // dropped last
}

impl DesktopView {
    pub fn open() -> Result<Self> {
        let com = Com::init();
        // SAFETY: documented COM calls on interfaces we own, in the apartment `Com` set up.
        unsafe {
            let windows: IShellWindows =
                CoCreateInstance(&ShellWindows, None, CLSCTX_ALL).context("IShellWindows")?;
            let mut hwnd = 0i32;
            let desktop = windows
                .FindWindowSW(
                    &variant_i4(CSIDL_DESKTOP),
                    &VARIANT::default(),
                    SWC_DESKTOP,
                    &mut hwnd,
                    SWFO_NEEDDISPATCH,
                )
                .context("finding the desktop's shell window")?;
            let provider: IServiceProvider = desktop.cast()?;
            let browser: IShellBrowser = provider
                .QueryService(&SID_STopLevelBrowser)
                .context("the desktop's IShellBrowser")?;
            let view = browser
                .QueryActiveShellView()
                .context("the desktop's shell view")?;
            let folder: IFolderView2 = view.cast().context("IFolderView2")?;
            Ok(Self {
                view,
                folder,
                _com: com,
            })
        }
    }

    pub fn icon_size(&self) -> Result<IconSize> {
        let mut mode = FVM_ICON;
        let mut pixels = 0i32;
        // SAFETY: both out pointers are valid locals.
        unsafe { self.folder.GetViewModeAndIconSize(&mut mode, &mut pixels) }?;
        Ok(IconSize::nearest(pixels))
    }

    pub fn set_icon_size(&self, size: IconSize) -> Result<()> {
        // SAFETY: plain COM call.
        Ok(unsafe { self.folder.SetViewModeAndIconSize(FVM_ICON, size.pixels()) }?)
    }

    /// The first sort column, if the view is sorted by one of [`SortKey::ALL`].
    pub fn sort(&self) -> Result<Option<(SortKey, bool)>> {
        // SAFETY: the buffer holds one column, as `GetSortColumns` is told.
        unsafe {
            if self.folder.GetSortColumnCount()? < 1 {
                return Ok(None);
            }
            let mut column = [SORTCOLUMN::default()];
            self.folder.GetSortColumns(&mut column)?;
            Ok(SortKey::of(&column[0].propkey)
                .map(|key| (key, column[0].direction == SORT_ASCENDING)))
        }
    }

    pub fn set_sort(&self, key: SortKey, ascending: bool) -> Result<()> {
        let column = SORTCOLUMN {
            propkey: key.property(),
            direction: if ascending {
                SORT_ASCENDING
            } else {
                SORT_DESCENDING
            },
        };
        // SAFETY: plain COM call with a one-element slice.
        Ok(unsafe { self.folder.SetSortColumns(&[column]) }?)
    }

    pub fn icons_visible(&self) -> Result<bool> {
        // SAFETY: plain COM call.
        let flags = unsafe { self.folder.GetCurrentFolderFlags() }?;
        Ok(flags & FWF_NOICONS.0 as u32 == 0)
    }

    pub fn set_icons_visible(&self, visible: bool) -> Result<()> {
        let mask = FWF_NOICONS.0 as u32;
        let flags = if visible { 0 } else { mask };
        // SAFETY: plain COM call.
        Ok(unsafe { self.folder.SetCurrentFolderFlags(mask, flags) }?)
    }

    pub fn refresh(&self) -> Result<()> {
        // SAFETY: plain COM call.
        Ok(unsafe { self.view.Refresh() }?)
    }

    /// Every icon's cell in screen pixels: its position from the view plus the view's
    /// spacing. Empty while the icons are hidden. A snapshot: it goes stale when icons move.
    pub fn icon_rects(&self) -> Result<Vec<Rect>> {
        if !self.icons_visible()? {
            return Ok(Vec::new());
        }
        // SAFETY: documented COM calls; each ID list from `Item` is freed after use.
        unsafe {
            let count = self.folder.ItemCount(SVGIO_ALLVIEW)?;
            let mut spacing = POINT::default();
            self.folder.GetSpacing(&mut spacing)?;
            let mut origin = POINT::default();
            let hwnd = self.view.GetWindow()?;
            let _ = windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut origin);
            let mut rects = Vec::with_capacity(count.max(0) as usize);
            for index in 0..count {
                let Ok(id) = self.folder.Item(index) else {
                    continue;
                };
                let position = self.folder.GetItemPosition(id);
                CoTaskMemFree(Some(id.cast()));
                if let Ok(p) = position {
                    rects.push(Rect {
                        x: origin.x + p.x,
                        y: origin.y + p.y,
                        width: spacing.x,
                        height: spacing.y,
                    });
                }
            }
            Ok(rects)
        }
    }

    /// Creates a new folder or text document on the desktop and starts renaming it, like
    /// Explorer's "New" menu.
    pub fn create(&self, item: NewItem) -> Result<()> {
        let desktop = desktop_folder()?;
        let (base, extension) = item.name();
        let name = unique_name(base, extension, |n| desktop.join(n).exists());
        let path = desktop.join(name);
        let event = match item {
            NewItem::Folder => {
                std::fs::create_dir(&path)?;
                SHCNE_MKDIR
            }
            NewItem::TextDocument => {
                std::fs::File::create_new(&path)?;
                SHCNE_CREATE
            }
        };
        let wide = HSTRING::from(path.as_os_str());
        // SAFETY: `wide` is a NUL-terminated path that outlives the calls; the flush makes the
        // view learn of the item before we select it. The ID list is freed after use.
        unsafe {
            SHChangeNotify(
                event,
                SHCNF_PATHW | SHCNF_FLUSH,
                Some(wide.as_ptr().cast()),
                None,
            );
            let mut id: *mut ITEMIDLIST = std::ptr::null_mut();
            SHParseDisplayName(PCWSTR(wide.as_ptr()), None, &mut id, 0, None)?;
            let flags = [
                SVSI_EDIT,
                SVSI_SELECT,
                SVSI_ENSUREVISIBLE,
                SVSI_FOCUSED,
                SVSI_DESELECTOTHERS,
            ]
            .iter()
            .fold(0u32, |all, flag| all | flag.0 as u32);
            let selected = self.view.SelectItem(ILFindLastID(id), flags);
            CoTaskMemFree(Some(id.cast()));
            selected?;
        }
        Ok(())
    }

    /// Pastes the clipboard into the desktop folder: the "paste" verb of the folder's own
    /// background context menu, so copy/move semantics and conflict prompts are Explorer's.
    pub fn paste(&self) -> Result<()> {
        // SAFETY: documented COM calls; the menu is only a carrier for the command ids and is
        // destroyed after the verb ran. `verb` outlives the call.
        unsafe {
            let menu: IContextMenu = self
                .view
                .GetItemObject(SVGIO_BACKGROUND)
                .context("the desktop's background menu")?;
            let hmenu = CreatePopupMenu()?;
            let result = (|| -> Result<()> {
                menu.QueryContextMenu(hmenu, 0, 1, 0x7FFF, CMF_NORMAL)
                    .ok()?;
                let verb = c"paste";
                let info = CMINVOKECOMMANDINFO {
                    cbSize: size_of::<CMINVOKECOMMANDINFO>() as u32,
                    fMask: 0, // an ANSI verb
                    hwnd: GetDesktopWindow(),
                    lpVerb: PCSTR(verb.as_ptr().cast()),
                    nShow: SW_SHOWNORMAL.0,
                    ..Default::default()
                };
                menu.InvokeCommand(&info)?;
                Ok(())
            })();
            let _ = DestroyMenu(hmenu);
            result
        }
    }

    /// The window of the desktop's view (`SHELLDLL_DefView`).
    pub fn window(&self) -> Result<HWND> {
        // SAFETY: plain COM call.
        Ok(unsafe { self.view.GetWindow() }?)
    }
}

/// Is a desktop icon at the screen point? Reads the icons from the desktop's folder view (a COM
/// call to Explorer: milliseconds, so ask on a button press, not on every move). `false` when
/// Explorer does not say.
pub fn on_icon(at: (i32, i32)) -> bool {
    DesktopView::open()
        .and_then(|view| view.icon_rects())
        .is_ok_and(|rects| crate::desktop_hit::IconMap::new(rects).hit(at.0, at.1))
}

/// The user's Desktop folder (honours OneDrive redirection).
pub fn desktop_folder() -> Result<PathBuf> {
    // SAFETY: the returned string is copied and freed before returning.
    unsafe {
        let path = SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT, None)?;
        let text = path.to_string();
        CoTaskMemFree(Some(path.as_ptr().cast()));
        let text = text?;
        if text.is_empty() {
            bail!("no Desktop folder");
        }
        Ok(Path::new(&text).to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_sizes_map_to_explorers_pixels_and_back_to_the_nearest() {
        for size in [IconSize::Small, IconSize::Medium, IconSize::Large] {
            assert_eq!(IconSize::nearest(size.pixels()), size);
        }
        // Whatever size the user dragged the view to lands on its closest preset.
        assert_eq!(IconSize::nearest(16), IconSize::Small);
        assert_eq!(IconSize::nearest(39), IconSize::Small);
        assert_eq!(IconSize::nearest(40), IconSize::Medium);
        assert_eq!(IconSize::nearest(71), IconSize::Medium);
        assert_eq!(IconSize::nearest(72), IconSize::Large);
        assert_eq!(IconSize::nearest(256), IconSize::Large);
    }

    #[test]
    fn sort_keys_round_trip_through_their_property_keys() {
        for key in SortKey::ALL {
            assert_eq!(SortKey::of(&key.property()), Some(key));
        }
        let unknown = PROPERTYKEY {
            fmtid: windows::core::GUID::zeroed(),
            pid: 1,
        };
        assert_eq!(SortKey::of(&unknown), None);
    }

    #[test]
    fn new_items_get_the_first_free_name_like_explorer() {
        let taken = |names: &'static [&'static str]| move |n: &str| names.contains(&n);
        assert_eq!(unique_name("New folder", "", taken(&[])), "New folder");
        assert_eq!(
            unique_name("New folder", "", taken(&["New folder"])),
            "New folder (2)"
        );
        assert_eq!(
            unique_name("New folder", "", taken(&["New folder", "New folder (2)"])),
            "New folder (3)"
        );
        assert_eq!(
            unique_name(
                "New Text Document",
                ".txt",
                taken(&["New Text Document.txt"])
            ),
            "New Text Document (2).txt"
        );
    }
}
