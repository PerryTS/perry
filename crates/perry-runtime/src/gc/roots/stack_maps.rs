//! Precise GC roots read from native frames, via LLVM statepoints.
//!
//! Under `PERRY_RS4GC=1` the compiler runs `RewriteStatepointsForGC`, which
//! records each live root as an LLVM-owned spill slot for a `gc.relocate`
//! value: a writable, frame-register-relative location in the emitted
//! stack-map section. This module finds that section in the running image,
//! walks the native frames, and hands each live slot to the collector as a
//! `MutableRootSlot` — mutable because evacuation rewrites through it.
//!
//! A second lowering used to exist, placing `llvm.experimental.stackmap`
//! before mapped calls and recording alloca addresses directly. It was
//! unsound and is deleted; only the statepoint path remains, so there is no
//! backend selection here and no fallback between them.
//!
//! Platform support is per-shape, not one target: Apple (macOS/iOS/iPadOS/
//! tvOS/watchOS) reads a concatenated `__PERRY_GCMAP` Mach-O section, Linux
//! reads `.perry_gcmap` from ELF, and Windows reads `.pgcmap` from the PE
//! image. Frame walking is per-platform too — an x29 chain walk where the
//! map proves every frame is chain-walkable, the Itanium unwinder elsewhere
//! on Unix, and `RtlVirtualUnwind` on Windows. Targets outside that set
//! return no roots, and the compiler refuses to emit a map for them rather
//! than producing a binary whose collector would silently free live objects.

use super::{MutableRootSlot, MutableRootSlotKind};
use crate::gc::telemetry::RootSourcesTraceStats;
// The Windows walker spells `core::ffi::c_void` inline; this import serves
// the Itanium/pthread declarations, which do not exist there.
#[cfg(not(target_os = "windows"))]
use std::ffi::c_void;
use std::sync::OnceLock;

/// Magic and version of the compact map the compiler emits
/// (`perry-codegen/src/gc_map.rs`). LLVM's own stack-map section is rewritten
/// at assembly time and never reaches the binary: >50% of it was the
/// statepoint constant preamble and base/derived duplicates that this parser
/// discarded anyway, and shipping it cost 3.9 MB on a real application.
const GC_MAP_MAGIC: &[u8; 4] = b"PGCM";
const GC_MAP_VERSION: u8 = 3;
const MAX_SAFEPOINT_RETURN_DELTA: usize = 16;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StackMapLocation {
    dwarf_reg: u16,
    offset: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StackMapRecord {
    pc: usize,
    /// Start address of the containing function, from the map's function
    /// table. Used to decode that function's prologue when an SP-relative
    /// location needs the FP-to-SP offset (see `fp_to_sp_offset`).
    function_address: usize,
    /// The containing function's total frame size from the function table.
    ///
    /// Decoded because the map carries it and `parse_gc_map`'s tests pin that
    /// the field is read at the right offset — NOT because a walker may build
    /// a root's base out of it. The unwinder fallback used to compute
    /// `CFA - stack_size` and that was #7392: the CFA a backtrace callback
    /// reports IS the frame's stack pointer, so the subtraction put every
    /// SP-relative root one frame too low.
    #[allow(dead_code)]
    stack_size: u64,
    /// Half-open range into `StackMapIndex::roots`.
    ///
    /// A range rather than an owned `Vec` because **77% of records have the
    /// identical live set as the record before them** — consecutive safepoints
    /// in a function usually share their roots — so the decoder points the
    /// repeats at one copy instead of duplicating 154k entries.
    roots_start: u32,
    roots_len: u32,
}

/// Parsed section plus the facts the fast walker's preconditions need.
///
/// `chain_walkable` is decided once at parse time: the raw x29-chain walk can
/// recover only the frame pointer (register 29) directly, plus the body SP
/// (register 31) derived from the header's per-function stack size. Any other
/// register anywhere in the maps disables the fast path for the whole image
/// rather than risking a wrong base mid-walk.
#[derive(Debug, Default)]
struct StackMapIndex {
    records: Vec<StackMapRecord>,
    /// Every root slot, referenced by `StackMapRecord`'s range. Shared between
    /// records whose live sets are identical.
    roots: Vec<StackMapLocation>,
    /// Sorted, deduplicated start address of every function that has records.
    /// Used to confirm a matched record belongs to the function `ip` is in.
    function_starts: Vec<usize>,
    chain_walkable: bool,
    min_pc: usize,
    max_pc: usize,
}

impl StackMapIndex {
    fn locations(&self, record: &StackMapRecord) -> &[StackMapLocation] {
        let start = record.roots_start as usize;
        let end = start + record.roots_len as usize;
        self.roots.get(start..end).unwrap_or(&[])
    }
}

static STACK_MAPS: OnceLock<StackMapIndex> = OnceLock::new();

// The two register numbers the compact format's short base tags stand for.
// These are aarch64's by definition of the FORMAT, on every architecture — see
// `gc_map.rs`, which deliberately keeps them literal so the compiler's idea of
// the target and this runtime's `target_arch` can never disagree.
const DWARF_REG_FP_AARCH64: u16 = 29;
const DWARF_REG_SP_AARCH64: u16 = 31;

// A frame record is two 64-bit words, so it needs EIGHT-byte alignment, not
// sixteen.
//
// The stack pointer is 16-byte aligned at a public interface on both supported
// ABIs, and on Darwin the frame record sits at the top of the frame, so there
// x29 is always 16-aligned as well and a `fp & 0xF` test never fires. AAPCS64
// does not promise that: §6.4.6 fixes the record's CONTENTS and leaves its
// location within the frame unspecified, and LLVM's AArch64 **ELF** frame
// lowering puts the `x29,x30` pair *below* the other callee-saved GPRs. With an
// odd number of those, the pair lands 8 mod 16.
//
// Measured on aarch64-unknown-linux-gnu (#7392), from the `.eh_frame` of a
// runtime frame that saves x19..x23 and v8:
//
//     LOC        CFA      x19  x20  x21  x22  x23  x29  ra   v8
//     ...        x29+56   c-8  c-16 c-24 c-32 c-40 c-56 c-48 c-64
//
// x29 = CFA - 56, and CFA is 16-aligned, so x29 ≡ 8 (mod 16) — a legal frame
// record the 16-byte test rejected. That abandoned the fast walk mid-stack and
// fell back to the unwinder, which had its own SP-base bug (see
// `unwind::walk_frame`), so the frame's roots were never rewritten after an
// evacuation and the mutator then dereferenced a stale from-space pointer.
//
// Only the fp-chain walker reads it, and that walker exists on aarch64 Unix
// alone — the same cfg, spelled out rather than approximated, so an x86-64 or
// Windows build does not warn on a constant it has no walker for.
#[cfg_attr(
    not(all(
        any(target_vendor = "apple", target_os = "linux"),
        target_arch = "aarch64"
    )),
    allow(dead_code)
)]
const FRAME_RECORD_ALIGN_MASK: usize = 0x7;

