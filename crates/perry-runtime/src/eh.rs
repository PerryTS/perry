//! Itanium-ABI exception transport for `try`/`catch` (`invoke`/`landingpad`).
//!
//! Replaces the `longjmp` transport for generated-code `try` handlers (#7302):
//! `js_throw` stores the thrown JS value in the GC-rooted TLS slot exactly as
//! before, then raises a payload-free `_Unwind_Exception` with class
//! `PERRYJS\0`. Generated functions containing `try` carry
//! `personality ptr @perry_eh_personality` and a `landingpad {ptr,i32}
//! catch ptr null` per handler; the personality below walks the LSDA and
//! transfers control there. The landing pad ignores the `{ptr,i32}` pair —
//! the value is read back via `js_get_exception()`, unchanged.
//!
//! The unwinder steps *through* runtime Rust frames without running any
//! cleanup (the runtime is built `panic=abort` + forced unwind tables — see
//! `docs/invoke-eh-experiment.md`), which is exactly the `longjmp` semantics
//! the savepoint-restore system in `exception.rs` was built for. Rust-side
//! catches (`js_call_catching`) never see a raise at all: an open Rust
//! handler is always innermost when it is the throw target, and `js_throw`
//! uses its private `longjmp` for those (see `HandlerKind`).
//!
//! The personality routine and LSDA walk are a port of Rust std's
//! `rust_eh_personality` / `sys::personality::dwarf` (MIT OR Apache-2.0),
//! trimmed to the encodings LLVM emits for Perry's targets and with the
//! type-table/filter logic dropped (Perry landing pads are always
//! `catch ptr null` — catch-all; there are no cleanups and no filters in
//! generated code).

#![allow(non_upper_case_globals)]

use crate::eh_lsda::find_landing_pad_in_lsda;
use core::ffi::c_int;

// ---------------------------------------------------------------------------
// Minimal libunwind / libgcc Itanium unwind API bindings.
// ---------------------------------------------------------------------------

pub(crate) type UnwindReasonCode = c_int;
pub(crate) const _URC_HANDLER_FOUND: UnwindReasonCode = 6;
pub(crate) const _URC_INSTALL_CONTEXT: UnwindReasonCode = 7;
pub(crate) const _URC_CONTINUE_UNWIND: UnwindReasonCode = 8;
pub(crate) const _URC_FATAL_PHASE1_ERROR: UnwindReasonCode = 3;

type UnwindAction = c_int;
const _UA_SEARCH_PHASE: UnwindAction = 1;

#[repr(C)]
pub struct UnwindException {
    pub class: u64,
    pub cleanup: Option<extern "C" fn(UnwindReasonCode, *mut UnwindException)>,
    // The SysV/Itanium header reserves 2 private words; some ports scribble
    // on more. Over-sizing is harmless — the unwinder only uses its own view.
    pub private: [usize; 6],
}

// An opaque unwind context handle passed to the personality routine.
#[repr(C)]
pub struct UnwindContext {
    _opaque: [u8; 0],
}

extern "C" {
    /// Returns only on failure (`_URC_END_OF_STACK` when no handler exists).
    fn _Unwind_RaiseException(exception: *mut UnwindException) -> UnwindReasonCode;
    fn _Unwind_GetLanguageSpecificData(ctx: *mut UnwindContext) -> *const u8;
    fn _Unwind_GetIPInfo(ctx: *mut UnwindContext, ip_before_insn: *mut c_int) -> usize;
    fn _Unwind_GetRegionStart(ctx: *mut UnwindContext) -> usize;
    fn _Unwind_SetGR(ctx: *mut UnwindContext, reg_index: c_int, value: usize);
    fn _Unwind_SetIP(ctx: *mut UnwindContext, value: usize);
    fn _Unwind_GetCFA(ctx: *mut UnwindContext) -> usize;
    fn _Unwind_Backtrace(
        trace: unsafe extern "C" fn(*mut UnwindContext, *mut core::ffi::c_void) -> UnwindReasonCode,
        arg: *mut core::ffi::c_void,
    ) -> UnwindReasonCode;
}

// ---------------------------------------------------------------------------
// Unwind-table self-check.
// ---------------------------------------------------------------------------

/// The exception transport requires the unwinder to step *through* runtime
/// Rust frames, which requires those frames to carry unwind tables. The
/// runtime is built `panic=abort` (no tables by default) plus
/// `-C force-unwind-tables=yes` — and that flag rides on RUSTFLAGS, which a
/// stray environment override silently drops. A runtime built that way
/// strands EVERY throw that crosses a helper frame. This check runs once, on
/// the first `js_eh_try_push` of the process: `_Unwind_Backtrace` uses the
/// same CFI the raise path does, so if it cannot see past this module's own
/// nested Rust frames, the raise path is broken too — abort loudly at the
/// first `try` instead of stranding the first cross-helper throw.
///
/// WASI has no unwinder to check yet: `try` blocks lower to plain calls and a
/// throw ends the program (#11378), so there is nothing to verify.
#[cfg(target_os = "wasi")]
pub(crate) fn verify_unwind_tables_once() {}

