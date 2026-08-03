//! Owned single-phase stack walker for the exception transport (#7302
//! follow-up): the fast path that replaces `_Unwind_RaiseException`'s
//! two-phase, decode-every-frame-every-throw walk.
//!
//! Why it exists: the system unwinder re-decodes CFI for every frame on
//! every throw, twice (search + cleanup). Perry knows the throw target
//! before it starts (the handler stack is the search result), and throw
//! paths repeat — so a single-phase walk over a **per-PC row cache** turns
//! the second and every later throw through a call site into a few loads
//! per frame instead of a DWARF evaluation.
//!
//! Phasing (each phase verified against the system unwinder before the
//! next builds on it):
//!   W0 (this file): capture the register context, decode `.eh_frame` via
//!       `gimli` with a per-PC cache, step the stack — and prove the frame
//!       chain matches `_Unwind_Backtrace` exactly (differential test).
//!   W1: predict the landing (pad PC + CFA) for a real throw and assert
//!       the prediction inside the personality while the system unwinder
//!       still performs the transfer.
//!   W2: direct register-install transfer + fallback to the system
//!       unwinder for undecodable frames (with a liveness counter), then
//!       flip `js_throw`'s raise path.
//!
//! aarch64-only for now (the dev + CI arm platforms); other arches keep
//! the system unwinder — same observable behavior, slower path.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use gimli::{
    BaseAddresses, CfaRule, EhFrame, NativeEndian, RegisterRule, UnwindContext, UnwindSection,
};

/// Registers the walker tracks on aarch64: the callee-saved integer set
/// (x19..x28), fp, lr, sp, and the callee-saved float halves d8..d15.
///
/// The float registers are never needed to *step* (no CFA rule routes
/// through one), but they must be restored when we install a context
/// ourselves — a handler frame holding a live `f64` in d8..d15 across the
/// `try` would otherwise resume with a stale value. Silent numeric
/// corruption, so they are tracked from the start rather than bolted on.
pub(crate) const N_TRACKED: usize = 21; // x19..x28, fp, lr, sp, d8..d15

/// Register context at a point in the walk. Indices: 0..=9 → x19..x28,
/// 10 → fp(x29), 11 → lr(x30), 12 → sp, 13..=20 → d8..d15.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WalkRegs {
    pub regs: [u64; N_TRACKED],
    pub pc: u64,
}

/// Largest stack span the walk will consider plausible (8 MB is the
/// default main-thread stack on macOS; perry worker threads are smaller).
/// Anything beyond this from the starting SP is a misdecode, not a frame.
const MAX_STACK_SPAN: u64 = 64 * 1024 * 1024;

const FP: usize = 10;
const LR: usize = 11;
const SP: usize = 12;
const D0: usize = 13;

