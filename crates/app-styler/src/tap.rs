//! The in-process half: the TAP ("tool attach point") DLL that XAML Diagnostics loads into an
//! allowlisted app when the helper calls `InitializeXamlDiagnosticsEx` for it.
//!
//! XAML Diagnostics creates our object through `DllGetClassObject` and hands it the diagnostics
//! site (`IObjectWithSite::SetSite`). A session thread then:
//!
//! 1. reads the attach data (helper pid, rules file, log) the helper passed,
//! 2. subscribes to the visual tree (`AdviseVisualTreeChange`, which first replays the whole
//!    existing tree as additions, then reports every element added or removed),
//! 3. sleeps on two handles: the helper process and a change notification on the rules folder.
//!
//! Each added element is matched against the rules on its own UI thread and styled there, with
//! the value it had before recorded. A removed element gets its values back first (it may come
//! back), a rules change re-styles every element on its own thread, and when the helper exits
//! for any reason (flag off, quit, panic hotkey, crash) every recorded value is put back and the
//! session ends: no thread, hook or timer of ours is left in the app. The DLL itself stays
//! mapped (a TAP can't unload safely).
//!
//! Nothing here may panic: release builds abort on panic, which would take the app down with
//! us. Every COM failure is logged and skipped.

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use mypaper_config::AppRules;
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, CloseHandle, E_INVALIDARG, E_NOTIMPL,
    E_POINTER, HANDLE, HINSTANCE, HMODULE, HWND, LPARAM, LRESULT, S_FALSE, S_OK, WAIT_OBJECT_0,
    WAIT_TIMEOUT, WPARAM,
};
use windows::Win32::Storage::FileSystem::{
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FindCloseChangeNotification,
    FindFirstChangeNotificationW, FindNextChangeNotification,
};
use windows::Win32::System::Com::{
    COINIT_MULTITHREADED, CoInitializeEx, IClassFactory, IClassFactory_Impl,
};
use windows::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    GetModuleHandleExW,
};
use windows::Win32::System::Ole::{IObjectWithSite, IObjectWithSite_Impl};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, GetCurrentThreadId, INFINITE, OpenProcess, PROCESS_SYNCHRONIZE,
    WaitForMultipleObjects,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_MESSAGE, PostMessageW, RegisterClassW,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WNDCLASSW,
};
use windows_core::{BOOL, BSTR, GUID, HRESULT, IUnknown, Interface, PCWSTR, Ref, implement, w};

use crate::selector::{Node, Selector};
use crate::xamlom::{
    IS_VALUE_HANDLE, IS_VALUE_NULL, IVisualTreeService3, IVisualTreeServiceCallback_Impl,
    IVisualTreeServiceCallback2, IVisualTreeServiceCallback2_Impl, IXamlDiagnostics,
    InstanceHandle, MUTATION_ADD, ParentChildRelation, PropertyChainSource, PropertyChainValue,
    SOURCE_LOCAL, VisualElement, free, free_chain, text,
};
use crate::{Attach, CLSID_TAP};

/// COM entry point: XAML Diagnostics asks for our class factory.
///
/// # Safety
/// Called by COM with valid pointers (checked for null anyway).
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    clsid: *const GUID,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if clsid.is_null() || iid.is_null() || out.is_null() {
        return E_POINTER;
    }
    // SAFETY: checked non-null above; COM passes valid pointers.
    unsafe {
        *out = std::ptr::null_mut();
        if *clsid != CLSID_TAP {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
        let factory: IClassFactory = Factory.into();
        factory.query(iid, out)
    }
}

/// A TAP never unloads: posted work and callbacks may still point into it.
#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    S_FALSE
}

