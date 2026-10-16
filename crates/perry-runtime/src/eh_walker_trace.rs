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
//! (`direct_row`) and by gimli for anything else. Nothing is cached and
//! nothing is allocated: the gimli unwind context lives on this stack with a
//! fixed, small rule store. A pc
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

impl Walker {
    fn trace_row(&self, site: u64, ctx: &mut UnwindContext<usize, SmallStore>) -> Row {
        let Some(image) = self.image_for(site) else {
            return Row::NoFde;
        };
        if let Some(row) = direct_row(&image.image, site) {
            return row;
        }
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

fn step(
    walker: &Walker,
    ctx: &mut UnwindContext<usize, SmallStore>,
    f: Frame,
    top: u64,
    low: u64,
) -> Option<Frame> {
    // The pc is a return address; the row that describes the frame is the
    // one covering the call instruction.
    let (cfa, rbp, ra) = match walker.trace_row(f.pc.wrapping_sub(1), ctx) {
        Row::Step {
            cfa_is_rbp,
            cfa_off,
            rbp_off,
            ra_off,
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
/// image has no unwind tables at all.
#[inline(never)]
pub(crate) fn capture_return_addresses(out: &mut [usize], top: usize) -> usize {
    let regs = capture_here();
    let Some(walker) = locked_walker() else {
        return 0;
    };
    let top = top as u64;
    let low = regs.regs[SP];
    let mut ctx: UnwindContext<usize, SmallStore> = UnwindContext::new_in();
    let mut frame = Frame {
        pc: regs.pc,
        rsp: regs.regs[SP],
        rbp: regs.regs[FP],
    };
    let mut n = 0usize;
    while n < out.len() {
        let Some(next) = step(&walker, &mut ctx, frame, top, low) else {
            break;
        };
        out[n] = next.pc as usize;
        n += 1;
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
        Some(self.take::<1>()?[0])
    }
    #[inline(always)]
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take()?))
    }
    #[inline(always)]
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take()?))
    }
    #[inline(always)]
    fn uleb(&mut self) -> Option<u64> {
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

/// The row for `site` read directly from `image`'s tables, or `None` to let
/// gimli decide (any table shape other than the one LLVM emits).
fn direct_row(image: &EhFrameImage, site: u64) -> Option<Row> {
    let hdr = image.hdr_bytes;
    // version 1; eh_frame_ptr pcrel|sdata4; fde_count udata4; table
    // datarel|sdata4 — the layout lld, gold and bfd all write.
    if hdr.len() < 12 || hdr[0] != 1 || hdr[1] != 0x1b || hdr[2] != 0x03 || hdr[3] != 0x3b {
        return None;
    }
    let count = u32::from_le_bytes(hdr[8..12].try_into().ok()?) as usize;
    let table = hdr.get(12..12usize.checked_add(count.checked_mul(8)?)?)?;
    let entry = |i: usize| -> (i64, i64) {
        let e = &table[i * 8..i * 8 + 8];
        (
            i32::from_le_bytes([e[0], e[1], e[2], e[3]]) as i64,
            i32::from_le_bytes([e[4], e[5], e[6], e[7]]) as i64,
        )
    };
    let rel = site as i64 - image.hdr_addr as i64;
    // Last entry whose initial location is <= site.
    let (mut lo, mut hi) = (0usize, count);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if entry(mid).0 <= rel {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    if lo == 0 {
        return Some(Row::NoFde);
    }
    let (start_rel, fde_rel) = entry(lo - 1);
    let func_start = (image.hdr_addr as i64).checked_add(start_rel)? as u64;
    let fde_addr = (image.hdr_addr as i64).checked_add(fde_rel)? as u64;
    let eh = image.bytes;
    let fde_off = usize::try_from(fde_addr.checked_sub(image.eh_frame_addr)?).ok()?;

    // FDE header.
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

    // CIE.
    let mut k = Cursor::new(eh, cie_off);
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
    let mut fde_has_aug = false;
    if aug.first() == Some(&b'z') {
        fde_has_aug = true;
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

    // FDE body: pc_begin (already known from the table), pc_range, the
    // augmentation data, then the program.
    let ptr = encoded_size(fde_enc)?;
    c.skip(ptr)?;
    let range = match ptr {
        8 => u64::from_le_bytes(c.take()?),
        4 => c.u32()? as u64,
        _ => c.u16()? as u64,
    };
    if site >= func_start.checked_add(range)? {
        return Some(Row::NoFde);
    }
    if fde_has_aug {
        let n = c.uleb()? as usize;
        c.skip(n)?;
    }

    let mut state = State {
        cfa_reg: DW_RSP as u64,
        cfa_off: 0,
        rbp: Rule::Same,
        ra: Rule::Undefined,
    };
    let blank = state;
    run_cfi(
        &mut k,
        cie_end,
        code_align,
        data_align,
        ra_reg,
        func_start,
        u64::MAX,
        &mut state,
        &blank,
    )?;
    let initial = state;
    run_cfi(
        &mut c, fde_end, code_align, data_align, ra_reg, func_start, site, &mut state, &initial,
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
    })
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
                    match direct_row(&image.image, site) {
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
