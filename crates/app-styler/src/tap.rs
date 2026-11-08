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
//! the value it had before recorded. Each UI thread's theme-brush overrides ([`resources`]) are
//! applied when its first element appears. A removed element gets its values back first (it may
//! come back), a rules change re-styles every thread and element, and when the helper exits for
//! any reason (flag off, quit, panic hotkey, crash) or writes this app's stop marker (its crash
//! guard, or the app switched off) or this process's (an Explorer folder process whose last
//! window closed), every recorded value and brush is put back and the session ends: no thread,
//! hook or timer of ours is left in the app. The DLL itself stays mapped (a TAP can't unload safely).
//!
//! XAML Diagnostics' handles don't keep objects alive (see [`Original`]): anything kept is held
//! by a strong reference and re-resolved to a fresh handle before use. Work for a UI thread is
//! posted as a job that carries no handles: it looks up what it works on when it runs, on that
//! thread, where the tree callbacks keep the map current. An element is held while it is styled
//! or restored, so nothing done meanwhile can destroy it under us.
//!
//! Nothing here may panic: release builds abort on panic, which would take the app down with
//! us. Every COM failure is logged and skipped.

mod inspect;
mod resources;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::io::Write;
use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use glaze_config::AppRules;
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
    CreateWindowExW, DefWindowProcW, DestroyWindow, GCW_ATOM, GWLP_USERDATA, GWLP_WNDPROC,
    GetClassLongPtrW, GetWindowLongPtrW, GetWindowThreadProcessId, HWND_MESSAGE, IsWindow,
    PostMessageW, RegisterClassW, SetWindowLongPtrW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
    WNDCLASSW,
};
use windows_core::{
    BOOL, BSTR, GUID, HRESULT, IInspectable, IUnknown, Interface, PCWSTR, Ref, implement, w,
};

use crate::selector::{Node, Selector};
use crate::xamlom::{
    IS_VALUE_HANDLE, IS_VALUE_NULL, IVisualTreeService3, IVisualTreeServiceCallback_Impl,
    IVisualTreeServiceCallback2, IVisualTreeServiceCallback2_Impl, IXamlDiagnostics,
    InstanceHandle, MUTATION_ADD, ParentChildRelation, PropertyChainSource, PropertyChainValue,
    RawBstr, SOURCE_LOCAL, VisualElement, free, free_chain, text,
};
use crate::{Attach, CLSID_TAP, Framework};

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
        set_log(Some(attach.log_path()));
        log(&format!("attached ({})", attach.framework));
        let diagnostics = Sendable(diagnostics);
        // `AdviseVisualTreeChange` from this thread hangs (it waits on the UI thread we are on),
        // so the session runs on its own thread.
        let spawned = std::thread::Builder::new()
            .name("glaze-app-styler".into())
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
        // One write per line, so threads logging at once don't interleave.
        let _ = file.write_all(format!("[tap {pid}] {message}\n").as_bytes());
    }
}

/// One attached XAML framework in this process.
struct Session {
    diagnostics: IXamlDiagnostics,
    tree: IVisualTreeService3,
    framework: Framework,
    rules_path: PathBuf,
    /// Set (under the state lock) once the helper is gone: nothing is styled any more, and each
    /// UI thread's clean-up job puts its values back.
    closing: AtomicBool,
    /// Set when a UI thread didn't clean up in time and the session detached anyway: the tree is
    /// no longer followed, so its late jobs don't touch XAML (they leak what they hold).
    abandoned: AtomicBool,
    state: Mutex<State>,
    /// Signalled whenever a UI thread's record leaves `State::threads` (cleaned up or gone).
    cleaned: Condvar,
    /// Restyle rounds started; a thread runs only the latest one queued for it.
    restyles: AtomicU64,
    /// Removals of elements the map doesn't have (logged when closing, for diagnosis).
    unknown_removals: AtomicU64,
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
    /// Values created from rule strings, by (UI thread, property type, text): shared between
    /// the elements of one thread (XAML objects belong to the thread that made them). Values no
    /// element uses any more are evicted after each restyle.
    values: HashMap<(u32, String, String), (InstanceHandle, Held)>,
    /// The UI threads we work on, by thread id.
    threads: HashMap<u32, UiThread>,
    /// (element type, property) pairs already reported missing, so each is logged once.
    reported: HashSet<(String, String)>,
    /// Other messages already logged once.
    reported_messages: HashSet<String>,
    /// Type names, shared by every element of a type.
    kinds: HashSet<Arc<str>>,
    /// The resource overrides the rules want: (key, value).
    resource_rules: Vec<(String, String)>,
    /// Resource overrides in effect, per UI thread.
    resources: HashMap<u32, resources::ThreadResources>,
    /// Brushes whose colour we set (one override per brush).
    recolored: HashSet<InstanceHandle>,
}

/// One UI thread we work on: the message-only window its jobs are posted to, and that window's
/// lifetime generation (also its `GWLP_USERDATA` and every job's `WPARAM`). A window that was
/// destroyed, or a thread id that came back as another thread, never runs a job meant for the
/// old one.
#[derive(Clone, Copy)]
struct UiThread {
    window: isize,
    generation: u64,
}