// Which DWARF register is the stack pointer on the machine this runtime was
// built for. Distinct from the format constants above and used only to choose
// how a base is resolved: `_Unwind_GetGR` is not a supported query for the SP
// column, so an SP-relative root must come from the CFA instead. On x86-64
// every root is `Indirect [RSP + off]` — DWARF 7, measured 56 of 56 on one
// probe — and reading it with `GetGR` returned garbage the collector then
// wrote through, which is the segfault the Linux gate hit.
#[cfg(target_arch = "aarch64")]
const ARCH_DWARF_SP: u16 = 31;
#[cfg(target_arch = "x86_64")]
const ARCH_DWARF_SP: u16 = 7;
#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
const ARCH_DWARF_SP: u16 = u16::MAX;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WalkerMode {
    /// x29-chain walk when `chain_walkable`, transparent unwinder fallback otherwise.
    Fast,
    /// Force the platform unwinder (bisection control).
    Unwind,
    /// Run both walks and panic unless they visit the identical slot set.
    /// This is the only check that can catch a fast walk that silently skips
    /// frames: forced-evacuation verification enumerates roots through the
    /// same walker, so it cannot see a slot the walker never reached.
    Verify,
}

fn walker_mode() -> WalkerMode {
    static MODE: OnceLock<WalkerMode> = OnceLock::new();
    *MODE.get_or_init(|| match std::env::var("PERRY_STACKMAP_WALKER").as_deref() {
        Ok("unwind") => WalkerMode::Unwind,
        Ok("verify") => WalkerMode::Verify,
        _ => WalkerMode::Fast,
    })
}

/// One root as a walker resolved it, carrying the provenance that says WHY.
///
/// The walkers used to hand the collector a bare `MutableRootSlot`, which is
/// all the collector needs and exactly nothing of what a disagreement between
/// two walkers is about. When `PERRY_STACKMAP_WALKER=verify` caught the
/// aarch64-ELF fp-chain walk and the unwinder resolving one root 96 bytes
/// apart (#7984), the panic could say "1 slot versus 1 slot" and print two
/// integers — from which neither the frame, the base register, nor the frame
/// whose base was used could be recovered. Every walker now reports where the
/// address came from, so `verify` names the disagreement instead of posing it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ResolvedRoot {
    /// Address of the slot: `base` displaced by the record's frame offset.
    pub(super) address: usize,
    /// The frame's return address — what `match_records` was keyed on.
    pub(super) ip: usize,
    /// Start of the function the matched record belongs to.
    pub(super) function_address: usize,
    /// The record's base register (29 = FP, 31 = SP on aarch64).
    pub(super) dwarf_reg: u16,
    /// The record's frame offset from that base.
    pub(super) offset: i32,
    /// The base the walker resolved that register to for this frame.
    pub(super) base: usize,
}

