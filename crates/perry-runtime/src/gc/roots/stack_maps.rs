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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{OnceLock, RwLock, RwLockReadGuard};

/// Magic and version of the compact map the compiler emits
/// (`perry-codegen/src/gc_map.rs`). LLVM's own stack-map section is rewritten
/// at assembly time and never reaches the binary: >50% of it was the
/// statepoint constant preamble and base/derived duplicates that this parser
/// discarded anyway, and shipping it cost 3.9 MB on a real application.
const GC_MAP_MAGIC: &[u8; 4] = b"PGCM";
/// v5: the function table is followed by a `u32 stream_offset` per function,
/// so ONE function's records can be found without decoding every function
/// before it. The record stream and the instruction-offset array are byte for
/// byte what v4 emitted; this is an added array and nothing else, costing
/// +1.3% of the section (+0.1% of a claude-code binary).
///
/// It is what makes the index lazy per FRAME rather than merely deferred:
/// v4 had to decode 2,078,970 records into a ~117 MB index to answer the 74
/// record lookups a `cc --help` run makes.
///
/// v4 (#7803): records carry DERIVED (interior) pointer slots paired with
/// their base roots — the for-of element cursors the RS4GC prelude hoists
/// across polls. v3 collapsed those pairs, so this walker chased
/// `&elements[i]` as an object start and never rewrote it as `base' + delta`
/// after a move. Version mismatch still fails closed (the parser returns
/// None and `stack_maps()` panics), so an older binary cannot run on this
/// runtime half-understood.
const GC_MAP_VERSION: u8 = 5;
const MAX_SAFEPOINT_RETURN_DELTA: usize = 16;
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct StackMapLocation {
    dwarf_reg: u16,
    offset: i32,
}

/// One derived (interior) pointer slot: `slot` holds `base + delta` for the
/// base root at `base_index` within the same record's roots range.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct StackMapDerived {
    base_index: u32,
    slot: StackMapLocation,
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
    /// Half-open range into `StackMapIndex::derived`, same sharing scheme.
    derived_start: u32,
    derived_len: u32,
}

/// The map, as the root scan reads it.
///
/// `functions` is the whole index in the default configuration: one 32-byte
/// entry per function that has records, sorted by address — 2.3 MB for
/// claude-code, replacing the ~117 MB of materialised records v4 built. A
/// frame's live set is decoded from the section when a walker asks for it,
/// which for a `cc --help` run is 74 records out of 2,078,970.
///
/// `eager` is the v4 index, retained and shipped rather than deleted: it backs
/// `PERRY_GC_STACK_MAP_EAGER=1`, and it is the oracle
/// `PERRY_GC_STACK_MAP_CROSSCHECK=1` compares every frame against. It is
/// `None` unless one of those is on.
#[derive(Debug, Default)]
struct StackMapIndex {
    /// How this index was built. Carried on the index rather than re-read from
    /// the environment at every lookup: the published index is the authority
    /// on whether it has an oracle to check itself against, and a test can
    /// build one without racing a process-wide `OnceLock`.
    mode: IndexMode,
    /// The loaded sections, indexed by `FunctionEntry::section`. Held so a
    /// function's bytes are reachable without storing a pointer per function.
    sections: Vec<&'static [u8]>,
    /// Sorted by address. Duplicates are kept, not deduplicated: two entries
    /// can share a relocated address and each brings its own records.
    functions: Vec<lazy::FunctionEntry>,
    eager: Option<EagerIndex>,
}

/// The v4 whole-section index.
#[derive(Debug, Default)]
struct EagerIndex {
    records: Vec<StackMapRecord>,
    /// Every root slot, referenced by `StackMapRecord`'s range. Shared between
    /// records whose live sets are identical.
    roots: Vec<StackMapLocation>,
    /// Every derived slot, referenced by `StackMapRecord`'s derived range.
    derived: Vec<StackMapDerived>,
    /// Sorted, deduplicated start address of every function that has records.
    function_starts: Vec<usize>,
}

impl EagerIndex {
    fn locations(&self, record: &StackMapRecord) -> &[StackMapLocation] {
        let start = record.roots_start as usize;
        let end = start + record.roots_len as usize;
        self.roots.get(start..end).unwrap_or(&[])
    }

    fn derived_locations(&self, record: &StackMapRecord) -> &[StackMapDerived] {
        let start = record.derived_start as usize;
        let end = start + record.derived_len as usize;
        self.derived.get(start..end).unwrap_or(&[])
    }
}

/// All maps visible to this runtime provider.
///
/// A provider can outlive any one app image, and hosts may `dlopen` another
/// app after the first call to `js_gc_init`. Keep the index replaceable so
/// each module initialization can take a fresh loader snapshot. Root scans
/// only take the read side; rebuilding and ELF/Mach-O parsing therefore stay
/// outside the collector's allocation-free critical section.
#[derive(Debug)]
struct PublishedStackMapIndex {
    generation: u64,
    index: StackMapIndex,
}

struct StackMapIndexStore {
    next_generation: AtomicU64,
    published: OnceLock<RwLock<PublishedStackMapIndex>>,
}

impl StackMapIndexStore {
    const fn new() -> Self {
        Self {
            next_generation: AtomicU64::new(0),
            published: OnceLock::new(),
        }
    }

    fn rebuild(&self) {
        self.rebuild_with(build_stack_map_index);
    }

    fn rebuild_with(&self, build: impl FnOnce() -> StackMapIndex) {
        // Reserve before taking the loader snapshot. Module initialization
        // starts only after that module has been loaded, so generation order
        // is also the minimum loader recency each rebuild must preserve.
        let generation = self
            .next_generation
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |generation| {
                generation.checked_add(1)
            })
            .unwrap_or_else(|_| panic!("perry: stack-map rebuild generation overflow"))
            + 1;
        let mut candidate = Some(PublishedStackMapIndex {
            generation,
            index: build(),
        });
        let maps = self.published.get_or_init(|| {
            RwLock::new(
                candidate
                    .take()
                    .expect("perry: initial stack-map candidate missing"),
            )
        });
        let Some(candidate) = candidate else {
            return;
        };
        let mut current = maps
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if candidate.generation > current.generation {
            *current = candidate;
        }
    }

    fn read(&self) -> RwLockReadGuard<'_, PublishedStackMapIndex> {
        self.published
            .get_or_init(|| {
                RwLock::new(PublishedStackMapIndex {
                    generation: 0,
                    index: StackMapIndex::default(),
                })
            })
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