#[implement(IClassFactory)]
struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<IUnknown>,
        iid: *const GUID,
        out: *mut *mut c_void,
    ) -> windows_core::Result<()> {
        if !outer.is_null() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let tap: IObjectWithSite = Tap.into();
        // SAFETY: COM passes a valid IID and out pointer.
        unsafe { tap.query(iid, out) }.ok()
    }

    fn LockServer(&self, _: BOOL) -> windows_core::Result<()> {
        Ok(())
    }
}

#[implement(IObjectWithSite)]
struct Tap;

impl IObjectWithSite_Impl for Tap_Impl {
    fn SetSite(&self, site: Ref<IUnknown>) -> windows_core::Result<()> {
        let Some(site) = site.as_ref() else {
            return Ok(());
        };
        let diagnostics: IXamlDiagnostics = site.cast()?;
        let mut raw = std::ptr::null();
        // SAFETY: a live interface and a valid out pointer; we own the returned BSTR.
        let data = unsafe {
            let hr = diagnostics.GetInitializationData(&mut raw);
            let data = text(raw);
            free(raw);
            hr.ok().map(|()| data)
        }?;
        let Some(attach) = Attach::decode(&data) else {
            return Err(E_INVALIDARG.into());
        };
        set_log(attach.log.clone());
        log(&format!("attached ({})", attach.framework));
        let diagnostics = Sendable(diagnostics);
        // `AdviseVisualTreeChange` from this thread hangs (it waits on the UI thread we are on),
        // so the session runs on its own thread.
        let spawned = std::thread::Builder::new()
            .name("mypaper-app-styler".into())
            .spawn(move || run(diagnostics, attach));
        if let Err(e) = spawned {
            log(&format!("no session thread: {e}"));
        }
        Ok(())
    }

    fn GetSite(&self, _: *const GUID, _: *mut *mut c_void) -> windows_core::Result<()> {
        Err(E_NOTIMPL.into())
    }
}

/// A COM pointer moved to the session thread. XAML Diagnostics' objects are free-threaded.
struct Sendable<T>(T);
// SAFETY: see above; the diagnostics site and tree service may be called from any thread.
unsafe impl<T> Send for Sendable<T> {}

static LOG: OnceLock<Option<PathBuf>> = OnceLock::new();

fn set_log(path: Option<PathBuf>) {
    let _ = LOG.set(path);
}

/// Appends a line to the helper's log file (`[tap pid] message`). Failures are ignored.
fn log(message: &str) {
    let Some(Some(path)) = LOG.get() else { return };
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        // SAFETY: no preconditions.
        let pid = unsafe { GetCurrentProcessId() };
        let _ = writeln!(file, "[tap {pid}] {message}");
    }
}

/// One attached XAML framework in this process.
struct Session {
    tree: IVisualTreeService3,
    rules_path: PathBuf,
    /// Set once the helper is gone: callbacks stop styling.
    closing: AtomicBool,
    state: Mutex<State>,
}

// SAFETY: the tree service is free-threaded; everything else is behind the mutex or atomic.
unsafe impl Send for Session {}
// SAFETY: as above.
unsafe impl Sync for Session {}

#[derive(Default)]
struct State {
    rules: Vec<Rule>,
    /// The rules file as last read, to skip rewrites that change nothing.
    source: String,
    nodes: HashMap<InstanceHandle, Element>,
    /// What we changed, per element.
    styled: HashMap<InstanceHandle, Vec<Applied>>,
    /// Values created from rule strings, by (property type, text): shared between elements.
    values: HashMap<(String, String), InstanceHandle>,
    /// One message-only window per UI thread, to run work there.
    threads: HashMap<u32, isize>,
    /// (element type, property) pairs already reported missing, so each is logged once.
    reported: HashSet<(String, String)>,
    /// Type names, shared by every element of a type.
    kinds: HashSet<Arc<str>>,
}

struct Element {
    parent: InstanceHandle,
    kind: Arc<str>,
    name: Box<str>,
    children: Vec<InstanceHandle>,
    /// The UI thread the element lives on.
    thread: u32,
}

/// What the rules want on one element: (property, value) pairs.
type Wanted = Vec<(String, String)>;

