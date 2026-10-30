//! `Error.stack` capture through the unwind tables, for the one target whose
//! runtime frames carry no frame-pointer chain: x86_64 Linux.
//!
//! rustc leaves `rbp` a general-purpose callee-saved register on
//! `x86_64-unknown-linux-{gnu,musl}`, so a walk that follows `[rbp]` loses the
//! innermost JS frame (its return address sits in a runtime frame with no
//! record) and every JS frame directly beneath a runtime frame — the caller of
//! `Array.prototype.map`, `sort`, `JSON.parse`, a Promise job. The repo used to
//! paper over that with `-C force-frame-pointers=yes` on the whole runtime,
//! which costs ~3% of runtime instructions on real programs (tsc +2.6%, Zod
//! +3.4%). This walk needs nothing from the runtime but the `.eh_frame` tables
//! it already ships for the exception transport.
//!
//! Each frame is stepped by the row the image's own `.eh_frame` gives its
//! call site: one binary search of `.eh_frame_hdr` plus evaluating that FDE's
//! CFI up to the pc, read directly for the table shape LLVM emits
//! (`direct_row`) and by gimli for anything else. Nothing survives a capture
//! and nothing is allocated: within one capture each CIE is decoded once
//! (`CieMemo`, on the capture's stack), and the gimli unwind context, built
//! only when a row needs it, lives on this stack with a fixed, small rule
//! store. A pc
//! the tables do not cover at all (generated code compiled without an FDE)
//! steps as a frame-pointer frame, which every generated function is
//! (`"frame-pointer"="non-leaf"`, perry-codegen `function.rs`); a pc whose row
//! cannot be expressed in `rsp`/`rbp` ends the walk. Every read is bounded to
//! this thread's stack between the capture point and its top, so a misdecoded
//! row ends the walk instead of reading a fabricated address.
//!
//! macOS needs none of this (rustc's Apple targets keep frame pointers), and
//! AArch64 Linux keeps them by default (`frame-pointer: non-leaf`); both keep
//! the two-load chain walk in `error_stack_frames.rs`.

use super::*;

/// Fixed-size gimli rule storage: the walk evaluates one FDE at a time and a
/// Rust or generated frame saves at most the six SysV callee-saved registers
/// plus the return address. A row with more rules (or a deeper
/// remember/restore stack) fails to evaluate and ends the walk — it is never
/// truncated into a wrong row.
struct SmallStore;

const SMALL_RULES: usize = 16;
const SMALL_STACK: usize = 2;

impl<T: gimli::ReaderOffset> gimli::UnwindContextStorage<T> for SmallStore {
    type Rules = [(gimli::Register, RegisterRule<T>); SMALL_RULES];
    type Stack = [gimli::UnwindTableRow<T, Self>; SMALL_STACK];
}

/// DWARF register numbers on x86_64.
const DW_RBP: u16 = 6;
const DW_RSP: u16 = 7;
const DW_RA: u16 = 16;

/// How the frame at one call site steps to its caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    /// `cfa = (rbp if cfa_is_rbp else rsp) + cfa_off`; the caller's `rbp` is
    /// reloaded from `cfa + rbp_off` when the frame saved it, else unchanged;
    /// the return address is at `cfa + ra_off`, or the frame is the outermost
    /// one when the tables mark it undefined.
    Step {
        cfa_is_rbp: bool,
        cfa_off: i64,
        rbp_off: Option<i64>,
        ra_off: Option<i64>,
        /// Start of the function the FDE covers: the exact extent the JS
        /// name registry is keyed by.
        func_start: u64,
    },
    /// No image's tables cover the pc's function: step it as a frame record.
    NoFde,
    /// Covered, but not by a row this walk can follow.
    Stop,
}

/// Register state of one frame during the walk.
#[derive(Clone, Copy)]
struct Frame {
    pc: u64,
    rsp: u64,
    rbp: u64,
}

/// What one capture carries between frames: the CIEs it has decoded and the
/// gimli context, built on first use. Created per capture, on its stack.
struct TraceScratch {
    cies: CieMemo,
    ctx: Option<UnwindContext<usize, SmallStore>>,
}

impl TraceScratch {
    fn new() -> Self {
        TraceScratch {
            cies: CieMemo::default(),
            ctx: None,
        }
    }
}

impl Walker {
    fn trace_row(&self, site: u64, scratch: &mut TraceScratch) -> Row {
        let Some(image) = self.image_for(site) else {
            return Row::NoFde;
        };
        if let Some(row) = direct_row(&image.image, site, &mut scratch.cies) {
            return row;
        }
        let ctx = scratch.ctx.get_or_insert_with(UnwindContext::new_in);
        gimli_row(image, site, ctx)
    }
}

