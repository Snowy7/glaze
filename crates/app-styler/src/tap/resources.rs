//! Theme resource overrides (`resources` in the rules): the brushes that controls' own templates
//! use, hover and pressed states included.
//!
//! Theme brushes (`ControlFillColorDefaultBrush`, `TextFillColorPrimaryBrush`,
//! `TabViewItemHeaderBackgroundSelected`...) are shared objects: every control that uses one
//! holds the same brush. An override sets that brush's `Color`, so everything using it changes at
//! once, live. Each theme has its own brush: a scratch element with `RequestedTheme` Light, then
//! Dark, resolves both, and `light-dark(LIGHT, DARK)` gives each its own colour. A key that is an
//! alias of another (the same brush) changes both; the first override wins.
//!
//! Only brushes: a value resource (`ControlCornerRadius`, a thickness) can be replaced in the
//! dictionary but not put back (its original is gone with it), and existing controls don't
//! follow anyway, so those are set with rules on the controls instead.
//!
//! Every change is recorded and undone like a property. Each UI thread has its own XAML objects,
//! so all of this runs per UI thread, on that thread.

use std::ffi::c_void;

use windows_core::{BSTR, GUID, HRESULT, IInspectable, IUnknown, Interface, PCWSTR};

use super::{Held, Original, Session, log};
use crate::Framework;
use crate::xamlom::InstanceHandle;

/// `ResourceType::ResourceTypeTheme` (xamlOM.h).
pub(super) const THEME_RESOURCE: i32 = 1;
/// `ResourceType::ResourceTypeStatic`.
pub(super) const STATIC_RESOURCE: i32 = 0;

/// One UI thread's resource overrides.
pub(super) struct ThreadResources {
    /// A `Border` outside the tree, to resolve theme resources per theme. Without it brushes
    /// aren't overridden on this thread.
    scratch: Option<Scratch>,
    /// The application's `Resources` dictionary.
    dictionary: Option<InstanceHandle>,
    /// Overrides in effect: key, value text, what to undo.
    applied: Vec<(String, String, Vec<Undo>)>,
}

#[derive(Clone)]
struct Scratch {
    border: InstanceHandle,
    /// Holds the scratch alive: nothing else references it.
    held: Held,
    background: u32,
    /// `RequestedTheme`'s index, if the element has it.
    theme: Option<u32>,
}

/// A brush's `Color` we set, and what to put back.
struct Undo {
    brush: InstanceHandle,
    /// Holds the brush (and so its handle) alive until it is put back.
    held: Held,
    index: u32,
    /// The colour type, to make the original again.
    kind: String,
    original: Original,
}

/// `light-dark(LIGHT, DARK)` -> both; anything else is the same in both themes.
pub(super) fn split_themes(value: &str) -> (&str, &str) {
    value
        .trim()
        .strip_prefix("light-dark(")
        .and_then(|rest| rest.strip_suffix(')'))
        .and_then(|inner| inner.split_once(','))
        .map_or((value, value), |(light, dark)| (light.trim(), dark.trim()))
}

/// A rule value that names a resource instead: `{ThemeResource Key}` or `{StaticResource Key}`.
pub(super) fn resource_reference(value: &str) -> Option<(i32, &str)> {
    let inner = value.trim().strip_prefix('{')?.strip_suffix('}')?.trim();
    let (kind, key) = inner.split_once(char::is_whitespace)?;
    let kind = match kind {
        "ThemeResource" => THEME_RESOURCE,
        "StaticResource" => STATIC_RESOURCE,
        _ => return None,
    };
    Some((kind, key.trim()))
}

pub(super) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