struct Rule {
    selector: Selector,
    set: Vec<(String, String)>,
}

/// A property we set and what to put back.
#[derive(Clone)]
struct Applied {
    property: String,
    index: u32,
    /// The property's type, which rule values are created as.
    kind: String,
    value: String,
    original: Original,
}

#[derive(Clone, Copy)]
enum Original {
    /// No local value: clearing the property brings back the style's.
    Unset,
    /// A value set on the element itself (by the app's code or markup).
    Local(InstanceHandle),
}

/// A local value as the property chain reports it.
enum LocalValue {
    /// The value object (`MetadataBit::IsValueHandle`).
    Handle(InstanceHandle),
    /// A plain value as text (may be empty when XAML can't print it).
    Text(String),
}

struct PropertyInfo {
    index: u32,
    kind: String,
    /// The element's own value, if it has one.
    local: Option<LocalValue>,
}

/// The matcher's view of an element in the map.
#[derive(Clone, Copy)]
struct At<'a> {
    nodes: &'a HashMap<InstanceHandle, Element>,
    element: &'a Element,
}

impl Node for At<'_> {
    fn kind(&self) -> &str {
        &self.element.kind
    }
    fn name(&self) -> &str {
        &self.element.name
    }
    fn parent(&self) -> Option<Self> {
        let element = self.nodes.get(&self.element.parent)?;
        Some(At {
            nodes: self.nodes,
            element,
        })
    }
}

fn run(diagnostics: Sendable<IXamlDiagnostics>, attach: Attach) {
    // SAFETY: plain COM init on our own thread.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let tree: IVisualTreeService3 = match diagnostics.0.cast() {
        Ok(tree) => tree,
        Err(e) => return log(&format!("no IVisualTreeService3: {e}")),
    };
    let session = Arc::new(Session {
        tree,
        rules_path: attach.rules.clone(),
        closing: AtomicBool::new(false),
        state: Mutex::new(State::default()),
    });
    session.reload_rules();

    // SAFETY: SYNCHRONIZE only; closed below.
    let helper = match unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, attach.helper) } {
        Ok(handle) => handle,
        Err(e) => return log(&format!("can't watch the helper ({}): {e}", attach.helper)),
    };
    let folder = attach
        .rules
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let folder = wide(&folder);
    // SAFETY: a NUL-terminated path; closed below.
    let changes = unsafe {
        FindFirstChangeNotificationW(
            PCWSTR(folder.as_ptr()),
            false,
            FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_FILE_NAME,
        )
    }
    .inspect_err(|e| log(&format!("rules won't apply live: {e}")))
    .ok();

    let callback: IVisualTreeServiceCallback2 = Callback {
        session: session.clone(),
    }
    .into();
    // SAFETY: a live callback object; the service AddRefs it.
    let advised = unsafe { session.tree.AdviseVisualTreeChange(callback.as_raw()) };
    log(&format!("watching the visual tree: {advised:?}"));

    let mut dump = attach.dump.clone();
    let handles: Vec<HANDLE> = [Some(helper), changes].into_iter().flatten().collect();
    loop {
        let timeout = if dump.is_some() { 3000 } else { INFINITE };
        // SAFETY: the handles stay open for the loop.
        let woke = unsafe { WaitForMultipleObjects(&handles, false, timeout) };
        if woke == WAIT_TIMEOUT {
            if let Some(path) = dump.take() {
                session.dump(&path);
            }
        } else if woke == WAIT_OBJECT_0 {
            break; // the helper exited
        } else if woke.0 == WAIT_OBJECT_0.0 + 1
            && let Some(changes) = changes
        {
            // SAFETY: the notification handle from above.
            let _ = unsafe { FindNextChangeNotification(changes) };
            if session.reload_rules() {
                session.restyle();
            }
        } else {
            log(&format!("wait failed ({woke:?}); detaching"));
            break;
        }
    }

    session.closing.store(true, Ordering::SeqCst);
    // SAFETY: the callback we advised.
    let unadvised = unsafe { session.tree.UnadviseVisualTreeChange(callback.as_raw()) };
    session.restore_all();
    log(&format!("detached: {unadvised:?}"));
    // SAFETY: handles opened above, closed once.
    unsafe {
        let _ = CloseHandle(helper);
        if let Some(changes) = changes {
            let _ = FindCloseChangeNotification(changes);
        }
    }
}

fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain([0]).collect()
}

impl Session {
    fn lock(&self) -> MutexGuard<'_, State> {
        // A poisoned lock only means a panic elsewhere; the data is still usable.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Re-reads the rules file. Returns whether the rules changed.
    fn reload_rules(&self) -> bool {
        let source = std::fs::read_to_string(&self.rules_path).unwrap_or_default();
        let mut state = self.lock();
        if source == state.source {
            return false;
        }
        let parsed: AppRules = if source.trim().is_empty() {
            AppRules::default()
        } else {
            match serde_json::from_str(&source) {
                Ok(rules) => rules,
                Err(e) => {
                    log(&format!("rules file: {e}; keeping the current rules"));
                    return false;
                }
            }
        };
        state.rules = compile(&parsed);
        state.source = source;
        log(&format!("{} rule(s)", state.rules.len()));
        true
    }

    /// An element joined the tree (on its UI thread).
    fn on_add(&self, handle: InstanceHandle, parent: InstanceHandle, kind: &str, name: String) {
        // SAFETY: no preconditions.
        let thread = unsafe { GetCurrentThreadId() };
        let wanted = {
            let mut state = self.lock();
            if !state.threads.contains_key(&thread)
                && let Some(window) = message_window()
            {
                state.threads.insert(thread, window.0 as isize);
            }
            let kind: Arc<str> = match state.kinds.get(kind) {
                Some(k) => k.clone(),
                None => {
                    let k: Arc<str> = kind.into();
                    state.kinds.insert(k.clone());
                    k
                }
            };
            // Re-parented: leave the old parent's children.
            let previous = state
                .nodes
                .get(&handle)
                .map(|e| (e.parent, e.children.clone()));
            if let Some((old, _)) = previous
                && let Some(p) = state.nodes.get_mut(&old)
            {
                p.children.retain(|&c| c != handle);
            }
            state.nodes.insert(
                handle,
                Element {
                    parent,
                    kind,
                    name: name.into(),
                    children: previous.map(|(_, c)| c).unwrap_or_default(),
                    thread,
                },
            );
            if let Some(p) = state.nodes.get_mut(&parent) {
                p.children.push(handle);
            }
            state.wanted(handle)
        };
        if !wanted.is_empty() && !self.closing.load(Ordering::SeqCst) {
            self.style(handle, &wanted);
        }
    }

    /// An element left the tree (on its UI thread): give it and everything below it its values
    /// back, since it may be added again, and forget them.
    fn on_remove(&self, handle: InstanceHandle) {
        let taken = {
            let mut state = self.lock();
            if let Some(parent) = state.nodes.get(&handle).map(|e| e.parent)
                && let Some(p) = state.nodes.get_mut(&parent)
            {
                p.children.retain(|&c| c != handle);
            }
            let mut stack = vec![handle];
            let mut taken = Vec::new();
            while let Some(h) = stack.pop() {
                if let Some(element) = state.nodes.remove(&h) {
                    stack.extend(element.children);
                }
                if let Some(applied) = state.styled.remove(&h) {
                    taken.push((h, applied));
                }
            }
            taken
        };
        for (h, applied) in taken {
            self.restore(h, &applied);
        }
    }