impl ResolvedRoot {
    fn slot(self) -> MutableRootSlot {
        MutableRootSlot {
            kind: MutableRootSlotKind::NativeStack,
            ptr: self.address as *mut u64,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::gc) struct NativeStackWalkStats {
    pub(in crate::gc) walks: usize,
    pub(in crate::gc) frames_visited: usize,
    pub(in crate::gc) records_matched: usize,
    pub(in crate::gc) locations_visited: usize,
    pub(in crate::gc) fp_walks: usize,
    pub(in crate::gc) fallback_walks: usize,
}

#[inline]
pub(in crate::gc) fn record_native_stack_walk_source(
    stats: NativeStackWalkStats,
    root_sources: &mut Option<&mut RootSourcesTraceStats>,
) {
    if let Some(sources) = root_sources {
        sources.native_stack_maps.record_walk(
            stats.walks,
            stats.frames_visited,
            stats.records_matched,
            stats.locations_visited,
            stats.fp_walks,
            stats.fallback_walks,
        );
    }
}

pub(in crate::gc) fn initialize() {
    let _ = stack_maps();
}

/// Whether this image carries any native stack-map records — i.e. whether
/// precise frame roots depend on mapped PCs at all. Consumed by the
/// `PERRY_GC_SAFEPOINT_ONLY` contract assert.
pub(in crate::gc) fn native_maps_active() -> bool {
    !stack_maps().records.is_empty()
}

fn stack_maps() -> &'static StackMapIndex {
    STACK_MAPS.get_or_init(|| {
        // No section at all is the ordinary shadow-stack build: there are no
        // native frame roots to find, and an empty index is the right answer.
        let Some(section) = loaded_stack_map_section() else {
            return StackMapIndex::default();
        };
        // A section that exists but does not decode is a different thing
        // entirely, and it must never degrade to "no roots". The two failure
        // shapes are indistinguishable downstream — both yield an empty index
        // — but their consequences are not: with statepoints as the only root
        // mechanism, an empty index means the collector frees live objects and
        // corrupts the heap with no diagnostic at all. That is CLAUDE.md's
        // fourth gate-failure mode (the gate runs, its subject never did), so
        // fail loudly instead. In practice this can only mean a binary whose
        // compiler and runtime disagree about the map format.
        let Some((mut records, roots)) = parse_gc_map(section) else {
            panic!(
                "perry: the GC map section (__perry_gcmap / .perry_gcmap, {} bytes) is \
                 present but could not be decoded — expected format {:?} v{}. This binary's \
                 compiler and runtime disagree about the map layout; continuing would run \
                 the collector with no roots and corrupt the heap silently.",
                section.len(),
                std::str::from_utf8(GC_MAP_MAGIC).unwrap_or("PGCM"),
                GC_MAP_VERSION,
            );
        };
        records.sort_unstable_by_key(|record| record.pc);
        index_records(records, roots)
    })
}

fn index_records(records: Vec<StackMapRecord>, roots: Vec<StackMapLocation>) -> StackMapIndex {
    // SP-relative locations are admitted here and resolved per FRAME in the
    // walker, which decodes the owning function's `add x29, sp, #imm`
    // prologue to get the body SP (#7173). Deciding it here would mean
    // dereferencing every function address at startup — unsafe for records
    // whose addresses are not live code, and unnecessary because the walker
    // already fails closed to the platform unwinder on any anomaly.
    // The decoder only ever produces these two bases, but keep the check: it
    // is what decides the fast walker is usable at all, and a format change
    // that introduced a third base must disable the chain walk, not be
    // trusted by it.
    let chain_walkable = roots.iter().all(|location| {
        matches!(
            location.dwarf_reg,
            DWARF_REG_FP_AARCH64 | DWARF_REG_SP_AARCH64
        )
    });
    let min_pc = records.first().map_or(usize::MAX, |record| record.pc);
    let max_pc = records.last().map_or(0, |record| record.pc);
    let mut function_starts: Vec<usize> = records
        .iter()
        .map(|record| record.function_address)
        .collect();
    function_starts.sort_unstable();
    function_starts.dedup();
    StackMapIndex {
        records,
        roots,
        function_starts,
        chain_walkable,
        min_pc,
        max_pc,
    }
}

/// Recover a function's frame-pointer-to-stack-pointer offset by decoding its
/// prologue (#7173).
///
/// AArch64 prologues set the frame pointer with a single
/// `add x29, sp, #imm` after saving the `[x29, x30]` pair, so the body SP is
/// `fp - imm`. On Darwin that offset is a constant (the pair sits at the top
/// of the frame) but on Linux it varies per function with the callee-save
/// area laid out below the pair — measured 0x30 and 0x60 in adjacent
/// generated functions, which is why no `(fp, stack_size)` formula works
/// there and the fast chain previously fell back to the DWARF unwinder for
/// every collection (~22% of samples on a Pi 5).
///
/// Instruction encoding: ADD (immediate, 64-bit) with Rn = 31 (sp) and
/// Rd = 29 (fp) — `word & 0xFF80_03FF == 0x9100_03FD`, immediate in bits
/// [21:10] and its `lsl #12` flag in bit 22. Scans a bounded prologue window
/// and fails closed (`None`) if the pattern is absent, in which case the
/// caller uses the platform unwinder.
#[cfg(target_arch = "aarch64")]
fn fp_to_sp_offset(function_address: usize) -> Option<usize> {
    // The `sh` bit (22) selects `lsl #12` on the immediate and is therefore
    // NOT part of the opcode match — masking it in (`0xFFC0_….`) restricts the
    // decoder to frames smaller than 4 KiB. See `immediate_of` below.
    const ADD_FP_SP_MASK: u32 = 0xFF80_03FF;
    const ADD_FP_SP_PATTERN: u32 = 0x9100_03FD;
    const PROLOGUE_WINDOW_INSNS: usize = 24;
    if function_address == 0 || function_address & 0x3 != 0 {
        return None;
    }
    // SUB (immediate, 64-bit) with Rn = Rd = 31 (sp):
    // `word & 0xFF80_03FF == 0xD100_03FF`, immediate in bits [21:10].
    const SUB_SP_SP_MASK: u32 = 0xFF80_03FF;
    const SUB_SP_SP_PATTERN: u32 = 0xD100_03FF;

    /// Decode an ADD/SUB (immediate) operand: `imm12` in bits [21:10], scaled
    /// by 4096 when the `sh` bit (22) is set.
    ///
    /// #7394: LLVM switches to `lsl #12` the moment a frame needs 4 KiB or
    /// more, and a generated function that spills several `[32 x double]`
    /// concat buffers crosses that line routinely — 80 of them in one gap-test
    /// binary. Ignoring `sh` did not merely lose the shifted term: the
    /// shifted `sub` failed the opcode match, which *ended the accumulation
    /// run*, so every later `sub sp` in the same prologue was dropped too.
    fn immediate_of(word: u32) -> usize {
        const SHIFT_12_BIT: u32 = 1 << 22;
        let imm12 = ((word >> 10) & 0xFFF) as usize;
        if word & SHIFT_12_BIT != 0 {
            imm12 << 12
        } else {
            imm12
        }
    }

    let mut fp_offset = None;
    for i in 0..PROLOGUE_WINDOW_INSNS {
        let word = unsafe { std::ptr::read((function_address + i * 4) as *const u32) };
        match fp_offset {
            None => {
                if word & ADD_FP_SP_MASK == ADD_FP_SP_PATTERN {
                    fp_offset = Some(immediate_of(word));
                }
            }
            // #7328: `add x29, sp, #imm` is NOT always the last stack
            // adjustment. LLVM emits a second allocation *after* establishing
            // the frame pointer when a function has a large or separately-laid-
            // out local area:
            //
            //     stp x29, x30, [sp, #0x90]
            //     add x29, sp, #0x90        <- fp established here
            //     sub sp, sp, #0x170        <- body SP drops a further 368
            //
            // Reading only the `add` left the fast walker 368 bytes high on
            // every slot in such a frame, so it enumerated the wrong addresses
            // and the collector missed live roots. That is a silent wrong
            // answer, visible only under `PERRY_STACKMAP_WALKER=verify`, which
            // is not the default. Accumulate every trailing `sub sp, sp, #imm`.
            Some(offset) => {
                if word & SUB_SP_SP_MASK == SUB_SP_SP_PATTERN {
                    fp_offset = Some(offset + immediate_of(word));
                    continue;
                }
                // The prologue's stack adjustments are contiguous; the first
                // instruction after them that is not a `sub sp` ends the run.
                // Anything later that touches sp is a body operation (a dynamic
                // alloca, a call-argument area) which the stack map's own
                // offsets already account for.
                break;
            }
        }
        // `ret` ends the prologue window for a leaf that never sets up fp.
        if word == 0xD65F_03C0 {
            break;
        }
    }
    fp_offset
}

#[cfg(not(target_arch = "aarch64"))]
fn fp_to_sp_offset(_function_address: usize) -> Option<usize> {
    None
}

fn closest_record_pc(maps: &[StackMapRecord], ip: usize) -> Option<usize> {
    let insertion = maps.partition_point(|record| record.pc < ip);
    let before = insertion
        .checked_sub(1)
        .and_then(|idx| maps.get(idx))
        .map(|record| record.pc);
    let at_or_after = maps.get(insertion).map(|record| record.pc);
    match (before, at_or_after) {
        (Some(before), Some(after)) => Some(if ip.abs_diff(before) <= ip.abs_diff(after) {
            before
        } else {
            after
        }),
        (Some(before), None) => Some(before),
        (None, Some(after)) => Some(after),
        (None, None) => None,
    }
}

impl StackMapIndex {
    /// The records describing the frame whose return address is `ip`: the
    /// ±16-byte nearest-PC match, which can select several records at one PC
    /// (plain maps sit just before the call, statepoints exactly at the
    /// return address).
    fn match_records(&self, ip: usize) -> &[StackMapRecord] {
        let Some(candidate_pc) = closest_record_pc(&self.records, ip) else {
            return &[];
        };
        if ip.abs_diff(candidate_pc) > MAX_SAFEPOINT_RETURN_DELTA {
            return &[];
        }
        // The ±16 window is a distance, not a containment check: nothing in it
        // says the matched record belongs to the function `ip` is executing.
        // Functions are adjacent in .text, so an `ip` early in B can sit within
        // the window of a safepoint at the end of A — and the walker would then
        // use A's frame offsets against B's frame and rewrite unrelated words.
        //
        // Require the record's function to be the one containing `ip`: the
        // greatest mapped function start <= ip. Measured across the probe
        // suite, every near-match is already same-function (deltas 8..64, all
        // `same=true`), so this rejects only the cross-function case — and
        // notably NOT the legitimate delta=8 match, which requiring an exact
        // pc would have discarded along with its roots.
        //
        // Residual gap, stated rather than papered over: a function with no
        // safepoints is absent from `function_starts`, so an `ip` inside one
        // resolves to the previous mapped function. Closing that needs a
        // per-function code extent, which Mach-O does not expose cheaply
        // (`Lfunc_end` covers only EH-carrying functions; there is no `.size`).
        let owning = self
            .function_starts
            .partition_point(|start| *start <= ip)
            .checked_sub(1)
            .map(|index| self.function_starts[index]);
        let first = self
            .records
            .partition_point(|record| record.pc < candidate_pc);
        let last = self
            .records
            .partition_point(|record| record.pc <= candidate_pc);
        let matched = &self.records[first..last];
        match (matched.first(), owning) {
            (Some(record), Some(owning)) if record.function_address == owning => matched,
            _ => &[],
        }
    }
}

pub(super) fn visit_stack_map_root_slots(
    visit: &mut impl FnMut(MutableRootSlot),
) -> NativeStackWalkStats {
    let index = stack_maps();
    if index.records.is_empty() {
        return NativeStackWalkStats::default();
    }
    match walker_mode() {
        WalkerMode::Unwind => unwind::visit(index, &mut |root: ResolvedRoot| visit(root.slot())),
        WalkerMode::Fast => {
            if index.chain_walkable {
                if let Some(stats) =
                    fp_chain::visit(index, &mut |root: ResolvedRoot| visit(root.slot()))
                {
                    return stats;
                }
            }
            let mut stats = unwind::visit(index, &mut |root: ResolvedRoot| visit(root.slot()));
            stats.fallback_walks = 1;
            stats
        }
        WalkerMode::Verify => verify::visit(index, visit),
    }
}

/// Decode every concatenated compact map in the section.
///
/// The linker concatenates one blob per object file, so this walks blob by
/// blob using each header's `total_len` rather than assuming a single map —
/// a decoder that reads only the first header silently drops every other
/// object's roots, which is invisible until a collection frees a live object.
fn parse_gc_map(bytes: &[u8]) -> Option<(Vec<StackMapRecord>, Vec<StackMapLocation>)> {
    let mut records = Vec::new();
    let mut roots: Vec<StackMapLocation> = Vec::new();
    let mut base = 0usize;

    while base + 16 <= bytes.len() {
        if bytes.get(base..base + 4)? != GC_MAP_MAGIC {
            // Linkers pad between input sections; a zero tail is the end.
            if bytes[base..].iter().all(|byte| *byte == 0) {
                break;
            }
            base += 1;
            continue;
        }
        if read_u8(bytes, base + 4)? != GC_MAP_VERSION {
            return None;
        }
        let function_count = read_u32(bytes, base + 8)? as usize;
        let total_len = read_u32(bytes, base + 12)? as usize;
        // Header flags, bit 0: the function-address field is 8 bytes wide. The
        // emitter writes the TARGET's pointer width (watchOS `arm64_32` is
        // ILP32), and compile target and run target are the same machine — so
        // a mismatch here means the binary's map was produced for a different
        // width and every function address would be misread. Fail closed.
        let flags = read_u16(bytes, base + 6)?;
        if (flags & 1 == 1) != (std::mem::size_of::<usize>() == 8) {
            return None;
        }
        let entry = if flags & 1 == 1 { 16 } else { 12 };
        // A blob must at least cover its header and function table. Without
        // this, a `total_len` of 0 leaves `base` unchanged — and because the
        // magic still matches at that offset the resynchronisation path below
        // is never reached, so the loop spins forever. This runs inside
        // `OnceLock::get_or_init`, so that is a process hang at the first
        // collection rather than the fail-closed panic in `stack_maps`.
        if total_len < 16 + function_count.checked_mul(entry)? {
            return None;
        }
        let table = base.checked_add(16)?;
        let stream_start = table.checked_add(function_count.checked_mul(entry)?)?;
        let blob_end = base.checked_add(total_len)?;
        if blob_end > bytes.len() || stream_start > blob_end {
            return None;
        }

        // Instruction offsets are a fixed-width array ahead of the varint
        // stream: at -O3 the compiler emits them as label differences the
        // assembler evaluates, so their values cannot be varint-encoded at
        // rewrite time.
        // Not `unwrap_or(0)`: a failed read here means the function table is
        // truncated, and treating that function as having zero records starts
        // `cursor` at the wrong offset so every later varint decodes from
        // misaligned bytes. A wrong live set is worse than no map.
        let mut record_total: usize = 0;
        for index in 0..function_count {
            record_total =
                record_total.checked_add(read_u32(bytes, table + index * 16 + 12)? as usize)?;
        }
        let offsets = stream_start;
        let mut cursor = offsets.checked_add(record_total.checked_mul(4)?)?;
        if cursor > blob_end {
            return None;
        }
        let mut record_index = 0usize;

        for index in 0..function_count {
            // Address width follows the header flag checked above, so the
            // stack-size and record-count offsets move with it.
            let base_off = table + index * entry;
            let addr_bytes = entry - 8;
            let function_address = if addr_bytes == 8 {
                read_u64(bytes, base_off)? as usize
            } else {
                read_u32(bytes, base_off)? as usize
            };
            let stack_size = u64::from(read_u32(bytes, base_off + addr_bytes)?);
            let record_count = read_u32(bytes, base_off + addr_bytes + 4)? as usize;

            let mut previous: Option<(u32, u32)> = None;
            for _ in 0..record_count {
                let instruction_offset = read_u32(bytes, offsets + record_index * 4)?;
                record_index += 1;

                let (header, next) = read_varint(bytes, cursor, blob_end)?;
                cursor = next;
                let range = if header & 1 == 1 {
                    // Repeat: this safepoint's live set is the previous one's.
                    previous?
                } else {
                    let count = (header >> 1) as usize;
                    let start = u32::try_from(roots.len()).ok()?;
                    let mut last: Option<i32> = None;
                    for _ in 0..count {
                        let (value, next) = read_varint(bytes, cursor, blob_end)?;
                        cursor = next;
                        // 2-bit base tag: 0 = FP, 1 = SP, 2 = explicit DWARF
                        // register in a following varint (LLVM uses x19 as a
                        // frame base in functions with dynamic allocation).
                        let dwarf_reg = match value & 3 {
                            0 => DWARF_REG_FP_AARCH64,
                            1 => DWARF_REG_SP_AARCH64,
                            2 => {
                                let (reg, next) = read_varint(bytes, cursor, blob_end)?;
                                cursor = next;
                                u16::try_from(reg).ok()?
                            }
                            _ => return None,
                        };
                        let delta = unzigzag((value >> 2) as u32);
                        let offset = match last {
                            None => delta,
                            Some(previous_offset) => previous_offset.wrapping_add(delta),
                        };
                        last = Some(offset);
                        roots.push(StackMapLocation { dwarf_reg, offset });
                    }
                    (start, u32::try_from(count).ok()?)
                };
                previous = Some(range);

                records.push(StackMapRecord {
                    pc: function_address.checked_add(instruction_offset as usize)?,
                    function_address,
                    stack_size,
                    roots_start: range.0,
                    roots_len: range.1,
                });
            }
        }

        let next = align_up(blob_end, 8)?;
        if next <= base {
            return None;
        }
        base = next;
    }

    Some((records, roots))
}

/// LEB128 read bounded by the blob it belongs to, so a corrupt length cannot
/// walk into the next blob or off the section.
fn read_varint(bytes: &[u8], mut at: usize, end: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    let mut shift = 0u32;
    loop {
        if at >= end || shift > 63 {
            return None;
        }
        let byte = *bytes.get(at)?;
        at += 1;
        value |= u64::from(byte & 0x7F) << shift;
        if byte & 0x80 == 0 {
            return Some((value, at));
        }
        shift += 7;
    }
}

fn unzigzag(value: u32) -> i32 {
    ((value >> 1) as i32) ^ -((value & 1) as i32)
}

fn align_up(value: usize, alignment: usize) -> Option<usize> {
    value
        .checked_add(alignment.checked_sub(1)?)
        .map(|value| value & !(alignment - 1))
}

fn read_u8(bytes: &[u8], offset: usize) -> Option<u8> {
    bytes.get(offset).copied()
}

/// Used by the map header's flags field and by ELF section headers. It was
/// briefly Linux-gated, which broke the Linux build the moment the map itself
/// needed a 16-bit read — keep it unconditional.
fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(offset..offset + 8)?.try_into().ok()?,
    ))
}