/// The general decoder: gimli over the same tables, for every shape
/// [`direct_row`] declines.
fn gimli_row(image: &WalkerImage, site: u64, ctx: &mut UnwindContext<usize, SmallStore>) -> Row {
    {
        let fde = if let Some(hdr) = image.image.hdr.as_ref() {
            let Some(table) = hdr.table() else {
                return Row::Stop;
            };
            match table.fde_for_address(
                &image.eh_frame,
                &image.bases,
                site,
                EhFrame::cie_from_offset,
            ) {
                Ok(fde) => fde,
                Err(_) => return Row::NoFde,
            }
        } else {
            let idx = &image.image.fde_index;
            let pos = idx.partition_point(|e| e.0 <= site);
            if pos == 0 || site >= idx[pos - 1].1 {
                return Row::NoFde;
            }
            match image.eh_frame.fde_from_offset(
                &image.bases,
                idx[pos - 1].2,
                EhFrame::cie_from_offset,
            ) {
                Ok(fde) => fde,
                Err(_) => return Row::Stop,
            }
        };
        let Ok(row) = fde.unwind_info_for_address(&image.eh_frame, &image.bases, ctx, site) else {
            return Row::Stop;
        };
        let (cfa_is_rbp, cfa_off) = match row.cfa() {
            CfaRule::RegisterAndOffset { register, offset } => match register.0 {
                DW_RSP => (false, *offset),
                DW_RBP => (true, *offset),
                _ => return Row::Stop,
            },
            CfaRule::Expression(_) => return Row::Stop,
        };
        let rbp_off = match row.register(gimli::Register(DW_RBP)) {
            None | Some(RegisterRule::SameValue) => None,
            Some(RegisterRule::Offset(off)) => Some(off),
            Some(_) => return Row::Stop,
        };
        let ra_off = match row.register(gimli::Register(DW_RA)) {
            Some(RegisterRule::Offset(off)) => Some(off),
            Some(RegisterRule::Undefined) => None,
            // x86_64 CIEs always define the return-address column; a row
            // that leaves it implicit or computes it is not one to follow.
            _ => return Row::Stop,
        };
        Row::Step {
            cfa_is_rbp,
            cfa_off,
            rbp_off,
            ra_off,
            func_start: fde.initial_address(),
        }
    }
}

/// Read the word at `addr` if it lies inside `[low, top)` and is aligned.
#[inline(always)]
fn read_stack(addr: u64, low: u64, top: u64) -> Option<u64> {
    if addr < low || addr.checked_add(8)? > top || addr % 8 != 0 {
        return None;
    }
    // SAFETY: `addr..addr+8` lies inside this thread's live stack, above the
    // capturing frame.
    Some(unsafe { core::ptr::read(addr as *const u64) })
}

/// Step frame `f` by `row` (the row of its call site) to its caller.
#[inline(always)]
fn apply(row: Row, f: Frame, top: u64, low: u64) -> Option<Frame> {
    let (cfa, rbp, ra) = match row {
        Row::Step {
            cfa_is_rbp,
            cfa_off,
            rbp_off,
            ra_off,
            ..
        } => {
            let base = if cfa_is_rbp { f.rbp } else { f.rsp };
            let cfa = (base as i64).checked_add(cfa_off)? as u64;
            if cfa <= f.rsp || cfa > top {
                return None;
            }
            let rbp = match rbp_off {
                Some(off) => read_stack((cfa as i64).checked_add(off)? as u64, low, top)?,
                None => f.rbp,
            };
            let ra = read_stack((cfa as i64).checked_add(ra_off?)? as u64, low, top)?;
            (cfa, rbp, ra)
        }
        Row::NoFde => {
            // A frame record: [rbp] = caller's rbp, [rbp+8] = return address.
            if f.rbp < f.rsp {
                return None;
            }
            let cfa = f.rbp.checked_add(16)?;
            let rbp = read_stack(f.rbp, low, top)?;
            let ra = read_stack(f.rbp + 8, low, top)?;
            (cfa, rbp, ra)
        }
        Row::Stop => return None,
    };
    if ra == 0 {
        return None;
    }
    Some(Frame {
        pc: ra,
        rsp: cfa,
        rbp,
    })
}

/// Collect the return addresses of the CALLER's frames, innermost first: the
/// first entry is the return address into the function that called this one,
/// exactly what the frame-pointer walk's first record yields. `top` is the
/// exclusive top of this thread's stack. Returns the count written; 0 when the
/// image has no unwind tables at all. Every frame is kept: this is the
/// diagnostics walk (`capture_ips`) and the differential tests' subject.
#[inline(never)]
pub(crate) fn capture_return_addresses(out: &mut [usize], top: usize) -> usize {
    let regs = capture_here();
    let Some(walker) = locked_walker() else {
        return 0;
    };
    let top = top as u64;
    let low = regs.regs[SP];
    let mut scratch = TraceScratch::new();
    let mut frame = Frame {
        pc: regs.pc,
        rsp: regs.regs[SP],
        rbp: regs.regs[FP],
    };
    let mut n = 0usize;
    while n < out.len() {
        // The pc is a return address; the row that describes the frame is
        // the one covering the call instruction.
        let row = walker.trace_row(frame.pc.wrapping_sub(1), &mut scratch);
        let Some(next) = apply(row, frame, top, low) else {
            break;
        };
        out[n] = next.pc as usize;
        n += 1;
        frame = next;
    }
    n
}

/// The C entry point of the executable, when it has one: frames beneath it
/// are the C runtime's (`__libc_start_main`, `_start`), never JS, so the
/// `Error` walk ends there instead of decoding them. Weak, so an image
/// without `main` (a test harness loaded some other way, a shared object)
/// links and simply walks to the bottom of the stack.
#[cfg(not(test))]
pub(crate) fn executable_main() -> u64 {
    extern "C" {
        #[linkage = "extern_weak"]
        static main: *const u8;
    }
    // SAFETY: an extern_weak static reads as the symbol's address, or null.
    unsafe { main as u64 }
}