    /// Sets `wanted` on the element (on its UI thread), keeping what it had before. Properties
    /// it already has from us keep their first original; ones no rule wants any more go back.
    fn style(&self, handle: InstanceHandle, wanted: &[(String, String)]) {
        let mut previous: HashMap<String, Applied> = self
            .lock()
            .styled
            .remove(&handle)
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.property.clone(), a))
            .collect();
        let chain = if wanted.iter().any(|(p, _)| !previous.contains_key(p)) {
            self.properties(handle)
        } else {
            HashMap::new()
        };
        let mut applied = Vec::new();
        for (property, value) in wanted {
            let slot = match previous.remove(property) {
                Some(ours) => ours,
                None => {
                    let Some(info) = chain.get(property) else {
                        self.report_missing(handle, property);
                        continue;
                    };
                    let Some(original) = self.original(handle, info, property) else {
                        continue;
                    };
                    Applied {
                        property: property.clone(),
                        index: info.index,
                        kind: info.kind.clone(),
                        value: String::new(),
                        original,
                    }
                }
            };
            if slot.value == *value {
                applied.push(slot);
                continue;
            }
            match self.value(&slot.kind, value) {
                Some(v) if self.set(handle, slot.index, v, property) => applied.push(Applied {
                    value: value.clone(),
                    ..slot
                }),
                // Ours before, and the new value failed: put the original back.
                _ if !slot.value.is_empty() => self.restore_one(handle, &slot),
                _ => {}
            }
        }
        for ours in previous.into_values() {
            self.restore_one(handle, &ours);
        }
        if !applied.is_empty() {
            self.lock().styled.insert(handle, applied);
        }
    }

    /// What to put back later: the element's own (local) value, or "unset".
    ///
    /// A local value is taken from the property chain: the value object's handle, or its text
    /// recreated as the property's type. (`GetProperty`'s handle for the same value is refused by
    /// `SetProperty` in WinUI 3.) Only when the chain has neither is `GetProperty` used.
    fn original(
        &self,
        handle: InstanceHandle,
        info: &PropertyInfo,
        name: &str,
    ) -> Option<Original> {
        let Some(local) = &info.local else {
            return Some(Original::Unset);
        };
        let value = match local {
            LocalValue::Handle(h) => Some(*h),
            LocalValue::Text(t) if !t.is_empty() => self.value(&info.kind, t),
            LocalValue::Text(_) => None,
        };
        if let Some(value) = value {
            return Some(Original::Local(value));
        }
        let mut current = 0;
        // SAFETY: a live service and a valid out pointer.
        match unsafe { self.tree.GetProperty(handle, info.index, &mut current) }.ok() {
            Ok(()) => Some(Original::Local(current)),
            Err(e) => {
                log(&format!("reading {name}: {e}; leaving it alone"));
                None
            }
        }
    }

    fn set(&self, handle: InstanceHandle, index: u32, value: InstanceHandle, name: &str) -> bool {
        // SAFETY: handles from the service.
        match unsafe { self.tree.SetProperty(handle, value, index) }.ok() {
            Ok(()) => true,
            Err(e) => {
                log(&format!("setting {name}: {e}"));
                false
            }
        }
    }

    fn restore(&self, handle: InstanceHandle, applied: &[Applied]) {
        for a in applied {
            self.restore_one(handle, a);
        }
    }

    /// Puts one property back: the local value it had, or cleared. If the local value can't be
    /// set back, clearing is still better than leaving ours.
    fn restore_one(&self, handle: InstanceHandle, a: &Applied) {
        let clear = || {
            // SAFETY: handles from the service; a stale handle only fails.
            unsafe { self.tree.ClearProperty(handle, a.index) }.ok()
        };
        let result = match a.original {
            // SAFETY: as above.
            Original::Local(value) => unsafe { self.tree.SetProperty(handle, value, a.index) }
                .ok()
                .or_else(|e| {
                    log(&format!(
                        "restoring {}: {e}; clearing it instead",
                        a.property
                    ));
                    clear()
                }),
            Original::Unset => clear(),
        };
        if let Err(e) = result {
            log(&format!("restoring {}: {e}", a.property));
        }
    }

    /// A value created from a rule's text as the property's type (cached).
    fn value(&self, kind: &str, text_value: &str) -> Option<InstanceHandle> {
        let key = (kind.to_owned(), text_value.to_owned());
        if let Some(&v) = self.lock().values.get(&key) {
            return Some(v);
        }
        let kind_b = BSTR::from(kind);
        let value_b = BSTR::from(text_value);
        let mut out = 0;
        // SAFETY: two live BSTRs and a valid out pointer.
        let created = unsafe {
            self.tree
                .CreateInstance(kind_b.as_ptr(), value_b.as_ptr(), &mut out)
        };
        if let Err(e) = created.ok() {
            log(&format!("{text_value:?} is not a {kind}: {e}"));
            return None;
        }
        self.lock().values.insert(key, out);
        Some(out)
    }

    /// The element's properties by name: index, declared type and whether it has a local value.
    fn properties(&self, handle: InstanceHandle) -> HashMap<String, PropertyInfo> {
        let mut source_count = 0;
        let mut sources: *mut PropertyChainSource = std::ptr::null_mut();
        let mut value_count = 0;
        let mut values: *mut PropertyChainValue = std::ptr::null_mut();
        // SAFETY: valid out pointers; the arrays are freed below.
        let got = unsafe {
            self.tree.GetPropertyValuesChain(
                handle,
                &mut source_count,
                &mut sources,
                &mut value_count,
                &mut values,
            )
        };
        let mut out: HashMap<String, PropertyInfo> = HashMap::new();
        if let Err(e) = got.ok() {
            log(&format!("reading properties: {e}"));
            return out;
        }
        // SAFETY: the counts describe the arrays the call returned, freed once at the end.
        unsafe {
            for i in 0..value_count as usize {
                let v = &*values.add(i);
                let local = v.PropertyChainIndex < source_count
                    && (*sources.add(v.PropertyChainIndex as usize)).Source == SOURCE_LOCAL;
                let entry = out
                    .entry(text(v.PropertyName))
                    .or_insert_with(|| PropertyInfo {
                        index: v.Index,
                        kind: text(v.Type),
                        local: None,
                    });
                // A local null can't be recreated; clearing is the closest way back.
                if local && v.MetadataBits & IS_VALUE_NULL == 0 {
                    let value = text(v.Value);
                    entry.local = Some(
                        match value
                            .parse()
                            .ok()
                            .filter(|_| v.MetadataBits & IS_VALUE_HANDLE != 0)
                        {
                            Some(handle) => LocalValue::Handle(handle),
                            None => LocalValue::Text(value),
                        },
                    );
                }
            }
            free_chain(sources, source_count, values, value_count);
        }
        out
    }

    fn report_missing(&self, handle: InstanceHandle, property: &str) {
        let mut state = self.lock();
        let kind = state
            .nodes
            .get(&handle)
            .map(|e| e.kind.to_string())
            .unwrap_or_default();
        if state.reported.insert((kind.clone(), property.to_owned())) {
            log(&format!("{kind} has no property {property}"));
        }
    }

    /// The rules changed: every element gets what the new rules want, on its own UI thread.
    fn restyle(self: &Arc<Self>) {
        let (plan, windows) = {
            let state = self.lock();
            let mut plan: HashMap<u32, Vec<(InstanceHandle, Wanted)>> = HashMap::new();
            for (&handle, element) in &state.nodes {
                let wanted = state.wanted(handle);
                if !wanted.is_empty() || state.styled.contains_key(&handle) {
                    plan.entry(element.thread)
                        .or_default()
                        .push((handle, wanted));
                }
            }
            (plan, state.threads.clone())
        };
        let count: usize = plan.values().map(Vec::len).sum();
        log(&format!("restyling {count} element(s)"));
        for (thread, elements) in plan {
            let Some(&window) = windows.get(&thread) else {
                continue;
            };
            let session = self.clone();
            post(thread, window, move || {
                for (handle, wanted) in elements {
                    if wanted.is_empty() {
                        let applied = session.lock().styled.remove(&handle);
                        if let Some(applied) = applied {
                            session.restore(handle, &applied);
                        }
                    } else {
                        session.style(handle, &wanted);
                    }
                }
            });
        }
    }

    /// Puts every recorded value back (each on its own UI thread) and closes the threads'
    /// message windows.
    fn restore_all(self: &Arc<Self>) {
        let (mut by_thread, windows) = {
            let mut state = self.lock();
            let mut by_thread: HashMap<u32, Vec<(InstanceHandle, Vec<Applied>)>> = HashMap::new();
            for (handle, applied) in std::mem::take(&mut state.styled) {
                let thread = state.nodes.get(&handle).map_or(0, |e| e.thread);
                by_thread.entry(thread).or_default().push((handle, applied));
            }
            state.nodes.clear();
            (by_thread, std::mem::take(&mut state.threads))
        };
        let count: usize = by_thread.values().map(Vec::len).sum();
        log(&format!("restoring {count} element(s)"));
        for (thread, window) in windows {
            let elements = by_thread.remove(&thread).unwrap_or_default();
            let session = self.clone();
            post(thread, window, move || {
                for (handle, applied) in elements {
                    session.restore(handle, &applied);
                }
                // SAFETY: our own message-only window, destroyed on its thread.
                let _ = unsafe { DestroyWindow(HWND(window as *mut c_void)) };
            });
        }
        if !by_thread.is_empty() {
            log("some elements were on threads without a window; not restored");
        }
    }

    /// Writes the visual tree as an indented outline (`mypaper-styler --dump`).
    fn dump(&self, path: &Path) {
        let state = self.lock();
        let mut stack: Vec<(InstanceHandle, usize)> = state
            .nodes
            .iter()
            .filter(|(_, e)| !state.nodes.contains_key(&e.parent))
            .map(|(&h, _)| (h, 0))
            .collect();
        let mut out = String::new();
        while let Some((h, depth)) = stack.pop() {
            let Some(e) = state.nodes.get(&h) else {
                continue;
            };
            out.push_str(&"  ".repeat(depth));
            out.push_str(&e.kind);
            if !e.name.is_empty() {
                out.push('#');
                out.push_str(&e.name);
            }
            if let Some(applied) = state.styled.get(&h) {
                let set: Vec<String> = applied
                    .iter()
                    .map(|a| format!("{}={}", a.property, a.value))
                    .collect();
                out.push_str(&format!("   <- {}", set.join(", ")));
            }
            out.push('\n');
            stack.extend(e.children.iter().rev().map(|&c| (c, depth + 1)));
        }
        let _ = std::fs::write(path, out);
        log(&format!(
            "dumped {} element(s) to {}",
            state.nodes.len(),
            path.display()
        ));
    }
}