/// Every 64-bit Apple platform, not only macOS. iOS, iPadOS (which reports as
/// iOS), tvOS and visionOS are all aarch64 + Mach-O and share this loader
/// verbatim; gating it to `target_os = "macos"` sent them to the stub below,
/// where the section is never found and the index is empty — a collector with
/// no native roots, silently, on exactly the platforms that cannot be debugged
/// easily.
///
/// 64-bit only: watchOS's `arm64_32` has 32-bit pointers, while the map stores
/// function addresses as `u64` and this code does `usize` arithmetic on them.
/// The compiler refuses that target for the same reason.
#[cfg(target_vendor = "apple")]
fn loaded_stack_map_section() -> Option<&'static [u8]> {
    use mach2::dyld::{_dyld_get_image_header, _dyld_get_image_vmaddr_slide};

    const LC_SEGMENT_64: u32 = 0x19;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct MachHeader64 {
        magic: u32,
        cpu_type: i32,
        cpu_subtype: i32,
        file_type: u32,
        command_count: u32,
        commands_size: u32,
        flags: u32,
        reserved: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct LoadCommand {
        command: u32,
        size: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct SegmentCommand64 {
        command: u32,
        size: u32,
        segment_name: [u8; 16],
        vm_address: u64,
        vm_size: u64,
        file_offset: u64,
        file_size: u64,
        max_protection: i32,
        initial_protection: i32,
        section_count: u32,
        flags: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Section64 {
        section_name: [u8; 16],
        segment_name: [u8; 16],
        address: u64,
        size: u64,
        offset: u32,
        alignment: u32,
        relocation_offset: u32,
        relocation_count: u32,
        flags: u32,
        reserved1: u32,
        reserved2: u32,
        reserved3: u32,
    }

    fn fixed_name_matches(actual: &[u8; 16], expected: &[u8]) -> bool {
        actual.get(..expected.len()) == Some(expected)
            && actual.get(expected.len()).copied().unwrap_or(0) == 0
    }

    unsafe {
        let raw_header = _dyld_get_image_header(0);
        if raw_header.is_null() {
            return None;
        }
        let header = &*(raw_header.cast::<MachHeader64>());
        let slide = _dyld_get_image_vmaddr_slide(0);
        let mut command_ptr = raw_header
            .cast::<u8>()
            .add(std::mem::size_of::<MachHeader64>());
        for _ in 0..header.command_count {
            let load = std::ptr::read_unaligned(command_ptr.cast::<LoadCommand>());
            if load.size < std::mem::size_of::<LoadCommand>() as u32 {
                return None;
            }
            if load.command == LC_SEGMENT_64 {
                let segment = std::ptr::read_unaligned(command_ptr.cast::<SegmentCommand64>());
                let mut section_ptr = command_ptr.add(std::mem::size_of::<SegmentCommand64>());
                for _ in 0..segment.section_count {
                    let section = std::ptr::read_unaligned(section_ptr.cast::<Section64>());
                    if fixed_name_matches(&section.segment_name, b"__PERRY_GCMAP")
                        && fixed_name_matches(&section.section_name, b"__perry_gcmap")
                    {
                        let address = (section.address as isize).checked_add(slide)? as usize;
                        let size = usize::try_from(section.size).ok()?;
                        if address == 0 || size == 0 {
                            return None;
                        }
                        return Some(std::slice::from_raw_parts(address as *const u8, size));
                    }
                    section_ptr = section_ptr.add(std::mem::size_of::<Section64>());
                }
            }
            command_ptr = command_ptr.add(load.size as usize);
        }
    }
    None
}

/// ELF (#7173): the `.perry_gcmap` section of the main executable.
///
/// Linker-provided `__start_`/`__stop_` symbols would need weak linkage
/// (unstable in Rust) or `-rdynamic` (not guaranteed), so instead: read
/// `/proc/self/exe`'s section headers for `.perry_gcmap` (sh_addr,
/// sh_size) and add the main object's load bias from the first
/// `dl_iterate_phdr` callback. Runtime-verified gates for this path are
/// pending a Linux host — tracked in #7173; the parser, index, matching,
/// and verify machinery above are platform-independent already.
#[cfg(target_os = "linux")]
fn loaded_stack_map_section() -> Option<&'static [u8]> {
    let bytes = std::fs::read("/proc/self/exe").ok()?;
    let (addr, size) = elf_section_vaddr(&bytes, b".perry_gcmap")?;
    let bias = main_object_load_bias()?;
    let start = bias.checked_add(addr)?;
    if start == 0 || size == 0 {
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts(start as *const u8, size) })
}