// Unit tests run on worker threads with no executable entry beneath them.
// Rust synthesizes the test harness's main in this crate; redeclaring that
// entry as weak data conflicts with its generated function during LTO.
#[cfg(test)]
pub(crate) fn executable_main() -> u64 {
    0
}

/// The `Error` capture: walk outward from `origin` — the register context a
/// still-live frame captured with [`capture_here`] (the error constructor's
/// own frame, so none of the capture's helper frames is ever stepped) — and
/// keep only the frames `is_js(pc, function_start)` accepts, until `out` is
/// full. `function_start` is the start of the function the unwind tables
/// say covers the call site, or 0 for a frame no table covers (stepped as a
/// frame record; the caller decides).
///
/// Each frame costs one `.eh_frame_hdr` search, which names its function.
/// A JS frame then steps as the frame record every generated function keeps
/// (`"frame-pointer"="non-leaf"`, the property the collector's own chain
/// walk relies on): at a call site, `cfa = rbp + 16`, with the caller's `rbp`
/// and the return address beneath it. Only a runtime frame pays for its FDE
/// row. The walk stops the moment the last wanted JS frame is found, so it
/// never decodes the frames beneath `Error.stackTraceLimit`, nor those
/// beneath the executable's `main`.
#[inline(never)]
pub(crate) fn capture_js_return_addresses(
    origin: WalkRegs,
    out: &mut [usize],
    top: usize,
    mut is_js: impl FnMut(u64, u64) -> bool,
) -> usize {
    if out.is_empty() {
        return 0;
    }
    let Some(walker) = locked_walker() else {
        return 0;
    };
    let top = top as u64;
    let low = origin.regs[SP];
    let mut scratch = TraceScratch::new();
    let mut frame = Frame {
        pc: origin.pc,
        rsp: origin.regs[SP],
        rbp: origin.regs[FP],
    };
    let main = executable_main();
    let mut n = 0usize;
    let mut at_origin = true;
    loop {
        // The pc is a return address; the call instruction is the site.
        let site = frame.pc.wrapping_sub(1);
        let image = walker.image_for(site);
        // `Some(Some(..))`: the function's start and FDE; `Some(None)`: no
        // table covers the site; `None`: only gimli can read this image.
        let covering = match image {
            Some(image) => hdr_function(&image.image.cfi_tables(), site),
            None => Some(None),
        };
        let start = match covering {
            Some(Some((start, _))) => Some(start),
            Some(None) => Some(0),
            None => None,
        };
        let classify = !at_origin;
        at_origin = false;
        let js = classify && start.is_some_and(|start| is_js(frame.pc, start));
        if js {
            out[n] = frame.pc as usize;
            n += 1;
            if n == out.len() {
                break;
            }
        }
        if main != 0 && start == Some(main) {
            break;
        }
        let row = match (image, covering) {
            _ if js => Row::NoFde,
            (Some(image), Some(Some((start, fde)))) => {
                match fde_row(
                    &image.image.cfi_tables(),
                    site,
                    start,
                    fde,
                    &mut scratch.cies,
                ) {
                    Some(row) => row,
                    None => gimli_row(
                        image,
                        site,
                        scratch.ctx.get_or_insert_with(UnwindContext::new_in),
                    ),
                }
            }
            (Some(image), None) => {
                let row = gimli_row(
                    image,
                    site,
                    scratch.ctx.get_or_insert_with(UnwindContext::new_in),
                );
                if let Row::Step { func_start, .. } = row {
                    if classify && is_js(frame.pc, func_start) {
                        out[n] = frame.pc as usize;
                        n += 1;
                        if n == out.len() {
                            break;
                        }
                    }
                    if main != 0 && func_start == main {
                        break;
                    }
                }
                row
            }
            _ => Row::NoFde,
        };
        let Some(next) = apply(row, frame, top, low) else {
            break;
        };
        frame = next;
    }
    n
}

// ---------------------------------------------------------------------------
// The direct decoder: the table search and the CFI program, without gimli.
//
// gimli is general and pays for it per frame (measured: ~2.6k instructions,
// 92% of an uncached capture in its pointer-encoding, CIE-parsing and
// instruction-iterator layers). The tables this walk reads come from one
// producer, LLVM, in one shape: a `.eh_frame_hdr` whose search table is
// `datarel|sdata4` pairs, CIEs with a `z` augmentation, and FDE programs made
// of the handful of opcodes a prologue needs. This decoder reads exactly that
// shape and answers `None` for anything else, which hands the frame to gimli —
// so an unusual table costs speed, never correctness.
// ---------------------------------------------------------------------------