#[cfg(not(target_os = "wasi"))]
pub(crate) fn verify_unwind_tables_once() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let frames = selfcheck_frame_a();
        // With tables present the backtrace sees at least the two
        // #[inline(never)] frames plus their callers; without them it stops
        // after the first frame (or errors out with a count of 0/1).
        if frames < 3 {
            eprintln!(
                "perry: FATAL: unwind tables are missing from this runtime \
                 build ({frames} frame(s) visible to the unwinder). The \
                 exception transport cannot cross runtime frames; rebuild \
                 with RUSTFLAGS=\"-C force-unwind-tables=yes\" (see \
                 docs/invoke-eh-experiment.md)."
            );
            std::process::abort();
        }
    });
}

#[cfg(not(target_os = "wasi"))]
#[inline(never)]
fn selfcheck_frame_a() -> usize {
    std::hint::black_box(selfcheck_frame_b()) + usize::from(std::hint::black_box(false))
}

#[cfg(not(target_os = "wasi"))]
#[inline(never)]
fn selfcheck_frame_b() -> usize {
    unsafe extern "C" fn count(
        _ctx: *mut UnwindContext,
        arg: *mut core::ffi::c_void,
    ) -> UnwindReasonCode {
        unsafe { *(arg as *mut usize) += 1 };
        // _URC_NO_REASON: the ONLY value that lets _Unwind_Backtrace keep
        // walking — any other reason code stops the trace after one frame.
        0
    }
    let mut n: usize = 0;
    unsafe {
        _Unwind_Backtrace(count, &mut n as *mut usize as *mut core::ffi::c_void);
    }
    std::hint::black_box(n)
}

// DWARF register numbers for the exception-pointer / exception-selector
// registers the landing pad reads (LLVM TargetLowering::getException*Register).
#[cfg(target_arch = "x86_64")]
const UNWIND_DATA_REG: (c_int, c_int) = (0, 1); // RAX, RDX
#[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
const UNWIND_DATA_REG: (c_int, c_int) = (0, 1); // R0/X0, R1/X1
#[cfg(target_arch = "x86")]
const UNWIND_DATA_REG: (c_int, c_int) = (0, 2); // EAX, EDX
                                                // WASI (#11377): clang's `__builtin_eh_return_data_regno` is 0/1 on
                                                // WebAssembly, the pair wasm libunwind's `_Unwind_SetGR` understands.
#[cfg(all(target_arch = "wasm32", target_os = "wasi"))]
const UNWIND_DATA_REG: (c_int, c_int) = (0, 1);

/// `PERRYJS\0` — vendor-tagged exception class. The personality is
/// class-agnostic (every Perry landing pad is a catch-all), but the tag keeps
/// Perry exceptions distinguishable from C++/Rust ones in a debugger and lets
/// a future mixed-runtime personality discriminate.
pub const PERRY_EXCEPTION_CLASS: u64 = u64::from_be_bytes(*b"PERRYJS\0");

extern "C" fn perry_exception_cleanup(_reason: UnwindReasonCode, _exc: *mut UnwindException) {
    // Per-thread static object, payload lives in the TLS exception slot:
    // nothing to free. Reached only if foreign code deletes our exception.
}

thread_local! {
    static EXC_OBJECT: std::cell::UnsafeCell<UnwindException> =
        const {
            std::cell::UnsafeCell::new(UnwindException {
                class: PERRY_EXCEPTION_CLASS,
                cleanup: Some(perry_exception_cleanup),
                private: [0; 6],
            })
        };
}

/// Address of this thread's `_Unwind_Exception` object — what a landing
/// pad receives in x0. The owned fast transport passes it explicitly
/// because it installs the context itself (#7302 follow-up).
pub(crate) fn exception_object_addr() -> u64 {
    EXC_OBJECT.with(|c| c.get()) as u64
}

/// Raise the per-thread Perry exception. Returns ONLY if the unwinder found
/// no handler (the caller reports the uncaught exception and exits) — with a
/// handler-stack entry present this indicates lost unwind tables between the
/// throw point and the handler frame (e.g. a stray `RUSTFLAGS` dropped
/// `-C force-unwind-tables` from the runtime build), which the caller must
/// report loudly rather than mask.
pub(crate) fn raise_perry_exception() -> UnwindReasonCode {
    let exc = EXC_OBJECT.with(|c| c.get());
    unsafe {
        // Re-arm the header on every raise: the unwinder scribbles on the
        // private words, and a rethrow-from-catch reuses this object (legal:
        // the previous unwind completed when control reached the pad).
        (*exc).class = PERRY_EXCEPTION_CLASS;
        (*exc).cleanup = Some(perry_exception_cleanup);
        (*exc).private = [0; 6];
        _Unwind_RaiseException(exc)
    }
}