/// Minimal ELF64 section-header walk: returns (sh_addr, sh_size) for the
/// named section. Same defensive read style as the stack-map parser.
#[cfg(target_os = "linux")]
fn elf_section_vaddr(bytes: &[u8], name: &[u8]) -> Option<(usize, usize)> {
    if bytes.get(..4)? != b"\x7fELF" || *bytes.get(4)? != 2 {
        return None; // not ELF64
    }
    let shoff = read_u64(bytes, 0x28)? as usize;
    let shentsize = read_u16(bytes, 0x3A)? as usize;
    let shnum = read_u16(bytes, 0x3C)? as usize;
    let shstrndx = read_u16(bytes, 0x3E)? as usize;
    let strtab_hdr = shoff.checked_add(shstrndx.checked_mul(shentsize)?)?;
    let strtab_off = read_u64(bytes, strtab_hdr.checked_add(0x18)?)? as usize;
    for i in 0..shnum {
        let hdr = shoff.checked_add(i.checked_mul(shentsize)?)?;
        let name_off = read_u32(bytes, hdr)? as usize;
        let name_pos = strtab_off.checked_add(name_off)?;
        let candidate = bytes.get(name_pos..name_pos.checked_add(name.len())?)?;
        let terminator = bytes.get(name_pos + name.len()).copied().unwrap_or(1);
        if candidate == name && terminator == 0 {
            let addr = read_u64(bytes, hdr.checked_add(0x10)?)? as usize;
            let size = read_u64(bytes, hdr.checked_add(0x20)?)? as usize;
            return Some((addr, size));
        }
    }
    None
}