/// Bounds-checked little-endian cursor over a section.
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    #[inline(always)]
    fn new(bytes: &'a [u8], at: usize) -> Self {
        Cursor { bytes, at }
    }
    #[inline(always)]
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let end = self.at.checked_add(N)?;
        let out: [u8; N] = self.bytes.get(self.at..end)?.try_into().ok()?;
        self.at = end;
        Some(out)
    }
    #[inline(always)]
    fn u8(&mut self) -> Option<u8> {
        let b = *self.bytes.get(self.at)?;
        self.at += 1;
        Some(b)
    }
    #[inline(always)]
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take()?))
    }
    #[inline(always)]
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take()?))
    }
    /// One-byte values (every offset and register a prologue names) take
    /// the first branch; longer ones the loop.
    #[inline(always)]
    fn uleb(&mut self) -> Option<u64> {
        let first = self.u8()?;
        if first & 0x80 == 0 {
            return Some(first as u64);
        }
        self.at -= 1;
        self.uleb_long()
    }
    #[inline(never)]
    fn uleb_long(&mut self) -> Option<u64> {
        let mut out = 0u64;
        let mut shift = 0u32;
        loop {
            let b = self.u8()?;
            if shift >= 64 {
                return None;
            }
            out |= ((b & 0x7f) as u64) << shift;
            shift += 7;
            if b & 0x80 == 0 {
                return Some(out);
            }
        }
    }
    #[inline(always)]
    fn sleb(&mut self) -> Option<i64> {
        let first = self.u8()?;
        if first & 0x80 == 0 {
            // Sign-extend the 7-bit value.
            return Some(((first as i64) << 57) >> 57);
        }
        self.at -= 1;
        self.sleb_long()
    }
    #[inline(never)]
    fn sleb_long(&mut self) -> Option<i64> {
        let mut out = 0i64;
        let mut shift = 0u32;
        loop {
            let b = self.u8()?;
            if shift >= 64 {
                return None;
            }
            out |= ((b & 0x7f) as i64) << shift;
            shift += 7;
            if b & 0x80 == 0 {
                if shift < 64 && b & 0x40 != 0 {
                    out |= -1i64 << shift;
                }
                return Some(out);
            }
        }
    }
    #[inline(always)]
    fn skip(&mut self, n: usize) -> Option<()> {
        let end = self.at.checked_add(n)?;
        if end > self.bytes.len() {
            return None;
        }
        self.at = end;
        Some(())
    }
}

/// Byte size of a pointer under DWARF encoding `enc` (its value format; the
/// application bits do not change the size). `None` for a format this
/// decoder does not read (LEB128 pointers, `omit`).
fn encoded_size(enc: u8) -> Option<usize> {
    if enc & 0x70 == 0x50 {
        return None; // DW_EH_PE_aligned pads to the word: not read here.
    }
    match enc & 0x0f {
        0x00 | 0x04 | 0x0c => Some(8),
        0x03 | 0x0b => Some(4),
        0x02 | 0x0a => Some(2),
        _ => None,
    }
}

/// A rule for one tracked register during interpretation.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Rule {
    /// Unchanged from the callee (`same_value`, or never mentioned).
    Same,
    /// The outermost frame's return address.
    Undefined,
    /// Saved at `cfa + n`.
    At(i64),
}

#[derive(Clone, Copy)]
struct State {
    cfa_reg: u64,
    cfa_off: i64,
    rbp: Rule,
    ra: Rule,
}

/// Run one CFI program over `[c.at, end)` from `state`, stopping before the
/// first row that starts above `target`. `loc` is the address the program's
/// first row describes. `initial` is the CIE's state, for `restore`.
#[allow(clippy::too_many_arguments)]
fn run_cfi(
    c: &mut Cursor<'_>,
    end: usize,
    code_align: u64,
    data_align: i64,
    ra_reg: u64,
    mut loc: u64,
    target: u64,
    state: &mut State,
    initial: &State,
) -> Option<()> {
    let mut stack = [*state; 2];
    let mut depth = 0usize;
    let set = |state: &mut State, reg: u64, rule: Rule| {
        if reg == DW_RBP as u64 {
            state.rbp = rule;
        } else if reg == ra_reg {
            state.ra = rule;
        }
    };
    let restore = |state: &mut State, reg: u64| {
        if reg == DW_RBP as u64 {
            state.rbp = initial.rbp;
        } else if reg == ra_reg {
            state.ra = initial.ra;
        }
    };
    while c.at < end {
        let op = c.u8()?;
        let advance = match op >> 6 {
            1 => Some((op & 0x3f) as u64),
            2 => {
                let off = c.uleb()? as i64 * data_align;
                set(state, (op & 0x3f) as u64, Rule::At(off));
                None
            }
            3 => {
                restore(state, (op & 0x3f) as u64);
                None
            }
            _ => match op {
                0x00 => None,
                0x02 => Some(c.u8()? as u64),
                0x03 => Some(c.u16()? as u64),
                0x04 => Some(c.u32()? as u64),
                0x05 => {
                    let reg = c.uleb()?;
                    let off = c.uleb()? as i64 * data_align;
                    set(state, reg, Rule::At(off));
                    None
                }
                0x06 => {
                    let reg = c.uleb()?;
                    restore(state, reg);
                    None
                }
                0x07 => {
                    let reg = c.uleb()?;
                    set(state, reg, Rule::Undefined);
                    None
                }
                0x08 => {
                    let reg = c.uleb()?;
                    set(state, reg, Rule::Same);
                    None
                }
                0x0a => {
                    if depth == stack.len() {
                        return None;
                    }
                    stack[depth] = *state;
                    depth += 1;
                    None
                }
                0x0b => {
                    depth = depth.checked_sub(1)?;
                    // The CFA rule is part of the remembered state, as in
                    // libgcc and gimli: LLVM brackets each epilogue with
                    // remember/restore and relies on it.
                    *state = stack[depth];
                    None
                }
                0x0c => {
                    state.cfa_reg = c.uleb()?;
                    state.cfa_off = c.uleb()? as i64;
                    None
                }
                0x0d => {
                    state.cfa_reg = c.uleb()?;
                    None
                }
                0x0e => {
                    state.cfa_off = c.uleb()? as i64;
                    None
                }
                0x11 => {
                    let reg = c.uleb()?;
                    let off = c.sleb()? * data_align;
                    set(state, reg, Rule::At(off));
                    None
                }
                0x12 => {
                    state.cfa_reg = c.uleb()?;
                    state.cfa_off = c.sleb()? * data_align;
                    None
                }
                0x13 => {
                    state.cfa_off = c.sleb()? * data_align;
                    None
                }
                0x2e => {
                    c.uleb()?; // GNU_args_size: no effect on the step.
                    None
                }
                // set_loc, register, expressions, val_*, vendor ops: not a
                // shape this decoder reads.
                _ => return None,
            },
        };
        if let Some(delta) = advance {
            let next = loc.checked_add(delta.checked_mul(code_align)?)?;
            if next > target {
                return Some(());
            }
            loc = next;
        }
    }
    Some(())
}