impl State {
    /// What the rules want on `handle`: (property, value), the most specific rule winning.
    fn wanted(&self, handle: InstanceHandle) -> Vec<(String, String)> {
        let Some(element) = self.nodes.get(&handle) else {
            return Vec::new();
        };
        let at = At {
            nodes: &self.nodes,
            element,
        };
        let mut wanted: Vec<(String, String)> = Vec::new();
        for rule in &self.rules {
            if rule.selector.matches(&at) {
                for (p, v) in &rule.set {
                    match wanted.iter_mut().find(|(q, _)| q == p) {
                        Some(slot) => slot.1 = v.clone(),
                        None => wanted.push((p.clone(), v.clone())),
                    }
                }
            }
        }
        wanted
    }
}

/// Parses the selectors (a bad one is logged and skipped) and orders the rules least specific
/// first, so a more specific rule's value wins.
fn compile(rules: &AppRules) -> Vec<Rule> {
    let mut out: Vec<Rule> = rules
        .rules
        .iter()
        .filter_map(
            |(selector, properties)| match selector.parse::<Selector>() {
                Ok(parsed) => Some(Rule {
                    selector: parsed,
                    set: properties
                        .iter()
                        .filter_map(|(p, v)| Some((p.clone(), v.clone()?)))
                        .collect(),
                }),
                Err(e) => {
                    log(&e.to_string());
                    None
                }
            },
        )
        .collect();
    out.sort_by_key(|r| r.selector.specificity());
    out
}