impl Session {
    fn namespace(&self) -> &'static str {
        match self.framework {
            Framework::System => "Windows.UI.Xaml",
            Framework::WinUi => "Microsoft.UI.Xaml",
        }
    }

    /// Brings this UI thread's resource overrides in line with the rules: undoes the ones no
    /// longer wanted (or wanted with another value), applies the new ones. On that thread.
    pub(super) fn apply_resources(&self, thread: u32) {
        let (wanted, current) = {
            let mut state = self.lock();
            (state.resource_rules.clone(), state.resources.remove(&thread))
        };
        if wanted.is_empty() && current.is_none() {
            return;
        }
        let mut current = current.unwrap_or_else(|| ThreadResources {
            scratch: self.scratch(),
            dictionary: self.app_dictionary(),
            applied: Vec::new(),
        });
        let (keep, stale): (Vec<_>, Vec<_>) = std::mem::take(&mut current.applied)
            .into_iter()
            .partition(|(k, v, _)| wanted.iter().any(|(wk, wv)| wk == k && wv == v));
        for (_, _, undo) in stale.iter().rev() {
            self.undo(undo);
        }
        current.applied = keep;
        for (key, value) in &wanted {
            if current.applied.iter().any(|(k, _, _)| k == key) {
                continue;
            }
            let undo = self.override_resource(&current, key, value);
            if !undo.is_empty() {
                current.applied.push((key.clone(), value.clone(), undo));
            }
        }
        if let Some(scratch) = &current.scratch {
            // Let go of the last brush the scratch resolved.
            // SAFETY: handles from the service.
            let _ = unsafe { self.tree.ClearProperty(self.handle_of(&scratch.held).unwrap_or(scratch.border), scratch.background) };
        }
        let count = current.applied.len();
        self.lock().resources.insert(thread, current);
        log(&format!("{count} resource override(s) on thread {thread}"));
    }

    /// Undoes every resource override on this UI thread (the helper is gone).
    pub(super) fn restore_resources(&self, thread: u32) {
        let taken = self.lock().resources.remove(&thread);
        if let Some(current) = taken {
            for (_, _, undo) in current.applied.iter().rev() {
                self.undo(undo);
            }
        }
    }

    fn undo(&self, undo: &[Undo]) {
        for u in undo {
            self.lock().recolored.remove(&u.brush);
            let brush = self.handle_of(&u.held).unwrap_or(u.brush);
            self.put_back(brush, u.index, &u.kind, &u.original);
        }
    }

    /// Overrides one resource; returns what to undo (empty if nothing changed).
    fn override_resource(&self, at: &ThreadResources, key: &str, value: &str) -> Vec<Undo> {
        let (Some(dictionary), Some(scratch)) = (at.dictionary, &at.scratch) else {
            return Vec::new();
        };
        let Some(item) = self.dictionary_item(dictionary, key) else {
            self.report_once(&format!("no resource {key}"));
            return Vec::new();
        };
        let class = self.class_of(item).unwrap_or_default();
        if !class.ends_with(".SolidColorBrush") {
            self.report_once(&format!(
                "{key} is a {class}: only brushes can be overridden; set the property with a rule"
            ));
            return Vec::new();
        }
        let (light, dark) = split_themes(value);
        [("Light", light), ("Dark", dark)]
            .into_iter()
            .filter_map(|(theme, color)| self.recolor(scratch, key, theme, color))
            .collect()
    }

    /// Sets the `Color` of `key`'s brush in `theme` to `color`.
    fn recolor(&self, scratch: &Scratch, key: &str, theme: &str, color: &str) -> Option<Undo> {
        let border = self.handle_of(&scratch.held).unwrap_or(scratch.border);
        if let Some(index) = scratch.theme {
            let theme_value = self.value(&format!("{}.ElementTheme", self.namespace()), theme)?;
            // SAFETY: handles from the service.
            unsafe { self.tree.SetProperty(border, theme_value, index) }
                .ok()
                .ok()?;
        }
        let name = wide(key);
        // SAFETY: a NUL-terminated name; handles from the service.
        unsafe {
            self.tree.ResolveResource(
                border,
                PCWSTR(name.as_ptr()),
                THEME_RESOURCE,
                scratch.background,
            )
        }
        .ok()
        .ok()?;
        let mut brush = 0;
        // SAFETY: as above.
        unsafe {
            self.tree
                .GetProperty(border, scratch.background, &mut brush)
        }
        .ok()
        .ok()?;
        // Two keys (or both themes) on one brush: the first override wins.
        if !self.lock().recolored.insert(brush) {
            return None;
        }
        let set = (|| {
            let held = self.hold(brush)?;
            let info = self.properties(brush).remove("Color")?;
            let original = self.original(brush, &info, "Color")?;
            let new = self.value(&info.kind, color)?;
            self.set(brush, info.index, new, key).then_some(Undo {
                brush,
                held,
                index: info.index,
                kind: info.kind,
                original,
            })
        })();
        if set.is_none() {
            self.lock().recolored.remove(&brush);
        }
        set
    }

    fn dictionary_item(&self, dictionary: InstanceHandle, key: &str) -> Option<InstanceHandle> {
        let name = wide(key);
        let mut item = 0;
        // SAFETY: a NUL-terminated name and a valid out pointer.
        unsafe {
            self.tree
                .GetDictionaryItem(dictionary, PCWSTR(name.as_ptr()), false.into(), &mut item)
        }
        .ok()
        .ok()?;
        (item != 0).then_some(item)
    }

    /// A `Border` outside the tree to resolve resources with, per theme.
    fn scratch(&self) -> Option<Scratch> {
        let kind = BSTR::from(format!("{}.Controls.Border", self.namespace()));
        let mut border = 0;
        // SAFETY: a live BSTR, no value, a valid out pointer.
        unsafe {
            self.tree
                .CreateInstance(kind.as_ptr(), std::ptr::null(), &mut border)
        }
        .ok()
        .inspect_err(|e| log(&format!("no scratch element: {e}")))
        .ok()?;
        let props = self.properties(border);
        Some(Scratch {
            border,
            held: self.hold(border)?,
            background: props.get("Background")?.index,
            theme: props.get("RequestedTheme").map(|t| t.index),
        })
    }

    /// The application's `Resources`, through `IApplication::get_Resources` (XAML Diagnostics
    /// has no call for it).
    fn app_dictionary(&self) -> Option<InstanceHandle> {
        // `Windows.UI.Xaml.IApplication` / `Microsoft.UI.Xaml.IApplication`.
        let iid = match self.framework {
            Framework::System => GUID::from_u128(0x74b861a1_7487_46a9_9a6e_c78b512726c5),
            Framework::WinUi => GUID::from_u128(0x06a8f4e7_1146_55af_820d_ebd55643b021),
        };
        let mut raw: *mut c_void = std::ptr::null_mut();
        // SAFETY: a valid out pointer; we own the returned reference.
        unsafe { self.diagnostics.GetApplication(&mut raw) }
            .ok()
            .inspect_err(|e| log(&format!("no application: {e}")))
            .ok()?;
        if raw.is_null() {
            return None;
        }
        // SAFETY: an owned IInspectable from the call above.
        let app = unsafe { IInspectable::from_raw(raw) };
        let mut application: *mut c_void = std::ptr::null_mut();
        // SAFETY: QueryInterface with a valid IID and out pointer.
        unsafe { app.query(&iid, &mut application) }
            .ok()
            .inspect_err(|e| log(&format!("no IApplication: {e}")))
            .ok()?;
        // SAFETY: `application` is an IApplication we own. Its vtable slot 6 (after IUnknown's
        // 3 and IInspectable's 3) is `get_Resources(ResourceDictionary**)` in both frameworks.
        let dictionary = unsafe {
            let application = IUnknown::from_raw(application);
            let vtable = *(application.as_raw() as *const *const usize);
            let get_resources: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT =
                std::mem::transmute(*vtable.add(6));
            let mut out: *mut c_void = std::ptr::null_mut();
            get_resources(application.as_raw(), &mut out)
                .ok()
                .inspect_err(|e| log(&format!("no application resources: {e}")))
                .ok()?;
            if out.is_null() {
                return None;
            }
            IInspectable::from_raw(out)
        };
        let mut handle = 0;
        // SAFETY: a live object and a valid out pointer.
        unsafe {
            self.diagnostics
                .GetHandleFromIInspectable(dictionary.as_raw(), &mut handle)
        }
        .ok()
        .inspect_err(|e| log(&format!("no handle for the application resources: {e}")))
        .ok()?;
        Some(handle)
    }

    /// The runtime class of the object behind `handle`.
    fn class_of(&self, handle: InstanceHandle) -> Option<String> {
        let mut raw: *mut c_void = std::ptr::null_mut();
        // SAFETY: a valid out pointer; we own the returned reference.
        unsafe { self.diagnostics.GetIInspectableFromHandle(handle, &mut raw) }
            .ok()
            .ok()?;
        if raw.is_null() {
            return None;
        }
        // SAFETY: an owned IInspectable from the call above.
        let object = unsafe { IInspectable::from_raw(raw) };
        object.GetRuntimeClassName().ok().map(|s| s.to_string())
    }

    fn report_once(&self, message: &str) {
        if self.lock().reported_messages.insert(message.to_owned()) {
            log(message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_dark_splits_and_plain_values_are_both() {
        assert_eq!(
            split_themes("light-dark(#0A000000, #14FFFFFF)"),
            ("#0A000000", "#14FFFFFF")
        );
        assert_eq!(split_themes("#20808080"), ("#20808080", "#20808080"));
        assert_eq!(
            split_themes("light-dark(#1)"),
            ("light-dark(#1)", "light-dark(#1)")
        );
    }

    #[test]
    fn resource_references_parse() {
        assert_eq!(
            resource_reference("{ThemeResource LayerFillColorDefaultBrush}"),
            Some((THEME_RESOURCE, "LayerFillColorDefaultBrush"))
        );
        assert_eq!(
            resource_reference(" { StaticResource  ControlCornerRadius } "),
            Some((STATIC_RESOURCE, "ControlCornerRadius"))
        );
        assert_eq!(resource_reference("{Binding Foo}"), None);
        assert_eq!(resource_reference("#FF000000"), None);
    }
}
