//! Shared `_setjmp` FFI declaration (issue #856).
//!
//! Before this module, both `gc.rs` (register-snapshot path) and `promise.rs`
//! (microtask-trap unwind path) declared their own `extern "C" fn setjmp(...)`
//! with conflicting parameter types — `*mut u64` vs `*mut i32`. The Rust
//! compiler emitted a `clashing_extern_declarations` warning. Both
//! declarations linked to the same C symbol (`_setjmp` on Apple,
//! `setjmp` elsewhere), so any one of them necessarily disagreed with
//! the real libc signature; we got away with it on macOS/aarch64 only
//! because the ABI happens to pass the pointer in `x0` regardless of
//! pointee type and the C side only reads/writes a fixed number of bytes.
//! That's undefined behaviour, not safety.
//!
//! The canonical libc shape is `int setjmp(int env[_JBLEN])`. On darwin
//! `_JBLEN = 48` (i.e. 48 `c_int`s = 192 bytes); on glibc Linux the
//! layout is also an `int[]` though the length differs. We declare the
//! one true extern here as `unsafe extern "C" fn(*mut c_int) -> c_int`
//! and have both call sites cast their working buffer to `*mut c_int`.
//! The callers can keep viewing the buffer through their preferred lens
//! (`u64` register slots in `gc.rs`, `i32` for the unwind-context byte
//! layout in `promise.rs`); only the FFI boundary needs to agree with
//! libc.
//!
//! ## Apple vs. non-Apple symbol choice
//!
//! On Apple platforms we deliberately want the *fast* `_setjmp(3)`
//! variant (Mach-O linker symbol `__setjmp`) — it skips the
//! `sigprocmask` / `__sigaltstack` syscalls that ordinary `setjmp(3)`
//! pays for on darwin. Profiling showed those syscalls accounted for
//! ~43% of CPU time in `js_promise_run_microtasks`, and on GC stack
//! scans they cost ~25 μs per call on arm64. Perry never `siglongjmp`s
//! out of a signal handler, so saving the signal mask is wasted work.
//!
//! On Linux glibc, plain `setjmp` already skips the signal-mask save
//! (POSIX leaves it implementation-defined; glibc opted for the fast
//! path). Other BSDs match macOS but we don't currently switch them
//! over — that's a separate perf measurement.
//!
//! See `promise.rs::js_promise_run_microtasks` and
//! `gc.rs::mark_stack_roots` for the original perf write-ups.

use std::os::raw::c_int;

#[cfg(target_vendor = "apple")]
extern "C" {
    /// `_setjmp(3)` — the fast, signal-state-skipping variant. The
    /// Mach-O linker symbol is `__setjmp` (the leading `_` is the
    /// C ABI prefix that Rust adds automatically on Apple). Verified
    /// via `nm /usr/lib/system/libsystem_platform.dylib | grep setjmp`.
    #[link_name = "_setjmp"]
    pub fn setjmp(env: *mut c_int) -> c_int;
}

#[cfg(not(target_vendor = "apple"))]
extern "C" {
    /// `setjmp(3)`. On glibc Linux this already doesn't save the
    /// signal mask, so it's the same fast path we want.
    pub fn setjmp(env: *mut c_int) -> c_int;
}

/// Minimum buffer size in bytes that any `setjmp` caller must allocate
/// to be safe across the platforms Perry currently supports.
///
/// - macOS arm64: `_JBLEN = 48` `c_int`s = 192 bytes.
/// - macOS x86_64: `_JBLEN = 37` `c_int`s = 148 bytes (rounded to 156).
/// - Linux x86_64 glibc: `__jmp_buf` is 8 `i64` = 64 bytes plus
///   ~12 bytes of signal-state fields = ~152 bytes for `jmp_buf`.
/// - Windows x64 MSVC: 16 doubles = 128 bytes for `_JBLEN`, padded
///   to 256 bytes of `_JUMP_BUFFER`.
///
/// We surface 192 here so callers can `const_assert!` against it.
pub const JMP_BUF_MIN_BYTES: usize = 192;

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip test: call `setjmp` against a buffer that satisfies
    /// the minimum size requirement. We never `longjmp` here — the
    /// goal is just to confirm the extern signature matches libc and
    /// the buffer is wide enough for libc to scribble into without
    /// corrupting adjacent stack frames. A return value of 0 means
    /// "first call, not coming from a longjmp."
    #[test]
    fn setjmp_smoke_via_c_int_buffer() {
        // 64 `c_int`s = 256 bytes, well above `JMP_BUF_MIN_BYTES`.
        let mut buf = [0 as c_int; 64];
        // SAFETY: `buf` is exclusively owned, lives for the duration
        // of this call, and exceeds `JMP_BUF_MIN_BYTES`. We never
        // longjmp into it, so the saved state is never read.
        let rv = unsafe { setjmp(buf.as_mut_ptr()) };
        assert_eq!(rv, 0, "first-time setjmp must return 0");
    }

    /// Exercise the same code path that `gc.rs::mark_stack_roots`
    /// uses — a `u64` register-snapshot blob cast at the FFI
    /// boundary. This is the regression test for issue #856: both
    /// the `u64` viewpoint (gc.rs) and the `i32` viewpoint
    /// (promise.rs / exception.rs) must funnel through the same
    /// `*mut c_int` extern without producing a clashing-declaration
    /// warning.
    #[test]
    fn setjmp_via_u64_buffer_cast() {
        let mut buf = [0u64; 32]; // 256 bytes — matches gc.rs
        let rv = unsafe { setjmp(buf.as_mut_ptr() as *mut c_int) };
        assert_eq!(rv, 0);
    }

    /// Exercise the `i32`/`c_int` buffer shape that
    /// `promise.rs::js_promise_run_microtasks` passes (via
    /// `exception.rs::js_try_push`'s `JmpBuf { data: [i32; 64] }`).
    #[test]
    fn setjmp_via_i32_buffer_cast() {
        let mut buf = [0i32; 64]; // 256 bytes — matches exception.rs
        let rv = unsafe { setjmp(buf.as_mut_ptr() as *mut c_int) };
        assert_eq!(rv, 0);
    }

    /// Compile-time check that our minimum-bytes constant is at least
    /// as large as the darwin arm64 `jmp_buf`.
    const _: () = {
        assert!(JMP_BUF_MIN_BYTES >= 48 * core::mem::size_of::<c_int>());
    };

    /// Issue #856 regression — drive the actual `promise.rs` setjmp
    /// site by draining microtasks with an empty queue. The microtask
    /// runner installs the unwind trap via `setjmp(trap_buf)` on
    /// every drain (the buffer is an `i32` slab from
    /// `exception.rs::js_try_push`). Before this fix, the conflicting
    /// `*mut u64` extern in `gc.rs` and `*mut i32` extern here meant
    /// one of them had to disagree with libc's real signature — the
    /// resulting buffer scribble was UB, masked by macOS arm64
    /// happening to read/write a fixed byte count regardless of
    /// pointee type. With the shared extern, this drain should
    /// complete cleanly even when no microtasks are queued.
    #[test]
    fn issue_856_promise_microtask_setjmp_does_not_crash() {
        // No microtasks queued; the run loop installs the setjmp
        // trap, sees an empty queue, calls `js_try_end`, and returns.
        // We just want to confirm that path doesn't trash the stack.
        let _ran = crate::promise::js_promise_run_microtasks();
    }
}