static STACK_MAPS: StackMapIndexStore = StackMapIndexStore::new();

/// Set by [`initialize`], read by the root scan.
///
/// The scan resolves native frame roots through the stack-map index, and an
/// index that was never built is INDISTINGUISHABLE downstream from an image
/// with no native roots: both are empty. The consequences are not the same.
/// With statepoints as the only root mechanism, scanning against an unbuilt
/// index means the collector finds no roots, frees live objects, and corrupts
/// the heap with no diagnostic — the same failure `build_stack_map_index`
/// already refuses to degrade into, asserted here at the consuming end.
///
/// It cannot fire today: `js_gc_init` builds the index eagerly, before any
/// mutator code runs. It is here so that it CAN fire if the build is ever made
/// lazy and a path into the scan is missed — turning a silent, delayed heap
/// corruption into a loud abort on the first test that reaches it. Landing it
/// while the build is still eager is deliberate: it proves the check is placed
/// on the path it claims to guard, before anything depends on it.
static STACK_MAPS_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Whether [`initialize`] has run. Exposed so a future lazy build can assert
/// the same property at whatever point it chooses to construct the index.
pub(in crate::gc) fn stack_maps_initialized() -> bool {
    STACK_MAPS_INITIALIZED.load(Ordering::Acquire)
}

/// Does any loaded image carry a gc-map section — i.e. are there native frame
/// roots for the scan to miss?
///
/// This inspects the loader's image list; it does not decode anything, and the
/// answer is cached because it cannot change for a process once its images are
/// loaded. An inspection error answers `true`, so an unreadable loader makes
/// the assert stricter rather than weaker.
fn image_has_stack_map_sections() -> bool {
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        loaded_stack_map_sections()
            .map(|sections| !sections.is_empty())
            .unwrap_or(true)
    })
}

// #7803 creation-cycle verifier (diagnostic, `PERRY_GC_NATIVE_SLOT_VERIFY=1`).
//
// Runs a SECOND, non-rewriting native-slot walk after the rewrite passes of
// a copying minor, while from-space is still classifiable, and aborts on the
// FIRST slot still naming a from-space address. A stale native slot whose
// target later becomes unclassifiable is skipped silently by every ordinary
// walk (`mark_addr` returns `None`), so the cycle that CREATED the staleness
// never printed anything — this names it, with the owning frame from the
// pin-latch context.
//
// `//` not `///`: rustdoc discards a doc comment on a macro invocation, and
// `rustc-warnings` runs with `-D warnings`, so `///` here is a hard error in
// that job (#8176). The text is worth keeping, so keep it as a plain comment.
crate::perry_thread_local! {
    /// The rewrite walk's stats for the CURRENT cycle, published so the
    /// #7803 native-slot verifier can compare its own traversal against the
    /// one that was supposed to rewrite (a rewrite walk that stopped early
    /// and a verify walk that did not is the difference between "slot
    /// skipped" and "slot unrewritable").
    static LAST_REWRITE_WALK: std::cell::Cell<(usize, usize, usize)> =
        const { std::cell::Cell::new((0, 0, 0)) };
}

pub(in crate::gc) fn publish_rewrite_walk_stats(stats: &NativeStackWalkStats) {
    LAST_REWRITE_WALK.with(|c| {
        c.set((
            stats.frames_visited,
            stats.records_matched,
            stats.locations_visited,
        ))
    });
}

pub(in crate::gc) fn verify_native_slots_post_walk(
    untraced: bool,
    classify: &dyn Fn(usize) -> String,
) {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    if !*ON.get_or_init(|| {
        matches!(
            std::env::var("PERRY_GC_NATIVE_SLOT_VERIFY").ok().as_deref(),
            Some("1") | Some("on") | Some("true")
        )
    }) {
        return;
    }
    let _phase = super::super::pin::CopyingWalkPhaseGuard::enter("native_slot_verify");
    let rewrite_stats = LAST_REWRITE_WALK.with(|c| c.get());
    let mut verify_frames = 0usize;
    let verify_stats = visit_stack_map_root_slots(&mut |slot| unsafe {
        verify_frames += 1;
        let bits = *slot.ptr;
        let Some(word) = super::super::root_words::decode_root_word(bits) else {
            return;
        };
        let target = word.addr();
        if !super::super::fromspace_scan::is_from_space(crate::arena::classify_heap_space(target)) {
            return;
        }
        let context = super::super::pin::native_root_slot_context();
        // Break the §40 contradiction: dump EVERY record `match_records`
        // returns for this ip (the ±16 window can match several) and every
        // slot value of each, resolving SP-relative addresses from the known
        // victim slot (base = slot_addr - offset). A double-matched frame or
        // a mis-attributed neighbor slot is visible here in one abort.
        if let Some(ctx) = context {
            // main's #8081 made the published index a guard; the walk reads
            // through its `index` field.
            let published = stack_maps();
            let index = &published.index;
            let base = ctx.slot_addr.wrapping_sub(ctx.offset as usize);
            let matched = index.match_records(ctx.ip);
            for (function_address, roots, derived) in index.materialise(matched.as_ref()) {
                eprintln!(
                    "[gc-native-slot-verify]   record in fn {function_address:#x} \
                     (ip fn+{:#x}):",
                    ctx.ip.wrapping_sub(function_address),
                );
                for location in &roots {
                    let addr = if location.offset < 0 {
                        base.wrapping_sub(location.offset.unsigned_abs() as usize)
                    } else {
                        base.wrapping_add(location.offset as usize)
                    };
                    let word = if location.dwarf_reg == ctx.dwarf_reg && addr % 8 == 0 {
                        format!("{:#018x}", *(addr as *const u64))
                    } else {
                        "<other base reg>".to_string()
                    };
                    eprintln!(
                        "[gc-native-slot-verify]     reg={} offset={} addr={addr:#x} word={word}",
                        location.dwarf_reg, location.offset,
                    );
                }
                for entry in &derived {
                    eprintln!(
                        "[gc-native-slot-verify]     DERIVED base_index={} reg={} offset={}",
                        entry.base_index, entry.slot.dwarf_reg, entry.slot.offset,
                    );
                }
            }
        }
        panic!(
            "[gc-native-slot-verify] a native stack-map slot still names a from-space \
             address AFTER this cycle's rewrite passes: slot={:#x} word={bits:#018x} \
             target={target:#x} target_space={:?} untraced_cycle={untraced} \
             rewrite_walk(frames,records,locations)={rewrite_stats:?} \
             collector_classify={} raw_header={:#018x} payload0={:#018x} \
             context={context:?} — this is the CREATION cycle of the stale slot the \
             pin-latch only catches many cycles later (#7803)",
            slot.ptr as usize,
            crate::arena::classify_heap_space(target),
            classify(target),
            *((target - 8) as *const u64),
            *(target as *const u64),
        );
    });
    let _ = (verify_frames, verify_stats);
}