/// Load bias of the main object: `dlpi_addr` of the first `dl_iterate_phdr`
/// callback (the executable itself on glibc and musl).
#[cfg(target_os = "linux")]
fn main_object_load_bias() -> Option<usize> {
    #[repr(C)]
    struct DlPhdrInfo {
        dlpi_addr: usize,
        dlpi_name: *const std::os::raw::c_char,
        // remaining fields unused
    }
    unsafe extern "C" {
        fn dl_iterate_phdr(
            callback: unsafe extern "C" fn(*mut DlPhdrInfo, usize, *mut c_void) -> i32,
            data: *mut c_void,
        ) -> i32;
    }
    unsafe extern "C" fn first(info: *mut DlPhdrInfo, _size: usize, data: *mut c_void) -> i32 {
        unsafe {
            *data.cast::<usize>() = (*info).dlpi_addr;
        }
        1 // stop after the first (main) object
    }
    let mut bias = usize::MAX;
    unsafe {
        dl_iterate_phdr(first, (&mut bias as *mut usize).cast::<c_void>());
    }
    (bias != usize::MAX).then_some(bias)
}

/// Windows/PE: the `.pgcmap` section of the running image.
///
/// The name is seven bytes because a PE image section header has an 8-byte name
/// field — `.perry_gcmap` would be truncated on the way into the image and the
/// lookup below could never match it. `gc_map::COFF_SECTION_NAME` is the
/// compiler-side half of that agreement.
///
/// `GetModuleHandleW(NULL)` returns the image base, which is also a valid
/// `IMAGE_DOS_HEADER`; the section table follows the optional header, whose
/// size the file header records rather than being fixed.
#[cfg(target_os = "windows")]
fn loaded_stack_map_section() -> Option<&'static [u8]> {
    const IMAGE_DOS_SIGNATURE: u16 = 0x5A4D; // "MZ"
    const IMAGE_NT_SIGNATURE: u32 = 0x0000_4550; // "PE\0\0"
    const SECTION_HEADER_SIZE: usize = 40;
    const SECTION_NAME: &[u8] = b".pgcmap";

    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut core::ffi::c_void;
    }

    unsafe {
        let base = GetModuleHandleW(std::ptr::null()) as *const u8;
        if base.is_null() {
            return None;
        }
        if std::ptr::read_unaligned(base as *const u16) != IMAGE_DOS_SIGNATURE {
            return None;
        }
        // e_lfanew sits at offset 0x3C of the DOS header.
        let nt_offset = std::ptr::read_unaligned(base.add(0x3C) as *const u32) as usize;
        let nt = base.add(nt_offset);
        if std::ptr::read_unaligned(nt as *const u32) != IMAGE_NT_SIGNATURE {
            return None;
        }
        // IMAGE_FILE_HEADER follows the 4-byte signature: NumberOfSections at
        // +2, SizeOfOptionalHeader at +16.
        let file_header = nt.add(4);
        let section_count = std::ptr::read_unaligned(file_header.add(2) as *const u16) as usize;
        let optional_size = std::ptr::read_unaligned(file_header.add(16) as *const u16) as usize;
        let sections = file_header.add(20).add(optional_size);

        for index in 0..section_count {
            let header = sections.add(index * SECTION_HEADER_SIZE);
            let name = std::slice::from_raw_parts(header, 8);
            // Names shorter than eight bytes are NUL-padded.
            let trimmed = match name.iter().position(|b| *b == 0) {
                Some(end) => &name[..end],
                None => name,
            };
            if trimmed != SECTION_NAME {
                continue;
            }
            let virtual_size = std::ptr::read_unaligned(header.add(8) as *const u32) as usize;
            let virtual_address = std::ptr::read_unaligned(header.add(12) as *const u32) as usize;
            if virtual_size == 0 || virtual_address == 0 {
                return None;
            }
            return Some(std::slice::from_raw_parts(
                base.add(virtual_address),
                virtual_size,
            ));
        }
    }
    None
}

#[cfg(not(any(target_vendor = "apple", target_os = "linux", target_os = "windows")))]
fn loaded_stack_map_section() -> Option<&'static [u8]> {
    None
}

// Same platform set as the loader above: the Itanium unwinder personality and
// `_Unwind_*` API are present on every Apple platform, not just macOS.
#[cfg(any(target_vendor = "apple", target_os = "linux"))]
mod unwind {
    use super::*;

    #[repr(C)]
    struct UnwindContext {
        _private: [u8; 0],
    }

    unsafe extern "C" {
        fn _Unwind_Backtrace(
            trace: unsafe extern "C" fn(*mut UnwindContext, *mut c_void) -> i32,
            argument: *mut c_void,
        ) -> i32;
        fn _Unwind_GetIP(context: *mut UnwindContext) -> usize;
        fn _Unwind_GetGR(context: *mut UnwindContext, register: i32) -> usize;
        /// The frame's canonical frame address — the supported way to reach a
        /// frame's stack pointer. `_Unwind_GetGR` on the SP column is not a
        /// supported query and returns garbage on x86-64.
        fn _Unwind_GetCFA(context: *mut UnwindContext) -> usize;
    }

