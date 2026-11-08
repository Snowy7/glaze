//! Minimal ABI-1 module: no dependency on the host or plugin runtime.
use std::{
    ffi::{CStr, c_char, c_void},
    ptr,
    sync::OnceLock,
    time::Duration,
};

#[repr(C)]
pub struct Host {
    abi_version: u32,
    context: *mut c_void,
    emit: unsafe extern "C" fn(*mut c_void, *const c_char, *const u8, usize),
}
struct Callback {
    context: usize,
    emit: unsafe extern "C" fn(*mut c_void, *const c_char, *const u8, usize),
}
static HOST: OnceLock<Callback> = OnceLock::new();

/// # Safety
/// `host` points to an ABI-1 host table valid for the life of the process.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mp_native_init(host: *const Host) -> i32 {
    if host.is_null() {
        return -1;
    }
    // SAFETY: the host guarantees this table is valid and process-long.
    let host = unsafe { &*host };
    if host.abi_version != 1 {
        return -1;
    }
    if HOST
        .set(Callback {
            context: host.context as usize,
            emit: host.emit,
        })
        .is_err()
    {
        return -1;
    }
    0
}

/// # Safety
/// All pointers follow `glaze_native.h`; output pointers are writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mp_native_call(
    method: *const c_char,
    input: *const u8,
    len: usize,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    // SAFETY: output parameters are valid per ABI; failures leave them empty.
    unsafe {
        *out = ptr::null_mut();
        *out_len = 0;
    }
    if method.is_null() || (input.is_null() && len > 0) {
        return -1;
    }
    // SAFETY: borrowed C method and input supplied by the host.
    let (method, input) = unsafe {
        (
            CStr::from_ptr(method).to_bytes(),
            if len == 0 {
                &[]
            } else {
                std::slice::from_raw_parts(input, len)
            },
        )
    };
    match method {
        b"crash" => std::process::abort(),
        b"hang" => loop {
            std::thread::sleep(Duration::from_secs(60));
        },
        b"background-crash" => {
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_millis(50));
                std::process::abort();
            });
        }
        b"background" => {
            let bytes = input.to_vec();
            std::thread::spawn(move || {
                if let Some(host) = HOST.get() {
                    // SAFETY: callback table and context remain valid until process exit.
                    unsafe {
                        (host.emit)(
                            host.context as *mut c_void,
                            c"background".as_ptr(),
                            bytes.as_ptr(),
                            bytes.len(),
                        )
                    };
                }
            });
        }
        b"echo" => (),
        _ => return -2,
    }
    let bytes = input.to_vec().into_boxed_slice();
    // SAFETY: ownership is transferred until mp_native_free; writable outputs.
    unsafe {
        *out_len = bytes.len();
        *out = Box::into_raw(bytes).cast::<u8>();
    }
    0
}

/// # Safety
/// Pointer and length must be the allocation returned by a successful call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mp_native_free(bytes: *mut u8, len: usize) {
    if !bytes.is_null() {
        // SAFETY: restores exactly the Box<[u8]> transferred by mp_native_call.
        unsafe { drop(Box::from_raw(ptr::slice_from_raw_parts_mut(bytes, len))) };
    }
}