/// Upper bound on how far a derived pointer may sit from its base before the
/// rewrite refuses to touch it. LLVM only pairs a derived pointer with the
/// base it was actually derived from, so a delta beyond any plausible object
/// means the map and this frame disagree — leave the slot alone rather than
/// manufacture an address. 64 MiB is far above the largest movable object
/// (`MAX_YOUNG_MOVE_BYTES` is 1 MiB) without being "any bits at all".
const MAX_DERIVED_DELTA: usize = 64 << 20;

/// Visit one record's base roots, then rewrite its DERIVED (interior) slots
/// as `new_base + (old_derived - old_base)` (#7803).
///
/// The order inside is the contract: old base words are captured BEFORE the
/// visitor runs (the visitor rewrites base slots in place), and the derived
/// slots are never handed to the visitor at all — a derived pointer is not an
/// object start, and treating it as one is exactly the defect the v4 map
/// exists to end (the collector chased `&elements[i]`, latched on element
/// bytes as a "header", and left the cursor pointing into from-space after a
/// move).
///
/// `resolve` maps a location to `(slot_address, base_register_value)` for
/// THIS frame; both walkers pass their own base math in. A visitor that does
/// not rewrite (the verify walker's collection passes) leaves base words
/// unchanged, which makes every derived rewrite a no-op by construction.
unsafe fn visit_record_slots(
    record: &lazy::DecodedRecord,
    ip: usize,
    resolve: &mut dyn FnMut(&StackMapLocation) -> Option<(usize, usize)>,
    stats: &mut NativeStackWalkStats,
    visit: &mut dyn FnMut(ResolvedRoot),
) {
    let slot_ok = |address: usize| address != 0 && address & (align_of::<u64>() - 1) == 0;
    let function_address = record.function_address;

    // Old base words, captured before the visitor rewrites anything. Only
    // needed when the record has derived slots — the common record (95.7% of
    // claude-code's) pays nothing, and reads the root list exactly once.
    let mut old_base: Vec<Option<(usize, u64)>> = Vec::new();
    if record.payload.derived_len > 0 {
        old_base.reserve(record.payload.roots_len as usize);
        let mut roots = record.roots();
        while let Some(location) = roots.next() {
            let Some(location) = location else {
                lazy::malformed(function_address)
            };
            old_base.push(resolve(&location).and_then(|(address, _)| {
                slot_ok(address).then(|| (address, *(address as *const u64)))
            }));
        }
    }

    let mut roots = record.roots();
    while let Some(location) = roots.next() {
        let Some(location) = location else {
            lazy::malformed(function_address)
        };
        stats.locations_visited = stats.locations_visited.saturating_add(1);
        let Some((address, base)) = resolve(&location) else {
            continue;
        };
        if !slot_ok(address) {
            continue;
        }
        visit(ResolvedRoot {
            address,
            ip,
            function_address,
            dwarf_reg: location.dwarf_reg,
            offset: location.offset,
            base,
        });
    }

    if record.payload.derived_len == 0 {
        return;
    }
    let mut bases = record.derived_base_indices();
    let Some(mut slots) = record.derived_slots() else {
        lazy::malformed(function_address)
    };
    while let Some(base_index) = bases.next() {
        let (Some(base_index), Some(slot)) = (base_index, slots.next()) else {
            lazy::malformed(function_address)
        };
        let Some(slot) = slot else {
            lazy::malformed(function_address)
        };
        stats.locations_visited = stats.locations_visited.saturating_add(1);
        let Some((derived_addr, _)) = resolve(&slot) else {
            continue;
        };
        if !slot_ok(derived_addr) {
            continue;
        }
        let Some(Some((base_addr, old_base_word))) = old_base.get(base_index as usize).copied()
        else {
            continue;
        };
        rewrite_derived_slot(derived_addr, base_addr, old_base_word);
    }
}

/// What one record's slots need from the frame, decided in a single pass.
///
/// v4 answered `chain_walkable` once for the whole image by scanning every
/// root slot in the section. A lazy index cannot, and should not: deciding it
/// per record is strictly narrower — one function using an exotic base no
/// longer disables the fast walk everywhere — and it fails closed the same
/// way, by abandoning the walk to the platform unwinder.
#[cfg(target_arch = "aarch64")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct RecordRegisters {
    /// Every base is one the x29 chain can reconstruct: FP directly, SP and
    /// x19 from the decoded prologue.
    chain_walkable: bool,
    uses_x19: bool,
    /// Any base other than the frame pointer, i.e. any slot that needs the
    /// reconstructed body SP.
    any_non_fp: bool,
}

/// Returns `None` on a malformed stream so the caller fails closed rather than
/// deciding from a partial list.
#[cfg(target_arch = "aarch64")]
fn record_slot_registers(record: &lazy::DecodedRecord) -> Option<RecordRegisters> {
    let mut out = RecordRegisters {
        chain_walkable: true,
        uses_x19: false,
        any_non_fp: false,
    };
    let mut check = |location: StackMapLocation| {
        if location.dwarf_reg == DWARF_REG_X19_AARCH64 {
            out.uses_x19 = true;
        }
        if location.dwarf_reg != DWARF_REG_FP_AARCH64 {
            out.any_non_fp = true;
        }
        if !matches!(
            location.dwarf_reg,
            DWARF_REG_FP_AARCH64 | DWARF_REG_SP_AARCH64 | DWARF_REG_X19_AARCH64
        ) {
            out.chain_walkable = false;
        }
    };
    let mut roots = record.roots();
    while let Some(location) = roots.next() {
        check(location?);
    }
    let mut slots = record.derived_slots()?;
    while let Some(location) = slots.next() {
        check(location?);
    }
    Some(out)
}

