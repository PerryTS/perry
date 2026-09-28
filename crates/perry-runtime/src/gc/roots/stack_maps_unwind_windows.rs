//! Windows x86-64 native-frame walker (`RtlVirtualUnwind`), the
//! `unwind` module for that target.
//!
//! Split out of `stack_maps.rs` to keep that file under the 2,000-line cap
//! (#10750); pure relocation. The module's doc comment stays on its `mod`
//! declaration in the parent.

use super::*;

/// x86-64 `CONTEXT` (winnt.h): 1232 bytes, 16-byte aligned. Declared by
/// hand because perry-runtime links no Windows API crate; only the
/// integer registers are read, so the FP/vector tail is opaque padding.
/// The compile-time asserts below pin the offsets this module relies on.
#[repr(C, align(16))]
struct Context {
    p_home: [u64; 6],
    context_flags: u32,
    mx_csr: u32,
    seg: [u16; 6],
    e_flags: u32,
    dr: [u64; 6],
    rax: u64,
    rcx: u64,
    rdx: u64,
    rbx: u64,
    rsp: u64,
    rbp: u64,
    rsi: u64,
    rdi: u64,
    r8: u64,
    r9: u64,
    r10: u64,
    r11: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
    rip: u64,
    /// XMM_SAVE_AREA32 (512) + 26 `M128A` vector registers (416) +
    /// VectorControl/DebugControl/LastBranch and LastException pairs (48).
    tail: [u8; 512 + 26 * 16 + 6 * 8],
}

// A drifted field offset would hand every walk garbage registers, so pin
// the layout at compile time rather than trusting the declaration above.
const _: () = assert!(std::mem::size_of::<Context>() == 1232);
const _: () = assert!(std::mem::offset_of!(Context, rax) == 0x78);
const _: () = assert!(std::mem::offset_of!(Context, rsp) == 0x98);
const _: () = assert!(std::mem::offset_of!(Context, rbp) == 0xA0);
const _: () = assert!(std::mem::offset_of!(Context, rip) == 0xF8);
const _: () = assert!(std::mem::offset_of!(Context, tail) == 0x100);

const UNW_FLAG_NHANDLER: u32 = 0;

unsafe extern "system" {
    fn RtlCaptureContext(context: *mut Context);
    fn RtlLookupFunctionEntry(
        control_pc: u64,
        image_base: *mut u64,
        history_table: *mut core::ffi::c_void,
    ) -> *mut core::ffi::c_void;
    fn RtlVirtualUnwind(
        handler_type: u32,
        image_base: u64,
        control_pc: u64,
        function_entry: *mut core::ffi::c_void,
        context: *mut Context,
        handler_data: *mut *mut core::ffi::c_void,
        establisher_frame: *mut u64,
        context_pointers: *mut core::ffi::c_void,
    ) -> *mut core::ffi::c_void;
    fn GetCurrentThreadStackLimits(low_limit: *mut usize, high_limit: *mut usize);
}

/// The frame's value of a root's base register, or `None` for a register
/// the virtual unwind cannot have restored.
///
/// Register numbers are SysV x86-64 DWARF numbers — LLVM's stack maps use
/// that numbering on every x86-64 OS, Windows included (measured: every
/// probe root arrives as `Indirect [RSP + off]`, DWARF 7). Only the Win64
/// *nonvolatile* set is trustworthy after an unwind step: RtlVirtualUnwind
/// restores exactly what the frame's unwind codes saved, and a nonvolatile
/// register a callee did not save was by definition not modified by it —
/// while a volatile register's `CONTEXT` slot still holds some inner
/// frame's value. Handing one out as a root base would give the collector
/// a wild address it then writes through, so the caller abandons the walk.
fn frame_base(context: &Context, dwarf_reg: u16) -> Option<usize> {
    let value = match dwarf_reg {
        3 => context.rbx,
        4 => context.rsi,
        5 => context.rdi,
        6 => context.rbp,
        ARCH_DWARF_SP => context.rsp,
        12 => context.r12,
        13 => context.r13,
        14 => context.r14,
        15 => context.r15,
        _ => return None,
    };
    Some(value as usize)
}

/// This thread's committed stack bounds, `[low, high)`. Every candidate
/// slot must fall inside them — a mapped root lives in its own frame.
fn stack_limits() -> Option<(usize, usize)> {
    let mut low = 0usize;
    let mut high = 0usize;
    unsafe { GetCurrentThreadStackLimits(&mut low, &mut high) };
    (low != 0 && low < high).then_some((low, high))
}

