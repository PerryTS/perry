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
}

// DWARF register numbers for the exception-pointer / exception-selector
// registers the landing pad reads (LLVM TargetLowering::getException*Register).
#[cfg(target_arch = "x86_64")]
const UNWIND_DATA_REG: (c_int, c_int) = (0, 1); // RAX, RDX
#[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
const UNWIND_DATA_REG: (c_int, c_int) = (0, 1); // R0/X0, R1/X1
#[cfg(target_arch = "x86")]
const UNWIND_DATA_REG: (c_int, c_int) = (0, 2); // EAX, EDX

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
        Err(()) => return _URC_FATAL_PHASE1_ERROR,
    };
    if actions & _UA_SEARCH_PHASE != 0 {
        match lpad {
            Some(_) => _URC_HANDLER_FOUND,
            None => _URC_CONTINUE_UNWIND,
        }
    } else {
        match lpad {
            Some(lpad) => {
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

/// The GCC-style LSDA layout: header (landing-pad base encoding + optional
/// base, type-table encoding + optional offset, call-site encoding), then the
/// call-site table sorted by start offset. Perry generates only catch-all
/// handlers, so the action/type tables need no interpretation: any non-zero
/// landing-pad offset is a handler.
unsafe fn find_landing_pad_in_lsda(
    lsda: *const u8,
    ip: usize,
    func_start: usize,
) -> Result<Option<usize>, ()> {
    let mut reader = DwarfReader::new(lsda);

    let start_encoding = reader.read_u8();
    let lpad_base = if start_encoding != DW_EH_PE_omit {
        read_encoded_pointer(&mut reader, start_encoding, func_start)?
    } else {
        func_start
    };

    let ttype_encoding = reader.read_u8();
    if ttype_encoding != DW_EH_PE_omit {
        // Class-info offset — skipped, we never inspect the type table.
        reader.read_uleb128();
    }

    let call_site_encoding = reader.read_u8();
    let call_site_table_length = reader.read_uleb128();
    let action_table = reader.ptr.add(call_site_table_length as usize);

    while reader.ptr < action_table {
        let cs_start = read_encoded_offset(&mut reader, call_site_encoding)?;
        let cs_len = read_encoded_offset(&mut reader, call_site_encoding)?;
        let cs_lpad = read_encoded_offset(&mut reader, call_site_encoding)?;
        let _cs_action = reader.read_uleb128();
        // Sorted by cs_start: once past the ip, stop.
        if ip < func_start.wrapping_add(cs_start) {
            break;
        }
        if ip < func_start.wrapping_add(cs_start + cs_len) {
            return Ok(if cs_lpad == 0 {
                None
            } else {
                Some(lpad_base.wrapping_add(cs_lpad))
            });
        }
    }
    // IP not in the table: a non-invoke call site — no handler in this frame.
    Ok(None)
}

// ---------------------------------------------------------------------------
// DWARF exception-header encoded values (LSB spec, "dwarfext").
// ---------------------------------------------------------------------------

const DW_EH_PE_omit: u8 = 0xFF;
const DW_EH_PE_absptr: u8 = 0x00;
const DW_EH_PE_uleb128: u8 = 0x01;
const DW_EH_PE_udata2: u8 = 0x02;
const DW_EH_PE_udata4: u8 = 0x03;
const DW_EH_PE_udata8: u8 = 0x04;
const DW_EH_PE_sleb128: u8 = 0x09;
const DW_EH_PE_sdata2: u8 = 0x0A;
const DW_EH_PE_sdata4: u8 = 0x0B;
const DW_EH_PE_sdata8: u8 = 0x0C;
const DW_EH_PE_pcrel: u8 = 0x10;
const DW_EH_PE_indirect: u8 = 0x80;

struct DwarfReader {
    ptr: *const u8,
}

impl DwarfReader {
    fn new(ptr: *const u8) -> Self {
        DwarfReader { ptr }
    }

    unsafe fn read_u8(&mut self) -> u8 {
        let v = *self.ptr;
        self.ptr = self.ptr.add(1);
        v
    }

    unsafe fn read_unaligned<T: Copy>(&mut self) -> T {
        let v = (self.ptr as *const T).read_unaligned();
        self.ptr = self.ptr.add(core::mem::size_of::<T>());
        v
    }

    unsafe fn read_uleb128(&mut self) -> u64 {
        let mut result: u64 = 0;
        let mut shift: u32 = 0;
        loop {
            let byte = self.read_u8();
            result |= u64::from(byte & 0x7F) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                return result;
            }
        }
    }

    unsafe fn read_sleb128(&mut self) -> i64 {
        let mut result: u64 = 0;
        let mut shift: u32 = 0;
        loop {
            let byte = self.read_u8();
            result |= u64::from(byte & 0x7F) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                // Sign-extend.
                if shift < 64 && byte & 0x40 != 0 {
                    result |= u64::MAX << shift;
                }
                return result as i64;
            }
        }
    }
}

/// Offset with a value-format-only encoding (application part must be zero —
/// LLVM uses these for the call-site table).
unsafe fn read_encoded_offset(reader: &mut DwarfReader, encoding: u8) -> Result<usize, ()> {
    if encoding == DW_EH_PE_omit || encoding & 0xF0 != 0 {
        return Err(());
    }
    Ok(match encoding & 0x0F {
        // LLVM uses absptr for offsets as well as pointers.
        DW_EH_PE_absptr => reader.read_unaligned::<usize>(),
        DW_EH_PE_uleb128 => reader.read_uleb128() as usize,
        DW_EH_PE_udata2 => reader.read_unaligned::<u16>() as usize,
        DW_EH_PE_udata4 => reader.read_unaligned::<u32>() as usize,
        DW_EH_PE_udata8 => reader.read_unaligned::<u64>() as usize,
        DW_EH_PE_sleb128 => reader.read_sleb128() as usize,
        DW_EH_PE_sdata2 => reader.read_unaligned::<i16>() as usize,
        DW_EH_PE_sdata4 => reader.read_unaligned::<i32>() as usize,
        DW_EH_PE_sdata8 => reader.read_unaligned::<i64>() as usize,
        _ => return Err(()),
    })
}

/// Pointer with an application part. Perry LSDAs use absptr or pcrel (the
/// encodings LLVM emits for the landing-pad base on Mach-O and ELF);
/// textrel/datarel/funcrel/aligned never appear and are rejected.
unsafe fn read_encoded_pointer(
    reader: &mut DwarfReader,
    encoding: u8,
    _func_start: usize,
) -> Result<usize, ()> {
    if encoding == DW_EH_PE_omit {
        return Err(());
    }
    let base: usize = match encoding & 0x70 {
        DW_EH_PE_absptr => 0,
        // Relative to the address of the encoded value itself.
        DW_EH_PE_pcrel => reader.ptr as usize,
        _ => return Err(()),
    };
    let mut ptr = if base == 0 {
        if encoding & 0x0F != DW_EH_PE_absptr {
            return Err(());
        }
        reader.read_unaligned::<usize>()
    } else {
        base.wrapping_add(read_encoded_offset(reader, encoding & 0x0F)?)
    };
    if encoding & DW_EH_PE_indirect != 0 {
        ptr = *(ptr as *const usize);
    }
    Ok(ptr)
}

// Keep the personality (and therefore this module's symbols) out of
// dead-strip's reach in the static archives: generated code references
// `perry_eh_personality` by name only.
#[used]
static _KEEP_PERSONALITY: unsafe extern "C" fn(
    c_int,
    UnwindAction,
    u64,
    *mut UnwindException,
    *mut UnwindContext,
) -> UnwindReasonCode = perry_eh_personality;

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::*;

    /// Build a synthetic LSDA (uleb128 call-site encoding, DW_EH_PE_omit
    /// bases — the shape LLVM emits for small functions) and check the walk.
    fn synth_lsda(call_sites: &[(u64, u64, u64, u64)]) -> Vec<u8> {
        fn uleb(out: &mut Vec<u8>, mut v: u64) {
            loop {
                let mut b = (v & 0x7F) as u8;
                v >>= 7;
                if v != 0 {
                    b |= 0x80;
                }
                out.push(b);
                if v == 0 {
                    break;
                }
            }
        }
        let mut body = Vec::new();
        for &(start, len, lpad, action) in call_sites {
            uleb(&mut body, start);
            uleb(&mut body, len);
            uleb(&mut body, lpad);
            uleb(&mut body, action);
        }
        let mut lsda = vec![
            DW_EH_PE_omit,    // lpstart: omitted → func_start
            DW_EH_PE_omit,    // ttype: omitted
            DW_EH_PE_uleb128, // call-site encoding
        ];
        uleb(&mut lsda, body.len() as u64);
        lsda.extend_from_slice(&body);
        lsda
    }

    #[test]
    fn walk_finds_covering_call_site() {
        let lsda = synth_lsda(&[(0x10, 0x8, 0x40, 1), (0x20, 0x10, 0x80, 1)]);
        let base = 0x1000usize;
        let f = |ip: usize| unsafe { find_landing_pad_in_lsda(lsda.as_ptr(), base + ip, base) };
        assert_eq!(f(0x14).unwrap(), Some(base + 0x40));
        assert_eq!(f(0x2F).unwrap(), Some(base + 0x80));
        // Outside every range: plain call site, no handler here.
        assert_eq!(f(0x0F).unwrap(), None);
        assert_eq!(f(0x19).unwrap(), None);
        assert_eq!(f(0x31).unwrap(), None);
    }

    #[test]
    fn zero_lpad_means_no_handler() {
        let lsda = synth_lsda(&[(0x10, 0x8, 0, 0)]);
        let base = 0x2000usize;
        let got = unsafe { find_landing_pad_in_lsda(lsda.as_ptr(), base + 0x12, base) };
        assert_eq!(got.unwrap(), None);
    }

    #[test]
    fn empty_call_site_table_is_no_handler() {
        let lsda = synth_lsda(&[]);
        let got = unsafe { find_landing_pad_in_lsda(lsda.as_ptr(), 0x3000, 0x3000) };
        assert_eq!(got.unwrap(), None);
    }

    #[test]
    fn leb128_readers() {
        let bytes = [0x7Fu8]; // sleb -1
        let mut r = DwarfReader::new(bytes.as_ptr());
        assert_eq!(unsafe { r.read_sleb128() }, -1);
        let bytes2 = [0xC0u8, 0x00]; // 0x40 with continuation, then 0 → 64
        let mut r2 = DwarfReader::new(bytes2.as_ptr());
        assert_eq!(unsafe { r2.read_sleb128() }, 64);
        let bytes3 = [0xE5u8, 0x8E, 0x26]; // uleb 624485 (DWARF spec example)
        let mut r3 = DwarfReader::new(bytes3.as_ptr());
        assert_eq!(unsafe { r3.read_uleb128() }, 624485);
    }
}