unsafe fn rewrite_derived_slot(derived_addr: usize, base_addr: usize, old_base_word: u64) {
    use super::super::root_words::decode_root_word;
    let new_base_word = *(base_addr as *const u64);
    if new_base_word == old_base_word {
        // The base did not move this cycle, so the derived offset from it is
        // still current.
        return;
    }
    let Some(old_base) = decode_root_word(old_base_word) else {
        return;
    };
    let Some(new_base) = decode_root_word(new_base_word) else {
        return;
    };
    let old_derived_word = *(derived_addr as *const u64);
    let Some(old_derived) = decode_root_word(old_derived_word) else {
        return;
    };
    let delta = old_derived.addr().wrapping_sub(old_base.addr());
    if delta > MAX_DERIVED_DELTA {
        return;
    }
    *(derived_addr as *mut u64) = old_derived.encode(new_base.addr().wrapping_add(delta));
}

// The two register numbers the compact format's short base tags stand for.
// These are aarch64's by definition of the FORMAT, on every architecture — see
// `gc_map.rs`, which deliberately keeps them literal so the compiler's idea of
// the target and this runtime's `target_arch` can never disagree.
const DWARF_REG_FP_AARCH64: u16 = 29;
const DWARF_REG_SP_AARCH64: u16 = 31;
// #8770: LLVM takes x19 as a frame base pointer for a function with a *dynamic*
// stack allocation (a VLA or a spread-argument area). The base is captured as
// `mov x19, sp` immediately after the fixed prologue and before the dynamic
// `sub sp, sp, xN`, with no realignment — so x19 holds exactly the body SP the
// fp chain reconstructs (`fp - fp_to_sp_offset`). The fast walker therefore
// resolves an x19-based root like an SP-based one, once `x19_is_body_sp` has
// confirmed that prologue shape for the owning function; a frame that does not
// match (e.g. a realigning one) fails closed to the platform unwinder. Before
// this these frames flipped the whole-image `chain_walkable` flag false and
// forced every walk onto the unwinder, whose root resolution the fast walker
// exists to avoid.
// Consulted only by the aarch64 fast walk, but documented for every target:
// the encoding it names is emitted on all of them, so the number belongs with
// the other two rather than inside a `cfg`.
#[cfg_attr(not(target_arch = "aarch64"), allow(dead_code))]
const DWARF_REG_X19_AARCH64: u16 = 19;

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
    /// Visit this slot with its provenance published for the pin-latch abort:
    /// the walker resolved the owning function, record and address, and until
    /// #7803 threw all of it away one call before the latch printed
    /// `mutable_root_slots/native_stack` with no owner. Two `Cell` stores per
    /// slot; the clear keeps a later phase from being blamed on this frame.
    fn visit_with_context(self, visit: &mut impl FnMut(MutableRootSlot)) {
        super::super::pin::set_native_root_slot_context(Some(
            super::super::pin::NativeRootSlotContext {
                ip: self.ip,
                function_address: self.function_address,
                dwarf_reg: self.dwarf_reg,
                offset: self.offset,
                slot_addr: self.address,
            },
        ));
        visit(self.slot());
        super::super::pin::set_native_root_slot_context(None);
    }

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

/// Mark the stack-map index as owed, without building it.
///
/// Decoding the gc-map section is the single largest cost in process startup
/// for a big image — 17.4MB, 72,713 functions, 2.15M records and a 117MB index
/// for claude-code — and a run that never collects never reads any of it.
/// `cc --version` is exactly that run.
///
/// The build is therefore deferred to [`ensure_built`], which every path that
/// can reach a root scan calls while allocation is still legal. Missing one of
/// those paths would mean scanning against an unbuilt index, which is
/// indistinguishable from "this image has no native roots" and frees live
/// objects silently — so the assert at the top of `visit_stack_map_root_slots`
/// (#9182) exists to turn that into a loud abort instead. It landed first, on
/// purpose.
///
/// Re-arms on every call: a newly loaded image calls `js_gc_init` again, and
/// the next collection must decode its section too.
pub(in crate::gc) fn initialize() {
    STACK_MAPS_INITIALIZED.store(false, Ordering::Release);
    if !lazy_stack_maps_enabled() {
        ensure_built();
    }
}

/// `PERRY_LAZY_STACK_MAPS=0` restores the eager decode at `js_gc_init`, for
/// A/B measurement of what the deferral is worth.
fn lazy_stack_maps_enabled() -> bool {
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_LAZY_STACK_MAPS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// Build the stack-map index if it is owed. Safe to call redundantly.
///
/// MUST be called before any path that can reach [`visit_stack_map_root_slots`],
/// and while allocation is still legal — the parser allocates its index once,
/// and root scans must stay allocation-free while the collector owns the heap.
pub(in crate::gc) fn ensure_built() {
    if STACK_MAPS_INITIALIZED.load(Ordering::Acquire) {
        return;
    }
    STACK_MAPS.rebuild();
    // Publish AFTER the rebuild: a reader that observes the flag must be able
    // to observe the index it promises.
    STACK_MAPS_INITIALIZED.store(true, Ordering::Release);
}

/// Whether this image carries any native stack-map records — i.e. whether
/// precise frame roots depend on mapped PCs at all. Consumed by the
/// `PERRY_GC_SAFEPOINT_ONLY` contract assert.
pub(in crate::gc) fn native_maps_active() -> bool {
    !stack_maps().index.is_empty()
}

fn stack_maps() -> RwLockReadGuard<'static, PublishedStackMapIndex> {
    STACK_MAPS.read()
}

/// Which index the collector reads. One binary, switchable, so the lazy path
/// can be A/B'd and cross-checked against the code it replaced.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum IndexMode {
    #[default]
    /// Default: per-function decode, nothing materialised.
    Lazy,
    /// `PERRY_GC_STACK_MAP_EAGER=1` — build the v4 whole-section index TOO,
    /// so its cost can be measured inside one binary. It does not change what
    /// the walkers read; the lazy index still answers every lookup.
    BuildBoth,
    /// `PERRY_GC_STACK_MAP_CROSSCHECK=1` — build both and assert, for every
    /// frame the collector walks, that they name the same roots.
    CrossCheck,
}

fn index_mode() -> IndexMode {
    static MODE: OnceLock<IndexMode> = OnceLock::new();
    *MODE.get_or_init(|| {
        if crate::gc::env_flag_enabled("PERRY_GC_STACK_MAP_CROSSCHECK") {
            IndexMode::CrossCheck
        } else if crate::gc::env_flag_enabled("PERRY_GC_STACK_MAP_EAGER") {
            IndexMode::BuildBoth
        } else {
            IndexMode::Lazy
        }
    })
}