pub(super) fn visit<F: FnMut(ResolvedRoot)>(
    index: &StackMapIndex,
    visit: &mut F,
) -> NativeStackWalkStats {
    let mut stats = NativeStackWalkStats {
        walks: 1,
        ..NativeStackWalkStats::default()
    };
    let Some((stack_low, stack_high)) = stack_limits() else {
        return stats;
    };
    // Zero-initialised is fine: RtlCaptureContext overwrites the whole
    // structure, ContextFlags included.
    let mut context: Context = unsafe { std::mem::zeroed() };
    unsafe { RtlCaptureContext(&mut context) };

    loop {
        stats.frames_visited = stats.frames_visited.saturating_add(1);
        if let Some(matched) = index.match_records(context.rip as usize) {
            let mut records = index.matched(&matched);
            while let Some(record) = records.next() {
                stats.records_matched = stats.records_matched.saturating_add(1);
                // This walker's contract is abandon-on-anomaly — return
                // before visiting ANY slot the moment one location cannot
                // be resolved and bounds-checked. Pre-validate every base
                // and derived location, then hand the record to the
                // shared visitor with a resolve that can no longer fail.
                let mut validate = |location: StackMapLocation| -> bool {
                    let Some(base) = frame_base(&context, location.dwarf_reg) else {
                        return false;
                    };
                    let address = if location.offset < 0 {
                        base.checked_sub(location.offset.unsigned_abs() as usize)
                    } else {
                        base.checked_add(location.offset as usize)
                    };
                    let Some(address) = address else {
                        return false;
                    };
                    address >= stack_low
                        && address.saturating_add(std::mem::size_of::<u64>()) <= stack_high
                        && address & (std::mem::align_of::<u64>() - 1) == 0
                };
                let mut roots = record.roots();
                while let Some(location) = roots.next() {
                    let Some(location) = location else {
                        lazy::malformed(record.function_address)
                    };
                    if !validate(location) {
                        return stats;
                    }
                }
                let Some(mut slots) = record.derived_slots() else {
                    lazy::malformed(record.function_address)
                };
                while let Some(location) = slots.next() {
                    let Some(location) = location else {
                        lazy::malformed(record.function_address)
                    };
                    if !validate(location) {
                        return stats;
                    }
                }
                let mut resolve = |location: &StackMapLocation| {
                    let base = frame_base(&context, location.dwarf_reg)?;
                    let address = if location.offset < 0 {
                        base.checked_sub(location.offset.unsigned_abs() as usize)
                    } else {
                        base.checked_add(location.offset as usize)
                    };
                    address.map(|address| (address, base))
                };
                unsafe {
                    visit_record_slots(
                        &record,
                        context.rip as usize,
                        &mut resolve,
                        &mut stats,
                        visit,
                    );
                }
            }
        }

        let mut image_base = 0u64;
        let entry =
            unsafe { RtlLookupFunctionEntry(context.rip, &mut image_base, std::ptr::null_mut()) };
        if entry.is_null() {
            // No unwind info. On Win64 only the innermost frame can be a
            // leaf (a function that has performed a call must carry
            // .pdata), and frame 0 here is this Rust function, which
            // called RtlCaptureContext — so this is either the end of the
            // walkable stack or an unrecognised frame. Do not attempt the
            // leaf `[Rsp]` pop heuristic mid-walk; stop.
            break;
        }
        let previous_sp = context.rsp;
        let mut handler_data: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut establisher_frame = 0u64;
        unsafe {
            RtlVirtualUnwind(
                UNW_FLAG_NHANDLER,
                image_base,
                context.rip,
                entry,
                &mut context,
                &mut handler_data,
                &mut establisher_frame,
                std::ptr::null_mut(),
            );
        }
        if context.rip == 0 {
            // Walked off the outermost frame — the ordinary end.
            break;
        }
        let sp = context.rsp as usize;
        // The stack grows down, so each caller's SP is strictly higher
        // than its callee's. This check is also the loop's termination
        // guarantee: SP increases monotonically and is bounded by the
        // stack top, so the walk cannot cycle.
        if context.rsp <= previous_sp || sp < stack_low || sp >= stack_high || sp & 7 != 0 {
            break;
        }
    }
    stats
}
