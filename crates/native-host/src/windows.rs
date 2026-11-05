//! Small Win32 boundary: pipes, restricted DLL search and kill-on-close jobs.
use std::{
    ffi::{c_char, c_void},
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    os::windows::{
        ffi::OsStrExt,
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr,
    time::Duration,
};

pub type Handle = *mut c_void;
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateNamedPipeW(
        name: *const u16,
        open: u32,
        mode: u32,
        instances: u32,
        out: u32,
        input: u32,
        timeout: u32,
        security: *const c_void,
    ) -> Handle;
    fn ConnectNamedPipe(pipe: Handle, overlapped: *mut c_void) -> i32;
    fn CreateEventW(security: *const c_void, manual: i32, initial: i32, name: *const u16)
    -> Handle;
    fn ResetEvent(event: Handle) -> i32;
    fn ReadFile(
        file: Handle,
        buffer: *mut c_void,
        len: u32,
        read: *mut u32,
        overlapped: *mut Overlapped,
    ) -> i32;
    fn WriteFile(
        file: Handle,
        buffer: *const c_void,
        len: u32,
        written: *mut u32,
        overlapped: *mut Overlapped,
    ) -> i32;
    fn GetOverlappedResult(
        file: Handle,
        overlapped: *mut Overlapped,
        count: *mut u32,
        wait: i32,
    ) -> i32;
    fn CancelIoEx(file: Handle, overlapped: *mut Overlapped) -> i32;
    fn WaitForSingleObject(handle: Handle, millis: u32) -> u32;
    fn GetNamedPipeClientProcessId(pipe: Handle, pid: *mut u32) -> i32;
    fn LoadLibraryExW(path: *const u16, file: Handle, flags: u32) -> Handle;
    fn GetProcAddress(module: Handle, name: *const c_char) -> *mut c_void;
    fn SetDefaultDllDirectories(flags: u32) -> i32;
    fn SetErrorMode(mode: u32) -> u32;
    fn GetCurrentProcess() -> Handle;
    fn TerminateProcess(process: Handle, code: u32) -> i32;
    fn CreateJobObjectW(security: *const c_void, name: *const u16) -> Handle;
    fn SetInformationJobObject(job: Handle, class: i32, info: *const c_void, size: u32) -> i32;
    fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
}

fn wide(path: &std::ffi::OsStr) -> Vec<u16> {
    path.encode_wide().chain(Some(0)).collect()
}
fn ok(value: i32) -> io::Result<()> {
    if value == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Skip vendor DLL detach handlers, which can deadlock while a call holds locks.
pub fn terminate(code: u32) -> ! {
    // SAFETY: terminate only this disposable host, using its own process handle.
    unsafe {
        TerminateProcess(GetCurrentProcess(), code);
    }
    std::process::abort()
}

#[repr(C)]
struct Overlapped {
    internal: usize,
    internal_high: usize,
    offset_low: u32,
    offset_high: u32,
    event: Handle,
}

/// Each clone has its own OVERLAPPED event, allowing a read and write concurrently.
pub struct Pipe {
    file: File,
    event: OwnedHandle,
}
impl Pipe {
    fn new(file: File) -> io::Result<Self> {
        // SAFETY: anonymous manual-reset event, initially unsignaled.
        let event = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
        if event.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: newly created event ownership transfers to RAII.
        let event = unsafe { OwnedHandle::from_raw_handle(event) };
        Ok(Self { file, event })
    }
    pub fn try_clone(&self) -> io::Result<Self> {
        Self::new(self.file.try_clone()?)
    }
    fn overlapped(&self) -> Overlapped {
        Overlapped {
            internal: 0,
            internal_high: 0,
            offset_low: 0,
            offset_high: 0,
            event: self.event.as_raw_handle(),
        }
    }
    fn transfer(&mut self, buffer: *mut c_void, len: usize, write: bool) -> io::Result<usize> {
        let mut overlapped = self.overlapped();
        let mut count = 0;
        // SAFETY: the caller borrows the buffer until this synchronous wrapper
        // finishes; OVERLAPPED and its event remain valid through completion.
        unsafe {
            ok(ResetEvent(self.event.as_raw_handle()))?;
            let success = if write {
                WriteFile(
                    self.file.as_raw_handle(),
                    buffer,
                    len.min(u32::MAX as usize) as u32,
                    &mut count,
                    &mut overlapped,
                )
            } else {
                ReadFile(
                    self.file.as_raw_handle(),
                    buffer,
                    len.min(u32::MAX as usize) as u32,
                    &mut count,
                    &mut overlapped,
                )
            };
            if success == 0 && io::Error::last_os_error().raw_os_error() != Some(997) {
                let error = io::Error::last_os_error();
                if !write && error.raw_os_error() == Some(109) {
                    return Ok(0);
                }
                return Err(error);
            }
            ok(GetOverlappedResult(
                self.file.as_raw_handle(),
                &mut overlapped,
                &mut count,
                1,
            ))?;
        }
        Ok(count as usize)
    }
}
impl Read for Pipe {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.transfer(bytes.as_mut_ptr().cast(), bytes.len(), false)
    }
}
impl Write for Pipe {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.transfer(bytes.as_ptr().cast_mut().cast(), bytes.len(), true)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub fn create_pipe(name: &str) -> io::Result<Pipe> {
    let name = wide(name.as_ref());
    // SAFETY: NUL-terminated name, byte pipe, local clients only, a single server.
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            3 | 0x00080000 | 0x40000000,
            8,
            1,
            65536,
            65536,
            0,
            ptr::null(),
        )
    };
    if handle as isize == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: ownership of a newly created, valid handle passes to File.
    Pipe::new(unsafe { File::from_raw_handle(handle) })
}