/// Generations of message windows, unique in the process (every session shares the class).
static GENERATION: AtomicU64 = AtomicU64::new(1);

impl UiThread {
    /// A message-only window on the calling (UI) thread, for [`post`].
    fn create() -> Option<Self> {
        let atom = class_atom()?;
        let module = own_module()?;
        // SAFETY: a registered class (by atom); a message-only window owned by this thread.
        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(usize::from(atom) as *const u16),
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
        .ok()?;
        // Unique across the TAP builds loaded in this process too: each has its own counter, so
        // it starts from this module's address.
        let generation = (module.0 as u64) ^ GENERATION.fetch_add(1, Ordering::Relaxed);
        // SAFETY: our own window.
        unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, generation as isize) };
        Some(Self {
            window: window.0 as isize,
            generation,
        })
    }

    fn hwnd(self) -> HWND {
        HWND(self.window as *mut c_void)
    }

    /// Whether the window is still this one: ours, on `thread`, of this generation.
    fn alive(self, thread: u32) -> bool {
        self.dead(thread).is_none()
    }

    /// Why the window isn't this one any more (`None`: it is). Every TAP build loaded in the
    /// process registers the same class name (one atom), so the window procedure is what says
    /// a window is this build's: a job posted to another build's window would be misread.
    fn dead(self, thread: u32) -> Option<&'static str> {
        let Some(atom) = class_atom() else {
            return Some("no class");
        };
        let hwnd = self.hwnd();
        let mut pid = 0;
        // SAFETY: plain queries; a stale handle just fails each check.
        unsafe {
            if !IsWindow(Some(hwnd)).as_bool() {
                Some("destroyed")
            } else if GetWindowThreadProcessId(hwnd, Some(&mut pid)) != thread
                || pid != GetCurrentProcessId()
            {
                Some("another thread's")
            } else if GetClassLongPtrW(hwnd, GCW_ATOM) != usize::from(atom)
                || GetWindowLongPtrW(hwnd, GWLP_WNDPROC) != window_proc as *const () as isize
            {
                Some("not ours")
            } else if GetWindowLongPtrW(hwnd, GWLP_USERDATA) != self.generation as isize {
                Some("another generation")
            } else {
                None
            }
        }
    }
}

struct Element {
    parent: InstanceHandle,
    kind: Arc<str>,
    name: Box<str>,
    children: Vec<InstanceHandle>,
    /// The UI thread the element lives on.
    thread: u32,
}

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

/// What a property had before we set it.
///
/// XAML Diagnostics' handle table doesn't keep objects alive: a handle whose object has died
/// fails, or worse, names a newer object. So an original object is held (a strong reference)
/// until it goes back, and a value type (a colour, a number) is kept as text and made again
/// right before it goes back.
#[derive(Clone)]
enum Original {
    /// No local value: clearing the property brings back the style's.
    Unset,
    /// A local value type, as text.
    Text(String),
    /// A local object (a brush...), held alive.
    Object(InstanceHandle, Held),
}

/// A strong reference that keeps a XAML object, and so its handle, alive. Dropped on the
/// object's UI thread (restores and the per-thread clean-up run there).
#[derive(Clone)]
struct Held(IInspectable);

