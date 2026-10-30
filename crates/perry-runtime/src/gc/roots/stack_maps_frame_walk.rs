//! The x86-64 Linux root walk: frames stepped by kind, not by the system
//! unwinder.
//!
//! `_Unwind_Backtrace` costs one `_Unwind_Find_FDE` search plus a libgcc CFI
//! interpretation for every frame, ~1,570 instructions, and most frames of a
//! collection's stack are not JS at all: the collector's own Rust frames
//! above the poll and the C start frames below `main`. This walk avoids all
//! three costs.
//!
//! * **Start.** A collection whose request carried its entry's caller frame
//!   ([`super::super::walk_origin`]) starts there, at the JS frame that called
//!   the poll or `gc()`. Otherwise the walk starts at its own frame.
//! * **JS frame** (a pc `match_records` finds records for): its roots resolve
//!   against its exact `rbp` (FP-based) and `sp` (SP-based: its stack pointer
//!   at the call, the callee's CFA). It steps by its frame record, two loads:
//!   every generated function is `"frame-pointer"="non-leaf"` and a frame on
//!   the stack is at a call. A record based on any other register fails the
//!   walk closed, and so does a frame whose map size contradicts its record.
//! * **Any other frame** (Rust, a shim, libc, an addon) steps by its unwind
//!   row through the owned direct decoder ([`crate::eh_walker::gc_cfi_step`]):
//!   one `.eh_frame_hdr` search in the object `_dl_find_object` names. Only
//!   `rsp`, `rbp` and the return address are tracked; nothing outlives the
//!   walk. A row the decoder cannot follow fails the walk closed.
//! * **End.** After `main`'s frame, or at the worker base trap
//!   `perry_trap_call_base`: everything outward of either is the C runtime or
//!   std's thread start. Both ranges are resolved once with the index; a frame
//!   costs two compares. A thread entered some other way ends where its
//!   tables do, as before.
//!
//! Every frame is validated (aligned, strictly outward, below the stack top)
//! before any of its slots is visited. Failing closed returns `None` and the
//! caller re-walks through the system unwinder; the slots visited so far were
//! real slots of validated frames, and visiting a slot twice is idempotent.

use super::*;
use crate::eh_walker::{gc_cfi_step, GcCfiScratch, GcCfiStep};

const DWARF_REG_RBP: u16 = 6;

/// The frames outward of which no JS can run: `main`'s and the worker base
/// trap's, as `(start, end)` code ranges from the unwind tables. An empty
/// range where a function is absent (an image without `main`, a unit test).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct RootFrames {
    ranges: [(usize, usize); 2],
}

impl RootFrames {
    pub(super) fn resolve() -> Self {
        let main = crate::eh_walker::executable_main() as usize;
        let base = crate::exception::worker_base_shim_addr();
        let extent = |address: usize| {
            if address == 0 {
                return (0, 0);
            }
            crate::eh_walker::cfi_function_extent(address as u64)
                .map_or((0, 0), |(start, end)| (start as usize, end as usize))
        };
        Self {
            ranges: [extent(main), extent(base)],
        }
    }

    #[cfg(test)]
    pub(super) fn of(ranges: [(usize, usize); 2]) -> Self {
        Self { ranges }
    }

    #[inline(always)]
    fn contains(&self, pc: usize) -> bool {
        let [(a, b), (c, d)] = self.ranges;
        pc.wrapping_sub(a) < b.wrapping_sub(a) || pc.wrapping_sub(c) < d.wrapping_sub(c)
    }
}

// Sabotage for `stack_maps_frame_walk_tests.rs`: skip the frame one CFI step
// lands on, so the agreement test can show it catches a walk that loses a
// frame.
#[cfg(test)]
crate::perry_thread_local! {
    pub(super) static SKIP_ONE_CFI_STEP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[inline(always)]
fn walk_trace_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *crate::once_init::get_or_init(&ON, || {
        crate::gc::env_flag_enabled("PERRY_GC_STACKMAP_TRACE")
    })
}

