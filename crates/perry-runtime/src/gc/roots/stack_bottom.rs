//! `get_stack_bottom` — the per-platform top of the current thread's stack.
//!
//! The conservative stack scan needs the address the stack grows down from,
//! and every OS answers differently (`pthread_get_stackaddr_np` on macOS,
//! `pthread_getattr_np` + `pthread_attr_getstack` on Linux, the TEB on
//! Windows, and a `0` fallback that disables stack scanning elsewhere). The
//! four `#[cfg]` arms plus their `extern "C"` blocks are the only
//! platform-conditional code in the root scanner, so they live here and
//! `roots.rs` stays under the file-size gate.
//!
//! The doc comment on the first arm describes a trace-phase mark helper, not
//! `get_stack_bottom`; it was already attached to this item and is moved
//! verbatim rather than re-pointed at the next unrelated item.

/// Specialized mark-and-enqueue for trace-phase field walks.
///
/// Descriptor-driven trace walks all share the same pattern: read a
/// heap-field word that is either a NaN-boxed JSValue or a raw I64
/// pointer at an object start, mark it if live, and push the marked
/// header onto the local worklist. The generic
/// `try_mark_value_or_raw` is general enough to also handle
/// conservative stack scans (raw interior pointers via
/// `enclosing_object`) and root scans (push to MARK_SEEDS so the
/// trace-marked-objects entry point can pick them up), but BOTH of
/// those features are pure overhead inside `drain_trace_worklist`:
///
/// 1. Field words never hold interior pointers — they're written via
///    `arr[i] = x` / `obj.f = x` / closure capture stores, all of
///    which use the object-start user pointer. Skipping
///    `enclosing_object` saves a binary-search lookup per field.
///
/// 2. The MARK_SEEDS push happens once per newly-marked object during
///    trace, but the same header is also pushed onto the local
///    worklist by the caller (so the trace drain visits it). The
///    extra MARK_SEEDS push goes onto a TLS vec, gets cleared at the
///    start of the next cycle, and is pure waste while we're already
///    in the trace phase. Skipping it saves a TLS slot deref +
///    Vec::push per marked object.
///
/// 3. The caller-side re-decode of the NaN-tag (to figure out
///    POINTER_MASK extraction vs raw-pointer extraction) is folded
///    into this function, so the caller doesn't pay that switch a
///    second time.
///
/// The valid-pointer hashset check is still load-bearing here — we
/// only elide the secondary `enclosing_object` fallback.
#[inline(always)]
#[cfg(target_os = "macos")]
pub(crate) fn get_stack_bottom() -> usize {
    extern "C" {
        fn pthread_self() -> *mut std::ffi::c_void;
        fn pthread_get_stackaddr_np(thread: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    }
    unsafe {
        let thread = pthread_self();
        pthread_get_stackaddr_np(thread) as usize
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn get_stack_bottom() -> usize {
    crate::native_stack::stack_top()
}

// Windows: read TEB.StackBase. Works on every supported Windows version
// (Windows 7+) without needing GetCurrentThreadStackLimits (Win8+), so it
// stays correct on the `--min-windows-version=7` build path. The TEB lives
// at GS:[0] on x86_64 (FS:[0] on x86); StackBase sits at offset 0x08
// (the highest address — i.e. where the stack starts and grows down from).
// This is the same pointer kernel32!GetCurrentThreadStackLimits returns as
// `HighLimit`, just read directly from the TEB to avoid the kernel32 dep.
//
// Without this, conservative stack scan early-returns with stack_bottom=0,
// the GC sees no stack roots, and any heap pointer that lives only in a
// stack slot during a callback gets swept (issues #385/#386/#387 — the
// `Array.prototype.map` / `JSON.parse(...).property` / supported_features
// segfaults all traced back to here).
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
pub(crate) fn get_stack_bottom() -> usize {
    let stack_base: usize;
    unsafe {
        std::arch::asm!(
            "mov {out}, gs:[0x08]",
            out = out(reg) stack_base,
            options(nostack, preserves_flags, readonly),
        );
    }
    stack_base
}

#[cfg(all(target_os = "windows", target_arch = "x86"))]
pub(crate) fn get_stack_bottom() -> usize {
    let stack_base: usize;
    unsafe {
        std::arch::asm!(
            "mov {out}, fs:[0x04]",
            out = out(reg) stack_base,
            options(nostack, preserves_flags, readonly),
        );
    }
    stack_base
}

#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
pub(crate) fn get_stack_bottom() -> usize {
    // ARM64 Windows: TEB pointer is in x18; StackBase at offset 0x08.
    let stack_base: usize;
    unsafe {
        let teb: usize;
        std::arch::asm!("mov {}, x18", out(reg) teb, options(nostack, preserves_flags, readonly));
        stack_base = *((teb + 0x08) as *const usize);
    }
    stack_base
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "linux",
    all(
        target_os = "windows",
        any(target_arch = "x86_64", target_arch = "x86", target_arch = "aarch64")
    ),
)))]
pub(crate) fn get_stack_bottom() -> usize {
    0 // Stack scanning not supported on this OS/arch
}