pub fn connect_server(pipe: &Pipe, expected_pid: u32, timeout: Duration) -> io::Result<()> {
    let mut overlapped = pipe.overlapped();
    // SAFETY: valid pipe and stable OVERLAPPED until completion or cancellation.
    let result = unsafe {
        ConnectNamedPipe(
            pipe.file.as_raw_handle(),
            (&mut overlapped as *mut Overlapped).cast(),
        )
    };
    if result == 0 {
        match io::Error::last_os_error().raw_os_error() {
            Some(535) => (),
            Some(997) => {
                // SAFETY: wait for our event only; timeout cancels and drains I/O
                // before releasing the stack storage used by OVERLAPPED.
                unsafe {
                    if WaitForSingleObject(
                        pipe.event.as_raw_handle(),
                        timeout.as_millis().clamp(1, 60_000) as u32,
                    ) != 0
                    {
                        CancelIoEx(pipe.file.as_raw_handle(), &mut overlapped);
                        let mut count = 0;
                        GetOverlappedResult(
                            pipe.file.as_raw_handle(),
                            &mut overlapped,
                            &mut count,
                            1,
                        );
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "native host pipe connection timed out",
                        ));
                    }
                    let mut count = 0;
                    ok(GetOverlappedResult(
                        pipe.file.as_raw_handle(),
                        &mut overlapped,
                        &mut count,
                        0,
                    ))?;
                }
            }
            _ => return Err(io::Error::last_os_error()),
        }
    }
    let mut pid = 0;
    // SAFETY: valid pipe and writable PID output.
    unsafe {
        ok(GetNamedPipeClientProcessId(
            pipe.file.as_raw_handle(),
            &mut pid,
        ))?;
    }
    if pid != expected_pid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unexpected native host pipe client",
        ));
    }
    Ok(())
}

pub fn open_pipe(name: &str) -> io::Result<Pipe> {
    Pipe::new(
        OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(0x40000000)
            .open(name)?,
    )
}

pub fn restrict_loader() -> io::Result<()> {
    // SAFETY: process-local configuration; exclude CWD, PATH and crash dialogs.
    unsafe {
        SetErrorMode(0x0001 | 0x0002 | 0x8000);
        ok(SetDefaultDllDirectories(0x800))
    }
}

pub fn load(path: &Path) -> io::Result<Handle> {
    let path = wide(path.as_os_str());
    // SAFETY: absolute canonical path; search only its directory and System32.
    let module = unsafe { LoadLibraryExW(path.as_ptr(), ptr::null_mut(), 0x100 | 0x800) };
    if module.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(module)
    }
}

pub fn symbol(module: Handle, name: &'static std::ffi::CStr) -> io::Result<*mut c_void> {
    // SAFETY: module was loaded for process lifetime; static NUL-terminated name.
    let address = unsafe { GetProcAddress(module, name.as_ptr()) };
    if address.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(address)
    }
}

#[repr(C)]
#[derive(Default)]
struct BasicLimits {
    per_process: i64,
    per_job: i64,
    flags: u32,
    minimum: usize,
    maximum: usize,
    active: u32,
    affinity: usize,
    priority: u32,
    scheduling: u32,
}
#[repr(C)]
#[derive(Default)]
struct ExtendedLimits {
    basic: BasicLimits,
    io: [u64; 6],
    process_memory: usize,
    job_memory: usize,
    peak_process: usize,
    peak_job: usize,
}

pub fn job(child: &std::process::Child) -> io::Result<OwnedHandle> {
    // SAFETY: anonymous, noninheritable job with default security.
    let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: transfers the new owned handle to RAII cleanup.
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    let mut limits = ExtendedLimits::default();
    limits.basic.flags = 0x2000; // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
    // SAFETY: correctly laid out extended limits, valid job and child handles.
    unsafe {
        ok(SetInformationJobObject(
            handle.as_raw_handle(),
            9,
            (&limits as *const ExtendedLimits).cast(),
            size_of::<ExtendedLimits>() as u32,
        ))?;
        ok(AssignProcessToJobObject(
            handle.as_raw_handle(),
            child.as_raw_handle(),
        ))?;
    }
    Ok(handle)
}