    struct WalkState<'a, F> {
        index: &'a StackMapIndex,
        visit: &'a mut F,
        stats: NativeStackWalkStats,
    }

    pub(super) fn visit<F: FnMut(ResolvedRoot)>(
        index: &StackMapIndex,
        visit: &mut F,
    ) -> NativeStackWalkStats {
        let mut state = WalkState {
            index,
            visit,
            stats: NativeStackWalkStats {
                walks: 1,
                ..NativeStackWalkStats::default()
            },
        };
        unsafe {
            _Unwind_Backtrace(
                walk_frame::<F>,
                (&mut state as *mut WalkState<'_, _>).cast::<c_void>(),
            );
        }
        state.stats
    }

    unsafe extern "C" fn walk_frame<F: FnMut(ResolvedRoot)>(
        context: *mut UnwindContext,
        argument: *mut c_void,
    ) -> i32 {
        let state = &mut *argument.cast::<WalkState<'_, F>>();
        state.stats.frames_visited = state.stats.frames_visited.saturating_add(1);
        let ip = _Unwind_GetIP(context);
        let matched = state.index.match_records(ip);
        if matched.is_empty() {
            return 0;
        }
        state.stats.records_matched = state.stats.records_matched.saturating_add(matched.len());
        for record in matched {
            for location in state.index.locations(record) {
                state.stats.locations_visited = state.stats.locations_visited.saturating_add(1);
                // SP-relative roots take the CFA as their base VERBATIM.
                //
                // Not `CFA - stack_size`, which is what the DWARF definition of
                // a CFA suggests and what this code used to compute. What
                // `_Unwind_GetCFA` returns inside an `_Unwind_Backtrace`
                // callback is the body stack pointer of the frame whose return
                // address `_Unwind_GetIP` just reported, so subtracting the
                // frame size lands one whole frame too low.
                //
                // MEASURED (#7392) by `unwind_cfa_is_the_frames_stack_pointer`
                // below, which records each frame's real SP and matches it
                // against the walk: the identity holds on aarch64 Linux
                // (libgcc), aarch64 macOS (Apple libunwind) and x86-64 Linux
                // alike — so there is no return-address adjustment to make and
                // no per-architecture constant left to get wrong.
                //
                // It stayed invisible because this is the FALLBACK path: on
                // aarch64 the x29 chain walk normally answers, and wherever it
                // bailed this read unrelated words instead of the roots, which
                // nothing downstream can notice — no code knows what a root slot
                // is supposed to contain. Cross-checked directly on
                // `02_survivor_promotion`: at the CFA the slot holds a NaN-boxed
                // pointer (`0x7ffd…`); one frame lower it holds a stack address.
                let base = if location.dwarf_reg == ARCH_DWARF_SP {
                    _Unwind_GetCFA(context)
                } else {
                    _Unwind_GetGR(context, i32::from(location.dwarf_reg))
                };
                let address = if location.offset < 0 {
                    base.checked_sub(location.offset.unsigned_abs() as usize)
                } else {
                    base.checked_add(location.offset as usize)
                };
                let Some(address) = address else {
                    continue;
                };
                if address == 0 || address & (std::mem::align_of::<u64>() - 1) != 0 {
                    continue;
                }
                (state.visit)(ResolvedRoot {
                    address,
                    ip,
                    function_address: record.function_address,
                    dwarf_reg: location.dwarf_reg,
                    offset: location.offset,
                    base,
                });
            }
        }
        0
    }
}

/// Windows x86-64 (#7354): walk native frames with `RtlVirtualUnwind`, the
/// documented Win64 unwinder. `_Unwind_Backtrace` does not exist here.
///
/// `RtlLookupFunctionEntry` + `RtlVirtualUnwind` step a `CONTEXT` outward one
/// frame at a time, and each step yields the frame's `Rip`, `Rsp` and `Rbp`
/// **directly** — so unlike the Itanium path above there is no CFA derivation:
/// an SP-relative root's base is the real `Rsp` the unwinder just restored.
/// (`Rip` after a step is the return address, same as `_Unwind_GetIP`, which
/// is what `match_records`' ±16 window plus containment check expects.)
///
/// Fail-closed contract, stricter than the Itanium module because a wrong base
/// here has no verifying backstop: any anomaly — a base register the virtual
/// unwind cannot have restored, a slot outside this thread's stack, a frame
/// with no unwind info, a step that does not move outward — abandons the walk
/// and returns, rather than visiting a slot the collector would then *write*
/// through.
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
mod unwind {
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
            let matched = index.match_records(context.rip as usize);
            if !matched.is_empty() {
                stats.records_matched = stats.records_matched.saturating_add(matched.len());
                for record in matched {
                    for location in index.locations(record) {
                        stats.locations_visited = stats.locations_visited.saturating_add(1);
                        let Some(base) = frame_base(&context, location.dwarf_reg) else {
                            return stats;
                        };
                        let address = if location.offset < 0 {
                            base.checked_sub(location.offset.unsigned_abs() as usize)
                        } else {
                            base.checked_add(location.offset as usize)
                        };
                        let Some(address) = address else {
                            return stats;
                        };
                        if address < stack_low
                            || address.saturating_add(std::mem::size_of::<u64>()) > stack_high
                            || address & (std::mem::align_of::<u64>() - 1) != 0
                        {
                            return stats;
                        }
                        visit(ResolvedRoot {
                            address,
                            ip: context.rip as usize,
                            function_address: record.function_address,
                            dwarf_reg: location.dwarf_reg,
                            offset: location.offset,
                            base,
                        });
                    }
                }
            }

            let mut image_base = 0u64;
            let entry = unsafe {
                RtlLookupFunctionEntry(context.rip, &mut image_base, std::ptr::null_mut())
            };
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
}

#[cfg(not(any(
    target_vendor = "apple",
    target_os = "linux",
    all(target_os = "windows", target_arch = "x86_64")
)))]
mod unwind {
    use super::*;

    pub(super) fn visit(
        _index: &StackMapIndex,
        _visit: &mut impl FnMut(ResolvedRoot),
    ) -> NativeStackWalkStats {
        NativeStackWalkStats::default()
    }
}

/// Raw x29-chain walker.
///
/// AArch64 prologues under `"frame-pointer"="non-leaf"` are
/// `stp x29, x30, [sp, #-16]!; mov x29, sp`, so every frame's x29 points at
/// a `[caller x29, return address]` pair. One hop is therefore two loads,
/// against a full unwind step (compact-unwind lookup plus register
/// recovery) — this is what turns the measured 350:1 frames-to-roots ratio
/// from a tax into noise.
///
/// Fail-closed everywhere: a misaligned, non-increasing, or out-of-bounds
/// frame pointer abandons the walk with `None` and the caller re-runs the
/// whole scan through the platform unwinder. Slot visitation is idempotent
/// (a rewritten slot no longer points at a forwarded object), so a partial
/// fast walk followed by a full unwinder walk is safe.
#[cfg(all(
    any(target_vendor = "apple", target_os = "linux"),
    target_arch = "aarch64"
))]
mod fp_chain {
    use super::*;