/// One decoded CIE: everything an FDE's row needs from it.
#[derive(Clone, Copy)]
struct Cie {
    /// Identity: the `.eh_frame` section it lives in and its offset there.
    section: usize,
    off: usize,
    code_align: u64,
    data_align: i64,
    /// Byte size of the FDE's `pc_begin` / `pc_range` (`R` augmentation).
    ptr_size: usize,
    /// The FDE carries augmentation data (`z`).
    has_aug: bool,
    /// The row the CIE's initial instructions leave.
    initial: State,
}

/// The CIEs one capture has decoded. LLVM gives an image's FDEs one shared
/// CIE (two when some functions carry a personality), so after the first
/// frame in each image the CIE costs a compare. Lives on the capture's stack
/// and dies with it: no state survives a capture.
#[derive(Default)]
struct CieMemo {
    slots: [Option<Cie>; 4],
    next: usize,
}

impl CieMemo {
    #[inline(always)]
    fn get(&mut self, eh: &[u8], off: usize) -> Option<Cie> {
        let section = eh.as_ptr() as usize;
        for cie in self.slots.iter().flatten() {
            if cie.off == off && cie.section == section {
                return Some(*cie);
            }
        }
        let cie = parse_cie(eh, off)?;
        self.slots[self.next] = Some(cie);
        self.next = (self.next + 1) % self.slots.len();
        Some(cie)
    }
}

/// Decode the CIE at `off` of `.eh_frame` `eh`, or `None` for a shape this
/// decoder does not read.
#[inline(never)]
fn parse_cie(eh: &[u8], off: usize) -> Option<Cie> {
    let mut k = Cursor::new(eh, off);
    let cie_len = k.u32()?;
    if cie_len == 0 || cie_len == u32::MAX {
        return None;
    }
    let cie_end = k.at.checked_add(cie_len as usize)?;
    if cie_end > eh.len() || k.u32()? != 0 {
        return None;
    }
    let version = k.u8()?;
    if version != 1 && version != 3 {
        return None;
    }
    let aug_at = k.at;
    let aug_len = eh.get(aug_at..cie_end)?.iter().position(|b| *b == 0)?;
    let aug = &eh[aug_at..aug_at + aug_len];
    k.at = aug_at + aug_len + 1;
    let code_align = k.uleb()?;
    let data_align = k.sleb()?;
    let ra_reg = if version == 1 {
        k.u8()? as u64
    } else {
        k.uleb()?
    };
    if ra_reg != DW_RA as u64 {
        return None;
    }
    let mut fde_enc = 0u8; // absptr unless 'R' says otherwise
    let mut has_aug = false;
    if aug.first() == Some(&b'z') {
        has_aug = true;
        let aug_data_len = k.uleb()? as usize;
        let aug_data_end = k.at.checked_add(aug_data_len)?;
        for ch in &aug[1..] {
            match ch {
                b'R' => fde_enc = k.u8()?,
                b'L' => {
                    k.u8()?;
                }
                b'P' => {
                    let enc = k.u8()?;
                    k.skip(encoded_size(enc)?)?;
                }
                b'S' => {}
                _ => return None,
            }
        }
        if k.at > aug_data_end {
            return None;
        }
        k.at = aug_data_end;
    } else if !aug.is_empty() {
        return None;
    }
    let ptr_size = encoded_size(fde_enc)?;
    let mut initial = State {
        cfa_reg: DW_RSP as u64,
        cfa_off: 0,
        rbp: Rule::Same,
        ra: Rule::Undefined,
    };
    let blank = initial;
    run_cfi(
        &mut k,
        cie_end,
        code_align,
        data_align,
        ra_reg,
        0,
        u64::MAX,
        &mut initial,
        &blank,
    )?;
    Some(Cie {
        section: eh.as_ptr() as usize,
        off,
        code_align,
        data_align,
        ptr_size,
        has_aug,
        initial,
    })
}

