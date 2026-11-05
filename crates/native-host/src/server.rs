use crate::{
    protocol::{Frame, Kind, MAX_FRAME},
    windows,
};
use std::{
    collections::HashMap,
    ffi::{CStr, CString, c_char, c_void},
    io,
    path::{Path, PathBuf},
    ptr,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

type Init = unsafe extern "C" fn(*const Host) -> i32;
type Call = unsafe extern "C" fn(*const c_char, *const u8, usize, *mut *mut u8, *mut usize) -> i32;
type Free = unsafe extern "C" fn(*mut u8, usize);
#[repr(C)]
struct Host {
    abi_version: u32,
    context: *mut c_void,
    emit: unsafe extern "C" fn(*mut c_void, *const c_char, *const u8, usize),
}
struct Events {
    writer: Arc<Mutex<windows::Pipe>>,
    plugin: String,
    module: String,
}
struct Module {
    call: Call,
    free: Free,
    path: PathBuf,
    _events: Box<Events>,
    _host: Box<Host>,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

/// The host rechecks canonical containment independently of the client.
pub(crate) fn module_path(folder: &Path, name: &str) -> io::Result<PathBuf> {
    if name.is_empty()
        || name.contains(['/', '\\', ':', '\0'])
        || !name.to_ascii_lowercase().ends_with(".dll")
    {
        return Err(invalid(
            "native module must be a DLL basename inside the plugin folder",
        ));
    }
    let folder = folder.canonicalize()?;
    let path = folder.join(name).canonicalize()?;
    if path.parent() != Some(folder.as_path()) || !path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "native DLL escapes the plugin folder",
        ));
    }
    Ok(path)
}

unsafe extern "C" fn emit(context: *mut c_void, name: *const c_char, bytes: *const u8, len: usize) {
    if context.is_null() || name.is_null() || len > MAX_FRAME - 1024 || (len > 0 && bytes.is_null())
    {
        return;
    }
    // SAFETY: ABI requires a valid context, NUL-terminated name and borrowed bytes.
    let (events, name, bytes) = unsafe {
        (
            &*context.cast::<Events>(),
            CStr::from_ptr(name),
            if len == 0 {
                &[]
            } else {
                std::slice::from_raw_parts(bytes, len)
            },
        )
    };
    let Ok(name) = name.to_str() else { return };
    let frame = Frame {
        kind: Kind::Event,
        request: 0,
        plugin: events.plugin.clone(),
        module: events.module.clone(),
        name: name.to_owned(),
        bytes: bytes.to_vec(),
    };
    if let Ok(mut writer) = events.writer.lock() {
        let _ = frame.write(&mut *writer);
    }
}

/// Includes DLL load, init, call and free. A stuck native thread cannot outlive this deadline.
fn watchdog() -> mpsc::Sender<Option<Instant>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut armed: Option<Instant> = None;
        loop {
            let next = if let Some(at) = armed {
                rx.recv_timeout(at.saturating_duration_since(Instant::now()))
            } else {
                rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)
            };
            match next {
                Ok(at) => armed = at,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // SAFETY: this is the disposable native host, never the plugin runtime.
                    unsafe { windows::ExitProcess(124) }
                }
            }
        }
    });
    tx
}

