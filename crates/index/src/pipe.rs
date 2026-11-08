//! Bounded JSON messages on a current-user-only, local named pipe. The shell client uses
//! overlapped I/O and cancels on its deadline; a missing service is an immediate fallback.

use std::sync::{Arc, RwLock};
use std::time::Instant;

use anyhow::{Result, bail, ensure};
use windows::Win32::Foundation::*;
use windows::Win32::Security::Authorization::*;
use windows::Win32::Security::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::IO::*;
use windows::Win32::System::Pipes::*;
use windows::Win32::System::Threading::*;
use windows::core::{PCWSTR, PWSTR};

use crate::{PIPE, Request, Response, query, scan::wide, service::State};

const INPUT: usize = 8192;
const OUTPUT: usize = 1024 * 1024;

struct Owned(HANDLE);
impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: owns a successful kernel handle, never copied into another owner.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn overlapped() -> Result<(Owned, OVERLAPPED)> {
    // SAFETY: unnamed manual-reset event, initially unsignaled.
    let event = Owned(unsafe { CreateEventW(None, true, false, PCWSTR::null())? });
    let operation = OVERLAPPED {
        hEvent: event.0,
        ..OVERLAPPED::default()
    };
    Ok((event, operation))
}

fn complete(
    handle: HANDLE,
    operation: &mut OVERLAPPED,
    result: windows::core::Result<()>,
    timeout: u32,
) -> Result<u32> {
    if let Err(error) = result {
        if error.code() != ERROR_IO_PENDING.to_hresult() {
            return Err(error.into());
        }
        // SAFETY: event and OVERLAPPED remain live through completion or cancellation.
        if unsafe { WaitForSingleObject(operation.hEvent, timeout) } != WAIT_OBJECT_0 {
            // Cancellation itself is asynchronous: drain it before freeing the buffers.
            // SAFETY: this live handle owns the still-pending operation and its buffers.
            unsafe {
                let _ = CancelIoEx(handle, Some(operation));
                let mut ignored = 0;
                let _ = GetOverlappedResult(handle, operation, &mut ignored, true);
            }
            bail!("index pipe timed out");
        }
    }
    let mut count = 0;
    // SAFETY: a completed operation and writable byte-count output.
    unsafe {
        GetOverlappedResult(handle, operation, &mut count, false)?;
    }
    Ok(count)
}

pub fn request(request: &Request, timeout_ms: u32) -> Result<Response> {
    let message = serde_json::to_vec(request)?;
    ensure!(message.len() <= INPUT, "index request too large");
    let pipe = wide(PIPE);
    // SAFETY: opens only the local named pipe; no waiting if the service isn't available.
    let handle = Owned(unsafe {
        CreateFileW(
            PCWSTR(pipe.as_ptr()),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_MODE(0),
            None,
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            None,
        )?
    });
    // SAFETY: client handle supports message reads; null optional outputs.
    unsafe {
        SetNamedPipeHandleState(handle.0, Some(&PIPE_READMODE_MESSAGE), None, None)?;
    }
    let (_event, mut operation) = overlapped()?;
    let mut output = vec![0; OUTPUT];
    let mut count = 0;
    // SAFETY: input/output buffers, operation and its event outlive completion.
    let result = unsafe {
        TransactNamedPipe(
            handle.0,
            Some(message.as_ptr().cast()),
            message.len() as u32,
            Some(output.as_mut_ptr().cast()),
            output.len() as u32,
            &mut count,
            Some(&mut operation),
        )
    };
    let count = complete(handle.0, &mut operation, result, timeout_ms)? as usize;
    Ok(serde_json::from_slice(&output[..count])?)
}

/// Explicit user SID, rather than the process's default DACL or all authenticated users.
fn descriptor() -> Result<PSECURITY_DESCRIPTOR> {
    let mut token = HANDLE::default();
    // SAFETY: query-only token, owned immediately after successful open.
    unsafe {
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
    }
    let token = Owned(token);
    let mut needed = 0;
    // SAFETY: first call obtains required storage size.
    let _ = unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut needed) };
    let mut storage = vec![0u64; (needed as usize).div_ceil(8)];
    let mut sid = PWSTR::null();
    // SAFETY: aligned token output of requested size, followed by valid SID conversion.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(storage.as_mut_ptr().cast()),
            needed,
            &mut needed,
        )?;
        let user = &*storage.as_ptr().cast::<TOKEN_USER>();
        ConvertSidToStringSidW(user.User.Sid, &mut sid)?;
    }
    // SAFETY: conversion returned a terminated allocated SID string.
    let text = unsafe { sid.to_string() };
    // SAFETY: SID string was allocated by the LocalAlloc family.
    let _ = unsafe { LocalFree(Some(HLOCAL(sid.0.cast()))) };
    let sddl = wide(&format!("D:P(A;;GA;;;{})", text?));
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: terminated SDDL, writable descriptor output, freed by caller after pipe creation.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )?;
    }
    Ok(descriptor)
}