/// The `.eh_frame_hdr` search: the start (header-relative) and FDE address
/// (header-relative) of the last table entry whose function starts at or
/// below `rel`. `None` for a header layout this decoder does not read;
/// `Some(None)` when no entry starts at or below `rel`.
#[inline(always)]
fn hdr_search(hdr: &[u8], rel: i64) -> Option<Option<(i64, i64)>> {
    // version 1; eh_frame_ptr pcrel|sdata4; fde_count udata4; table
    // datarel|sdata4 — the layout lld, gold and bfd all write.
    if hdr.len() < 12 || hdr[0] != 1 || hdr[1] != 0x1b || hdr[2] != 0x03 || hdr[3] != 0x3b {
        return None;
    }
    let count = u32::from_le_bytes([hdr[8], hdr[9], hdr[10], hdr[11]]) as usize;
    let table = hdr.get(12..12usize.checked_add(count.checked_mul(8)?)?)?;
    if count == 0 {
        return Some(None);
    }
    let base = table.as_ptr();
    // SAFETY: `i < count` and `table` holds `count` 8-byte entries.
    let start =
        |i: usize| -> i64 { unsafe { (base.add(i * 8) as *const i32).read_unaligned() as i64 } };
    // Branch-free lower bound: the answer stays in [lo, lo + size).
    let mut lo = 0usize;
    let mut size = count;
    while size > 1 {
        let half = size / 2;
        let mid = lo + half;
        lo = if start(mid) <= rel { mid } else { lo };
        size -= half;
    }
    if start(lo) > rel {
        return Some(None);
    }
    // SAFETY: as above.
    let fde = unsafe { (base.add(lo * 8 + 4) as *const i32).read_unaligned() as i64 };
    Some(Some((start(lo), fde)))
}

/// The row for `site` read directly from `image`'s tables, or `None` to let
/// gimli decide (any table shape other than the one LLVM emits).
fn direct_row(image: &EhFrameImage, site: u64, cies: &mut CieMemo) -> Option<Row> {
    let tables = image.cfi_tables();
    let Some((func_start, fde_addr)) = hdr_function(&tables, site)? else {
        return Some(Row::NoFde);
    };
    fde_row(&tables, site, func_start, fde_addr, cies)
}

/// The two tables the direct decoder reads, as plain slices: an image the
/// `Error` walker enumerated, or the object `_dl_find_object` names for the
/// collector's frame walk (`gc_cfi_step`).
#[derive(Clone, Copy)]
struct CfiTables {
    /// `.eh_frame`, from its first byte to (at most) the end of its object.
    eh: &'static [u8],
    eh_frame_addr: u64,
    /// `.eh_frame_hdr`, likewise bounded by its object.
    hdr: &'static [u8],
    hdr_addr: u64,
}

impl EhFrameImage {
    fn cfi_tables(&self) -> CfiTables {
        CfiTables {
            eh: self.bytes,
            eh_frame_addr: self.eh_frame_addr,
            hdr: self.hdr_bytes,
            hdr_addr: self.hdr_addr,
        }
    }
}

/// The function `.eh_frame_hdr` says covers `site`: `(start, FDE address)`;
/// `Some(None)` when no table entry starts at or below it; `None` for a
/// header layout this decoder does not read.
#[inline(always)]
fn hdr_function(tables: &CfiTables, site: u64) -> Option<Option<(u64, u64)>> {
    let rel = site as i64 - tables.hdr_addr as i64;
    let Some((start_rel, fde_rel)) = hdr_search(tables.hdr, rel)? else {
        return Some(None);
    };
    Some(Some((
        (tables.hdr_addr as i64).checked_add(start_rel)? as u64,
        (tables.hdr_addr as i64).checked_add(fde_rel)? as u64,
    )))
}

/// The FDE at `fde_addr`, read up to its program: a cursor at the first
/// instruction, the FDE's end, its `pc_range` and its CIE.
fn fde_header(
    tables: &CfiTables,
    fde_addr: u64,
    cies: &mut CieMemo,
) -> Option<(Cursor<'static>, usize, u64, Cie)> {
    let eh = tables.eh;
    let fde_off = usize::try_from(fde_addr.checked_sub(tables.eh_frame_addr)?).ok()?;
    let mut c = Cursor::new(eh, fde_off);
    let len = c.u32()?;
    if len == 0 || len == u32::MAX {
        return None;
    }
    let fde_end = c.at.checked_add(len as usize)?;
    if fde_end > eh.len() {
        return None;
    }
    let cie_ptr_at = c.at;
    let cie_off = cie_ptr_at.checked_sub(c.u32()? as usize)?;
    let cie = cies.get(eh, cie_off)?;
    // pc_begin (already known from the table), then pc_range.
    c.skip(cie.ptr_size)?;
    let range = match cie.ptr_size {
        8 => u64::from_le_bytes(c.take()?),
        4 => c.u32()? as u64,
        _ => c.u16()? as u64,
    };
    Some((c, fde_end, range, cie))
}