fn build_stack_map_index() -> StackMapIndex {
    // No section at all is the ordinary shadow-stack build: there are no
    // native frame roots to find, and an empty index is the right answer.
    let sections = loaded_stack_map_sections().unwrap_or_else(|error| {
        panic!(
            "perry: could not inspect every loaded image for native GC roots: {error}; \
             refusing to publish an incomplete stack-map index"
        )
    });
    if sections.is_empty() {
        return StackMapIndex::default();
    }
    build_index_from_sections(sections, index_mode())
}

/// Build the index from already-located sections. Shared with the tests, so
/// what they exercise is the path the collector takes.
fn build_index_from_sections(sections: Vec<&'static [u8]>, mode: IndexMode) -> StackMapIndex {
    // A section that exists but does not decode is a different thing
    // entirely, and it must never degrade to "no roots". The two failure
    // shapes are indistinguishable downstream — both yield an empty index
    // — but their consequences are not: with statepoints as the only root
    // mechanism, an empty index means the collector frees live objects and
    // corrupts the heap with no diagnostic at all. That is CLAUDE.md's
    // fourth gate-failure mode (the gate runs, its subject never did), so
    // fail loudly instead. In practice this can only mean a binary whose
    // compiler and runtime disagree about the map format.
    let mut functions = Vec::new();
    for (index, section) in sections.iter().enumerate() {
        let section_index = u16::try_from(index).unwrap_or_else(|_| {
            panic!("perry: {} loaded images carry a GC map section; the index addresses them with a u16", sections.len())
        });
        if lazy::parse_function_table(section_index, section, &mut functions).is_none() {
            undecodable_section(section.len());
        }
    }
    // Sorted by address, duplicates KEPT. `match_records` resolves the whole
    // run of equal addresses: two object files can emit a map for the same
    // symbol, or the linker can fold identical code, and each entry brings its
    // own records. Deduplicating would drop one set silently.
    functions.sort_unstable_by_key(|entry| entry.address);

    let eager = match mode {
        IndexMode::Lazy => None,
        IndexMode::BuildBoth | IndexMode::CrossCheck => Some(build_eager_index(&sections)),
    };
    StackMapIndex {
        mode,
        sections,
        functions,
        eager,
    }
}

/// The default-configuration build, for tests that only need the lazy index.
#[cfg(test)]
fn build_index_from_sections_lazy(sections: Vec<&'static [u8]>) -> StackMapIndex {
    build_index_from_sections(sections, IndexMode::Lazy)
}

