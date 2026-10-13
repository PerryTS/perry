//! Exception transport for `try`/`catch` on windows-msvc (#7302, #7354).
//!
//! The throw side is `RaiseException` with a Perry-owned code; the thrown JS
//! value stays in the GC-rooted TLS slot exactly as on every other target.
//!
//! The *catch* side used to be funclet EH — `catchswitch`/`catchpad` under
//! `__C_specific_handler`, with a filter matching the code below. That shape
//! is fundamentally incompatible with precise moving-GC roots: LLVM's
//! `rewrite-statepoints-for-gc` does not support funclet EH and crashes with
//! an access violation on `catchswitch`/`catchpad` (#7354 — still reproducible
//! on LLVM 22.1.8). Every Windows module containing a `try` therefore had to
//! choose between statepoints and compiling at all, and since essentially all
//! async code lowers to a `try`, Windows was effectively barred from the
//! precise roots every other target gets.
//!
//! So windows-msvc now uses the SAME `invoke`/`landingpad` lowering as
//! ELF/Mach-O, with Perry's own personality — for generated `try` and for the
//! runtime's trap shim (`ffi/perry_trap.ll`) alike. LLVM classifies a non-MSVC
//! personality as non-funclet EH and emits, for a COFF x64 target:
//!
//! ```text
//!   .seh_handler perry_eh_personality, @unwind, @except
//!   .section .xdata,"dr"
//!   GCC_except_table0:
//! ```
//!
//! i.e. our routine installed as the frame's x64 *language handler*, plus an
//! Itanium-format LSDA. The table is byte-identical to the ELF/Mach-O one, so
//! the decoder is shared (`eh_lsda.rs`) and only the ABI around it differs:
//! the OS hands us a `DISPATCHER_CONTEXT` instead of an `_Unwind_Context`, and
//! control transfer is `RtlUnwindEx` instead of setting registers and
//! returning `_URC_INSTALL_CONTEXT`.
//!
//! MSVC x64 unwind tables (.pdata/.xdata) are mandatory for all functions, so
//! the cross-Rust-frame story needs no `force-unwind-tables` analogue: the
//! dispatcher steps runtime helper frames unconditionally, running no Rust
//! cleanups under panic=abort — the longjmp-equivalent semantics the savepoint
//! restores in `exception.rs` assume.

use crate::eh_lsda::find_landing_pad_in_lsda;
use core::ffi::c_void;

/// `0xE0000000 | "PJS"` — customer-defined (bit 29 set), noncontinuable by
/// use. The personality below refuses every other code, so a genuine access
/// violation or a foreign SEH exception keeps unwinding past JS handlers
/// instead of being swallowed as a JS throw.
pub const PERRY_SEH_CODE: u32 = 0xE050_4A53;

const EXCEPTION_NONCONTINUABLE: u32 = 0x1;
const EXCEPTION_UNWINDING: u32 = 0x2;
const EXCEPTION_EXIT_UNWIND: u32 = 0x4;

const EXCEPTION_CONTINUE_SEARCH: i32 = 1;

unsafe extern "system" {
    fn RaiseException(code: u32, flags: u32, n_args: u32, args: *const usize);
    fn RtlUnwindEx(
        target_frame: u64,
        target_ip: u64,
        exception_record: *mut ExceptionRecord,
        return_value: *mut c_void,
        context_record: *mut c_void,
        history_table: *mut c_void,
    ) -> !;
}

/// Raise the Perry SEH exception. Returns only if the exception came back
/// (no handler accepted it and something continued execution) — the caller
/// treats that as transport failure and aborts loudly.
pub(crate) fn raise_perry_exception() -> i32 {
    crate::gc::allocation_pacing::forbid_throw_in_collection();
    unsafe {
        RaiseException(
            PERRY_SEH_CODE,
            EXCEPTION_NONCONTINUABLE,
            0,
            core::ptr::null(),
        );
    }
    -1
}

#[repr(C)]
pub struct ExceptionRecord {
    exception_code: u32,
    exception_flags: u32,
    exception_record: *mut ExceptionRecord,
    exception_address: *mut c_void,
    number_parameters: u32,
    exception_information: [usize; 15],
}

/// x64 `RUNTIME_FUNCTION`. Only `BeginAddress` is read — added to the module's
/// image base it gives the function start the LSDA's offsets are relative to,
/// the COFF equivalent of `_Unwind_GetRegionStart`.
#[repr(C)]
struct RuntimeFunction {
    begin_address: u32,
    end_address: u32,
    unwind_data: u32,
}

/// x64 `DISPATCHER_CONTEXT`, as handed to a language handler. Field order is
/// load-bearing — the OS writes this — so the layout is asserted below.
#[repr(C)]
pub struct DispatcherContext {
    control_pc: u64,
    image_base: u64,
    function_entry: *const RuntimeFunction,
    establisher_frame: u64,
    target_ip: u64,
    context_record: *mut c_void,
    language_handler: *mut c_void,
    /// The bytes following the frame's unwind info — for a `@except` handler
    /// that is exactly the `GCC_except_table` LLVM emitted.
    handler_data: *const u8,
    history_table: *mut c_void,
    scope_index: u32,
    fill0: u32,
}