fn create(first: bool) -> Result<Owned> {
    let security = descriptor()?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: security.0,
        bInheritHandle: false.into(),
    };
    let name = wide(PIPE);
    let first_flag = if first {
        FILE_FLAG_FIRST_PIPE_INSTANCE
    } else {
        FILE_FLAGS_AND_ATTRIBUTES(0)
    };
    // SAFETY: explicit noninheritable DACL remains alive through synchronous creation.
    let handle = unsafe {
        CreateNamedPipeW(
            PCWSTR(name.as_ptr()),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | first_flag,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            4,
            64 * 1024,
            INPUT as u32,
            50,
            Some(&attributes),
        )
    };
    // SAFETY: SDDL conversion uses LocalAlloc.
    let _ = unsafe { LocalFree(Some(HLOCAL(security.0))) };
    ensure!(
        !handle.is_invalid(),
        "creating index pipe: {}",
        windows::core::Error::from_thread()
    );
    Ok(Owned(handle))
}

fn serve_one(handle: HANDLE, state: &RwLock<State>) -> Result<()> {
    let (_event, mut operation) = overlapped()?;
    let mut input = [0; INPUT];
    // SAFETY: valid buffer and operation, completed before either is dropped.
    let result = unsafe { ReadFile(handle, Some(&mut input), None, Some(&mut operation)) };
    let bytes = complete(handle, &mut operation, result, 2000)? as usize;
    let request: Request = serde_json::from_slice(&input[..bytes])?;
    let started = Instant::now();
    let mut response = {
        let state = state.read().expect("index state");
        let queried = !matches!(&request, Request::Status);
        let hits = match request {
            Request::Search { query: text, limit } => query::search(&state.store, &text, limit),
            Request::Sizes { path, limit } => state.store.sizes(&path, limit),
            Request::Status => Vec::new(),
            Request::Rescan => {
                state.rescan()?;
                Vec::new()
            }
        };
        if queried {
            state.queried();
        }
        Response {
            hits,
            entries: state.store.len().saturating_sub(1),
            scanning: state.scanning,
            ..Response::default()
        }
    };
    response.elapsed_us = started.elapsed().as_micros() as u64;
    let output = serde_json::to_vec(&response)?;
    ensure!(output.len() <= OUTPUT, "index response too large");
    let (_event, mut operation) = overlapped()?;
    // SAFETY: output and operation remain live through completion/cancellation.
    let result = unsafe { WriteFile(handle, Some(&output), None, Some(&mut operation)) };
    complete(handle, &mut operation, result, 2000)?;
    // DisconnectNamedPipe can discard unread buffered output. Wait for the transaction client
    // to close, bounded like every other I/O, before reusing this listener.
    let (_event, mut operation) = overlapped()?;
    let mut eof = [0; 1];
    // SAFETY: EOF probe buffer and operation stay alive until completion/cancellation.
    let result = unsafe { ReadFile(handle, Some(&mut eof), None, Some(&mut operation)) };
    let _ = complete(handle, &mut operation, result, 2000);
    Ok(())
}

/// Isolate an individual transaction when the binary is built with unwinding. A request
/// panic releases its read guard and the listener still disconnects/reuses its pipe.
fn transaction(serve: impl FnOnce() -> Result<()>) -> Result<()> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(serve))
        .map_err(|_| anyhow::anyhow!("index pipe transaction panicked"))?
}

pub fn start(state: Arc<RwLock<State>>) -> Result<()> {
    // Create all listeners before starting workers. The first-instance check rejects squatting.
    let listeners = [
        create(true)?,
        create(false)?,
        create(false)?,
        create(false)?,
    ];
    for (number, listener) in listeners.into_iter().enumerate() {
        let raw = listener.0.0 as usize;
        std::mem::forget(listener);
        let state = state.clone();
        std::thread::Builder::new()
            .name(format!("index-pipe-{number}"))
            .spawn(move || {
                let handle = Owned(HANDLE(raw as *mut _));
                loop {
                    let Ok((_event, mut operation)) = overlapped() else {
                        break;
                    };
                    // SAFETY: listener and operation stay live through the blocking wait.
                    let connect = unsafe { ConnectNamedPipe(handle.0, Some(&mut operation)) };
                    let connected = connect
                        .as_ref()
                        .err()
                        .is_some_and(|e| e.code() == ERROR_PIPE_CONNECTED.to_hresult());
                    if (connected || complete(handle.0, &mut operation, connect, INFINITE).is_ok())
                        && let Err(error) = transaction(|| serve_one(handle.0, &state))
                    {
                        eprintln!("index pipe client: {error:#}");
                    }
                    // SAFETY: disconnect only this worker's completed client transaction.
                    let _ = unsafe { DisconnectNamedPipe(handle.0) };
                }
            })?;
    }
    Ok(())
}

// This workspace's Windows Cranelift debug backend cannot unwind. Exercise containment
// under the same LLVM backend as deployment: cargo test --release -p glaze-index.
#[cfg(all(test, not(debug_assertions)))]
mod tests {
    use super::*;

    #[test]
    fn panicked_client_releases_read_guard_and_listener_serves_next_transaction() {
        let store = RwLock::new(0);
        assert!(
            transaction(|| {
                let _read = store.read().unwrap();
                panic!("injected client panic");
            })
            .is_err()
        );
        transaction(|| {
            *store.write().unwrap() += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(*store.read().unwrap(), 1);
    }
}
