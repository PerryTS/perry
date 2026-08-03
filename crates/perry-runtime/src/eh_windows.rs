//! SEH exception transport for `try`/`catch` on Windows (#7302).
//!
//! The Itanium unwinder does not exist on windows-msvc; the invoke-EH
//! lowering emits funclet EH there instead (`catchswitch`/`catchpad` with
//! personality `__C_specific_handler` and a filter matching Perry's
//! exception code — see `perry-codegen`'s `declare_seh_machinery`). The
//! throw side is `RaiseException` with a Perry-owned code; the thrown JS
//! value stays in the GC-rooted TLS slot exactly as on every other target.
//!
//! MSVC x64 unwind tables (.pdata/.xdata) are mandatory for all functions,
//! so the cross-Rust-frame story needs no `force-unwind-tables` analogue:
//! the dispatcher steps runtime helper frames unconditionally, running no
//! Rust cleanups under panic=abort — the longjmp-equivalent semantics the
//! savepoint restores in `exception.rs` assume.

/// `0xE0000000 | "PJS"` — customer-defined (bit 29 set), noncontinuable by
/// use. Must match the `-531609005` immediate in `perry_seh_filter`
/// (perry-codegen `declare_seh_machinery`).
pub const PERRY_SEH_CODE: u32 = 0xE050_4A53;

const EXCEPTION_NONCONTINUABLE: u32 = 0x1;

extern "system" {
    fn RaiseException(code: u32, flags: u32, n_args: u32, args: *const usize);
}

/// Raise the Perry SEH exception. Returns only if the exception came back
/// (no filter accepted it and something continued execution) — the caller
/// treats that as transport failure and aborts loudly.
pub(crate) fn raise_perry_exception() -> i32 {
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