/// Map a DWARF register number (aarch64) to a tracked index.
/// X0..X30 = 0..30, SP = 31, V0..V31 = 64..95 (so d8 = 72).
fn dwarf_to_idx(reg: u16) -> Option<usize> {
    match reg {
        19..=28 => Some(reg as usize - 19),
        29 => Some(FP),
        30 => Some(LR),
        31 => Some(SP),
        72..=79 => Some(D0 + (reg as usize - 72)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Context capture (aarch64).
// ---------------------------------------------------------------------------

#[cfg(target_arch = "aarch64")]
mod capture {
    /// Layout must match the store order in `perry_eh_capture_context`
    /// and the load order in `perry_eh_install_context`: x19..x28, fp, lr,
    /// sp, d8..d15 (21 slots — see `N_TRACKED`).
    #[repr(C)]
    pub struct RawCtx {
        pub x: [u64; 21],
    }

    unsafe extern "C" {
        /// Stores the tracked register set into `out`. Returns lr (the
        /// capture call site's return address) so the walk starts at our
        /// own caller.
        pub fn perry_eh_capture_context(out: *mut RawCtx) -> u64;

        /// Install `ctx` and jump to `pad`: restores the callee-saved
        /// integer and float registers, sets sp, passes the exception
        /// object in x0 and a zero selector in x1 (what a `landingpad`
        /// reads), then branches. Never returns.
        pub fn perry_eh_install_context(ctx: *const RawCtx, pad: u64, exc: u64) -> !;
    }

    core::arch::global_asm!(
        ".p2align 2",
        ".globl _perry_eh_capture_context",
        "_perry_eh_capture_context:",
        "stp x19, x20, [x0, #0]",
        "stp x21, x22, [x0, #16]",
        "stp x23, x24, [x0, #32]",
        "stp x25, x26, [x0, #48]",
        "stp x27, x28, [x0, #64]",
        "stp x29, x30, [x0, #80]",
        "mov x9, sp",
        "str x9, [x0, #96]",
        "stp d8, d9, [x0, #104]",
        "stp d10, d11, [x0, #120]",
        "stp d12, d13, [x0, #136]",
        "stp d14, d15, [x0, #152]",
        "mov x0, x30",
        "ret",
        // ---- install ----
        // x0 = ctx, x1 = pad, x2 = exception object.
        // Load sp and pad into scratch registers BEFORE clobbering the
        // callee-saved file, then move sp last: once sp moves, the ctx
        // pointer must already be in a register we are not restoring.
        ".p2align 2",
        ".globl _perry_eh_install_context",
        "_perry_eh_install_context:",
        "ldr x9, [x0, #96]", // target sp
        "mov x10, x1",       // target pad
        "mov x11, x2",       // exception object
        "ldp d8, d9,   [x0, #104]",
        "ldp d10, d11, [x0, #120]",
        "ldp d12, d13, [x0, #136]",
        "ldp d14, d15, [x0, #152]",
        "ldp x19, x20, [x0, #0]",
        "ldp x21, x22, [x0, #16]",
        "ldp x23, x24, [x0, #32]",
        "ldp x25, x26, [x0, #48]",
        "ldp x27, x28, [x0, #64]",
        "ldp x29, x30, [x0, #80]",
        "mov sp, x9",
        "mov x0, x11", // landingpad's {ptr, _}
        "mov x1, #0",  // landingpad's {_, i32 selector}
        "br x10",
    );
}

/// Capture the register context of OUR CALLER: the walk starts at the
/// function that invoked this one (its pc = our return address, its sp =
/// sp after our frame is popped — we have no frame, the asm is a leaf).
#[cfg(target_arch = "aarch64")]
#[inline(never)]
pub(crate) fn capture_here() -> WalkRegs {
    let mut raw = capture::RawCtx { x: [0; N_TRACKED] };
    let ret_addr = unsafe { capture::perry_eh_capture_context(&mut raw) };
    WalkRegs {
        regs: raw.x,
        pc: ret_addr,
    }
}

// ---------------------------------------------------------------------------
// .eh_frame discovery (Mach-O; ELF arm lands with the Linux arm).
// ---------------------------------------------------------------------------

struct EhFrameImage {
    /// Runtime address of the `__eh_frame` section.
    eh_frame_addr: u64,
    bytes: &'static [u8],
    /// Runtime address of `__text` (BaseAddresses wants it for pc-rel).
    text_addr: u64,
    /// Sorted (function_start, function_end, fde_offset) index, built once.
    fde_index: Vec<(u64, u64, gimli::EhFrameOffset)>,
    /// Image load address (mach header) — compact-unwind offsets are
    /// relative to it.
    image_base: u64,
    /// Sorted (function_addr, compact encoding) — one entry per function,
    /// flattened from `__unwind_info`'s second-level pages. On macOS this
    /// is the AUTHORITATIVE index: functions expressible compactly have NO
    /// .eh_frame FDE, and DWARF-mode entries carry their FDE's section
    /// offset in the low 24 bits.
    compact_index: Vec<(u64, u32)>,
    /// Sorted (function_addr, lsda_addr) from the LSDA index arrays.
    lsda_index: Vec<(u64, u64)>,
}

// arm64 compact-unwind encoding (mach-o/compact_unwind_encoding.h).
const CU_MODE_MASK: u32 = 0x0F00_0000;
const CU_MODE_FRAMELESS: u32 = 0x0200_0000;
const CU_MODE_DWARF: u32 = 0x0300_0000;
const CU_MODE_FRAME: u32 = 0x0400_0000;
const CU_DWARF_SECTION_OFFSET: u32 = 0x00FF_FFFF;
const CU_FRAMELESS_STACK_SIZE_MASK: u32 = 0x00FF_F000;
/// (mask bit, first tracked idx of the pair). d-pairs advance the save
/// cursor but are not tracked until the W2 install phase.
const CU_X_PAIRS: [(u32, usize); 5] = [(0x001, 0), (0x002, 2), (0x004, 4), (0x008, 6), (0x010, 8)];
const CU_D_PAIRS: [(u32, usize); 4] = [
    (0x100, D0),
    (0x200, D0 + 2),
    (0x400, D0 + 4),
    (0x800, D0 + 6),
];

/// Parse `__unwind_info` into flat sorted (function, encoding) + LSDA
/// indexes. Mirrors the layout libunwind's UnwindCursor reads.
fn parse_unwind_info(ui: &[u8], image_base: u64) -> (Vec<(u64, u32)>, Vec<(u64, u64)>) {
    let u32at =
        |off: usize| -> u32 { u32::from_le_bytes(ui[off..off + 4].try_into().unwrap_or([0; 4])) };
    let u16at =
        |off: usize| -> u16 { u16::from_le_bytes(ui[off..off + 2].try_into().unwrap_or([0; 2])) };
    let mut funcs = Vec::new();
    let mut lsdas = Vec::new();
    if ui.len() < 28 || u32at(0) != 1 {
        return (funcs, lsdas);
    }
    let common_off = u32at(4) as usize;
    let common_count = u32at(8) as usize;
    let index_off = u32at(20) as usize;
    let index_count = u32at(24) as usize;
    let common: Vec<u32> = (0..common_count)
        .map(|i| u32at(common_off + 4 * i))
        .collect();
    for i in 0..index_count.saturating_sub(1) {
        let entry = index_off + 12 * i;
        let page_off = u32at(entry + 4) as usize;
        let lsda_start = u32at(entry + 8) as usize;
        let lsda_end = u32at(index_off + 12 * (i + 1) + 8) as usize;
        let mut off = lsda_start;
        while off + 8 <= lsda_end {
            lsdas.push((
                image_base + u32at(off) as u64,
                image_base + u32at(off + 4) as u64,
            ));
            off += 8;
        }
        if page_off == 0 {
            continue;
        }
        let kind = u32at(page_off);
        if kind == 2 {
            let e_off = u16at(page_off + 4) as usize;
            let count = u16at(page_off + 6) as usize;
            for e in 0..count {
                let at = page_off + e_off + 8 * e;
                funcs.push((image_base + u32at(at) as u64, u32at(at + 4)));
            }
        } else if kind == 3 {
            let fn_base = u32at(entry) as u64;
            let e_off = u16at(page_off + 4) as usize;
            let count = u16at(page_off + 6) as usize;
            let enc_off = u16at(page_off + 8) as usize;
            let enc_count = u16at(page_off + 10) as usize;
            for e in 0..count {
                let raw = u32at(page_off + e_off + 4 * e);
                let idx = (raw >> 24) as usize;
                let enc = if idx < common.len() {
                    common[idx]
                } else {
                    u32at(page_off + enc_off + 4 * (idx - common.len()))
                };
                funcs.push((image_base + fn_base + (raw & 0x00FF_FFFF) as u64, enc));
            }
        }
    }
    funcs.sort_unstable_by_key(|e| e.0);
    lsdas.sort_unstable_by_key(|e| e.0);
    (funcs, lsdas)
}

#[cfg(target_os = "macos")]
fn find_eh_frame_image() -> Option<EhFrameImage> {
    use core::ffi::{c_char, c_ulong};
    unsafe extern "C" {
        fn _dyld_image_count() -> u32;
        fn _dyld_get_image_header(idx: u32) -> *const libc::c_void;
        fn getsectiondata(
            mhp: *const libc::c_void,
            segname: *const c_char,
            sectname: *const c_char,
            size: *mut c_ulong,
        ) -> *mut u8;
    }
    // The main executable: generated code AND the runtime staticlib both
    // live there. dladdr on one of our own functions pins the right image.
    let probe = capture_here as *const ();
    let mut info: libc::Dl_info = unsafe { core::mem::zeroed() };
    if unsafe { libc::dladdr(probe as *const _, &mut info) } == 0 {
        return None;
    }
    let n = unsafe { _dyld_image_count() };
    for i in 0..n {
        let hdr = unsafe { _dyld_get_image_header(i) };
        if hdr as usize != info.dli_fbase as usize {
            continue;
        }
        let mut size: core::ffi::c_ulong = 0;
        let eh =
            unsafe { getsectiondata(hdr, c"__TEXT".as_ptr(), c"__eh_frame".as_ptr(), &mut size) };
        if eh.is_null() || size == 0 {
            return None;
        }
        let mut tsize: core::ffi::c_ulong = 0;
        let text =
            unsafe { getsectiondata(hdr, c"__TEXT".as_ptr(), c"__text".as_ptr(), &mut tsize) };
        let mut usize_: core::ffi::c_ulong = 0;
        let ui = unsafe {
            getsectiondata(
                hdr,
                c"__TEXT".as_ptr(),
                c"__unwind_info".as_ptr(),
                &mut usize_,
            )
        };
        let bytes = unsafe { core::slice::from_raw_parts(eh as *const u8, size as usize) };
        let (compact_index, lsda_index) = if ui.is_null() || usize_ == 0 {
            (Vec::new(), Vec::new())
        } else {
            let ui_bytes = unsafe { core::slice::from_raw_parts(ui as *const u8, usize_ as usize) };
            parse_unwind_info(ui_bytes, hdr as u64)
        };
        return Some(EhFrameImage {
            eh_frame_addr: eh as u64,
            bytes,
            text_addr: text as u64,
            fde_index: Vec::new(),
            image_base: hdr as u64,
            compact_index,
            lsda_index,
        });
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn find_eh_frame_image() -> Option<EhFrameImage> {
    // Linux: dl_iterate_phdr + PT_GNU_EH_FRAME. Lands with the Linux CI
    // arm; until then the walker reports unavailable and the system
    // unwinder carries all throws.
    None
}

// ---------------------------------------------------------------------------
// Row cache + stepping.
// ---------------------------------------------------------------------------

/// One decoded unwind row: everything needed to step a frame, cached per
/// call-site PC. `cfa = regs[cfa_reg] + cfa_off`; each (idx, off) pair
/// reloads tracked register `idx` from `cfa + off`; the caller's pc is the
/// restored lr; the caller's sp is the CFA.
#[derive(Clone, Debug)]
pub(crate) struct StepRow {
    cfa_reg: usize,
    cfa_off: i64,
    reloads: Vec<(usize, i64)>,
}

/// Diff-mode decode diagnostics; always returns None.
fn diag_decline(pc: u64, why: &str) -> Option<StepRow> {
    if std::env::var("PERRY_EH_WALKER").as_deref() == Ok("diff") {
        eprintln!("perry: eh-walker diff: decode declined at {pc:#x}: {why}");
    }
    None
}

pub(crate) struct Walker {
    image: EhFrameImage,
    eh_frame: EhFrame<gimli::EndianSlice<'static, NativeEndian>>,
    bases: BaseAddresses,
    rows: HashMap<u64, Option<StepRow>>,
}

static WALKER: OnceLock<Option<Mutex<Walker>>> = OnceLock::new();

fn walker() -> Option<&'static Mutex<Walker>> {
    WALKER
        .get_or_init(|| {
            let mut image = find_eh_frame_image()?;
            let eh_frame = EhFrame::new(image.bytes, NativeEndian);
            let bases = BaseAddresses::default()
                .set_eh_frame(image.eh_frame_addr)
                .set_text(image.text_addr);
            // Index every FDE once: (start, end, offset), sorted by start.
            let mut entries = eh_frame.entries(&bases);
            let mut index = Vec::new();
            while let Ok(Some(entry)) = entries.next() {
                if let gimli::CieOrFde::Fde(partial) = entry {
                    if let Ok(fde) = partial.parse(EhFrame::cie_from_offset) {
                        index.push((
                            fde.initial_address(),
                            fde.initial_address() + fde.len(),
                            fde.offset().into(),
                        ));
                    }
                }
            }
            index.sort_unstable_by_key(|e| e.0);
            image.fde_index = index;
            Some(Mutex::new(Walker {
                image,
                eh_frame,
                bases,
                rows: HashMap::new(),
            }))
        })
        .as_ref()
}

impl Walker {
    /// Decode (or fetch cached) the step row covering `pc`.
    fn row_for(&mut self, pc: u64) -> Option<StepRow> {
        if let Some(cached) = self.rows.get(&pc) {
            return cached.clone();
        }
        let row = self.decode_row(pc);
        self.rows.insert(pc, row.clone());
        row
    }

    fn decode_row(&self, pc: u64) -> Option<StepRow> {
        // Compact unwind is authoritative on macOS: FRAME/FRAMELESS
        // functions have no .eh_frame FDE at all, and DWARF-mode entries
        // carry their FDE's section offset.
        if !self.image.compact_index.is_empty() {
            let ci = &self.image.compact_index;
            let pos = ci.partition_point(|e| e.0 <= pc);
            if pos == 0 {
                return diag_decline(pc, "below compact index");
            }
            let (_fstart, enc) = ci[pos - 1];
            return match enc & CU_MODE_MASK {
                CU_MODE_FRAME => {
                    // stp fp, lr, [sp, #-16]!; mov fp, sp — CFA = fp+16;
                    // saved fp at [fp] (cfa-16), lr at [fp+8] (cfa-8); csr
                    // singles descend from fp-8 (cfa-24) in mask-bit order
                    // (libunwind CompactUnwinder stepWithCompactEncodingFrame).
                    let mut reloads = vec![(FP, -16i64), (LR, -8i64)];
                    let mut loc: i64 = -24;
                    for (bit, idx0) in CU_X_PAIRS {
                        if enc & bit != 0 {
                            reloads.push((idx0, loc));
                            reloads.push((idx0 + 1, loc - 8));
                            loc -= 16;
                        }
                    }
                    for (bit, idx0) in CU_D_PAIRS {
                        if enc & bit != 0 {
                            reloads.push((idx0, loc));
                            reloads.push((idx0 + 1, loc - 8));
                            loc -= 16;
                        }
                    }
                    Some(StepRow {
                        cfa_reg: FP,
                        cfa_off: 16,
                        reloads,
                    })
                }
                CU_MODE_FRAMELESS => {
                    // CFA = sp + stacksize; csrs at the top of the frame,
                    // descending; lr is live in the register (a frameless
                    // function that calls must save lr and would not be
                    // encoded frameless).
                    let stack = (((enc & CU_FRAMELESS_STACK_SIZE_MASK) >> 12) as i64) * 16;
                    let mut reloads = Vec::new();
                    let mut loc: i64 = -8;
                    for (bit, idx0) in CU_X_PAIRS {
                        if enc & bit != 0 {
                            reloads.push((idx0, loc));
                            reloads.push((idx0 + 1, loc - 8));
                            loc -= 16;
                        }
                    }
                    for (bit, idx0) in CU_D_PAIRS {
                        if enc & bit != 0 {
                            reloads.push((idx0, loc));
                            reloads.push((idx0 + 1, loc - 8));
                            loc -= 16;
                        }
                    }
                    Some(StepRow {
                        cfa_reg: SP,
                        cfa_off: stack,
                        reloads,
                    })
                }
                CU_MODE_DWARF => {
                    let off = gimli::EhFrameOffset::from((enc & CU_DWARF_SECTION_OFFSET) as usize);
                    self.decode_dwarf_row(pc, off)
                }
                _ => diag_decline(pc, "compact mode 0 (no unwind info)"),
            };
        }
        // Non-macOS (or no __unwind_info): linear FDE index.
        let idx = &self.image.fde_index;
        let pos = idx.partition_point(|e| e.0 <= pc);
        if pos == 0 {
            return diag_decline(pc, "below index");
        }
        let (start, end, offset) = idx[pos - 1];
        if pc < start || pc >= end {
            return diag_decline(pc, "no FDE covers pc");
        }
        self.decode_dwarf_row(pc, offset)
    }

    fn decode_dwarf_row(&self, pc: u64, offset: gimli::EhFrameOffset) -> Option<StepRow> {
        let Ok(fde) = self
            .eh_frame
            .fde_from_offset(&self.bases, offset, EhFrame::cie_from_offset)
        else {
            return diag_decline(pc, "FDE parse failed");
        };
        let mut ctx = UnwindContext::new();
        let Ok(row) = fde.unwind_info_for_address(&self.eh_frame, &self.bases, &mut ctx, pc) else {
            return diag_decline(pc, "no unwind row for pc");
        };
        let (cfa_reg, cfa_off) = match row.cfa() {
            CfaRule::RegisterAndOffset { register, offset } => {
                let Some(r) = dwarf_to_idx(register.0) else {
                    return diag_decline(pc, "CFA register untracked");
                };
                (r, *offset)
            }
            CfaRule::Expression(_) => return diag_decline(pc, "CFA is a DWARF expression"),
        };
        let mut reloads = Vec::new();
        for &(reg, ref rule) in row.registers() {
            let Some(idx) = dwarf_to_idx(reg.0) else {
                continue;
            };
            match rule {
                RegisterRule::Offset(off) => reloads.push((idx, *off)),
                RegisterRule::SameValue | RegisterRule::Undefined => {}
                _ => return diag_decline(pc, "unsupported register rule"),
            }
        }
        Some(StepRow {
            cfa_reg,
            cfa_off,
            reloads,
        })
    }

    /// Step one frame: given the register state AT `regs.pc`, produce the
    /// caller's state. None = undecodable (caller falls back).
    pub(crate) fn step(&mut self, regs: &WalkRegs, stack_low: u64) -> Option<WalkRegs> {
        // The stored pc is a return address: the relevant row is the one
        // covering the call instruction.
        let row = self.row_for(regs.pc.wrapping_sub(1))?;
        let base = regs.regs[row.cfa_reg];
        let cfa = (base as i64).checked_add(row.cfa_off)? as u64;

        // FAIL-SAFE, NOT FAIL-DANGEROUS. A misdecoded row yields a bogus
        // CFA, and every reload below is a raw dereference — an unchecked
        // walk turns a decoding gap into a wild read (observed: this walker
        // segfaulted stepping libtest's frames, whose shapes the compiled-
        // program path never produces). Declining costs the speedup for one
        // throw; the system unwinder then carries it with identical
        // semantics. So: the CFA must move monotonically UP a plausible
        // stack, and every slot we read must lie inside the region between
        // the walk's starting SP and that ceiling.
        if cfa <= regs.regs[SP] || cfa <= stack_low {
            return None;
        }
        let ceiling = stack_low.checked_add(MAX_STACK_SPAN)?;
        if cfa > ceiling {
            return None;
        }

        let mut next = *regs;
        for &(idx, off) in &row.reloads {
            let addr = (cfa as i64).checked_add(off)? as u64;
            if addr < stack_low || addr >= ceiling || addr % 8 != 0 {
                return None;
            }
            next.regs[idx] = unsafe { core::ptr::read(addr as *const u64) };
        }
        next.regs[SP] = cfa;
        next.pc = next.regs[LR];
        Some(next)
    }
}

/// Walk from the current call site upward, collecting up to `max` frame
/// PCs (return addresses). Returns None if the walker is unavailable on
/// this platform/build.
#[cfg(target_arch = "aarch64")]
pub(crate) fn walk_pcs_from_here(max: usize) -> Option<Vec<u64>> {
    let w = walker()?;
    let mut guard = w.lock().ok()?;
    let mut regs = capture_here();
    let stack_low = regs.regs[SP];
    let mut pcs = Vec::with_capacity(max.min(64));
    while pcs.len() < max {
        pcs.push(regs.pc);
        match guard.step(&regs, stack_low) {
            Some(next) => {
                // Terminate on a non-progressing or clearly-bottom frame.
                if next.pc == 0 || next.regs[SP] <= regs.regs[SP] && next.pc == regs.pc {
                    break;
                }
                regs = next;
            }
            None => break,
        }
    }
    Some(pcs)
}

#[cfg(not(target_arch = "aarch64"))]
pub(crate) fn walk_pcs_from_here(_max: usize) -> Option<Vec<u64>> {
    None
}

// ---------------------------------------------------------------------------
// W1: landing prediction, verified against the system unwinder per throw.
// ---------------------------------------------------------------------------

/// `PERRY_EH_WALKER=diff` — before every raise, run the owned walk to
/// predict (landing-pad pc, handler CFA) and stash it; the personality's
/// install branch calls [`verify_prediction`] with the system unwinder's
/// answer and aborts on mismatch. Zero work when unset (one lazy bool).
fn diff_mode() -> bool {
    static MODE: OnceLock<bool> = OnceLock::new();
    *MODE.get_or_init(|| {
        let mode = std::env::var("PERRY_EH_WALKER");
        let on = matches!(mode.as_deref(), Ok("diff"));
        if on || matches!(mode.as_deref(), Ok("stats")) {
            // Report the tally at exit, so a run that verified nothing —
            // or that silently stopped taking the fast path — says so out
            // loud instead of looking clean.
            extern "C" fn at_exit() {
                report_diff_stats();
                report_stats();
            }
            unsafe { libc::atexit(at_exit) };
        }
        on
    })
}

thread_local! {
    /// (predicted pad pc, predicted CFA at the handler frame). Cleared on
    /// verification so a stale prediction can never carry to a later throw.
    static PREDICTION: core::cell::Cell<Option<(u64, u64)>> =
        const { core::cell::Cell::new(None) };
}

impl Walker {
    /// LSDA pointer + function start for the FDE covering `pc`, if any.
    fn lsda_for(&self, pc: u64) -> Option<(u64, u64)> {
        // On macOS the compact-unwind LSDA index is authoritative: it
        // covers FRAME/FRAMELESS `try` functions, which have no FDE at all
        // (89% of our functions are DWARF-mode, but the try-containing
        // ones are exactly the frame-shaped minority).
        if !self.image.compact_index.is_empty() {
            let ci = &self.image.compact_index;
            let pos = ci.partition_point(|e| e.0 <= pc);
            if pos == 0 {
                return None;
            }
            let fstart = ci[pos - 1].0;
            let li = &self.image.lsda_index;
            let lpos = li.partition_point(|e| e.0 <= fstart);
            if lpos == 0 {
                return None;
            }
            let (lfunc, lsda) = li[lpos - 1];
            // The LSDA array is per-function and sparse: an entry only
            // belongs to this frame if it names this exact function.
            if lfunc != fstart {
                return None;
            }
            return Some((lsda, fstart));
        }
        let idx = &self.image.fde_index;
        let pos = idx.partition_point(|e| e.0 <= pc);
        if pos == 0 {
            return None;
        }
        let (start, end, offset) = idx[pos - 1];
        if pc < start || pc >= end {
            return None;
        }
        let fde = self
            .eh_frame
            .fde_from_offset(&self.bases, offset, EhFrame::cie_from_offset)
            .ok()?;
        match fde.lsda() {
            Some(gimli::Pointer::Direct(addr)) => Some((addr, start)),
            _ => None,
        }
    }
}

/// Walk from `start` and find the throw's landing: the first frame whose
/// LSDA maps its call site to a landing pad. Returns (pad pc, the register
/// context to resume that frame with).
///
/// The caller passes the starting context so the walk begins at ITS frame
/// — `capture_here()` must be inlined into the caller's body, not this
/// one. The walker mutex is released before returning, which matters for
/// the install path: jumping away while holding it would strand the guard
/// and deadlock every later throw.
#[cfg(target_arch = "aarch64")]
fn find_handler(start: WalkRegs) -> Option<(u64, WalkRegs)> {
    let w = walker()?;
    let mut guard = w.lock().ok()?;
    let stack_low = start.regs[SP];
    let mut regs = start;
    for _ in 0..4096 {
        // The stored pc is a return address; the call site is pc-1.
        let site = regs.pc.wrapping_sub(1);
        if let Some((lsda, func_start)) = guard.lsda_for(site) {
            if let Ok(Some(pad)) = unsafe {
                crate::eh::find_landing_pad_in_lsda(
                    lsda as *const u8,
                    site as usize,
                    func_start as usize,
                )
            } {
                return Some((pad as u64, regs));
            }
        }
        match guard.step(&regs, stack_low) {
            Some(next) => regs = next,
            None => {
                if diff_mode() {
                    // Name the frame the decoder declined.
                    let mut info: libc::Dl_info = unsafe { core::mem::zeroed() };
                    let name = if unsafe { libc::dladdr(regs.pc as *const _, &mut info) } != 0
                        && !info.dli_sname.is_null()
                    {
                        unsafe { core::ffi::CStr::from_ptr(info.dli_sname) }
                            .to_string_lossy()
                            .into_owned()
                    } else {
                        "<unknown>".to_string()
                    };
                    eprintln!(
                        "perry: eh-walker: step declined at pc={:#x} ({name})",
                        regs.pc
                    );
                }
                return None;
            }
        }
        if regs.pc == 0 {
            return None;
        }
    }
    None
}

/// Diff-mode prediction: (pad, CFA) for the throw about to be raised.
#[cfg(target_arch = "aarch64")]
#[inline(never)]
fn predict_landing() -> Option<(u64, u64)> {
    let start = capture_here();
    let (pad, regs) = find_handler(start)?;
    Some((pad, regs.regs[SP]))
}

#[cfg(not(target_arch = "aarch64"))]
fn predict_landing() -> Option<(u64, u64)> {
    None
}

/// Throws carried by the owned walker, and throws that fell back to the
/// system unwinder. Reported by [`report_stats`]: a fallback rate that
/// creeps toward 100% would otherwise be invisible — the program stays
/// correct and merely loses the speedup, which is exactly the kind of
/// silent regression that makes an optimization look permanent while it
/// has actually stopped happening.
pub(crate) static FAST_TRANSPORTS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
pub(crate) static FALLBACKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// `PERRY_EH_WALKER=off` reverts to the system unwinder for every throw
/// (bisection escape hatch). Any other value — including unset — uses the
/// owned transport where it can, falling back per-throw where it cannot.
fn fast_transport_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_EH_WALKER").as_deref(),
            Ok("off") | Ok("0") | Ok("system")
        )
    })
}

