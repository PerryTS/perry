//! Synchronous Preview 1 syscalls over a borrowed guest memory span.
//!
//! A context belongs to a collecting runtime native cell, never the shared
//! wasmi Store. Calls cannot invoke JS or collect, and borrow fresh memory on
//! every invocation, including after memory.grow. Dropping the cell releases
//! its preopens and descriptors without waiting for the interpreter to die.
use std::ffi::{c_char, c_void, CString};
use std::task::{Context, Poll, Waker};
use wasi_common::{sync::WasiCtxBuilder, WasiCtx};

#[derive(serde::Deserialize)]
struct Config {
    args: Vec<String>,
    env: Vec<String>,
    preopens: Vec<(String, String)>,
}

fn error(out: *mut *mut c_char, message: impl ToString) {
    if !out.is_null() {
        let message = message.to_string().replace('\0', "\\0");
        unsafe { *out = CString::new(message).unwrap().into_raw() };
    }
}

// Borrow the runtime's duplicated OS handle and give wasi-common its own copy.
// Guest fd_close and context teardown must never close the embedder's handle.
fn file(raw: i64) -> std::io::Result<Box<dyn wasi_common::WasiFile>> {
    #[cfg(unix)]
    let file = {
        use std::os::fd::BorrowedFd;
        let owned = unsafe { BorrowedFd::borrow_raw(raw as i32) }.try_clone_to_owned()?;
        std::fs::File::from(owned)
    };
    #[cfg(windows)]
    let file = {
        use std::os::windows::io::BorrowedHandle;
        let owned =
            unsafe { BorrowedHandle::borrow_raw(raw as *mut c_void) }.try_clone_to_owned()?;
        std::fs::File::from(owned)
    };
    Ok(Box::new(wasi_common::sync::file::File::from_cap_std(
        cap_std::fs::File::from_std(file),
    )))
}

fn build(config: Config, stdio: &[i64]) -> Result<WasiCtx, String> {
    let mut builder = WasiCtxBuilder::new();
    builder.args(&config.args).map_err(|e| e.to_string())?;
    for entry in config.env {
        let (key, value) = entry.split_once('=').unwrap_or((&entry, ""));
        builder.env(key, value).map_err(|e| e.to_string())?;
    }
    builder.inherit_stdio();
    for (index, raw) in stdio.iter().copied().enumerate() {
        if raw == -1 {
            continue;
        }
        let file = file(raw).map_err(|e| e.to_string())?;
        match index {
            0 => {
                builder.stdin(file);
            }
            1 => {
                builder.stdout(file);
            }
            2 => {
                builder.stderr(file);
            }
            _ => unreachable!(),
        }
    }
    for (guest, host) in config.preopens {
        let dir =
            wasi_common::sync::Dir::open_ambient_dir(&host, wasi_common::sync::ambient_authority())
                .map_err(|e| format!("preopen {host:?}: {e}"))?;
        builder
            .preopened_dir(dir, guest)
            .map_err(|e| e.to_string())?;
    }
    Ok(builder.build())
}