    fn current_frame_pointer() -> usize {
        let fp: usize;
        unsafe {
            core::arch::asm!("mov {fp}, x29", fp = out(reg) fp, options(nomem, nostack));
        }
        fp
    }

    // `pthread_get_stackaddr_np` is Apple-wide, not macOS-only. Gating it to
    // macOS is what broke the iOS build outright — which is the good outcome:
    // the alternative was this module quietly not existing there.
    #[cfg(target_vendor = "apple")]
    fn stack_top() -> usize {
        unsafe extern "C" {
            fn pthread_self() -> usize;
            fn pthread_get_stackaddr_np(thread: usize) -> *mut c_void;
        }
        unsafe { pthread_get_stackaddr_np(pthread_self()) as usize }
    }

    /// Linux (#7173): stack bounds via pthread attrs — the returned address
    /// is the LOW end, so the exclusive top is addr + size. Runtime gates
    /// pending a Linux host; a failure here returns 0 and the caller falls
    /// back to the platform unwinder (fail-closed like every other anomaly).
    #[cfg(target_os = "linux")]
    fn stack_top() -> usize {
        unsafe extern "C" {
            fn pthread_self() -> usize;
            fn pthread_getattr_np(thread: usize, attr: *mut u8) -> i32;
            fn pthread_attr_getstack(
                attr: *const u8,
                stackaddr: *mut *mut c_void,
                stacksize: *mut usize,
            ) -> i32;
            fn pthread_attr_destroy(attr: *mut u8) -> i32;
        }
        // pthread_attr_t is at most 64 bytes on glibc/musl for the supported
        // targets; over-allocate defensively.
        let mut attr = [0u8; 128];
        let mut addr: *mut c_void = std::ptr::null_mut();
        let mut size: usize = 0;
        unsafe {
            if pthread_getattr_np(pthread_self(), attr.as_mut_ptr()) != 0 {
                return 0;
            }
            let ok = pthread_attr_getstack(attr.as_ptr(), &mut addr, &mut size) == 0;
            pthread_attr_destroy(attr.as_mut_ptr());
            if !ok {
                return 0;
            }
        }
        (addr as usize).saturating_add(size)
    }

    pub(super) fn visit<F: FnMut(ResolvedRoot)>(
        index: &StackMapIndex,
        visit: &mut F,
    ) -> Option<NativeStackWalkStats> {
        if !index.chain_walkable {
            return None;
        }
        let top = stack_top();
        if top == 0 {
            return None;
        }
        let mut stats = NativeStackWalkStats {
            walks: 1,
            fp_walks: 1,
            ..NativeStackWalkStats::default()
        };
        let low_pc = index.min_pc.saturating_sub(MAX_SAFEPOINT_RETURN_DELTA);
        let high_pc = index.max_pc.saturating_add(MAX_SAFEPOINT_RETURN_DELTA);
        let mut fp = current_frame_pointer();
        while fp != 0 {
            if fp & FRAME_RECORD_ALIGN_MASK != 0 || fp.checked_add(16)? > top {
                return None;
            }
            let return_address = unsafe { *((fp + 8) as *const usize) };
            let caller_fp = unsafe { *(fp as *const usize) };
            stats.frames_visited = stats.frames_visited.saturating_add(1);
            if return_address == 0 {
                break;
            }
            if return_address >= low_pc && return_address <= high_pc {
                let matched = index.match_records(return_address);
                {
                    if !matched.is_empty() {
                        // The record describes the caller's frame; its
                        // locations are relative to the caller's own x29,
                        // which is exactly the saved word we just read.
                        //
                        // It gets the SAME validation `fp` gets at the top of
                        // the loop, and it gets it BEFORE the root loop rather
                        // than after. Every FP-relative root is based on this
                        // word, and `fp_to_sp_offset` subtracts from it for the
                        // SP-relative ones; downstream the only filters are
                        // non-zero and 8-byte alignment, so an unvalidated
                        // `caller_fp` lets a corrupt frame produce addresses
                        // outside the stack that the collector then reads and
                        // rewrites. Fail closed to the platform unwinder.
                        if caller_fp == 0
                            || caller_fp & FRAME_RECORD_ALIGN_MASK != 0
                            || caller_fp <= fp
                            || caller_fp.checked_add(16)? > top
                        {
                            return None;
                        }
                        stats.records_matched = stats.records_matched.saturating_add(matched.len());
                        for record in matched {
                            // Body SP = fp - (prologue's `add x29, sp, #imm`).
                            // `chain_walkable` proved this decodes for every
                            // SP-relative record in the image (#7173).
                            let sp = fp_to_sp_offset(record.function_address)
                                .and_then(|off| caller_fp.checked_sub(off));
                            for location in index.locations(record) {
                                stats.locations_visited = stats.locations_visited.saturating_add(1);
                                let base = if location.dwarf_reg == DWARF_REG_FP_AARCH64 {
                                    Some(caller_fp)
                                } else {
                                    sp
                                };
                                let Some(base) = base else {
                                    return None;
                                };
                                let address = if location.offset < 0 {
                                    base.checked_sub(location.offset.unsigned_abs() as usize)
                                } else {
                                    base.checked_add(location.offset as usize)
                                };
                                let Some(address) = address else {
                                    continue;
                                };
                                if address == 0 || address & (std::mem::align_of::<u64>() - 1) != 0
                                {
                                    continue;
                                }
                                visit(ResolvedRoot {
                                    address,
                                    ip: return_address,
                                    function_address: record.function_address,
                                    dwarf_reg: location.dwarf_reg,
                                    offset: location.offset,
                                    base,
                                });
                            }
                        }
                    }
                }
            }
            if caller_fp != 0 && caller_fp <= fp {
                return None;
            }
            fp = caller_fp;
        }
        Some(stats)
    }
}

#[cfg(not(all(
    any(target_vendor = "apple", target_os = "linux"),
    target_arch = "aarch64"
)))]
mod fp_chain {
    use super::*;

    pub(super) fn visit(
        _index: &StackMapIndex,
        _visit: &mut impl FnMut(ResolvedRoot),
    ) -> Option<NativeStackWalkStats> {
        None
    }
}

// `verify` mode, and the report it prints when the two walkers disagree. Its
// own file because this one is close to the 2000-line cap.
#[path = "stack_maps_verify.rs"]
mod verify;

// The contract the Itanium fallback rests on, asserted against a real walk
// rather than against DWARF's definition of a CFA — the two disagree, and
// believing the definition was #7392. Its own file because this one is close to
// the 2000-line cap.
#[cfg(all(
    test,
    any(target_vendor = "apple", target_os = "linux"),
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
#[path = "stack_maps_unwind_contract.rs"]
mod unwind_contract;

#[cfg(test)]
#[path = "stack_maps_decode_tests.rs"]
mod decode_tests;