const _: () = assert!(core::mem::offset_of!(DispatcherContext, handler_data) == 0x38);
const _: () = assert!(core::mem::offset_of!(DispatcherContext, history_table) == 0x40);

/// The x64 language handler for Perry-generated functions.
///
/// Called by `RtlDispatchException` during the search phase with the frame's
/// `.xdata` handler data. If this frame has a landing pad covering the faulting
/// call site, transfer control to it with `RtlUnwindEx` — which unwinds every
/// intervening frame and does not return. Otherwise report
/// `ExceptionContinueSearch` so the dispatcher moves outward, which is the
/// deliberate semantic for a throw escaping a frame with no enclosing `try`.
///
/// Unwind-phase calls are declined: we perform the transfer ourselves during
/// the search phase, so there is nothing for this routine to do on the way
/// back down.
///
/// `PERRY_EH_TRACE=1` prints one line per invocation — the instrument a
/// "transport failed / no landing pad" hunt needs, since it names the frame
/// the walk gave up on.
///
/// # Safety
/// Called by the OS exception dispatcher with a live dispatcher context.
#[no_mangle]
pub unsafe extern "system" fn perry_eh_personality(
    exception_record: *mut ExceptionRecord,
    establisher_frame: u64,
    context_record: *mut c_void,
    dispatcher_context: *mut DispatcherContext,
) -> i32 {
    let record = &*exception_record;

    // Phase 2 (unwinding) has nothing for us: the transfer already happened.
    if record.exception_flags & (EXCEPTION_UNWINDING | EXCEPTION_EXIT_UNWIND) != 0 {
        return EXCEPTION_CONTINUE_SEARCH;
    }

    // Only Perry's own throws are JS exceptions. An access violation or a
    // foreign SEH exception must keep unwinding past JS handlers rather than
    // be caught as though it were a `throw` — the contract the old
    // `perry_seh_filter` carried.
    if record.exception_code != PERRY_SEH_CODE {
        return EXCEPTION_CONTINUE_SEARCH;
    }

    let dc = &*dispatcher_context;
    if dc.handler_data.is_null() || dc.function_entry.is_null() {
        return EXCEPTION_CONTINUE_SEARCH;
    }

    let func_start = dc
        .image_base
        .wrapping_add((*dc.function_entry).begin_address as u64) as usize;

    // A worker-base handoff (`exception::hand_to_worker_base`) passes every
    // handler but the base trap's shim.
    if crate::exception::worker_base_handoff_pending()
        && func_start != crate::exception::worker_base_shim_addr()
    {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    // `ControlPc` is a return address — one byte past the call — which could
    // fall into the NEXT call-site range. Same bias the Itanium path applies.
    let ip = (dc.control_pc as usize).wrapping_sub(1);

    let pad = find_landing_pad_in_lsda(dc.handler_data, ip, func_start);

    if eh_trace_enabled() {
        eprintln!(
            "[perry-eh] personality frame={:#x} region={:#x} ip_off={:#x} -> {:?}",
            establisher_frame,
            func_start,
            ip.wrapping_sub(func_start),
            pad
        );
    }

    match pad {
        Ok(Some(target)) => {
            // Unwinds every frame between here and `establisher_frame`, then
            // resumes at the landing pad. Does not return.
            //
            // The pad reads the thrown value back from the runtime's rooted
            // TLS slot via `js_get_exception`, so the `{ptr, i32}` pair the
            // Itanium ABI would pass in RAX/RDX is deliberately not set up.
            RtlUnwindEx(
                establisher_frame,
                target as u64,
                exception_record,
                core::ptr::null_mut(),
                context_record,
                dc.history_table,
            )
        }
        _ => EXCEPTION_CONTINUE_SEARCH,
    }
}

// ---------------------------------------------------------------------------
// Single-phase transport (the same algorithm as `eh_walker.rs` on Itanium
// targets, over the OS decoder).
// ---------------------------------------------------------------------------

/// x64 `CONTEXT` (winnt.h): 1232 bytes, 16-byte aligned. Only rsp/rip are
/// read or written here; the rest is restored verbatim by
/// `RtlRestoreContext`, which is why it must be the whole, real layout
/// (`RtlVirtualUnwind` keeps xmm6..xmm15 in the tail up to date).
#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct Context {
    p_home: [u64; 6],
    context_flags: u32,
    mx_csr: u32,
    seg: [u16; 6],
    e_flags: u32,
    dr: [u64; 6],
    gpr: [u64; 4], // rax rcx rdx rbx
    rsp: u64,
    gpr2: [u64; 11], // rbp rsi rdi r8..r15
    rip: u64,
    tail: [u8; 512 + 26 * 16 + 6 * 8],
}

const _: () = assert!(core::mem::size_of::<Context>() == 1232);
const _: () = assert!(core::mem::offset_of!(Context, rsp) == 0x98);
const _: () = assert!(core::mem::offset_of!(Context, rip) == 0xF8);