// SAFETY: only moved between threads, never called: it is created and dropped on the object's
// UI thread (a thread that is gone leaks it instead, see `forget_thread`).
unsafe impl Send for Held {}
// SAFETY: as above.
unsafe impl Sync for Held {}

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
    let stop = attach.stop_path();
    // SAFETY: no preconditions.
    let own_stop = attach.process_stop_path(unsafe { GetCurrentProcessId() });
    if stop.exists() || own_stop.exists() {
        return log("the helper stopped styling this app; not attaching");
    }
    let session = Arc::new(Session {
        diagnostics: diagnostics.0,
        tree,
        framework: attach.framework,
        rules_path: attach.rules_path(),
        closing: AtomicBool::new(false),
        abandoned: AtomicBool::new(false),
        state: Mutex::new(State::default()),
        cleaned: Condvar::new(),
        restyles: AtomicU64::new(0),
        unknown_removals: AtomicU64::new(0),
    });
    session.reload_rules();

    // SAFETY: SYNCHRONIZE only; closed below.
    let helper = match unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, attach.helper) } {
        Ok(handle) => handle,
        Err(e) => return log(&format!("can't watch the helper ({}): {e}", attach.helper)),
    };
    let folder = session
        .rules_path
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

    let mut dump = attach.dump.then(|| {
        // SAFETY: no preconditions.
        let pid = unsafe { GetCurrentProcessId() };
        attach.dir.join(format!("tree-{pid}.txt"))
    });
    let handles: Vec<HANDLE> = [Some(helper), changes].into_iter().flatten().collect();
    loop {
        let timeout = if dump.is_some() { 3000 } else { INFINITE };
        // SAFETY: the handles stay open for the loop.
        let woke = unsafe { WaitForMultipleObjects(&handles, false, timeout) };
        if woke == WAIT_TIMEOUT {
            if let Some(path) = dump.take() {
                session.dump(&path);
                if let Ok(selector) = std::fs::read_to_string(attach.dir.join("props.txt")) {
                    session.dump_props(&path, selector.trim());
                }
            }
        } else if woke == WAIT_OBJECT_0 {
            break; // the helper exited
        } else if woke.0 == WAIT_OBJECT_0.0 + 1
            && let Some(changes) = changes
        {
            // SAFETY: the notification handle from above.
            let _ = unsafe { FindNextChangeNotification(changes) };
            if stop.exists() {
                log("the helper stopped styling this app (crash guard or config)");
                break;
            }
            if own_stop.exists() {
                log("the helper let go of this process (its last folder window closed)");
                break;
            }
            if session.reload_rules() {
                session.restyle();
            }
        } else {
            log(&format!("wait failed ({woke:?}); detaching"));
            break;
        }
    }

    // Every UI thread puts its values back while the tree is still followed (removals keep the
    // map true until each clean-up has run); only then stop following it.
    session.close();
    // SAFETY: the callback we advised.
    let unadvised = unsafe { session.tree.UnadviseVisualTreeChange(callback.as_raw()) };
    session.forget_rest();
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
        state.resource_rules = parsed
            .resources
            .iter()
            .filter_map(|(k, v)| Some((k.clone(), v.clone()?)))
            .collect();
        state.source = source;
        log(&format!(
            "{} rule(s), {} resource(s)",
            state.rules.len(),
            state.resource_rules.len()
        ));
        true
    }

    /// An element joined the tree (on its UI thread).
    fn on_add(&self, handle: InstanceHandle, parent: InstanceHandle, kind: &str, name: String) {
        // SAFETY: no preconditions.
        let thread = unsafe { GetCurrentThreadId() };
        let (wanted, new_thread, roots) = {
            let mut state = self.lock();
            // Read under the lock `close` sets it under, so a thread recorded here is one whose
            // clean-up `close` posts.
            if self.closing.load(Ordering::SeqCst) {
                return;
            }
            let new_thread = match state.threads.get(&thread).copied() {
                Some(ui) if ui.alive(thread) => false,
                stale => {
                    // The thread id came back as a new thread: the old one is gone.
                    if let Some(old) = stale {
                        let why = old.dead(thread).unwrap_or_default();
                        let n = forget_locked(&mut state, thread, old.generation);
                        log(&format!(
                            "thread {thread}: its window is {why}; forgot {n} element(s)"
                        ));
                        self.cleaned.notify_all();
                    }
                    match UiThread::create() {
                        Some(ui) => {
                            state.threads.insert(thread, ui);
                            true
                        }
                        None => {
                            log(&format!("thread {thread}: no message window"));
                            false
                        }
                    }
                }
            };
            let kind: Arc<str> = match state.kinds.get(kind) {
                Some(k) => k.clone(),
                None => {
                    let k: Arc<str> = kind.into();
                    state.kinds.insert(k.clone());
                    k
                }
            };
            // Re-parented without a removal: leave the old parent's children. An entry from
            // another thread is a handle that named an object we never saw leave: forget it.
            let previous = state.nodes.remove(&handle);
            let children = match previous {
                Some(old) if old.thread == thread => {
                    if let Some(p) = state.nodes.get_mut(&old.parent) {
                        p.children.retain(|&c| c != handle);
                    }
                    old.children
                }
                Some(_) => {
                    std::mem::forget(state.styled.remove(&handle));
                    Vec::new()
                }
                None => Vec::new(),
            };
            state.nodes.insert(
                handle,
                Element {
                    parent,
                    kind,
                    name: name.into(),
                    children,
                    thread,
                },
            );
            let roots = match state.nodes.get_mut(&parent) {
                Some(p) => {
                    p.children.push(handle);
                    Vec::new()
                }
                // A new root (a window or popup opening): the thread's other roots, to see
                // whether a closed window's tree went without a removal.
                None => state
                    .nodes
                    .iter()
                    .filter(|&(&h, e)| {
                        h != handle && e.thread == thread && !state.nodes.contains_key(&e.parent)
                    })
                    .map(|(&h, _)| h)
                    .collect(),
            };
            (state.wanted(handle), new_thread, roots)
        };
        // The thread's first element: its theme resources first, so rules see them.
        if new_thread {
            self.apply_resources(thread);
        }
        if !wanted.is_empty() {
            self.style(handle, &wanted);
        }
        let gone: Vec<InstanceHandle> = roots
            .into_iter()
            .filter(|&r| self.hold(r).is_none())
            .collect();
        self.forget_gone(thread, &gone);
    }

    /// Forgets elements (with everything below them) whose objects are gone without a removal
    /// (Explorer closes a window but keeps its thread, and the tree just goes): their handles
    /// don't resolve any more, so there is nothing to put back. On their UI thread.
    fn forget_gone(&self, thread: u32, gone: &[InstanceHandle]) {
        if gone.is_empty() {
            return;
        }
        let (count, styled) = {
            let mut state = self.lock();
            let mut stack = gone.to_vec();
            let mut count = 0;
            let mut styled = Vec::new();
            for &h in gone {
                if let Some(parent) = state.nodes.get(&h).map(|e| e.parent)
                    && let Some(p) = state.nodes.get_mut(&parent)
                {
                    p.children.retain(|&c| c != h);
                }
            }
            while let Some(h) = stack.pop() {
                if let Some(element) = state.nodes.remove(&h) {
                    count += 1;
                    stack.extend(element.children);
                }
                styled.extend(state.styled.remove(&h));
            }
            (count, styled)
        };
        // What they held (originals) is released here, on its thread, outside the lock.
        drop(styled);
        log(&format!(
            "thread {thread}: {count} element(s) went without a removal; forgot them"
        ));
    }

    /// An element left the tree (on its UI thread): give it and everything below it its values
    /// back, since it may be added again, and forget them. When the thread's last element goes
    /// (its window closed), the thread's own state is retired too, once this work is done.
    fn on_remove(self: &Arc<Self>, handle: InstanceHandle) {
        // SAFETY: no preconditions.
        let thread = unsafe { GetCurrentThreadId() };
        let (taken, emptied) = {
            let mut state = self.lock();
            let Some(parent) = state.nodes.get(&handle).map(|e| e.parent) else {
                self.unknown_removals.fetch_add(1, Ordering::Relaxed);
                return;
            };
            let root = match state.nodes.get_mut(&parent) {
                Some(p) => {
                    p.children.retain(|&c| c != handle);
                    false
                }
                None => true,
            };
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
            // Only a root leaving can empty a thread; checking then is rare and cheap.
            let emptied = root && !state.nodes.values().any(|e| e.thread == thread);
            (taken, emptied)
        };
        self.restore_taken(&taken);
        if emptied {
            let session = self.clone();
            run_or_defer(Box::new(move || session.clean_thread(thread, true)));
        }
    }

    /// Puts back what `taken` recorded, each element held meanwhile so restoring one can't
    /// destroy another under us, and through its current handle. An element that can't be held
    /// is gone, with nothing to put back. On the elements' UI thread.
    fn restore_taken(&self, taken: &[(InstanceHandle, Vec<Applied>)]) {
        let held: Vec<Option<Held>> = taken.iter().map(|(h, _)| self.hold(*h)).collect();
        for ((handle, applied), held) in taken.iter().zip(&held) {
            if let Some(held) = held {
                self.restore(self.handle_of(held).unwrap_or(*handle), applied);
            }
        }
    }

    /// Sets `wanted` on the element (on its UI thread), keeping what it had before. Properties
    /// it already has from us keep their first original; ones no rule wants any more go back
    /// (an empty `wanted` puts everything back).
    fn style(&self, handle: InstanceHandle, wanted: &[(String, String)]) {
        // Held while we work: nothing done meanwhile can destroy it and hand its handle on.
        let Some(_keep) = self.hold(handle) else {
            // Its handle doesn't resolve: the object is gone without a removal.
            // SAFETY: no preconditions.
            self.forget_gone(unsafe { GetCurrentThreadId() }, &[handle]);
            return;
        };
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
            if let Some((kind, key)) = resources::resource_reference(value) {
                if self.reference(handle, slot.index, kind, key) {
                    applied.push(Applied {
                        value: value.clone(),
                        ..slot
                    });
                } else if !slot.value.is_empty() {
                    self.restore_one(handle, &slot);
                }
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
        let mut state = self.lock();
        if !state.nodes.contains_key(&handle) {
            // It left the tree while we styled it (our own calls can change the tree): it is
            // still held, so give its values back now.
            drop(state);
            self.restore(handle, &applied);
        } else if !applied.is_empty() {
            state.styled.insert(handle, applied);
        }
    }

    /// What to put back later: the element's own (local) value, or "unset". `None` when it
    /// can't be kept (then the property is left alone).
    ///
    /// A local value is taken from the property chain: the value object (held), or its text.
    /// (`GetProperty`'s handle for the same value is refused by `SetProperty` in WinUI 3.) Only
    /// when the chain has neither is `GetProperty`'s object held.
    fn original(
        &self,
        handle: InstanceHandle,
        info: &PropertyInfo,
        name: &str,
    ) -> Option<Original> {
        let held = |h: InstanceHandle| self.hold(h).map(|held| Original::Object(h, held));
        match &info.local {
            None => return Some(Original::Unset),
            Some(LocalValue::Handle(h)) => return held(*h),
            Some(LocalValue::Text(t)) if !t.is_empty() => return Some(Original::Text(t.clone())),
            Some(LocalValue::Text(_)) => {}
        }
        let mut current = 0;
        // SAFETY: a live service and a valid out pointer.
        match unsafe { self.tree.GetProperty(handle, info.index, &mut current) }.ok() {
            Ok(()) => held(current),
            Err(e) => {
                log(&format!("reading {name}: {e}; leaving it alone"));
                None
            }
        }
    }

    /// A strong reference to the object behind `handle`.
    fn hold(&self, handle: InstanceHandle) -> Option<Held> {
        let mut raw: *mut c_void = std::ptr::null_mut();
        // SAFETY: a valid out pointer; we own the returned reference.
        unsafe { self.diagnostics.GetIInspectableFromHandle(handle, &mut raw) }
            .ok()
            .ok()?;
        // SAFETY: an owned IInspectable from the call above (null is None).
        (!raw.is_null()).then(|| Held(unsafe { IInspectable::from_raw(raw) }))
    }

    /// Puts `original` back into property `index` of `handle`, or clears it.
    fn put_back(&self, handle: InstanceHandle, index: u32, kind: &str, original: &Original) {
        let clear = || {
            // SAFETY: handles from the service; a stale handle only fails.
            unsafe { self.tree.ClearProperty(handle, index) }.ok()
        };
        let set = |value: InstanceHandle| {
            // SAFETY: as above; `value` is fresh or held.
            unsafe { self.tree.SetProperty(handle, value, index) }.ok()
        };
        let result = match original {
            Original::Unset => clear(),
            Original::Text(text) => match self.value(kind, text) {
                Some(value) => set(value),
                None => clear(),
            },
            Original::Object(value, held) => {
                set(self.handle_of(held).unwrap_or(*value)).or_else(|e| {
                    log(&format!(
                        "putting back a {kind} object: {e}; clearing instead"
                    ));
                    clear()
                })
            }
        };
        if let Err(e) = result {
            log(&format!("putting a {kind} back on {handle}: {e}"));
        }
    }

    /// The current handle of a held object (a handle can change while we hold it).
    fn handle_of(&self, held: &Held) -> Option<InstanceHandle> {
        let mut handle = 0;
        // SAFETY: a live object and a valid out pointer.
        unsafe {
            self.diagnostics
                .GetHandleFromIInspectable(held.0.as_raw(), &mut handle)
        }
        .ok()
        .ok()?;
        Some(handle)
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

    /// Points the property at a theme (or static) resource, resolved from the element's place
    /// in the tree, so it follows light/dark and our resource overrides.
    fn reference(&self, handle: InstanceHandle, index: u32, kind: i32, key: &str) -> bool {
        let name = resources::wide(key);
        // SAFETY: a NUL-terminated name; handles from the service.
        let resolved = unsafe {
            self.tree
                .ResolveResource(handle, PCWSTR(name.as_ptr()), kind, index)
        };
        match resolved.ok() {
            Ok(()) => true,
            Err(e) => {
                log(&format!("resolving {key}: {e}"));
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
        self.put_back(handle, a.index, &a.kind, &a.original);
    }

    /// A value created from a rule's text as the property's type.
    ///
    /// Objects (brushes, font families) are held and cached per UI thread, shared between
    /// elements. A value type (colour, number, thickness, enum...) only lives until it is set,
    /// so it is made fresh every time: call this right before the `SetProperty`.
    fn value(&self, kind: &str, text_value: &str) -> Option<InstanceHandle> {
        // SAFETY: no preconditions.
        let thread = unsafe { GetCurrentThreadId() };
        let shared = kind.ends_with("Brush") || kind.ends_with("FontFamily");
        let key = (thread, kind.to_owned(), text_value.to_owned());
        let cached = shared
            .then(|| self.lock().values.get(&key).cloned())
            .flatten();
        if let Some((v, held)) = cached {
            return Some(self.handle_of(&held).unwrap_or(v));
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
        if shared {
            let held = self.hold(out)?;
            self.lock().values.insert(key, (out, held));
        }
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

    /// The rules changed: each UI thread is asked to bring its resource overrides and elements
    /// up to date. Threads that are gone are forgotten.
    fn restyle(self: &Arc<Self>) {
        let round = self.restyles.fetch_add(1, Ordering::SeqCst) + 1;
        let threads: Vec<(u32, UiThread)> = self
            .lock()
            .threads
            .iter()
            .map(|(&t, &ui)| (t, ui))
            .collect();
        for (thread, ui) in threads {
            let session = self.clone();
            if !post(thread, ui, move || session.restyle_thread(thread, round)) {
                self.forget_thread(thread, ui.generation);
            }
        }
    }

    /// One UI thread's part of restyle `round`, on that thread: its resources, then every
    /// element of it the rules want something on or that has something from us, as the map has
    /// them now. Values no element uses any more are let go of afterwards. A round that a newer
    /// one (queued behind it) supersedes does nothing.
    fn restyle_thread(&self, thread: u32, round: u64) {
        if self.closing.load(Ordering::SeqCst) || self.restyles.load(Ordering::SeqCst) != round {
            return;
        }
        self.apply_resources(thread);
        let (handles, known) = {
            let state = self.lock();
            let handles: Vec<InstanceHandle> = state
                .nodes
                .iter()
                .filter(|&(&h, e)| {
                    e.thread == thread
                        && (state.styled.contains_key(&h) || !state.wanted(h).is_empty())
                })
                .map(|(&h, _)| h)
                .collect();
            (handles, state.nodes.len())
        };
        log(&format!(
            "restyling {} element(s) on thread {thread} ({known} known in all)",
            handles.len()
        ));
        for handle in handles {
            if self.closing.load(Ordering::SeqCst) {
                break;
            }
            // Styling one element can add or remove others: ask the map again for each.
            let wanted = {
                let state = self.lock();
                match state.nodes.get(&handle) {
                    Some(e) if e.thread == thread => state.wanted(handle),
                    _ => continue,
                }
            };
            self.style(handle, &wanted);
        }
        self.evict_values(thread);
    }

    /// Lets go of this UI thread's rule values (brushes, fonts) that no styled element uses any
    /// more. On that thread.
    fn evict_values(&self, thread: u32) {
        let unused: Vec<_> = {
            let mut state = self.lock();
            let in_use: HashSet<(String, String)> = state
                .styled
                .iter()
                .filter(|(h, _)| state.nodes.get(h).is_some_and(|e| e.thread == thread))
                .flat_map(|(_, applied)| applied.iter().map(|a| (a.kind.clone(), a.value.clone())))
                .collect();
            state
                .values
                .extract_if(|(t, kind, value), _| {
                    *t == thread && !in_use.contains(&(kind.clone(), value.clone()))
                })
                .collect()
        };
        if !unused.is_empty() {
            log(&format!(
                "let go of {} unused value(s) on thread {thread}",
                unused.len()
            ));
        }
        drop(unused);
    }

    /// A UI thread that is gone: drop what we knew about it (if `generation` is still its
    /// record's). Its held objects are leaked, not released from the wrong thread.
    fn forget_thread(&self, thread: u32, generation: u64) {
        let mut state = self.lock();
        if state
            .threads
            .get(&thread)
            .is_some_and(|ui| ui.generation == generation)
        {
            let n = forget_locked(&mut state, thread, generation);
            log(&format!("thread {thread} is gone; forgot {n} element(s)"));
            self.cleaned.notify_all();
        }
    }

    /// Cleans up this UI thread, on it: puts back every value we set on its elements and its
    /// resource overrides, lets go of what we hold there and closes its message window. With
    /// `only_if_empty` (its last element left), only if nothing came back on it meanwhile.
    fn clean_thread(&self, thread: u32, only_if_empty: bool) {
        let (taken, ui) = {
            let mut state = self.lock();
            if only_if_empty && state.nodes.values().any(|e| e.thread == thread) {
                return;
            }
            if self.abandoned.load(Ordering::SeqCst) {
                // The tree isn't followed any more: the map may name dead objects.
                let ui = state.threads.get(&thread).copied();
                if let Some(ui) = ui {
                    forget_locked(&mut state, thread, ui.generation);
                }
                (Vec::new(), ui)
            } else {
                let handles: Vec<InstanceHandle> = state
                    .nodes
                    .iter()
                    .filter(|(_, e)| e.thread == thread)
                    .map(|(&h, _)| h)
                    .collect();
                let mut taken = Vec::new();
                for h in handles {
                    state.nodes.remove(&h);
                    if let Some(applied) = state.styled.remove(&h) {
                        taken.push((h, applied));
                    }
                }
                (taken, state.threads.get(&thread).copied())
            }
        };
        if !self.abandoned.load(Ordering::SeqCst) {
            self.restore_taken(&taken);
            self.restore_resources(thread);
            let values: Vec<_> = self
                .lock()
                .values
                .extract_if(|(t, _, _), _| *t == thread)
                .collect();
            drop(values);
            log(&format!(
                "thread {thread}: put back {} element(s){}",
                taken.len(),
                if only_if_empty {
                    " (its window closed)"
                } else {
                    ""
                }
            ));
        }
        {
            let mut state = self.lock();
            if let Some(ui) = ui
                && state
                    .threads
                    .get(&thread)
                    .is_some_and(|r| r.generation == ui.generation)
            {
                state.threads.remove(&thread);
            }
        }
        self.cleaned.notify_all();
        // Only while it is still ours: a recycled handle could name one of the app's windows.
        if let Some(ui) = ui
            && ui.alive(thread)
        {
            // SAFETY: our own message-only window, destroyed on its thread.
            let _ = unsafe { DestroyWindow(ui.hwnd()) };
        }
    }

    /// The helper is gone: stop styling, then have every UI thread clean up after whatever it
    /// is doing (a job queued behind its in-flight work), and wait for them. A thread that
    /// doesn't answer in time is abandoned and leaks what we hold there.
    fn close(self: &Arc<Self>) {
        let threads: Vec<(u32, UiThread)> = {
            let state = self.lock();
            self.closing.store(true, Ordering::SeqCst);
            state.threads.iter().map(|(&t, &ui)| (t, ui)).collect()
        };
        let (count, known) = {
            let state = self.lock();
            (state.styled.len(), state.nodes.len())
        };
        log(&format!(
            "restoring {count} element(s) on {} thread(s) ({known} known; {} removal(s) of \
             elements never seen)",
            threads.len(),
            self.unknown_removals.load(Ordering::Relaxed)
        ));
        for &(thread, ui) in &threads {
            let session = self.clone();
            if !post(thread, ui, move || session.clean_thread(thread, false)) {
                self.forget_thread(thread, ui.generation);
            }
        }
        let pending = |state: &mut State| {
            threads.iter().any(|(t, ui)| {
                state
                    .threads
                    .get(t)
                    .is_some_and(|r| r.generation == ui.generation)
            })
        };
        let state = self.lock();
        let (state, waited) = self
            .cleaned
            .wait_timeout_while(state, CLEAN_UP_TIMEOUT, pending)
            .unwrap_or_else(|e| e.into_inner());
        if waited.timed_out() {
            self.abandoned.store(true, Ordering::SeqCst);
            log(&format!(
                "{} thread(s) didn't clean up in {CLEAN_UP_TIMEOUT:?}; leaving them",
                state.threads.len()
            ));
        }
    }

    /// After detaching: whatever is still recorded belongs to threads that never cleaned up.
    /// Leak it rather than release XAML objects from this thread.
    fn forget_rest(&self) {
        let mut state = self.lock();
        let threads: Vec<(u32, u64)> = state
            .threads
            .iter()
            .map(|(&t, ui)| (t, ui.generation))
            .collect();
        for (thread, generation) in threads {
            forget_locked(&mut state, thread, generation);
        }
        if !state.styled.is_empty() {
            log(&format!(
                "{} element(s) on threads without a window; not restored",
                state.styled.len()
            ));
        }
        std::mem::forget(std::mem::take(&mut state.styled));
        std::mem::forget(std::mem::take(&mut state.values));
        std::mem::forget(std::mem::take(&mut state.resources));
        state.nodes.clear();
    }

    /// Writes the visual tree as an indented outline (`glaze-styler --dump`).
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

/// What a UI thread's record and its objects were: leak (forget) everything held there. With the
/// state locked; the caller notifies `Session::cleaned`. Returns how many elements it knew.
fn forget_locked(state: &mut State, thread: u32, generation: u64) -> usize {
    if state
        .threads
        .get(&thread)
        .is_some_and(|ui| ui.generation == generation)
    {
        state.threads.remove(&thread);
    }
    if let Some(resources) = state.resources.remove(&thread) {
        for brush in resources.brushes() {
            state.recolored.remove(&brush);
        }
        std::mem::forget(resources);
    }
    let values: Vec<_> = state
        .values
        .extract_if(|(t, _, _), _| *t == thread)
        .collect();
    std::mem::forget(values);
    let gone: Vec<InstanceHandle> = state
        .nodes
        .iter()
        .filter(|(_, e)| e.thread == thread)
        .map(|(&h, _)| h)
        .collect();
    let count = gone.len();
    for h in gone {
        state.nodes.remove(&h);
        std::mem::forget(state.styled.remove(&h));
    }
    count
}

/// How long closing waits for every UI thread to clean up.
const CLEAN_UP_TIMEOUT: Duration = Duration::from_secs(10);

/// Work for one UI thread. A job holds no XAML object or handle (it looks up what it works on
/// when it runs, on its thread), so dropping one unrun anywhere is safe.
type Job = Box<dyn FnOnce() + Send>;

thread_local! {
    /// How deep this thread is in our own work (a tree callback or a job). XAML calls can pump
    /// messages, so without this a posted job could run in the middle of a callback's styling.
    static DEPTH: Cell<u32> = const { Cell::new(0) };
    /// Jobs that arrived while this thread was in our work, run once it is done. Never dropped
    /// at thread exit (no destructor runs under the loader lock); a dead thread leaks its few.
    static DEFERRED: ManuallyDrop<RefCell<Vec<Job>>> =
        const { ManuallyDrop::new(RefCell::new(Vec::new())) };
}

/// Runs `work` as our own work on this thread; jobs that arrive meanwhile wait until the
/// outermost such work is done, then run in order.
fn guarded<R>(work: impl FnOnce() -> R) -> R {
    DEPTH.set(DEPTH.get() + 1);
    let result = work();
    DEPTH.set(DEPTH.get() - 1);
    if DEPTH.get() == 0 {
        while let Some(job) = DEFERRED.with(|d| {
            let mut d = d.borrow_mut();
            (!d.is_empty()).then(|| d.remove(0))
        }) {
            guarded(job);
        }
    }
    result
}

/// Runs `job` now, or after the work this thread is in the middle of.
fn run_or_defer(job: Job) {
    if DEPTH.get() > 0 {
        DEFERRED.with(|d| d.borrow_mut().push(job));
    } else {
        guarded(job);
    }
}

/// Runs `job` on `thread`: here if that is us, else posted to its message window if that is
/// still the one `ui` names. Returns `false` (the job dropped unrun) when the thread is gone.
fn post(thread: u32, ui: UiThread, job: impl FnOnce() + Send + 'static) -> bool {
    // SAFETY: no preconditions.
    if thread == unsafe { GetCurrentThreadId() } {
        run_or_defer(Box::new(job));
        return true;
    }
    if let Some(why) = ui.dead(thread) {
        log(&format!("thread {thread}: its window is {why}"));
        return false;
    }
    let raw = Box::into_raw(Box::new(Box::new(job) as Job));
    // SAFETY: the window runs the job on its thread and frees it (`window_proc`).
    let posted = unsafe {
        PostMessageW(
            Some(ui.hwnd()),
            WM_JOB,
            WPARAM(ui.generation as usize),
            LPARAM(raw as isize),
        )
    };
    if let Err(e) = posted {
        log(&format!("thread {thread}: posting failed: {e}"));
        // SAFETY: not posted, so the box is still ours.
        drop(unsafe { Box::from_raw(raw) });
        return false;
    }
    true
}

const WM_JOB: u32 = WM_APP + 0x51;

/// Our message windows' class, registered once (0 if that failed).
fn class_atom() -> Option<u16> {
    static CLASS: OnceLock<u16> = OnceLock::new();
    let atom = *CLASS.get_or_init(|| {
        let Some(module) = own_module() else {
            return 0;
        };
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: HINSTANCE(module.0),
            lpszClassName: w!("glaze-app-styler"),
            ..Default::default()
        };
        // SAFETY: a valid class description with a 'static name.
        unsafe { RegisterClassW(&class) }
    });
    (atom != 0).then_some(atom)
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
        let job = unsafe { Box::from_raw(lparam.0 as *mut Job) };
        // Only a job for this window's lifetime runs (see `UiThread`).
        // SAFETY: our own window.
        if wparam.0 as isize == unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } {
            run_or_defer(*job);
        }
        return LRESULT(0);
    }
    // SAFETY: default handling for everything else.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// The strings of a `VisualElement`, which XAML gives to the callback to free (WinUI's
/// `LiveVisualTree.cpp`: "the contract is that we are giving ownership of these strings to the
/// callback"; system XAML keeps the same contract for Visual Studio). Freed on drop.
struct ElementStrings([RawBstr; 4]);

impl Drop for ElementStrings {
    fn drop(&mut self) {
        for raw in self.0 {
            // SAFETY: ours to free, once (null is a no-op).
            unsafe { free(raw) };
        }
    }
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
        let _strings = ElementStrings([
            element.Type,
            element.Name,
            element.SrcInfo.FileName,
            element.SrcInfo.Hash,
        ]);
        let session = &self.session;
        guarded(|| {
            if mutation == MUTATION_ADD {
                // Nothing new is styled once closing (checked again under the lock).
                if session.closing.load(Ordering::SeqCst) {
                    return;
                }
                // SAFETY: the strings live until `_strings` drops.
                let (kind, name) = unsafe { (text(element.Type), text(element.Name)) };
                session.on_add(element.Handle, relation.Parent, &kind, name);
            } else {
                // Removals are followed while closing too: they keep the map true until every
                // thread has cleaned up.
                session.on_remove(element.Handle);
            }
        });
        S_OK
    }
}

impl IVisualTreeServiceCallback2_Impl for Callback_Impl {
    unsafe fn OnElementStateChanged(&self, _: InstanceHandle, _: i32, _: PCWSTR) -> HRESULT {
        S_OK
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW};

    fn pump() {
        let mut msg = MSG::default();
        // SAFETY: a plain message pump on the test thread.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&msg);
            }
        }
    }

    #[test]
    fn jobs_that_arrive_during_our_work_run_after_it_in_order() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let push = |what: &'static str| {
            let seen = seen.clone();
            Box::new(move || seen.lock().unwrap().push(what)) as Job
        };
        guarded(|| {
            run_or_defer(push("first job"));
            guarded(|| run_or_defer(push("second job")));
            seen.lock().unwrap().push("work");
        });
        assert_eq!(*seen.lock().unwrap(), ["work", "first job", "second job"]);
    }

    #[test]
    fn a_job_reaches_only_its_own_live_window() {
        // SAFETY: no preconditions.
        let thread = unsafe { GetCurrentThreadId() };
        let ui = UiThread::create().expect("a message window");
        assert!(ui.alive(thread));
        assert!(!ui.alive(thread + 1), "another thread's id");
        let older = UiThread {
            generation: ui.generation + 1,
            ..ui
        };
        assert!(!older.alive(thread), "another generation");

        let ran = Arc::new(AtomicBool::new(false));
        let job = |ran: &Arc<AtomicBool>| {
            let ran = ran.clone();
            move || ran.store(true, Ordering::SeqCst)
        };
        // A stale generation is refused before posting...
        let posted = std::thread::scope(|s| s.spawn(|| post(thread, older, job(&ran))).join());
        assert!(!posted.unwrap());
        // ...and one that reaches the window anyway is dropped unrun.
        let raw = Box::into_raw(Box::new(Box::new(job(&ran)) as Job));
        // SAFETY: our window; `window_proc` takes the box.
        unsafe {
            PostMessageW(
                Some(ui.hwnd()),
                WM_JOB,
                WPARAM(older.generation as usize),
                LPARAM(raw as isize),
            )
        }
        .unwrap();
        pump();
        assert!(!ran.load(Ordering::SeqCst));

        let posted = std::thread::scope(|s| s.spawn(|| post(thread, ui, job(&ran))).join());
        assert!(posted.unwrap());
        pump();
        assert!(ran.load(Ordering::SeqCst));

        // SAFETY: our own window.
        unsafe { DestroyWindow(ui.hwnd()) }.unwrap();
        assert!(!ui.alive(thread), "destroyed");
    }
}