#[inline(never)]
pub(super) fn visit<F: FnMut(ResolvedRoot)>(
    index: &StackMapIndex,
    visit: &mut F,
) -> Option<NativeStackWalkStats> {
    let top = crate::native_stack::stack_top();
    if top == 0 {
        return None;
    }
    let origin = super::super::walk_origin::active();
    let (mut pc, mut sp, mut fp) = match origin {
        Some(origin) => (origin.pc, origin.sp, origin.fp),
        None => {
            // This frame stays live for the whole walk, so its own registers
            // are a valid start; it is stepped by its unwind row like any
            // Rust frame.
            let here = crate::eh_walker::capture_here();
            (
                here.pc as usize,
                here.regs[crate::eh_walker::SP_INDEX] as usize,
                here.regs[crate::eh_walker::FP_INDEX] as usize,
            )
        }
    };
    let low = sp;
    let mut stats = NativeStackWalkStats {
        walks: 1,
        fp_walks: 1,
        ..NativeStackWalkStats::default()
    };
    let mut scratch = GcCfiScratch::default();
    let roots = &index.root_frames;
    let trace = walk_trace_enabled();
    loop {
        if sp & 7 != 0 || sp >= top || pc == 0 {
            return None;
        }
        stats.frames_visited = stats.frames_visited.saturating_add(1);
        let is_root = roots.contains(pc);
        if let Some(matched) = index.match_records(pc) {
            // A JS frame. Its record and its frame size are checked before
            // any slot of it is visited.
            if fp & 7 != 0 || fp < sp || fp.checked_add(16)? > top {
                return None;
            }
            // The map's frame size covers the frame from `sp` up to and
            // including the saved `rbp`: equal for a fixed frame, smaller
            // when the frame realigns or adjusts `sp` around the call. More
            // means this `rbp` is not this frame's.
            if let Some(size) = index.frame_size(&matched) {
                if sp.checked_add(size as usize)? > fp + 8 {
                    return None;
                }
            }
            let mut foreign_base = false;
            let mut records = index.matched(&matched);
            while let Some(record) = records.next() {
                stats.records_matched = stats.records_matched.saturating_add(1);
                let mut resolve = |location: &StackMapLocation| {
                    let base = match location.dwarf_reg {
                        DWARF_REG_RBP => fp,
                        ARCH_DWARF_SP => sp,
                        _ => {
                            foreign_base = true;
                            return None;
                        }
                    };
                    let address = if location.offset < 0 {
                        base.checked_sub(location.offset.unsigned_abs() as usize)
                    } else {
                        base.checked_add(location.offset as usize)
                    };
                    address.map(|address| (address, base))
                };
                unsafe {
                    visit_record_slots(&record, pc, &mut resolve, &mut stats, visit);
                }
                // A base this walk cannot reconstruct (a realigned frame's
                // base pointer): the unwinder resolves every register.
                if foreign_base {
                    return None;
                }
            }
            if trace {
                eprintln!(
                    "[gc-stackmap-walk] fast frame {} ip={pc:#x} js",
                    stats.frames_visited
                );
            }
            if is_root {
                break;
            }
            // SAFETY: `fp..fp+16` was bounds-checked above.
            let (caller_fp, caller_pc) =
                unsafe { (*(fp as *const usize), *((fp + 8) as *const usize)) };
            stats.record_steps = stats.record_steps.saturating_add(1);
            if caller_pc == 0 {
                break;
            }
            pc = caller_pc;
            sp = fp + 16;
            fp = caller_fp;
            continue;
        }
        if trace {
            eprintln!(
                "[gc-stackmap-walk] fast frame {} ip={pc:#x} cfi",
                stats.frames_visited
            );
        }
        if is_root {
            break;
        }
        stats.cfi_steps = stats.cfi_steps.saturating_add(1);
        match gc_cfi_step(
            pc as u64,
            sp as u64,
            fp as u64,
            low as u64,
            top as u64,
            &mut scratch,
        ) {
            GcCfiStep::Caller {
                pc: caller_pc,
                rsp,
                rbp,
            } => {
                if rsp as usize <= sp {
                    return None;
                }
                pc = caller_pc as usize;
                sp = rsp as usize;
                fp = rbp as usize;
                #[cfg(test)]
                if SKIP_ONE_CFI_STEP.with(|skip| skip.replace(false)) {
                    match gc_cfi_step(
                        pc as u64,
                        sp as u64,
                        fp as u64,
                        low as u64,
                        top as u64,
                        &mut scratch,
                    ) {
                        GcCfiStep::Caller { pc: next, rsp, rbp } => {
                            pc = next as usize;
                            sp = rsp as usize;
                            fp = rbp as usize;
                        }
                        _ => return None,
                    }
                }
            }
            GcCfiStep::Outermost => break,
            GcCfiStep::Unreadable => return None,
        }
    }
    if trace {
        eprintln!(
            "[gc-stackmap-walk] fast walk frames={} records={} cfi_steps={} record_steps={} origin={}",
            stats.frames_visited,
            stats.records_matched,
            stats.cfi_steps,
            stats.record_steps,
            origin.is_some()
        );
    }
    Some(stats)
}