/// The row at `site` from the FDE at `fde_addr` (covering the function at
/// `func_start`), or `None` to let gimli decide.
fn fde_row(
    tables: &CfiTables,
    site: u64,
    func_start: u64,
    fde_addr: u64,
    cies: &mut CieMemo,
) -> Option<Row> {
    let (mut c, fde_end, range, cie) = fde_header(tables, fde_addr, cies)?;
    if site >= func_start.checked_add(range)? {
        return Some(Row::NoFde);
    }
    if cie.has_aug {
        let n = c.uleb()? as usize;
        c.skip(n)?;
    }

    let mut state = cie.initial;
    run_cfi(
        &mut c,
        fde_end,
        cie.code_align,
        cie.data_align,
        DW_RA as u64,
        func_start,
        site,
        &mut state,
        &cie.initial,
    )?;

    let cfa_is_rbp = match state.cfa_reg {
        r if r == DW_RSP as u64 => false,
        r if r == DW_RBP as u64 => true,
        _ => return None,
    };
    Some(Row::Step {
        cfa_is_rbp,
        cfa_off: state.cfa_off,
        rbp_off: match state.rbp {
            Rule::At(off) => Some(off),
            Rule::Same => None,
            Rule::Undefined => return None,
        },
        ra_off: match state.ra {
            Rule::At(off) => Some(off),
            Rule::Undefined => None,
            Rule::Same => return None,
        },
        func_start,
    })
}

// ---------------------------------------------------------------------------
// The collector's CFI step (`gc/roots/stack_maps_frame_walk.rs`).
//
// The root scan steps a Rust, shim or foreign frame by its row here, and
// nothing else: JS frames step by their frame record and never reach this.
// The object is found by `_dl_find_object` (the loader's lock-free lookup over
// the loaded objects, the one libgcc's unwinder uses), so the walk takes no
// lock, keeps no image list and needs no rebuild when objects come and go.
// Every row is read directly; a shape the direct decoder declines is
// `Unreadable`, and the collector then fails closed to the system unwinder.
// ---------------------------------------------------------------------------

/// glibc's `struct dl_find_object` on x86-64 (no `dlfo_eh_dbase` there).
#[cfg(target_env = "gnu")]
#[repr(C)]
struct DlFindObject {
    flags: u64,
    map_start: *mut core::ffi::c_void,
    map_end: *mut core::ffi::c_void,
    link_map: *mut core::ffi::c_void,
    eh_frame: *mut core::ffi::c_void,
    reserved: [u64; 7],
}

#[cfg(target_env = "gnu")]
type DlFindObjectFn =
    unsafe extern "C" fn(*mut core::ffi::c_void, *mut DlFindObject) -> core::ffi::c_int;

/// `_dl_find_object` (glibc 2.35), weak: an older loader leaves it null and
/// every collector walk takes the system unwinder, as before.
#[cfg(target_env = "gnu")]
fn dl_find_object() -> Option<DlFindObjectFn> {
    extern "C" {
        #[linkage = "extern_weak"]
        static _dl_find_object: *const u8;
    }
    // SAFETY: an extern_weak static reads as the symbol's address, or null;
    // a non-null address is glibc's function of exactly this signature.
    let address = unsafe { _dl_find_object };
    (!address.is_null())
        .then(|| unsafe { core::mem::transmute::<*const u8, DlFindObjectFn>(address) })
}

/// The tables of the object containing `pc`. `None`: no loaded object holds
/// `pc`, the object has no `.eh_frame_hdr`, or the loader cannot say.
#[cfg(target_env = "gnu")]
fn tables_for(pc: u64) -> Option<CfiTables> {
    let find = dl_find_object()?;
    // SAFETY: plain-old-data out parameter.
    let mut info: DlFindObject = unsafe { core::mem::zeroed() };
    if unsafe { find(pc as *mut core::ffi::c_void, &mut info) } != 0 {
        return None;
    }
    let (start, end) = (info.map_start as u64, info.map_end as u64);
    let hdr_addr = info.eh_frame as u64;
    if hdr_addr < start || hdr_addr >= end {
        return None;
    }
    // SAFETY: inside the object's mapping; every read below is bounded by
    // the header's own count and the FDE lengths, all checked against this.
    let hdr: &'static [u8] =
        unsafe { core::slice::from_raw_parts(hdr_addr as *const u8, (end - hdr_addr) as usize) };
    // eh_frame_ptr is pcrel|sdata4 (`hdr_search` checks the remaining bytes).
    if hdr.len() < 12 || hdr[0] != 1 || hdr[1] != 0x1b {
        return None;
    }
    let rel = i32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]) as i64;
    let eh_frame_addr = (hdr_addr as i64).checked_add(4)?.checked_add(rel)? as u64;
    if eh_frame_addr < start || eh_frame_addr >= end {
        return None;
    }
    // SAFETY: as above.
    let eh: &'static [u8] = unsafe {
        core::slice::from_raw_parts(eh_frame_addr as *const u8, (end - eh_frame_addr) as usize)
    };
    Some(CfiTables {
        eh,
        eh_frame_addr,
        hdr,
        hdr_addr,
    })
}

/// One CFI step of the collector's walk.
#[cfg(target_env = "gnu")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GcCfiStep {
    /// The caller's frame: its pc (a return address), its sp at the call
    /// (this frame's CFA) and its rbp.
    Caller { pc: u64, rsp: u64, rbp: u64 },
    /// The tables mark this frame the outermost one (undefined return
    /// address: `_start`, a thread's `clone3`).
    Outermost,
    /// No table covers the pc, or not with a row this step can follow.
    Unreadable,
}