const UNW_FLAG_EHANDLER: u32 = 1;

unsafe extern "system" {
    fn RtlCaptureContext(context: *mut Context);
    fn RtlLookupFunctionEntry(
        control_pc: u64,
        image_base: *mut u64,
        history_table: *mut c_void,
    ) -> *const RuntimeFunction;
    fn RtlVirtualUnwind(
        handler_type: u32,
        image_base: u64,
        control_pc: u64,
        function_entry: *const RuntimeFunction,
        context: *mut Context,
        handler_data: *mut *mut c_void,
        establisher_frame: *mut u64,
        context_pointers: *mut c_void,
    ) -> *mut c_void;
    fn RtlRestoreContext(context: *mut Context, exception_record: *mut ExceptionRecord) -> !;
    fn GetCurrentThreadStackLimits(low_limit: *mut usize, high_limit: *mut usize);
}

/// Throws carried by the single-phase transport / handed to `RaiseException`.
pub(crate) static FAST_TRANSPORTS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
pub(crate) static FALLBACKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// `PERRY_EH_WALKER=off|0|system`: every throw takes `RaiseException`.
fn fast_transport_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *crate::once_init::get_or_init(&ON, || {
        !matches!(
            std::env::var("PERRY_EH_WALKER").as_deref(),
            Ok("off") | Ok("0") | Ok("system")
        )
    })
}

/// The single-phase transport for the throw in progress: walk outward with
/// `RtlLookupFunctionEntry` + `RtlVirtualUnwind`, and at the first frame
/// whose language handler is [`perry_eh_personality`] and whose LSDA maps
/// the call site to a landing pad (the frame the personality would accept,
/// handoff rule included), resume that frame at the pad with
/// `RtlRestoreContext`. The context is the frame's own state at its call
/// site, nonvolatile registers (integer and xmm6..15) as the virtual unwind
/// restored them: what `RtlUnwindEx` would hand the pad, minus the visits
/// to the frames in between (no termination handlers run, as with
/// `RaiseException` under panic=abort, where Rust frames have none).
///
/// Returns when the walk declines (no function entry, a non-rising sp, an
/// unparsable LSDA, or the transport disabled); the caller then raises.
#[inline(never)]
pub(crate) fn try_fast_transport() {
    use std::sync::atomic::Ordering::Relaxed;
    if !fast_transport_enabled() {
        FALLBACKS.fetch_add(1, Relaxed);
        return;
    }
    let handoff = crate::exception::worker_base_handoff_pending();
    let (mut low, mut high) = (0usize, 0usize);
    unsafe { GetCurrentThreadStackLimits(&mut low, &mut high) };
    let mut context: Context = unsafe { core::mem::zeroed() };
    unsafe { RtlCaptureContext(&mut context) };
    for _ in 0..4096 {
        let mut image_base = 0u64;
        let entry =
            unsafe { RtlLookupFunctionEntry(context.rip, &mut image_base, core::ptr::null_mut()) };
        if entry.is_null() {
            break;
        }
        // The frame's own state at its call site, before stepping out of it.
        let frame = context;
        let mut handler_data: *mut c_void = core::ptr::null_mut();
        let mut establisher = 0u64;
        let handler = unsafe {
            RtlVirtualUnwind(
                UNW_FLAG_EHANDLER,
                image_base,
                context.rip,
                entry,
                &mut context,
                &mut handler_data,
                &mut establisher,
                core::ptr::null_mut(),
            )
        };
        if handler as usize == perry_eh_personality as *const () as usize && !handler_data.is_null()
        {
            let func_start = image_base.wrapping_add(unsafe { (*entry).begin_address } as u64);
            if !handoff || func_start as usize == crate::exception::worker_base_shim_addr() {
                // Return address minus one, as the personality biases it.
                let ip = (frame.rip as usize).wrapping_sub(1);
                match unsafe {
                    find_landing_pad_in_lsda(handler_data as *const u8, ip, func_start as usize)
                } {
                    Ok(Some(pad)) => {
                        FAST_TRANSPORTS.fetch_add(1, Relaxed);
                        let mut target = frame;
                        target.rip = pad as u64;
                        unsafe { RtlRestoreContext(&mut target, core::ptr::null_mut()) }
                    }
                    Ok(None) => {}
                    Err(()) => break,
                }
            }
        }
        let sp = context.rsp as usize;
        if context.rip == 0 || context.rsp <= frame.rsp || sp < low || sp >= high || sp & 7 != 0 {
            break;
        }
    }
    FALLBACKS.fetch_add(1, Relaxed);
}

fn eh_trace_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *crate::once_init::get_or_init(&ON, || std::env::var_os("PERRY_EH_TRACE").is_some())
}

// Keep the personality out of dead-strip's reach in the static archives:
// generated code references `perry_eh_personality` by name only.
#[used]
static _KEEP_PERSONALITY: unsafe extern "system" fn(
    *mut ExceptionRecord,
    u64,
    *mut c_void,
    *mut DispatcherContext,
) -> i32 = perry_eh_personality;
