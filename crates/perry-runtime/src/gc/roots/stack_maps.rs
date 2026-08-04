//! Research precise-root backend for LLVM stack maps.
//!
//! The plain-map prototype places `llvm.experimental.stackmap` immediately
//! before mapped calls and records the address of each native root alloca.
//! The statepoint prototype instead records LLVM-owned spill slots for
//! `gc.relocate` values. Both are writable frame-register-relative locations
//! in the emitted stack-map section.
//!
//! This first implementation deliberately targets macOS, where the experiment
//! is being measured. It discovers the concatenated `__PERRY_GCMAP` section
//! in the main Mach-O image and uses the platform unwinder to recover the
//! frame-register value for each active generated frame. Unsupported targets
//! return no roots; neither native-stack experiment may be used for correctness
//! there.

use super::{MutableRootSlot, MutableRootSlotKind};
use crate::gc::telemetry::RootSourcesTraceStats;
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

// How far below the CFA this frame's return address sits, if it sits on the
// stack at all. This is NOT a constant across architectures and getting it
// wrong shifts every SP-relative root by a word:
//
//   x86-64: `call` PUSHES the return address, so CFA is the caller's SP before
//           the call and the callee's SP starts one slot lower.
//   aarch64: `bl` writes the return address to x30. Nothing is pushed, so the
//           frame's SP is simply CFA - stack_size.
//
// The aarch64 case is easy to miss because `chain_walkable` is true there, so
// the fast x29 walker normally runs and this path is only the fallback — an
// eight-byte error would stay latent until the fast walk bailed.
#[cfg(target_arch = "x86_64")]
const CFA_RETURN_ADDRESS_BYTES: usize = std::mem::size_of::<usize>();
#[cfg(not(target_arch = "x86_64"))]
const CFA_RETURN_ADDRESS_BYTES: usize = 0;

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
/// Instruction encoding: ADD (immediate, 64-bit, shift 0) with Rn = 31 (sp)
/// and Rd = 29 (fp) — `word & 0xFFC0_03FF == 0x9100_03FD`, immediate in bits
/// [21:10]. Scans a bounded prologue window and fails closed (`None`) if the
/// pattern is absent, in which case the caller uses the platform unwinder.
#[cfg(target_arch = "aarch64")]
fn fp_to_sp_offset(function_address: usize) -> Option<usize> {
    const ADD_FP_SP_MASK: u32 = 0xFFC0_03FF;
    const ADD_FP_SP_PATTERN: u32 = 0x9100_03FD;
    const PROLOGUE_WINDOW_INSNS: usize = 24;
    if function_address == 0 || function_address & 0x3 != 0 {
        return None;
    }
    // SUB (immediate, 64-bit, shift 0) with Rn = Rd = 31 (sp):
    // `word & 0xFFC0_03FF == 0xD100_03FF`, immediate in bits [21:10].
    const SUB_SP_SP_MASK: u32 = 0xFFC0_03FF;
    const SUB_SP_SP_PATTERN: u32 = 0xD100_03FF;

    let mut fp_offset = None;
    for i in 0..PROLOGUE_WINDOW_INSNS {
        let word = unsafe { std::ptr::read((function_address + i * 4) as *const u32) };
        match fp_offset {
            None => {
                if word & ADD_FP_SP_MASK == ADD_FP_SP_PATTERN {
                    fp_offset = Some(((word >> 10) & 0xFFF) as usize);
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
                    let imm = ((word >> 10) & 0xFFF) as usize;
                    fp_offset = Some(offset + imm);
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
        WalkerMode::Unwind => unwind::visit(index, visit),
        WalkerMode::Fast => {
            if index.chain_walkable {
                if let Some(stats) = fp_chain::visit(index, visit) {
                    return stats;
                }
            }
            let mut stats = unwind::visit(index, visit);
            stats.fallback_walks = 1;
            stats
        }
        WalkerMode::Verify => verify_visit(index, visit),
    }
}

/// Debug-only cross-check: the fast walk reads slot addresses without
/// mutating, then the unwinder performs the real visitation while recording
/// what it reached. Any set difference is a missed or invented frame and
/// panics immediately — this is the liveness gate for the fast walker itself.
fn verify_visit(
    index: &StackMapIndex,
    visit: &mut impl FnMut(MutableRootSlot),
) -> NativeStackWalkStats {
    let mut fast_addresses: Vec<usize> = Vec::new();
    let fast_stats = fp_chain::visit(index, &mut |slot: MutableRootSlot| {
        fast_addresses.push(slot.ptr as usize);
    });
    let Some(fast_stats) = fast_stats else {
        panic!(
            "PERRY_STACKMAP_WALKER=verify: fast walk unavailable \
             (chain_walkable={}, anomaly or unsupported target)",
            index.chain_walkable
        );
    };
    let mut unwind_addresses: Vec<usize> = Vec::new();
    let mut stats = unwind::visit(index, &mut |slot: MutableRootSlot| {
        unwind_addresses.push(slot.ptr as usize);
        visit(slot);
    });
    fast_addresses.sort_unstable();
    fast_addresses.dedup();
    unwind_addresses.sort_unstable();
    unwind_addresses.dedup();
    assert_eq!(
        fast_addresses,
        unwind_addresses,
        "PERRY_STACKMAP_WALKER=verify: fast walk visited {} unique slots, \
         unwinder visited {}",
        fast_addresses.len(),
        unwind_addresses.len()
    );
    stats.fp_walks = fast_stats.fp_walks;
    stats
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

#[cfg(not(any(target_vendor = "apple", target_os = "linux")))]
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

    pub(super) fn visit<F: FnMut(MutableRootSlot)>(
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

    unsafe extern "C" fn walk_frame<F: FnMut(MutableRootSlot)>(
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
                // SP-relative roots derive their base from the CFA. By the
                // SysV/AAPCS definition the CFA is the caller's stack pointer
                // immediately before the call, so this frame's body stack
                // pointer sits one return-address slot plus this function's own
                // frame below it — and `stack_size` is exactly that frame,
                // recorded per function in the map.
                let base = if location.dwarf_reg == ARCH_DWARF_SP {
                    let cfa = _Unwind_GetCFA(context);
                    match cfa
                        .checked_sub(CFA_RETURN_ADDRESS_BYTES)
                        .and_then(|v| v.checked_sub(record.stack_size as usize))
                    {
                        Some(sp) => sp,
                        None => continue,
                    }
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
                (state.visit)(MutableRootSlot {
                    kind: MutableRootSlotKind::NativeStack,
                    ptr: address as *mut u64,
                });
            }
        }
        0
    }
}

#[cfg(not(any(target_vendor = "apple", target_os = "linux")))]
mod unwind {
    use super::*;

    pub(super) fn visit(
        _index: &StackMapIndex,
        _visit: &mut impl FnMut(MutableRootSlot),
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

    pub(super) fn visit<F: FnMut(MutableRootSlot)>(
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
            if fp & 0xF != 0 || fp.checked_add(16)? > top {
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
                            || caller_fp & 0xF != 0
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
                                visit(MutableRootSlot {
                                    kind: MutableRootSlotKind::NativeStack,
                                    ptr: address as *mut u64,
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
        _visit: &mut impl FnMut(MutableRootSlot),
    ) -> Option<NativeStackWalkStats> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_varint(out: &mut Vec<u8>, mut value: u64) {
        while value >= 0x80 {
            out.push((value as u8 & 0x7F) | 0x80);
            value >>= 7;
        }
        out.push(value as u8);
    }

    fn zigzag(value: i32) -> u64 {
        ((value << 1) ^ (value >> 31)) as u32 as u64
    }

    /// Build one compact blob, mirroring `perry-codegen/src/gc_map.rs`.
    /// `records` is `(instruction_offset, roots)`, roots as `(dwarf_reg, offset)`;
    /// an empty root slice with `repeat` set encodes the repeat flag.
    fn one_map(function: u64, records: &[(u32, Vec<(u16, i32)>, bool)]) -> Vec<u8> {
        let mut offsets = Vec::new();
        let mut stream = Vec::new();
        for (instruction_offset, roots, repeat) in records {
            offsets.extend_from_slice(&instruction_offset.to_le_bytes());
            if *repeat {
                push_varint(&mut stream, 1);
                continue;
            }
            push_varint(&mut stream, (roots.len() as u64) << 1);
            let mut last: Option<i32> = None;
            for (reg, offset) in roots {
                let tag = match *reg {
                    DWARF_REG_FP_AARCH64 => 0u64,
                    DWARF_REG_SP_AARCH64 => 1,
                    _ => 2,
                };
                let delta = match last {
                    None => *offset,
                    Some(previous) => offset.wrapping_sub(previous),
                };
                push_varint(&mut stream, (zigzag(delta) << 2) | tag);
                if tag == 2 {
                    push_varint(&mut stream, u64::from(*reg));
                }
                last = Some(*offset);
            }
        }

        // Build for THIS host's pointer width, mirroring the emitter: the
        // decoder rejects a blob whose recorded width disagrees with its own.
        let ptr64 = std::mem::size_of::<usize>() == 8;
        let entry = if ptr64 { 16 } else { 12 };
        let total_len = 16 + entry + offsets.len() + stream.len();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(GC_MAP_MAGIC);
        bytes.push(GC_MAP_VERSION);
        bytes.push(0);
        bytes.extend_from_slice(&u16::from(ptr64).to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&(total_len as u32).to_le_bytes());
        if ptr64 {
            bytes.extend_from_slice(&function.to_le_bytes());
        } else {
            bytes.extend_from_slice(&(function as u32).to_le_bytes());
        }
        bytes.extend_from_slice(&32u32.to_le_bytes());
        bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&offsets);
        bytes.extend_from_slice(&stream);
        while bytes.len() % 8 != 0 {
            bytes.push(0);
        }
        bytes
    }

    fn simple(function: u64, offset: u32, frame_offset: i32) -> Vec<u8> {
        one_map(function, &[(offset, vec![(29, frame_offset)], false)])
    }

    #[test]
    fn decodes_frame_location() {
        let bytes = simple(0x1000, 0x10, -8);
        let (records, roots) = parse_gc_map(&bytes).expect("valid map");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].pc, 0x1010);
        assert_eq!(records[0].function_address, 0x1000);
        assert_eq!(records[0].stack_size, 32);
        assert_eq!(
            roots,
            vec![StackMapLocation {
                dwarf_reg: 29,
                offset: -8,
            }]
        );
    }

    #[test]
    fn decodes_linker_concatenated_input_sections() {
        let mut bytes = simple(0x1000, 0x10, -8);
        bytes.extend_from_slice(&simple(0x2000, 0x20, -16));
        let (records, _) = parse_gc_map(&bytes).expect("concatenated maps");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].pc, 0x1010);
        assert_eq!(records[1].pc, 0x2020);
    }

    #[test]
    fn repeated_live_sets_share_one_copy() {
        // Three safepoints, the last two repeating the first's live set: the
        // whole point of the format, and the reason the in-memory index does
        // not hold 154k duplicated entries on a real application.
        let bytes = one_map(
            0x1000,
            &[
                (0x10, vec![(29, -8), (29, -16)], false),
                (0x20, vec![], true),
                (0x30, vec![], true),
            ],
        );
        let (records, roots) = parse_gc_map(&bytes).expect("valid map");
        assert_eq!(records.len(), 3);
        assert_eq!(roots.len(), 2, "the repeats must not append new roots");
        for record in &records {
            assert_eq!(record.roots_start, 0);
            assert_eq!(record.roots_len, 2);
        }
    }

    #[test]
    fn decodes_negative_and_ascending_root_offsets() {
        let bytes = one_map(0x1000, &[(0, vec![(29, -64), (29, -8), (31, 24)], false)]);
        let (_, roots) = parse_gc_map(&bytes).expect("valid map");
        assert_eq!(
            roots,
            vec![
                StackMapLocation {
                    dwarf_reg: 29,
                    offset: -64
                },
                StackMapLocation {
                    dwarf_reg: 29,
                    offset: -8
                },
                StackMapLocation {
                    dwarf_reg: 31,
                    offset: 24
                },
            ]
        );
    }

    #[test]
    fn decodes_an_explicit_base_register() {
        // LLVM uses x19 as a frame base pointer in functions with dynamic
        // stack allocation — 66 root slots in one real module. A single FP/SP
        // bit cannot express that, which is what forced the 2-bit base tag.
        let bytes = one_map(0x1000, &[(0x10, vec![(19, -40), (29, -8)], false)]);
        let (_, roots) = parse_gc_map(&bytes).expect("valid map");
        assert_eq!(
            roots,
            vec![
                StackMapLocation {
                    dwarf_reg: 19,
                    offset: -40
                },
                StackMapLocation {
                    dwarf_reg: 29,
                    offset: -8
                },
            ]
        );
    }

    #[test]
    fn an_explicit_base_register_disables_the_fast_walk() {
        // The x29-chain walker can only recover FP and SP; anything else must
        // fall back to the platform unwinder, which can.
        let index = index_records(
            vec![StackMapRecord {
                pc: 0x1000,
                function_address: 0x1000,
                stack_size: 64,
                roots_start: 0,
                roots_len: 1,
            }],
            vec![StackMapLocation {
                dwarf_reg: 19,
                offset: -40,
            }],
        );
        assert!(!index.chain_walkable);
    }

    #[test]
    fn rejects_a_blob_built_for_the_other_pointer_width() {
        // The header records the width the emitter used. A blob claiming the
        // other width would have every function address misread, so it must be
        // refused rather than decoded — watchOS `arm64_32` is ILP32 while every
        // other supported target is LP64.
        let mut bytes = simple(0x1000, 0x10, -8);
        let flags = u16::from_le_bytes([bytes[6], bytes[7]]);
        bytes[6..8].copy_from_slice(&(flags ^ 1).to_le_bytes());
        assert!(
            parse_gc_map(&bytes).is_none(),
            "a map built for the other pointer width must be refused"
        );
    }

    #[test]
    fn rejects_a_blob_whose_length_cannot_advance_the_cursor() {
        // `total_len` comes straight from the header. A zero (or too-small)
        // value leaves `base` where it was, and because the magic still
        // matches there the resync path never runs — the loop spins forever
        // inside `OnceLock::get_or_init`, hanging the process at the first
        // collection instead of failing closed.
        let mut bytes = simple(0x1000, 0x10, -8);
        bytes[12..16].copy_from_slice(&0u32.to_le_bytes());
        assert!(
            parse_gc_map(&bytes).is_none(),
            "a blob that cannot advance the cursor must be rejected, not looped on"
        );

        // Long enough to look plausible, still short of header + function table.
        let mut bytes = simple(0x1000, 0x10, -8);
        bytes[12..16].copy_from_slice(&20u32.to_le_bytes());
        assert!(parse_gc_map(&bytes).is_none());
    }

    #[test]
    fn rejects_a_truncated_function_table() {
        // The record counts size the fixed-width offset array; a short read
        // there must not be rounded down to zero, or every later varint is
        // decoded from the wrong offset.
        let bytes = simple(0x1000, 0x10, -8);
        let truncated = &bytes[..20];
        assert!(parse_gc_map(truncated).is_none());
    }

    #[test]
    fn rejects_truncated_or_wrong_version_sections() {
        assert!(parse_gc_map(&[]).is_none() || parse_gc_map(&[]).unwrap().0.is_empty());
        let mut bytes = simple(0x1000, 0x10, -8);
        bytes[4] = GC_MAP_VERSION + 1;
        assert!(
            parse_gc_map(&bytes).is_none(),
            "an unknown version must not be guessed at"
        );
        // A total_len that runs past the section must fail rather than read on.
        let mut bytes = simple(0x1000, 0x10, -8);
        let len = bytes.len();
        bytes[12..16].copy_from_slice(&((len as u32) + 64).to_le_bytes());
        assert!(parse_gc_map(&bytes).is_none());
    }

    #[test]
    fn chain_walkable_index_accepts_fp_and_sp_locations_only() {
        let rec = |pc: usize| StackMapRecord {
            pc,
            function_address: pc,
            stack_size: 160,
            roots_start: 0,
            roots_len: 1,
        };
        // FP and SP are both walkable: SP resolves per frame by decoding the
        // owning function's prologue (#7173).
        let walkable = index_records(
            vec![rec(0x1000), rec(0x2000)],
            vec![
                StackMapLocation {
                    dwarf_reg: DWARF_REG_FP_AARCH64,
                    offset: -8,
                },
                StackMapLocation {
                    dwarf_reg: DWARF_REG_SP_AARCH64,
                    offset: -8,
                },
            ],
        );
        assert!(walkable.chain_walkable);
        assert_eq!(walkable.min_pc, 0x1000);
        assert_eq!(walkable.max_pc, 0x2000);
        // Any other register disqualifies the whole image.
        assert!(
            !index_records(
                vec![rec(0x1000)],
                vec![StackMapLocation {
                    dwarf_reg: 1,
                    offset: -8
                }],
            )
            .chain_walkable,
            "a non-FP/SP register must disable the fast walk"
        );
    }

    #[test]
    fn rejects_a_record_from_an_adjacent_function() {
        // A safepoint at the end of A must not be matched for an `ip` early in
        // B just because it falls inside the +-16 window: the walker would use
        // A's frame offsets against B's frame.
        let index = index_records(
            vec![
                StackMapRecord {
                    pc: 0x1ffc,
                    function_address: 0x1000,
                    stack_size: 32,
                    roots_start: 0,
                    roots_len: 1,
                },
                StackMapRecord {
                    pc: 0x2040,
                    function_address: 0x2000,
                    stack_size: 32,
                    roots_start: 0,
                    roots_len: 1,
                },
            ],
            vec![StackMapLocation {
                dwarf_reg: 29,
                offset: -8,
            }],
        );
        // 0x2004 is 8 bytes past A's last safepoint but lives in B.
        assert!(
            index.match_records(0x2004).is_empty(),
            "a record from the previous function must not match"
        );
        // A same-function near-match is still accepted — requiring an exact pc
        // would drop it, and the measured suite has one.
        assert_eq!(index.match_records(0x2038).len(), 1);
    }

    #[test]
    fn matches_plain_maps_before_and_statepoints_after_unwinder_ips() {
        let rec = |pc: usize| StackMapRecord {
            pc,
            function_address: pc,
            stack_size: 32,
            roots_start: 0,
            roots_len: 0,
        };
        let maps = vec![rec(0x1000), rec(0x1020)];
        assert_eq!(closest_record_pc(&maps, 0x1004), Some(0x1000));
        assert_eq!(closest_record_pc(&maps, 0x101c), Some(0x1020));
        assert_eq!(closest_record_pc(&maps, 0x1020), Some(0x1020));
    }
}

#[cfg(all(test, target_arch = "aarch64"))]
mod fp_offset_trailing_sub_tests {
    use super::fp_to_sp_offset;

    /// Assemble a prologue into executable-ish memory and decode it. The
    /// decoder only reads words, so a plain aligned buffer is enough.
    fn decode(words: &[u32]) -> Option<usize> {
        let buf = words.to_vec().into_boxed_slice();
        let addr = buf.as_ptr() as usize;
        let out = fp_to_sp_offset(addr);
        drop(buf);
        out
    }

    const ADD_X29_SP_0X90: u32 = 0x9102_43FD; // add x29, sp, #0x90
    const SUB_SP_SP_0X170: u32 = 0xD105_C3FF; // sub sp, sp, #0x170
    const RET: u32 = 0xD65F_03C0;
    const NOP: u32 = 0xD503_201F;

    /// #7328: `add x29, sp, #imm` is not always the last stack adjustment.
    /// LLVM emits a further `sub sp, sp, #N` after establishing the frame
    /// pointer, and reading only the `add` left the fast walker N bytes high
    /// on every slot in that frame — a silent wrong answer, since the walker
    /// then enumerated addresses the collector treated as roots.
    #[test]
    fn a_sub_after_the_fp_setup_is_included() {
        assert_eq!(
            decode(&[ADD_X29_SP_0X90, SUB_SP_SP_0X170, NOP, RET]),
            Some(0x90 + 0x170),
            "the trailing `sub sp, sp, #0x170` must be added to the fp offset"
        );
    }

    /// The common shape — fp established last — must be unchanged.
    #[test]
    fn a_prologue_with_no_trailing_sub_is_unchanged() {
        assert_eq!(decode(&[ADD_X29_SP_0X90, NOP, RET]), Some(0x90));
    }

    /// Only a contiguous run of `sub sp` immediately after the `add` counts.
    /// A later `sub sp` is a body operation (dynamic alloca, call-argument
    /// area) already accounted for by the stack map's own slot offsets.
    #[test]
    fn a_sub_after_the_prologue_run_is_not_counted() {
        assert_eq!(
            decode(&[ADD_X29_SP_0X90, NOP, SUB_SP_SP_0X170, RET]),
            Some(0x90),
            "a `sub sp` separated from the prologue run must not be folded in"
        );
    }

    /// A leaf that never sets up fp still fails closed, so the caller falls
    /// back to the platform unwinder rather than inventing an offset.
    #[test]
    fn a_leaf_without_fp_setup_still_fails_closed() {
        assert_eq!(decode(&[NOP, RET]), None);
    }
}