fn build_eager_index(sections: &[&'static [u8]]) -> EagerIndex {
    let mut records = Vec::new();
    let mut roots = Vec::new();
    let mut derived = Vec::new();
    for section in sections {
        if append_gc_map_section(&mut records, &mut roots, &mut derived, section).is_none() {
            undecodable_section(section.len());
        }
    }
    records.sort_unstable_by_key(|record| record.pc);
    index_records(records, roots, derived)
}

#[cold]
fn undecodable_section(len: usize) -> ! {
    panic!(
        "perry: a GC map section (__perry_gcmap / .perry_gcmap, {len} bytes) is \
         present but could not be decoded — expected format {:?} v{}. This binary's \
         compiler and runtime disagree about the map layout; continuing would run \
         the collector with missing roots and corrupt the heap silently.",
        std::str::from_utf8(GC_MAP_MAGIC).unwrap_or("PGCM"),
        GC_MAP_VERSION,
    );
}

fn append_gc_map_section(
    records: &mut Vec<StackMapRecord>,
    roots: &mut Vec<StackMapLocation>,
    derived: &mut Vec<StackMapDerived>,
    section: &[u8],
) -> Option<()> {
    let (mut section_records, section_roots, section_derived) = parse_gc_map(section)?;
    let root_base = u32::try_from(roots.len()).ok()?;
    let derived_base = u32::try_from(derived.len()).ok()?;
    for record in &mut section_records {
        record.roots_start = record.roots_start.checked_add(root_base)?;
        record.derived_start = record.derived_start.checked_add(derived_base)?;
    }
    records.append(&mut section_records);
    roots.extend(section_roots);
    derived.extend(section_derived);
    Some(())
}

fn index_records(
    records: Vec<StackMapRecord>,
    roots: Vec<StackMapLocation>,
    derived: Vec<StackMapDerived>,
) -> EagerIndex {
    // SP-relative locations are admitted here and resolved per FRAME in the
    // walker, which decodes the owning function's `add x29, sp, #imm`
    // prologue to get the body SP (#7173). Deciding it here would mean
    // dereferencing every function address at startup — unsafe for records
    // whose addresses are not live code, and unnecessary because the walker
    // already fails closed to the platform unwinder on any anomaly.
    let mut function_starts: Vec<usize> = records
        .iter()
        .map(|record| record.function_address)
        .collect();
    function_starts.sort_unstable();
    function_starts.dedup();
    EagerIndex {
        records,
        roots,
        derived,
        function_starts,
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
                // #7984: an SVE stack adjustment scales by the RUNTIME vector
                // length, which is nowhere in the instruction. There is no
                // correct number to return, so return none of one — the
                // caller falls back to the platform unwinder, which reads the
                // frame's DWARF CFI and does not need VG for an fp-based
                // frame.
                //
                // This is not hypothetical and it is not rare. Perry tunes a
                // host build with `-mcpu=native`; on any Neoverse-class core
                // that turns SVE on, and LLVM then emits the module body's
                // prologue as (measured on `01_nursery_churn`, aarch64 Linux,
                // `-mcpu=neoverse-n2`):
                //
                //     add   x29, sp, #0x20     <- fp established here
                //     stp   x28, x27, [sp, #48]
                //     ... four more callee-save pairs ...
                //     sub   sp, sp, #0x50      <- 80 bytes
                //     addvl sp, sp, #-2        <- and 2 x VL more
                //
                // The same probe built `-mcpu=neoverse-n1` has neither the
                // interleaved stores nor the `addvl`, which is why this was an
                // ARM-Linux-runner-only failure that no macOS arm could see.
                if writes_sp_by_vector_length(word) {
                    // The multiplier is in the instruction; the unit is not.
                    // Read it once from the kernel — `?` fails the whole
                    // decode where it cannot be read, because half a frame
                    // size is a wrong answer, not a partial one.
                    fp_offset = Some(offset + sve_sp_allocation_bytes(word)?);
                    continue;
                }
                // A store INTO the frame does not move sp, so it cannot end
                // the run of stack adjustments — and LLVM interleaves exactly
                // these between the frame-pointer setup and the local-area
                // allocation in the shape above. Treating one as the end of
                // the prologue is what made the decoder report 0x20 for a
                // frame whose body SP is 144 bytes below the frame pointer,
                // placing every SP-relative root in it 112 bytes too high.
                if is_frame_store_through_sp(word) {
                    continue;
                }
                // Anything else ends the prologue. Something later that
                // touches sp is a body operation (a dynamic alloca, a
                // call-argument area) which the stack map's own offsets
                // already account for. A frame that needs a base pointer for
                // either reason records its roots against x19 — and x19 is
                // captured as `mov x19, sp` right here, at the end of the fixed
                // prologue, so it equals the body SP this function returns.
                // `x19_is_body_sp` confirms that shape and the fast walker then
                // resolves those roots off this same offset (#8770); it is no
                // longer true that the walker never sees an x19 frame.
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

/// True iff `function_address` establishes its x19 frame base as `mov x19, sp`
/// after only the fixed stack adjustments `fp_to_sp_offset` already folds in —
/// the shape (#8770) in which x19 equals the body SP the fp chain reconstructs,
/// so an x19-based root resolves exactly like an SP-based one at the same
/// offset.
///
/// LLVM takes a base pointer (x19) for a frame with a *dynamic* stack
/// allocation and captures it right after the fixed prologue, before the
/// dynamic `sub sp, sp, xN`; that capture is `mov x19, sp` (`add x19, sp, #0`,
/// 0x9100_03F3). A *realigning* frame instead masks SP (`and sp, sp, #-align`)
/// before taking the base, and a base captured after a `sub sp, sp, xN` sits
/// below a dynamic adjustment — in both cases x19 is a runtime SP the chain
/// cannot reconstruct, so this returns false and the caller fails closed to the
/// platform unwinder, exactly as for any other frame the fast walk cannot
/// resolve. The accepted set between the frame-pointer setup and the base
/// capture is therefore precisely the one `fp_to_sp_offset` accumulates.
#[cfg(all(
    any(target_vendor = "apple", target_os = "linux"),
    target_arch = "aarch64"
))]
fn x19_is_body_sp(function_address: usize) -> bool {
    // `add x19, sp, #0` — the base-pointer capture. `mov x19, sp` assembles to
    // exactly this (Rn=sp=31, Rd=x19=19, imm=0).
    const MOV_X19_SP: u32 = 0x9100_03F3;
    const ADD_FP_SP_MASK: u32 = 0xFF80_03FF;
    const ADD_FP_SP_PATTERN: u32 = 0x9100_03FD;
    const SUB_SP_SP_MASK: u32 = 0xFF80_03FF;
    const SUB_SP_SP_PATTERN: u32 = 0xD100_03FF;
    const PROLOGUE_WINDOW_INSNS: usize = 24;
    if function_address == 0 || function_address & 0x3 != 0 {
        return false;
    }
    let mut fp_set = false;
    for i in 0..PROLOGUE_WINDOW_INSNS {
        let word = unsafe { std::ptr::read((function_address + i * 4) as *const u32) };
        if word == MOV_X19_SP {
            // The base is captured from sp; it equals the reconstructed body SP
            // only once the frame pointer — the walker's anchor — is set.
            return fp_set;
        }
        if !fp_set {
            fp_set = word & ADD_FP_SP_MASK == ADD_FP_SP_PATTERN;
            // Everything before the frame pointer is set (the callee-save
            // stores, the initial pre-index `stp`) leaves the fp<->sp
            // relationship `fp_to_sp_offset` reconstructs intact, so it may
            // precede the base capture.
            continue;
        }
        // After the frame pointer is set, only the adjustments `fp_to_sp_offset`
        // itself folds in may separate it from the base capture — anything else
        // (a realigning `and sp`, a dynamic `sub sp, sp, xN`) means x19 is not
        // the SP the walker reconstructs.
        if word & SUB_SP_SP_MASK == SUB_SP_SP_PATTERN
            || writes_sp_by_vector_length(word)
            || is_frame_store_through_sp(word)
        {
            continue;
        }
        return false;
    }
    false
}

/// `stp`/`str` with SP as the base register and no writeback.
///
/// These are the callee-save spills LLVM emits, and they do not modify sp — so
/// one appearing after the frame-pointer setup says nothing about whether the
/// prologue's stack adjustments are finished. Enumerated rather than inferred:
/// an instruction this does not recognise ends the run, which is the safe
/// direction. Every opcode below was read out of a real aarch64-Linux binary
/// (`objdump -d`, `01_nursery_churn` built `-mcpu=neoverse-n2`), not from
/// memory.
#[cfg(target_arch = "aarch64")]
fn is_frame_store_through_sp(word: u32) -> bool {
    // Base register, bits [9:5]. 31 is SP in a load/store base position (it is
    // never XZR there), so no ambiguity to resolve.
    if (word >> 5) & 0x1F != u32::from(DWARF_REG_SP_AARCH64) {
        return false;
    }
    matches!(
        word & 0xFFC0_0000,
        0xA900_0000     // stp  Xt1, Xt2, [sp, #imm]   (measured: a9036ffc)
        | 0x6D00_0000   // stp  Dt1, Dt2, [sp, #imm]   (measured: 6d0123e9)
        | 0xAD00_0000   // stp  Qt1, Qt2, [sp, #imm]
        | 0xF900_0000   // str  Xt,       [sp, #imm]
        | 0xFD00_0000   // str  Dt,       [sp, #imm]
        | 0x3D80_0000 // str  Qt,       [sp, #imm]
    )
}

/// `addvl`/`addpl` writing SP — an adjustment in units of the runtime SVE
/// vector length.
///
/// The instruction carries a multiplier, not a byte count, so the frame's real
/// size is unknowable from the text. `fp_to_sp_offset` fails closed on one
/// rather than returning the unscaled figure.
///
/// Encoding, verified against `043f57df` = `addvl sp, sp, #-2` in a real
/// binary: bits [31:24] `0000_0100`, [23:21] `001`, [20:16] Rn, [15:11] `01010`
/// (`addvl`) or `01011` (`addpl`), [10:5] imm6, [4:0] Rd.
#[cfg(target_arch = "aarch64")]
fn writes_sp_by_vector_length(word: u32) -> bool {
    word & 0x1F == u32::from(DWARF_REG_SP_AARCH64)
        && matches!(word & SVE_ADD_OPCODE_MASK, SVE_ADDVL | SVE_ADDPL)
}