/// Attempt the owned single-phase transport for the throw in progress.
///
/// On success this NEVER RETURNS: it restores the handler frame's
/// callee-saved registers and stack pointer and branches to its landing
/// pad — the same state transition `_URC_INSTALL_CONTEXT` performs, minus
/// the search phase and minus re-decoding every frame.
///
/// Returns (normally) when the walk could not carry the throw, in which
/// case the caller raises through the system unwinder. Correctness is
/// identical on both paths; only speed differs.
#[cfg(target_arch = "aarch64")]
#[inline(never)]
pub(crate) fn try_fast_transport(exception_object: u64) {
    use std::sync::atomic::Ordering::Relaxed;
    // Diff mode is a VERIFICATION mode: the system unwinder must perform
    // the transfer so the personality can compare its answer with our
    // prediction. Installing here would bypass the very check being run.
    if diff_mode() || !fast_transport_enabled() {
        FALLBACKS.fetch_add(1, Relaxed);
        return;
    }
    let start = capture_here();
    // The walker mutex is released inside `find_handler`, before we jump.
    let Some((pad, regs)) = find_handler(start) else {
        FALLBACKS.fetch_add(1, Relaxed);
        return;
    };
    FAST_TRANSPORTS.fetch_add(1, Relaxed);
    let ctx = capture::RawCtx { x: regs.regs };
    unsafe { capture::perry_eh_install_context(&ctx, pad, exception_object) }
}