fn load(
    frame: &Frame,
    writer: Arc<Mutex<windows::Pipe>>,
    modules: &mut HashMap<(String, String), Module>,
) -> io::Result<Vec<u8>> {
    let folder =
        std::str::from_utf8(&frame.bytes[4..]).map_err(|_| invalid("folder is not UTF-8"))?;
    let path = module_path(Path::new(folder), &frame.module)?;
    let key = (frame.plugin.clone(), frame.module.clone());
    if let Some(module) = modules.get(&key) {
        return if module.path == path {
            Ok(Vec::new())
        } else {
            Err(invalid("module already loaded from a different folder"))
        };
    }
    if modules.values().any(|module| module.path == path) {
        return Err(invalid("a DLL path cannot serve two plugin identities"));
    }
    let library = windows::load(&path)?;
    // SAFETY: the three mandatory symbols have the signatures specified in our C header.
    let (init, call, free) = unsafe {
        (
            std::mem::transmute::<*mut c_void, Init>(windows::symbol(library, c"mp_native_init")?),
            std::mem::transmute::<*mut c_void, Call>(windows::symbol(library, c"mp_native_call")?),
            std::mem::transmute::<*mut c_void, Free>(windows::symbol(library, c"mp_native_free")?),
        )
    };
    let mut events = Box::new(Events {
        writer,
        plugin: frame.plugin.clone(),
        module: frame.module.clone(),
    });
    let host = Box::new(Host {
        abi_version: 1,
        context: (&mut *events as *mut Events).cast(),
        emit,
    });
    // Retain tables even after failed init: native code may already have started threads.
    let module = Module {
        call,
        free,
        path,
        _events: events,
        _host: host,
    };
    // SAFETY: host table has stable process-long storage; initialization is serialized.
    let result = unsafe { init(&*module._host) };
    if result != 0 {
        std::mem::forget(module);
        return Err(invalid(format!("mp_native_init returned {result}")));
    }
    modules.insert(key, module);
    Ok(Vec::new())
}

fn call(frame: &Frame, modules: &HashMap<(String, String), Module>) -> io::Result<Vec<u8>> {
    let module = modules
        .get(&(frame.plugin.clone(), frame.module.clone()))
        .ok_or_else(|| invalid("native module is not loaded"))?;
    let method = CString::new(frame.name.as_bytes()).map_err(|_| invalid("method contains NUL"))?;
    let input = &frame.bytes[4..];
    let mut output = ptr::null_mut();
    let mut len = 0;
    // SAFETY: validated method, borrowed input and writable out parameters per C ABI.
    let result = unsafe {
        (module.call)(
            method.as_ptr(),
            input.as_ptr(),
            input.len(),
            &mut output,
            &mut len,
        )
    };
    if result != 0 {
        return Err(invalid(format!("mp_native_call returned {result}")));
    }
    let valid = len <= MAX_FRAME - 1024 && (len == 0 || !output.is_null());
    let bytes = if valid && len > 0 {
        // SAFETY: a successful ABI call guarantees `output` holds `len` initialized bytes.
        unsafe { std::slice::from_raw_parts(output, len).to_vec() }
    } else {
        Vec::new()
    };
    // SAFETY: returned allocation is freed by the same module, on its dispatch thread.
    unsafe { (module.free)(output, len) };
    if !valid {
        return Err(invalid(
            "native module returned an invalid or oversized buffer",
        ));
    }
    Ok(bytes)
}

pub(crate) fn run(pipe: &str) -> io::Result<()> {
    windows::restrict_loader()?;
    let mut reader = windows::open_pipe(pipe)?;
    let writer = Arc::new(Mutex::new(reader.try_clone()?));
    let watchdog = watchdog();
    let mut modules = std::mem::ManuallyDrop::new(HashMap::new());
    loop {
        let frame = Frame::read(&mut reader)?;
        if frame.kind == Kind::Shutdown {
            break;
        }
        if !matches!(frame.kind, Kind::Load | Kind::Call) || frame.bytes.len() < 4 {
            return Err(invalid("invalid native request"));
        }
        let timeout = u32::from_le_bytes(frame.bytes[..4].try_into().unwrap()).clamp(1, 60_000);
        let _ = watchdog.send(Some(Instant::now() + Duration::from_millis(timeout.into())));
        let result = if frame.kind == Kind::Load {
            load(&frame, writer.clone(), &mut modules)
        } else {
            call(&frame, &modules)
        };
        let _ = watchdog.send(None);
        let response = Frame {
            kind: if result.is_ok() {
                Kind::Ok
            } else {
                Kind::Error
            },
            bytes: result.unwrap_or_else(|error| error.to_string().into_bytes()),
            ..frame
        };
        response.write(&mut *writer.lock().map_err(|_| invalid("pipe writer poisoned"))?)?;
    }
    // Callback state must outlive arbitrary native threads until process termination.
    Ok(())
}