#[cfg(target_arch = "aarch64")]
const SVE_ADD_OPCODE_MASK: u32 = 0xFFE0_F800;
#[cfg(target_arch = "aarch64")]
const SVE_ADDVL: u32 = 0x0420_5000;
#[cfg(target_arch = "aarch64")]
const SVE_ADDPL: u32 = 0x0420_5800;

/// How many bytes an `addvl`/`addpl` writing SP takes OFF the stack.
///
/// `addvl Rd, Rn, #imm6` is `Rd = Rn + imm6 * VL`, where VL is the vector
/// length in bytes; `addpl` uses an eighth of it (the predicate length). A
/// prologue allocation is a NEGATIVE multiplier, so a non-negative one is not
/// an allocation and is refused rather than guessed at.
///
/// `None` — vector length unavailable, or not an allocation — fails the whole
/// decode, which puts the frame on the platform unwinder. Half a frame size is
/// a wrong answer, not a partial one.
#[cfg(target_arch = "aarch64")]
fn sve_sp_allocation_bytes(word: u32) -> Option<usize> {
    // imm6, bits [10:5], signed.
    let raw = ((word >> 5) & 0x3F) as i32;
    let multiplier = if raw & 0x20 != 0 { raw - 0x40 } else { raw };
    let allocation = usize::try_from(-multiplier).ok().filter(|n| *n > 0)?;
    let vector_length = sve_vector_length_bytes()?;
    match word & SVE_ADD_OPCODE_MASK {
        SVE_ADDVL => allocation.checked_mul(vector_length),
        // `addpl`'s unit is VL/8, and a vector length is always a multiple of
        // 16 bytes, so the division is exact.
        SVE_ADDPL => allocation.checked_mul(vector_length / 8),
        _ => None,
    }
}

/// The calling thread's SVE vector length in bytes.
///
/// Read from the kernel rather than executed: `rdvl` would be the direct way
/// and it faults on a core without SVE, which is most of them — including
/// every Apple one, where this returns `None` and any `addvl` in a decoded
/// prologue therefore fails closed. `prctl(PR_SVE_GET_VL)` costs one syscall,
/// answers on a thread that has never touched SVE, and is cached for the
/// process because nothing in Perry calls `PR_SVE_SET_VL`.
///
/// The walking thread is the right thread to ask: the prologue whose `addvl`
/// is being decoded executed on it, with this same length.
#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
fn sve_vector_length_bytes() -> Option<usize> {
    static VECTOR_LENGTH: OnceLock<Option<usize>> = OnceLock::new();
    *VECTOR_LENGTH.get_or_init(|| {
        // <linux/prctl.h>
        const PR_SVE_GET_VL: i32 = 51;
        const PR_SVE_VL_LEN_MASK: i32 = 0xffff;
        unsafe extern "C" {
            fn prctl(option: i32, ...) -> i32;
        }
        let raw = unsafe { prctl(PR_SVE_GET_VL) };
        // Negative is -1/errno: no SVE, or a kernel without the interface. A
        // zero length would be nonsense; refuse it rather than scale by it.
        (raw > 0).then(|| (raw & PR_SVE_VL_LEN_MASK) as usize)
    })
}

#[cfg(all(target_arch = "aarch64", not(target_os = "linux")))]
fn sve_vector_length_bytes() -> Option<usize> {
    // No non-Linux aarch64 target Perry supports implements SVE, and neither
    // backend for them emits `addvl`. Fail closed if one ever does, rather
    // than invent a length.
    None
}

pub(super) fn visit_stack_map_root_slots(
    visit: &mut impl FnMut(MutableRootSlot),
) -> NativeStackWalkStats {
    // The invariant is not "initialize ran" but "an empty index means this
    // image genuinely has no native roots". Stating it that way keeps the
    // check live in EVERY configuration: perry-runtime's unit tests reach the
    // scan without `js_gc_init`, and they pass because their harness carries
    // no gc-map section — the right reason — rather than by being exempted
    // from the check. Exempting them by build config would leave no check in
    // precisely the configuration where the index is legitimately unbuilt,
    // which is a hole the moment a test binary does carry statepoint frames.
    assert!(
        stack_maps_initialized() || !image_has_stack_map_sections(),
        "perry: the native root scan ran before the stack-map index was built. \
         An unbuilt index is indistinguishable from an image with no native \
         roots, so the collector would find no roots on this frame and free \
         live objects silently. This means a path into the root scan does not \
         pass through the point that builds the index."
    );
    let published = stack_maps();
    let index = &published.index;
    if index.is_empty() {
        return NativeStackWalkStats::default();
    }
    match walker_mode() {
        WalkerMode::Unwind => unwind::visit(index, &mut |root: ResolvedRoot| {
            root.visit_with_context(visit)
        }),
        WalkerMode::Fast => {
            // No whole-image `chain_walkable` precondition any more. v4 decided
            // it once by scanning every root slot in the section — 4.9M of them
            // for claude-code — which a lazy index cannot do and should not:
            // the fast walk now checks the frame it is about to resolve and
            // fails closed to the unwinder if THAT record uses a base it cannot
            // reconstruct. That is strictly narrower than disabling the fast
            // path for the whole image because one function somewhere uses an
            // exotic register, and it reuses the mid-walk bail this walker
            // already performs for `x19_is_body_sp` and an unvalidated
            // `caller_fp`.
            if let Some(stats) = fp_chain::visit(index, &mut |root: ResolvedRoot| {
                root.visit_with_context(visit)
            }) {
                return stats;
            }
            let mut stats = unwind::visit(index, &mut |root: ResolvedRoot| {
                root.visit_with_context(visit)
            });
            stats.fallback_walks = 1;
            stats
        }
        WalkerMode::Verify => verify::visit(index, visit),
    }
}

// Same platform set as the section loader (`stack_maps_sections.rs`): the
// Itanium unwinder personality and
// `_Unwind_*` API are present on every Apple platform, not just macOS.
#[cfg(any(target_vendor = "apple", target_os = "linux"))]
mod unwind {
    use super::*;