/// The CIEs one collector walk has decoded; on the walk's stack, gone after.
#[cfg(target_env = "gnu")]
#[derive(Default)]
pub(crate) struct GcCfiScratch {
    cies: CieMemo,
}

/// Step the frame whose pc is the return address `pc` (with `rsp` its sp at
/// that call and `rbp` its rbp) to its caller. Reads stay in `[low, top)`.
#[cfg(target_env = "gnu")]
pub(crate) fn gc_cfi_step(
    pc: u64,
    rsp: u64,
    rbp: u64,
    low: u64,
    top: u64,
    scratch: &mut GcCfiScratch,
) -> GcCfiStep {
    // The pc is a return address; the row that describes the frame is the
    // one covering the call instruction.
    let site = pc.wrapping_sub(1);
    let Some(tables) = tables_for(site) else {
        return GcCfiStep::Unreadable;
    };
    let Some(Some((start, fde))) = hdr_function(&tables, site) else {
        return GcCfiStep::Unreadable;
    };
    let Some(row) = fde_row(&tables, site, start, fde, &mut scratch.cies) else {
        return GcCfiStep::Unreadable;
    };
    match row {
        Row::Step { ra_off: None, .. } => GcCfiStep::Outermost,
        Row::Step { .. } => match apply(row, Frame { pc, rsp, rbp }, top, low) {
            Some(caller) => GcCfiStep::Caller {
                pc: caller.pc,
                rsp: caller.rsp,
                rbp: caller.rbp,
            },
            None => GcCfiStep::Unreadable,
        },
        Row::NoFde | Row::Stop => GcCfiStep::Unreadable,
    }
}

/// `(start, end)` of the function the unwind tables say contains `addr`.
#[cfg(target_env = "gnu")]
pub(crate) fn cfi_function_extent(addr: u64) -> Option<(u64, u64)> {
    let tables = tables_for(addr)?;
    let (start, fde) = hdr_function(&tables, addr)??;
    let (_, _, range, _) = fde_header(&tables, fde, &mut CieMemo::default())?;
    let end = start.checked_add(range)?;
    (addr < end).then_some((start, end))
}

/// The module that holds `pc`, for a diagnostic: its path, or `None`.
#[cfg(target_env = "gnu")]
pub(crate) fn module_name(pc: u64) -> Option<String> {
    // SAFETY: plain-old-data out parameter; dladdr only reads loader state.
    let mut info: libc::Dl_info = unsafe { core::mem::zeroed() };
    if unsafe { libc::dladdr(pc as *const core::ffi::c_void, &mut info) } == 0
        || info.dli_fname.is_null()
    {
        return None;
    }
    Some(
        unsafe { std::ffi::CStr::from_ptr(info.dli_fname) }
            .to_string_lossy()
            .into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The direct decoder must give gimli's row at every pc it answers for,
    /// and must answer for nearly every pc: a decoder that silently declined
    /// everything would pass the first half and cost the whole speedup.
    /// Sampled across every FDE of this test binary (Rust code, the runtime's
    /// own shapes): the start, a few prologue offsets, the middle, the end.
    #[test]
    fn direct_rows_match_gimli_across_every_fde() {
        let walker = locked_walker().expect("walker");
        let mut ctx: UnwindContext<usize, SmallStore> = UnwindContext::new_in();
        let mut cies = CieMemo::default();
        let (mut answered, mut declined, mut fdes) = (0usize, 0usize, 0usize);
        for image in &walker.images {
            let hdr = image.image.hdr_bytes;
            if hdr.len() < 12 {
                continue;
            }
            let count = u32::from_le_bytes(hdr[8..12].try_into().unwrap()) as usize;
            for i in 0..count {
                let at = 12 + i * 8;
                let start_rel = i32::from_le_bytes(hdr[at..at + 4].try_into().unwrap()) as i64;
                let start = (image.image.hdr_addr as i64 + start_rel) as u64;
                let next = if i + 1 < count {
                    let n = i32::from_le_bytes(hdr[at + 8..at + 12].try_into().unwrap()) as i64;
                    (image.image.hdr_addr as i64 + n) as u64
                } else {
                    start + 64
                };
                fdes += 1;
                let span = next.saturating_sub(start).max(1);
                for off in [0, 1, 2, 4, 5, 8, 12, 16, 24, span / 2, span - 1] {
                    let site = start + off.min(span - 1);
                    match direct_row(&image.image, site, &mut cies) {
                        Some(row) => {
                            answered += 1;
                            let gimli = gimli_row(image, site, &mut ctx);
                            // gimli ends the walk on a few rows the direct
                            // decoder declines differently; every row gimli
                            // can step must agree exactly.
                            if gimli != Row::Stop {
                                assert_eq!(row, gimli, "site {site:#x} (fde start {start:#x})");
                            }
                        }
                        None => declined += 1,
                    }
                }
            }
        }
        assert!(fdes > 1000, "only {fdes} FDEs sampled");
        assert!(
            declined * 100 <= answered,
            "the direct decoder declined {declined} of {} sites",
            answered + declined
        );
    }
}