#[cfg(not(target_arch = "aarch64"))]
pub(crate) fn try_fast_transport(_exception_object: u64) {
    FALLBACKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// Transport tally, printed when `PERRY_EH_WALKER=diff|stats`.
pub(crate) fn report_stats() {
    use std::sync::atomic::Ordering::Relaxed;
    eprintln!(
        "perry: eh-walker: fast={} fallback={}",
        FAST_TRANSPORTS.load(Relaxed),
        FALLBACKS.load(Relaxed)
    );
}

/// Throws whose prediction was checked against the system unwinder, and
/// throws where the walk declined. Reported by [`report_diff_stats`] so a
/// silent run can never be mistaken for a verified one — the same
/// "assert the subject was live" rule the GC gates learned the hard way
/// (#7024/#7025).
pub(crate) static DIFF_VERIFIED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
pub(crate) static DIFF_DECLINED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Called by `js_throw` immediately before the raise (diff mode only).
pub(crate) fn predict_before_raise() {
    if !diff_mode() {
        return;
    }
    let p = predict_landing();
    if p.is_none() {
        DIFF_DECLINED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        eprintln!("perry: eh-walker diff: NO PREDICTION for this throw (walk failed)");
    }
    PREDICTION.with(|c| c.set(p));
}

/// Print the diff-mode tally at process exit. A run that reports
/// `verified=0` proves nothing, however green it looks.
pub(crate) fn report_diff_stats() {
    if !diff_mode() {
        return;
    }
    use std::sync::atomic::Ordering::Relaxed;
    eprintln!(
        "perry: eh-walker diff: verified={} declined={}",
        DIFF_VERIFIED.load(Relaxed),
        DIFF_DECLINED.load(Relaxed)
    );
}

/// Called by the personality's install branch with the system unwinder's
/// answer. Mismatch = walker bug — abort with both answers in hand.
pub(crate) fn verify_prediction(actual_pad: u64, actual_cfa: u64) {
    if !diff_mode() {
        return;
    }
    let Some((pad, cfa)) = PREDICTION.with(|c| c.take()) else {
        return; // walk declined (already reported); system carries the throw
    };
    if pad != actual_pad || cfa != actual_cfa {
        eprintln!(
            "perry: FATAL: eh-walker misprediction:\n  pad: ours {pad:#x} vs system {actual_pad:#x}\n  cfa: ours {cfa:#x} vs system {actual_cfa:#x}"
        );
        std::process::abort();
    }
    DIFF_VERIFIED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(all(test, target_arch = "aarch64", target_os = "macos"))]
mod tests {
    use super::*;

    /// Collect frame PCs via the SYSTEM unwinder (_Unwind_Backtrace) —
    /// the oracle the owned walk must match.
    fn system_pcs(max: usize) -> Vec<u64> {
        use core::ffi::{c_int, c_void};
        unsafe extern "C" {
            fn _Unwind_Backtrace(
                trace: extern "C" fn(*mut c_void, *mut c_void) -> c_int,
                arg: *mut c_void,
            ) -> c_int;
            fn _Unwind_GetIP(ctx: *mut c_void) -> u64;
        }
        extern "C" fn cb(ctx: *mut c_void, arg: *mut c_void) -> c_int {
            let v = unsafe { &mut *(arg as *mut Vec<u64>) };
            unsafe { v.push(_Unwind_GetIP(ctx)) };
            0
        }
        let mut v: Vec<u64> = Vec::with_capacity(max);
        unsafe {
            _Unwind_Backtrace(cb, &mut v as *mut Vec<u64> as *mut c_void);
        }
        v
    }

    /// W0 differential: the owned walk must reproduce the system
    /// unwinder's frame chain over the frames both can see.
    ///
    /// The two captures happen at slightly different depths (each helper
    /// adds its own frame), so compare from the first COMMON pc onward.
    #[test]
    #[inline(never)]
    fn owned_walk_matches_system_backtrace() {
        let Some(ours) = walk_pcs_from_here(64) else {
            panic!("walker unavailable on the primary dev platform");
        };
        let sys = system_pcs(80);
        assert!(
            ours.len() >= 4,
            "owned walk saw only {} frames: {:x?}",
            ours.len(),
            ours
        );
        // The two captures sit at different call sites inside this test
        // fn, so the chains only share frames from the test's CALLER
        // upward. Anchor on the first pc present in both, then require a
        // 1:1 match to the bottom of the shorter chain.
        let (i0, j0) = ours
            .iter()
            .enumerate()
            .find_map(|(i, p)| sys.iter().position(|q| q == p).map(|j| (i, j)))
            .unwrap_or_else(|| panic!("no common anchor\nours: {ours:x?}\nsys: {sys:x?}"));
        let common = (ours.len() - i0).min(sys.len() - j0);
        assert!(
            common >= 4,
            "too little overlap ({common})\nours: {ours:x?}\nsys: {sys:x?}"
        );
        for k in 0..common {
            assert_eq!(
                ours[i0 + k],
                sys[j0 + k],
                "divergence at frame {k}\nours: {ours:x?}\nsys:  {sys:x?}"
            );
        }
    }

    /// Cache behavior: a second walk over the same path must be served
    /// from the row cache (same result, and the cache is populated).
    #[test]
    #[inline(never)]
    fn second_walk_hits_cache_and_agrees() {
        let a = walk_pcs_from_here(32).expect("walker");
        let b = walk_pcs_from_here(32).expect("walker");
        // The chains differ only at THIS function's frame (two distinct
        // call sites → two return addresses); everything above it must be
        // identical.
        assert_eq!(a.len(), b.len());
        let diff: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
        assert!(
            diff.len() <= 1,
            "chains diverge beyond the caller frame: {diff:?}\na: {a:x?}\nb: {b:x?}"
        );
        let w = walker().unwrap();
        let cached = w.lock().unwrap().rows.len();
        assert!(cached >= a.len() - 1, "cache unpopulated: {cached}");
    }
}