    type UnwindContext = crate::eh::UnwindContext;

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

    fn walk_trace_enabled() -> bool {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        // `env_flag_enabled`, not `var_os(..).is_some()`: presence-testing makes
        // `PERRY_GC_STACKMAP_TRACE=0` ENABLE the trace, which is the opposite of
        // what every other GC knob does and what anyone typing `=0` means. The
        // shared parser fails toward the knob's documented default (OFF here).
        *ON.get_or_init(|| crate::gc::env_flag_enabled("PERRY_GC_STACKMAP_TRACE"))
    }

    unsafe extern "C" fn walk_frame<F: FnMut(ResolvedRoot)>(
        context: *mut UnwindContext,
        argument: *mut c_void,
    ) -> i32 {
        let state = &mut *argument.cast::<WalkState<'_, F>>();
        state.stats.frames_visited = state.stats.frames_visited.saturating_add(1);
        let ip = _Unwind_GetIP(context);
        if walk_trace_enabled() {
            let mut info: libc::Dl_info = std::mem::zeroed();
            let name =
                if libc::dladdr(ip as *const c_void, &mut info) != 0 && !info.dli_sname.is_null() {
                    std::ffi::CStr::from_ptr(info.dli_sname)
                        .to_string_lossy()
                        .into_owned()
                } else {
                    String::from("?")
                };
            eprintln!(
                "[gc-stackmap-walk] frame {} ip={ip:#x} ({name})",
                state.stats.frames_visited
            );
        }
        let index = state.index;
        let Some(matched) = index.match_records(ip) else {
            return 0;
        };
        let mut records = index.matched(&matched);
        while let Some(record) = records.next() {
            state.stats.records_matched = state.stats.records_matched.saturating_add(1);
            let mut resolve = |location: &StackMapLocation| {
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
                address.map(|address| (address, base))
            };
            visit_record_slots(
                &record,
                ip,
                &mut resolve,
                &mut state.stats,
                &mut state.visit,
            );
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
            fn pthread_self() -> *mut c_void;
            fn pthread_get_stackaddr_np(thread: *mut c_void) -> *mut c_void;
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
        let top = stack_top();
        if top == 0 {
            return None;
        }
        let mut stats = NativeStackWalkStats {
            walks: 1,
            fp_walks: 1,
            ..NativeStackWalkStats::default()
        };
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
            // No pc pre-filter. v4 kept the image's min/max record pc to
            // reject a frame before searching a 2M-record array; the function
            // table answers containment exactly and in one binary search, and
            // a filter that is even slightly too NARROW drops a real frame's
            // roots — which is not a tradeoff worth making to save a compare.
            if let Some(matched) = index.match_records(return_address) {
                // The record describes the caller's frame; its locations are
                // relative to the caller's own x29, which is exactly the saved
                // word we just read.
                //
                // It gets the SAME validation `fp` gets at the top of the
                // loop, and it gets it BEFORE the root loop rather than after.
                // Every FP-relative root is based on this word, and
                // `fp_to_sp_offset` subtracts from it for the SP-relative
                // ones; downstream the only filters are non-zero and 8-byte
                // alignment, so an unvalidated `caller_fp` lets a corrupt
                // frame produce addresses outside the stack that the collector
                // then reads and rewrites. Fail closed to the platform
                // unwinder.
                if caller_fp == 0
                    || caller_fp & FRAME_RECORD_ALIGN_MASK != 0
                    || caller_fp <= fp
                    || caller_fp.checked_add(16)? > top
                {
                    return None;
                }
                let mut records = index.matched(&matched);
                while let Some(record) = records.next() {
                    stats.records_matched = stats.records_matched.saturating_add(1);
                    let Some(registers) = record_slot_registers(&record) else {
                        lazy::malformed(record.function_address)
                    };
                    // A base this walk cannot reconstruct: hand the whole walk
                    // to the platform unwinder rather than guess one frame.
                    if !registers.chain_walkable {
                        return None;
                    }
                    // Body SP = fp - (prologue's `add x29, sp, #imm`).
                    let sp = fp_to_sp_offset(record.function_address)
                        .and_then(|off| caller_fp.checked_sub(off));
                    // #8770: an x19-based root resolves like an SP-based one
                    // (x19 == body SP) ONLY when the owning function captured
                    // its base as `mov x19, sp` after the fixed prologue.
                    // Confirm that per frame before trusting the `sp` base for
                    // its x19 slots.
                    if registers.uses_x19 && !x19_is_body_sp(record.function_address) {
                        return None;
                    }
                    // An SP-relative (or x19-relative) location with no
                    // decodable prologue used to abandon the walk from inside
                    // the location loop; keep that fail-closed answer, decided
                    // before any slot is visited.
                    if sp.is_none() && registers.any_non_fp {
                        return None;
                    }
                    let mut resolve = |location: &StackMapLocation| {
                        // FP-based → the caller's x29; SP- and x19-based → the
                        // reconstructed body SP (x19 was proven equal to it
                        // above).
                        let base = if location.dwarf_reg == DWARF_REG_FP_AARCH64 {
                            caller_fp
                        } else {
                            sp?
                        };
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
                            return_address,
                            &mut resolve,
                            &mut stats,
                            visit,
                        );
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

// The compact-map decoder. Its own file because this one is close to the
// 2000-line cap; the re-export is named rather than a glob because a glob does
// not propagate through the transitive re-exports this module sits behind.
#[path = "stack_maps_lazy.rs"]
mod lazy;

/// Frame lookup and the cross-check, in a sibling file for the same reason
/// `stack_maps_verify.rs` is: the parent is at the repo's 2000-line cap.
#[path = "stack_maps_index.rs"]
mod index;

#[path = "stack_maps_decode.rs"]
mod decode;
use decode::parse_gc_map;

// Finding the map section in the running image, per object file format. Its own
// file for the same reason, and re-exported by name for the same reason.
#[path = "stack_maps_sections.rs"]
mod sections;
use sections::loaded_stack_map_sections;

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

// The only test anywhere that runs BOTH aarch64 walkers over a frame whose
// layout is known, and requires each to land on the word the record names.
// Same platform set as `fp_chain` itself.
#[cfg(all(
    test,
    any(target_vendor = "apple", target_os = "linux"),
    target_arch = "aarch64"
))]
#[path = "stack_maps_walker_agreement.rs"]
mod walker_agreement;