/// Runs `job` on `thread`: right here if that is us, else posted to its message window. A job
/// owns everything it uses, so it is safe whenever it runs; one for a thread that is gone is
/// dropped unrun.
fn post(thread: u32, window: isize, job: impl FnOnce() + Send + 'static) {
    // SAFETY: no preconditions.
    if thread == unsafe { GetCurrentThreadId() } {
        return job();
    }
    let raw = Box::into_raw(Box::new(Box::new(job) as Box<dyn FnOnce() + Send>));
    // SAFETY: the window runs the job on its thread and frees it (`window_proc`).
    let posted = unsafe {
        PostMessageW(
            Some(HWND(window as *mut c_void)),
            WM_JOB,
            WPARAM(0),
            LPARAM(raw as isize),
        )
    };
    if posted.is_err() {
        // SAFETY: not posted, so still ours.
        drop(unsafe { Box::from_raw(raw) });
    }
}

const WM_JOB: u32 = WM_APP + 0x51;

/// A message-only window on the calling (UI) thread, for [`post`].
fn message_window() -> Option<HWND> {
    static CLASS: OnceLock<bool> = OnceLock::new();
    let module = own_module()?;
    let registered = *CLASS.get_or_init(|| {
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: HINSTANCE(module.0),
            lpszClassName: w!("mypaper-app-styler"),
            ..Default::default()
        };
        // SAFETY: a valid class description with a 'static name.
        unsafe { RegisterClassW(&class) != 0 }
    });
    if !registered {
        return None;
    }
    // SAFETY: a registered class; a message-only window owned by this thread.
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("mypaper-app-styler"),
            None,
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(HINSTANCE(module.0)),
            None,
        )
    }
    .ok()
}