#[no_mangle]
pub unsafe extern "C" fn perry_wasm_host_wasi_new(
    config: *const u8,
    len: usize,
    stdio: *const i64,
    out_err: *mut *mut c_char,
) -> *mut c_void {
    if config.is_null() || stdio.is_null() {
        error(out_err, "missing WASI configuration");
        return std::ptr::null_mut();
    }
    let result = serde_json::from_slice(std::slice::from_raw_parts(config, len))
        .map_err(|e| e.to_string())
        .and_then(|config| build(config, std::slice::from_raw_parts(stdio, 3)));
    match result {
        Ok(ctx) => Box::into_raw(Box::new(ctx)).cast(),
        Err(e) => {
            error(out_err, e);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn perry_wasm_host_wasi_drop(context: *mut c_void) {
    if !context.is_null() {
        drop(Box::from_raw(context.cast::<WasiCtx>()));
    }
}

// The sync provider completes each future in one poll (including blocking
// poll_oneoff). A pending future is a host error, never successful errno=0.
fn scalar_errno(error: &wiggle::GuestError) -> Option<i32> {
    use wiggle::GuestError::*;
    match error {
        InvalidFlagValue { .. } | InvalidEnumValue { .. } => Some(28),
        InFunc { err, .. } => scalar_errno(err),
        _ => None,
    }
}
fn complete<F: std::future::Future<Output = Result<i32, wiggle::anyhow::Error>>>(
    future: F,
) -> Result<i32, wiggle::anyhow::Error> {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(Err(error)) => match error
            .downcast_ref::<wiggle::GuestError>()
            .and_then(scalar_errno)
        {
            Some(errno) => Ok(errno),
            None => Err(error),
        },
        Poll::Ready(result) => result,
        Poll::Pending => Err(wiggle::anyhow::anyhow!("pending synchronous WASI syscall")),
    }
}
macro_rules! result_code {
    ($result:ident, i32) => {
        $result
    };
    ($result:ident, ()) => {{
        let () = $result;
        0
    }};
}
macro_rules! dispatch {
    ($(fn $name:ident($($arg:ident: $ty:ident),* $(,)?) -> $ret:tt;)*) => {
        fn invoke(ctx: &mut WasiCtx, memory: &mut [u8], name: &str, args: &[u64])
            -> Result<i32, wiggle::anyhow::Error>
        {
            // Node accepts the full u32 exitcode domain; wasi-common limits
            // command process statuses to <126, so handle this control effect
            // at our embedding boundary before entering the provider.
            if name == "proc_exit" {
                return Err(wasi_common::I32Exit(args[0] as i32).into());
            }
            let mut memory = wiggle::GuestMemory::Unshared(memory);
            match name {
                $(stringify!($name) => {
                    #[allow(unused_mut, unused_variables)]
                    let mut args = args.iter();
                    $(let $arg = *args.next().expect("validated syscall arity") as $ty;)*
                    complete(async {
                        let result = wasi_common::snapshots::preview_1::wasi_snapshot_preview1::$name(
                            ctx, &mut memory, $($arg),*
                        ).await?;
                        Ok(result_code!(result, $ret))
                    })
                },)*
                _ => Err(wiggle::anyhow::anyhow!("unknown WASI syscall")),
            }
        }
    }
}
perry_abi::wasi_preview1_functions!(dispatch);

/// Returns a WASI errno, IMPORT_EXIT, or IMPORT_TRAP. The caller handles exit
/// only after this function has returned, so Rust resources always unwind.
#[no_mangle]
pub unsafe extern "C" fn perry_wasm_host_wasi_call(
    context: *mut c_void,
    syscall: usize,
    memory: *mut u8,
    memory_len: usize,
    args: *const u64,
    argc: usize,
    out_err: *mut *mut c_char,
) -> i32 {
    use perry_abi::wasi::{IMPORT_EXIT, IMPORT_TRAP, NAMES, PARAMS};
    if context.is_null()
        || memory.is_null()
        || (argc != 0 && args.is_null())
        || PARAMS
            .get(syscall)
            .is_none_or(|params| params.len() != argc)
    {
        error(out_err, "invalid WASI call boundary");
        return IMPORT_TRAP;
    }
    let args = if argc == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(args, argc)
    };
    match invoke(
        &mut *context.cast::<WasiCtx>(),
        std::slice::from_raw_parts_mut(memory, memory_len),
        NAMES[syscall],
        args,
    ) {
        Ok(errno) => errno,
        Err(e) => match e.downcast::<wasi_common::I32Exit>() {
            Ok(_) => IMPORT_EXIT,
            Err(e) => {
                error(out_err, e);
                IMPORT_TRAP
            }
        },
    }
}

#[cfg(test)]
mod tests;