// ---------------------------------------------------------------------------
// Personality routine.
// ---------------------------------------------------------------------------

/// The personality for Perry-generated functions (Itanium two-phase model).
///
/// Search phase: report `HANDLER_FOUND` iff the current IP sits inside a
/// call-site range with a landing pad (Perry pads are all catch-all handlers).
/// Cleanup phase: install the landing pad. IPs outside every range mean the
/// active call site was not `invoke`-protected — continue unwinding (that is
/// the deliberate semantic for throws escaping a frame with no enclosing
/// `try`; the C++ personality would `terminate` here instead).
///
/// `PERRY_EH_TRACE=1` prints one line per personality invocation (phase,
/// owning function, ip offset, decoded pad). Diagnostic only: it changes no
/// verdict, and the env probe is a cached `OnceLock` so the throw path pays
/// one branch. This is the instrument a "transport failed / no landing pad"
/// hunt needs — it names the frame the walk gave up on, which no other
/// output does.
fn eh_trace_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *crate::once_init::get_or_init(&ON, || std::env::var_os("PERRY_EH_TRACE").is_some())
}

/// # Safety
/// Called by the system unwinder with a live unwind context.
#[no_mangle]
pub unsafe extern "C" fn perry_eh_personality(
    version: c_int,
    actions: UnwindAction,
    _exception_class: u64,
    exception_object: *mut UnwindException,
    context: *mut UnwindContext,
) -> UnwindReasonCode {
    if version != 1 {
        return _URC_FATAL_PHASE1_ERROR;
    }
    let lpad = match find_landing_pad(context) {
        Ok(l) => l,
        Err(()) => {
            if eh_trace_enabled() {
                eprintln!(
                    "[perry-eh] personality actions={:#x} region={:#x}: LSDA parse FAILED",
                    actions,
                    _Unwind_GetRegionStart(context),
                );
            }
            return _URC_FATAL_PHASE1_ERROR;
        }
    };
    if eh_trace_enabled() {
        let mut before: c_int = 0;
        let ip = _Unwind_GetIPInfo(context, &mut before);
        let region = _Unwind_GetRegionStart(context);
        #[cfg(not(target_os = "wasi"))]
        let name = {
            let mut info: libc::Dl_info = std::mem::zeroed();
            if libc::dladdr(region as *const libc::c_void, &mut info) != 0
                && !info.dli_sname.is_null()
            {
                std::ffi::CStr::from_ptr(info.dli_sname)
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::from("?")
            }
        };
        // WASI has no `dladdr` (#11377).
        #[cfg(target_os = "wasi")]
        let name = String::from("?");
        eprintln!(
            "[perry-eh] personality actions={:#x} region={:#x} ({name}) ip=+{:#x} lpad={:?}",
            actions,
            region,
            ip.wrapping_sub(region),
            lpad,
        );
    }
    if actions & _UA_SEARCH_PHASE != 0 {
        match lpad {
            Some(_) => _URC_HANDLER_FOUND,
            None => _URC_CONTINUE_UNWIND,
        }
    } else {
        match lpad {
            Some(lpad) => {
                // W1 diff mode (#7302 follow-up): the owned walker predicted
                // where this throw lands before the raise; the system
                // unwinder is the oracle. Any mismatch is a walker bug —
                // fail loudly here, where both answers are in hand.
                crate::eh_walker::verify_prediction(lpad as u64, _Unwind_GetCFA(context) as u64);
                _Unwind_SetGR(context, UNWIND_DATA_REG.0, exception_object as usize);
                _Unwind_SetGR(context, UNWIND_DATA_REG.1, 0);
                _Unwind_SetIP(context, lpad);
                _URC_INSTALL_CONTEXT
            }
            None => _URC_CONTINUE_UNWIND,
        }
    }
}

/// LSDA walk: map the frame's current IP to its landing pad, if any.
unsafe fn find_landing_pad(context: *mut UnwindContext) -> Result<Option<usize>, ()> {
    let lsda = _Unwind_GetLanguageSpecificData(context);
    if lsda.is_null() {
        return Ok(None);
    }
    let mut ip_before_insn: c_int = 0;
    let ip = _Unwind_GetIPInfo(context, &mut ip_before_insn);
    // The return address points one byte past the call instruction, which
    // could fall into the next call-site range.
    let ip = if ip_before_insn != 0 {
        ip
    } else {
        ip.wrapping_sub(1)
    };
    let func_start = _Unwind_GetRegionStart(context);
    find_landing_pad_in_lsda(lsda, ip, func_start)
}

// Keep the personality (and therefore this module's symbols) out of
// dead-strip's reach in the static archives: generated code references
// `perry_eh_personality` by name only.
#[used(compiler)]
static _KEEP_PERSONALITY: unsafe extern "C" fn(
    c_int,
    UnwindAction,
    u64,
    *mut UnwindException,
    *mut UnwindContext,
) -> UnwindReasonCode = perry_eh_personality;