fn own_module() -> Option<HMODULE> {
    let mut module = HMODULE::default();
    // SAFETY: looks up the module containing this function; no refcount change.
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(own_module as *const () as *const u16),
            &mut module,
        )
    }
    .ok()?;
    Some(module)
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_JOB && lparam.0 != 0 {
        // SAFETY: posted by `post`, which gave up ownership of the box.
        let job = unsafe { Box::from_raw(lparam.0 as *mut Box<dyn FnOnce() + Send>) };
        job();
        return LRESULT(0);
    }
    // SAFETY: default handling for everything else.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

#[implement(IVisualTreeServiceCallback2)]
struct Callback {
    session: Arc<Session>,
}

impl IVisualTreeServiceCallback_Impl for Callback_Impl {
    unsafe fn OnVisualTreeChange(
        &self,
        relation: ParentChildRelation,
        element: VisualElement,
        mutation: i32,
    ) -> HRESULT {
        let session = &self.session;
        if session.closing.load(Ordering::SeqCst) {
            return S_OK;
        }
        if mutation == MUTATION_ADD {
            // SAFETY: the strings are the caller's, valid for this call.
            let (kind, name) = unsafe { (text(element.Type), text(element.Name)) };
            session.on_add(element.Handle, relation.Parent, &kind, name);
        } else {
            session.on_remove(element.Handle);
        }
        S_OK
    }
}

impl IVisualTreeServiceCallback2_Impl for Callback_Impl {
    unsafe fn OnElementStateChanged(&self, _: InstanceHandle, _: i32, _: PCWSTR) -> HRESULT {
        S_OK
    }
}
