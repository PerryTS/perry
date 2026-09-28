//! #10812: a runaway recursion throws a catchable
//! `RangeError: Maximum call stack size exceeded` instead of running into the
//! guard page and dying with SIGSEGV (exit 139, no diagnostic).
//!
//! Every compiled JS function, method and closure body begins with a stack
//! check (`perry-codegen/src/expr/stack_guard.rs`): it reads this agent's
//! limit from [`AGENT_PTR_STACK_LIMIT`] in the per-agent pointer block and, if
//! the frame address is below it, calls [`js_stack_overflow`]. Recursion of
//! any shape passes through a compiled body on every cycle, so the check
//! fires before the native stack is exhausted.
//!
//! The limit sits [`reserve`] bytes above the lowest usable stack address, so
//! the runtime frames between two checks and the throw itself (building the
//! error, capturing its stack, the unwinder) still have room. A null slot
//! means "no limit known" and never fires: the slot is published once per
//! thread from `tls_hot::fill`, which every agent runs before its first
//! allocation (on the main thread, during `js_gc_init`).

use crate::agent_ptrs::{publish, AGENT_PTR_STACK_LIMIT};
use std::cell::Cell;

/// Headroom kept below the limit: at most 1 MiB, at most a quarter of the
/// stack. A main-thread stack is 8 MiB (Linux default rlimit) and a runtime
/// thread's floor is 32 MiB (`gc::raise_default_thread_stack_floor`).
fn reserve(size: usize) -> usize {
    (1 << 20).min(size / 4)
}

thread_local! {
    /// `(limit, relaxed)` this thread published: the armed limit, and the
    /// lower one in force while [`js_stack_overflow`] builds its error.
    static LIMITS: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

/// `(lowest usable address, size)` of the calling thread's stack.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn stack_bounds() -> Option<(usize, usize)> {
    let mut attr = std::mem::MaybeUninit::<libc::pthread_attr_t>::uninit();
    let mut addr: *mut libc::c_void = std::ptr::null_mut();
    let mut size: usize = 0;
    // SAFETY: getstack/destroy run only after getattr_np initialized attr;
    // both outputs are live locals and the address is never dereferenced.
    let ok = unsafe {
        if libc::pthread_getattr_np(libc::pthread_self(), attr.as_mut_ptr()) != 0 {
            return None;
        }
        let ok = libc::pthread_attr_getstack(attr.as_ptr(), &mut addr, &mut size) == 0;
        libc::pthread_attr_destroy(attr.as_mut_ptr());
        ok
    };
    (ok && !addr.is_null() && size != 0).then_some((addr as usize, size))
}

/// `(lowest usable address, size)` of the calling thread's stack.
#[cfg(target_vendor = "apple")]
fn stack_bounds() -> Option<(usize, usize)> {
    // #11625: call `libc`'s own declarations of these two Darwin extensions
    // instead of redeclaring them locally — a second, independently hand-
    // rolled `extern "C"` block for `pthread_get_stackaddr_np` in
    // `error_stack_frames.rs` typed the parameter as `*mut c_void` rather
    // than `libc::pthread_t` (`uintptr_t`/`usize` on Apple targets), and
    // `-D warnings` promotes the resulting `clashing_extern_declarations`
    // into a hard build failure.
    // SAFETY: plain queries about the calling thread.
    let (top, size) = unsafe {
        let me = libc::pthread_self();
        (
            libc::pthread_get_stackaddr_np(me) as usize,
            libc::pthread_get_stacksize_np(me),
        )
    };
    (top != 0 && size != 0 && top > size).then(|| (top - size, size))
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
fn stack_bounds() -> Option<(usize, usize)> {
    None
}

/// Publish this thread's stack limit. Idempotent; a thread whose bounds are
/// unknown keeps the null slot and is never checked.
pub(crate) fn publish_stack_limit() {
    if LIMITS.with(|l| l.get()).0 != 0 {
        return;
    }
    let Some((low, size)) = stack_bounds() else {
        return;
    };
    let limit = low + reserve(size);
    LIMITS.with(|l| l.set((limit, low + reserve(size) / 2)));
    publish(AGENT_PTR_STACK_LIMIT, limit as *const u8);
}

/// Called by a function prologue whose frame is below this thread's limit.
/// Never returns (it throws, or exits when the error itself overflows).
///
/// The throw needs stack of its own (the error object, its captured stack,
/// the unwinder), and building the error can run user code
/// (`Error.prepareStackTrace`), whose own prologues would re-enter here. So
/// the limit is relaxed to half the reserve while the error is built and
/// restored before the throw; an overflow inside that window is fatal.
#[no_mangle]
#[cold]
#[inline(never)]
pub extern "C-unwind" fn js_stack_overflow() -> ! {
    let (limit, relaxed) = LIMITS.with(|l| l.get());
    let armed = crate::agent_ptrs::read(AGENT_PTR_STACK_LIMIT) as usize;
    if limit == 0 || armed != limit {
        eprintln!("RangeError: Maximum call stack size exceeded");
        std::process::exit(1);
    }
    publish(AGENT_PTR_STACK_LIMIT, relaxed as *const u8);
    let msg = b"Maximum call stack size exceeded";
    let msg = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_rangeerror_new(msg);
    publish(AGENT_PTR_STACK_LIMIT, limit as *const u8);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[cfg(test)]
mod tests {
    #[test]
    fn reserve_never_exceeds_a_quarter_of_a_small_stack() {
        assert_eq!(super::reserve(256 * 1024), 64 * 1024);
        assert_eq!(super::reserve(8 << 20), 1 << 20);
    }

    #[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
    #[test]
    fn published_limit_is_below_the_current_frame() {
        std::thread::spawn(|| {
            super::publish_stack_limit();
            let (limit, relaxed) = super::LIMITS.with(|l| l.get());
            assert!(relaxed < limit);
            let here = &limit as *const usize as usize;
            assert!(
                limit != 0 && limit < here,
                "limit {limit:#x} frame {here:#x}"
            );
        })
        .join()
        .unwrap();
    }
}
