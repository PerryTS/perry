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

/// Callee-saved integer registers the walker tracks on aarch64, plus fp,
/// lr, sp. d8–d15 join in W2 (the install phase); stepping does not need
/// them (no CFA rule ever routes through a float register).
pub(crate) const N_TRACKED: usize = 13; // x19..x28, fp(x29), lr(x30), sp

/// Register context at a point in the walk. Indices: 0..=9 → x19..x28,
/// 10 → x29/fp, 11 → x30/lr, 12 → sp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WalkRegs {
    pub regs: [u64; N_TRACKED],
    pub pc: u64,
}

const FP: usize = 10;
const LR: usize = 11;
const SP: usize = 12;

/// Map a DWARF register number (aarch64) to a tracked index.
fn dwarf_to_idx(reg: u16) -> Option<usize> {
    match reg {
        19..=28 => Some(reg as usize - 19),
        29 => Some(FP),
        30 => Some(LR),
        31 => Some(SP),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Context capture (aarch64).
// ---------------------------------------------------------------------------

#[cfg(target_arch = "aarch64")]
mod capture {
    /// Layout must match the store order in `perry_eh_capture_context`:
    /// x19..x28, x29, x30, sp — then pc is filled by the caller from lr.
    #[repr(C)]
    pub struct RawCtx {
        pub x: [u64; 13],
    }

    unsafe extern "C" {
        /// Stores x19..x28, x29(fp), x30(lr), sp into `out`. Returns lr
        /// (the capture call site's return address) so the walk starts at
        /// our own caller.
        pub fn perry_eh_capture_context(out: *mut RawCtx) -> u64;
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
        "mov x0, x30",
        "ret",
    );
}

/// Capture the register context of OUR CALLER: the walk starts at the
/// function that invoked this one (its pc = our return address, its sp =
/// sp after our frame is popped — we have no frame, the asm is a leaf).
#[cfg(target_arch = "aarch64")]
#[inline(never)]
pub(crate) fn capture_here() -> WalkRegs {
    let mut raw = capture::RawCtx { x: [0; 13] };
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
        let bytes = unsafe { core::slice::from_raw_parts(eh as *const u8, size as usize) };
        return Some(EhFrameImage {
            eh_frame_addr: eh as u64,
            bytes,
            text_addr: text as u64,
            fde_index: Vec::new(),
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
        // Binary search the FDE index for the function containing pc.
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
        let mut ctx = UnwindContext::new();
        let row = fde
            .unwind_info_for_address(&self.eh_frame, &self.bases, &mut ctx, pc)
            .ok()?;
        let (cfa_reg, cfa_off) = match row.cfa() {
            CfaRule::RegisterAndOffset { register, offset } => (dwarf_to_idx(register.0)?, *offset),
            // DWARF expressions in the CFA rule: rare; fall back.
            CfaRule::Expression(_) => return None,
        };
        let mut reloads = Vec::new();
        for &(reg, ref rule) in row.registers() {
            let Some(idx) = dwarf_to_idx(reg.0) else {
                continue;
            };
            match rule {
                RegisterRule::Offset(off) => reloads.push((idx, *off)),
                RegisterRule::SameValue | RegisterRule::Undefined => {}
                // ValOffset/Register/Expression etc.: rare on aarch64
                // frame shapes; decline and let the system unwinder take
                // this throw.
                _ => return None,
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
    pub(crate) fn step(&mut self, regs: &WalkRegs) -> Option<WalkRegs> {
        // The stored pc is a return address: the relevant row is the one
        // covering the call instruction.
        let row = self.row_for(regs.pc.wrapping_sub(1))?;
        let cfa = (regs.regs[row.cfa_reg] as i64 + row.cfa_off) as u64;
        let mut next = *regs;
        for &(idx, off) in &row.reloads {
            let addr = (cfa as i64 + off) as u64;
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
    let mut pcs = Vec::with_capacity(max.min(64));
    while pcs.len() < max {
        pcs.push(regs.pc);
        match guard.step(&regs) {
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
