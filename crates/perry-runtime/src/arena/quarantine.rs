//! From-space quarantine, poison and page protection (#7154 tooling).
//!
//! # What this exists to fix
//!
//! After an evacuating minor, `copying_reset_from_spaces_and_flip` sets every
//! Eden / active-survivor block's `offset` back to 0 and the bump allocator
//! starts handing the same bytes out again on the very next allocation. A
//! mutator register that still holds a *from-space* address — the #7154 family
//! of codegen rooting bugs — therefore does not fault. It reads a perfectly
//! valid, freshly-constructed, completely unrelated object. The program keeps
//! running and dies one or more cycles later, in a different function, as
//! `TypeError: value is not a function`.
//!
//! Detection latency is the whole problem: the fault is separated from its
//! cause by an arbitrary amount of execution.
//!
//! This module removes the recycling. Under `PERRY_GC_PROTECT_FROMSPACE` the
//! evacuated from-space blocks are not reset — they are **detached from the
//! arena** into a bounded quarantine ring, poisoned, and (in the default mode)
//! `mprotect(PROT_NONE)`d. A stale dereference then faults **at the faulting
//! instruction**, with the holder still on the stack, and the installed
//! SIGSEGV/SIGBUS reporter names the address, the minor that retired it, and
//! the last-known object that lived there.
//!
//! # What the knob actually gates (read this before trusting a run)
//!
//! `PERRY_GC_PROTECT_FROMSPACE` gates **only** the from-space reset performed
//! by the **copying (evacuating) minor** — `copying_reset_from_spaces_and_flip`
//! and nothing else. It does *not*:
//!
//! - touch the non-moving minor's in-place sweep (`arena_reset_empty_blocks`),
//! - touch the full mark-sweep's block reclaim,
//! - touch old-gen defrag or the malloc registry sweep,
//! - make a collection happen that would not otherwise have happened.
//!
//! So a run with this knob on and **zero copying minors** protects nothing and
//! proves nothing. `quarantine_stats()` reports `sets_retired` precisely so a
//! green result can be checked against its subject having been live (CLAUDE.md,
//! "four ways a gate can be unable to fail", #4). Pair it with
//! `PERRY_GC_ZEAL=1` to guarantee evacuating minors actually run.
//!
//! # Modes
//!
//! | value | effect |
//! |---|---|
//! | unset / `0` / `off` / `false` | inert — the normal reset path runs, byte-for-byte |
//! | `1` / `on` / `true` | poison **and** `mprotect(PROT_NONE)` the page-aligned interior |
//! | `poison` | poison only, no `mprotect` (for hosts/tests where faulting is not wanted) |
//!
//! `mprotect` needs page granularity and arena blocks are `alloc`'d at 16-byte
//! alignment, so the *page-aligned interior* of each block is protected and the
//! (at most two) sub-page edge fragments are poison-filled instead. Both are
//! counted separately — a protected-byte total that silently collapsed to zero
//! would read exactly like a clean run.
//!
//! # Platform support
//!
//! Page protection and the fault reporter are **Unix-only**: they are built on
//! `mprotect` / `sigaction` / `sysconf`, none of which the `libc` crate exposes
//! on `x86_64-pc-windows-msvc` — a target `perry-runtime` really is compiled
//! for (`test.yml`'s `windows-build` job, and `release-packages.yml` via
//! `perry-ui-windows` → `perry-runtime`). On non-Unix targets `ProtectPages`
//! degrades to poison-only rather than failing to build. That degradation is
//! *observable, not silent*: `mprotect_range` reports failure, so `protected`
//! stays `None` and `bytes_protected` stays 0 while `bytes_poisoned` counts the
//! whole retired range — which is exactly what the separate counters exist to
//! reveal.
//!
//! # Memory bound
//!
//! `PERRY_GC_PROTECT_FROMSPACE_DEPTH` (default 4) caps how many retired
//! page-sets stay quarantined. Evicting a set restores `PROT_READ|PROT_WRITE`
//! and hands the blocks **back to Eden** rather than freeing them, so the
//! quarantine is a ring buffer: steady-state footprint is bounded by
//! `depth × from-space bytes` and no `mprotect`ed page is ever handed to
//! `dealloc`.
//!
//! **Depth is the knob to raise when a suspected bug does not fault.** A stale
//! pointer is only caught while the page-set it names is still quarantined, and
//! under `PERRY_GC_ZEAL=1` a value can cross *hundreds* of collections between
//! its last valid observation and its stale use — one per loop back-edge poll.
//! Measured on #7154's `new C(…)` reproducer: the constructor body runs 600
//! polls, so the caller's stale register is 600 retirements old by the time
//! `js_ctor_return_override` publishes it, and the default depth of 4 misses it
//! silently. `PERRY_GC_PROTECT_FROMSPACE_DEPTH=800` faults on the first use.
//! Rule of thumb: depth ≥ the number of safepoints the suspect value survives.
//!
//! Two consequences worth knowing before reading numbers off a protected run,
//! both direct and both only under the knob:
//!
//! - Quarantined bytes are subtracted from `ARENA_TOTAL_BYTES` when the block
//!   leaves the arena, so `arena_total_bytes()` — and therefore the arena-bytes
//!   GC trigger — under-reports real RSS by up to `depth × from-space bytes`.
//!   Fewer automatic triggers, not more. Pair with `PERRY_GC_ZEAL=1` if the
//!   point of the run is collection frequency.
//! - RSS is genuinely higher than an unprotected run for the same reason. This
//!   is a debug instrument; do not benchmark under it.

use super::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering};
use std::sync::Mutex;

/// Poison fill word. Chosen so that reading the first 8 bytes as a `GcHeader`
/// yields `obj_type = 0xDE` — outside every `GC_TYPE_*` constant — and a
/// nonsense `size`, so a walker or type dispatch trips immediately instead of
/// wandering. As a NaN-boxed value it is not a pointer tag either.
pub(crate) const QUARANTINE_POISON_WORD: u64 =
    0xDEAD_BEEF_BAAD_F000 | QUARANTINE_POISON_OBJ_TYPE as u64;

/// `obj_type` byte a poisoned allocation presents — the low byte of
/// [`QUARANTINE_POISON_WORD`], because `GcHeader` is `#[repr(C)]` with
/// `obj_type` first. Deliberately not a member of the `GC_TYPE_*` family.
pub(crate) const QUARANTINE_POISON_OBJ_TYPE: u8 = 0xDE;

const DEFAULT_QUARANTINE_DEPTH: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum FromSpaceProtection {
    /// Knob unset or explicitly off — the ordinary reset path runs.
    Off,
    /// Poison the retired bytes, do not change page protection.
    PoisonOnly,
    /// Poison, then `mprotect(PROT_NONE)` the page-aligned interior.
    ProtectPages,
}

/// Pure knob parse, so the mapping is testable without mutating process
/// environment (the live reader caches in a `OnceLock`, as every other GC knob
/// does, and a cached knob cannot be re-read by a later test).
pub(crate) fn parse_protection_mode(raw: Option<&str>) -> FromSpaceProtection {
    match raw {
        Some("1") | Some("on") | Some("true") => FromSpaceProtection::ProtectPages,
        Some("poison") => FromSpaceProtection::PoisonOnly,
        _ => FromSpaceProtection::Off,
    }
}

#[cfg(test)]
thread_local! {
    /// Test-only override. Thread-local, so one test enabling the instrument
    /// cannot change the collector's behaviour for any other test.
    static MODE_OVERRIDE: Cell<Option<FromSpaceProtection>> = const { Cell::new(None) };
}

pub(crate) fn fromspace_protection_mode() -> FromSpaceProtection {
    #[cfg(test)]
    if let Some(mode) = MODE_OVERRIDE.with(Cell::get) {
        return mode;
    }
    use std::sync::OnceLock;
    static CACHED: OnceLock<FromSpaceProtection> = OnceLock::new();
    *CACHED.get_or_init(|| {
        parse_protection_mode(std::env::var("PERRY_GC_PROTECT_FROMSPACE").ok().as_deref())
    })
}

/// RAII test override for the protection mode.
#[cfg(test)]
pub(crate) struct ProtectionModeGuard(Option<FromSpaceProtection>);

#[cfg(test)]
impl ProtectionModeGuard {
    pub(crate) fn set(mode: FromSpaceProtection) -> Self {
        let previous = MODE_OVERRIDE.with(|cell| cell.replace(Some(mode)));
        Self(previous)
    }
}

#[cfg(test)]
impl Drop for ProtectionModeGuard {
    fn drop(&mut self) {
        MODE_OVERRIDE.with(|cell| cell.set(self.0));
    }
}

#[inline]
pub(crate) fn protect_fromspace_enabled() -> bool {
    fromspace_protection_mode() != FromSpaceProtection::Off
}

/// Pure knob parse for the quarantine depth. `0` and unparsable values are
/// rejected: a depth of zero would evict every set on the cycle it was created,
/// making the instrument inert while still reading as ON.
pub(crate) fn parse_quarantine_depth(raw: Option<&str>) -> usize {
    raw.and_then(|raw| raw.parse::<usize>().ok())
        .map(|depth| depth.max(1))
        .unwrap_or(DEFAULT_QUARANTINE_DEPTH)
}

/// How many retired page-sets stay quarantined before the oldest is recycled.
pub(crate) fn quarantine_depth() -> usize {
    use std::sync::OnceLock;
    static CACHED: OnceLock<usize> = OnceLock::new();
    *CACHED.get_or_init(|| {
        parse_quarantine_depth(
            std::env::var("PERRY_GC_PROTECT_FROMSPACE_DEPTH")
                .ok()
                .as_deref(),
        )
    })
}

#[cfg(unix)]
fn page_size() -> usize {
    use std::sync::OnceLock;
    static CACHED: OnceLock<usize> = OnceLock::new();
    *CACHED.get_or_init(|| {
        let raw = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if raw > 0 {
            raw as usize
        } else {
            4096
        }
    })
}

/// Non-Unix: no `sysconf`. Only used to size the would-be protected interior,
/// which `mprotect_range` then declines to protect, so the value is inert.
#[cfg(not(unix))]
fn page_size() -> usize {
    4096
}

/// One object that lived in a quarantined block, kept so a fault reporter can
/// say *what* the stale pointer used to point at. 12 bytes packed; a 1 MB block
/// of 48-byte objects costs ~256 KB of census.
#[derive(Clone, Copy)]
struct CensusEntry {
    /// Byte offset of the **user pointer** (header + 8) within the block.
    user_offset: u32,
    size: u32,
    obj_type: u8,
}

struct QuarantinedBlock {
    data: *mut u8,
    size: usize,
    /// Bytes that were in use when the block was retired.
    used: usize,
    /// `mprotect`ed sub-range, if any: `(base, len)`.
    protected: Option<(usize, usize)>,
    census: Vec<CensusEntry>,
}

// SAFETY: the raw pointer is an owned arena block. The registry that holds
// these is only read (a) by the owning thread and (b) by the fault reporter,
// which reads addresses and never dereferences the payload.
unsafe impl Send for QuarantinedBlock {}

struct QuarantinedSet {
    /// Which retirement this was; monotonically increasing per process.
    seq: u64,
    blocks: Vec<QuarantinedBlock>,
}

static QUARANTINE_SEQ: AtomicU64 = AtomicU64::new(0);
static SETS_RETIRED: AtomicU64 = AtomicU64::new(0);
static BLOCKS_QUARANTINED: AtomicU64 = AtomicU64::new(0);
static BYTES_PROTECTED: AtomicU64 = AtomicU64::new(0);
static BYTES_POISONED: AtomicU64 = AtomicU64::new(0);
static BLOCKS_RECYCLED: AtomicU64 = AtomicU64::new(0);

/// Process-global so the fault reporter — which can run on any thread and must
/// not touch a `thread_local!` — can resolve a faulting address. Read from the
/// signal handler with `try_lock` only.
static REGISTRY: Mutex<Vec<QuarantinedSet>> = Mutex::new(Vec::new());

/// Live snapshot for tests and for `PERRY_GC_DIAG` reporting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QuarantineStats {
    /// Number of copying minors whose from-space this instrument retired. A
    /// clean verdict with `sets_retired == 0` means the instrument never ran.
    pub sets_retired: u64,
    pub blocks_quarantined: u64,
    pub bytes_protected: u64,
    pub bytes_poisoned: u64,
    /// Blocks whose quarantine expired and which went back into Eden.
    pub blocks_recycled: u64,
    /// Page-sets currently held.
    pub sets_held: usize,
}

pub fn quarantine_stats() -> QuarantineStats {
    QuarantineStats {
        sets_retired: SETS_RETIRED.load(AtomicOrdering::Relaxed),
        blocks_quarantined: BLOCKS_QUARANTINED.load(AtomicOrdering::Relaxed),
        bytes_protected: BYTES_PROTECTED.load(AtomicOrdering::Relaxed),
        bytes_poisoned: BYTES_POISONED.load(AtomicOrdering::Relaxed),
        blocks_recycled: BLOCKS_RECYCLED.load(AtomicOrdering::Relaxed),
        sets_held: REGISTRY.lock().map(|sets| sets.len()).unwrap_or(0),
    }
}

/// Walk `[data, data + used)` header-by-header, recording what lived there.
///
/// Mirrors the arena walker's layout assumptions (8-byte `GcHeader`, `size`
/// covers header + payload, 8-byte stride). Stops at the first inconsistency
/// rather than guessing — a partial census still names most addresses, and a
/// wrong one would be a lying instrument.
///
/// # Safety
/// `data` must point at `used` readable bytes of a retired arena block.
unsafe fn build_census(data: *mut u8, used: usize) -> Vec<CensusEntry> {
    let mut census = Vec::new();
    let mut pos = 0usize;
    while pos + crate::gc::GC_HEADER_SIZE <= used {
        let header = data.add(pos) as *const crate::gc::GcHeader;
        let size = (*header).size as usize;
        if size < crate::gc::GC_HEADER_SIZE || pos + size > used {
            break;
        }
        census.push(CensusEntry {
            user_offset: (pos + crate::gc::GC_HEADER_SIZE) as u32,
            size: size as u32,
            obj_type: (*header).obj_type,
        });
        pos += (size + 7) & !7;
    }
    census
}

/// Which censused object, if any, covers `offset` within its block.
///
/// Matches an object's WHOLE extent, header included:
/// `[user_offset - GC_HEADER_SIZE, user_offset - GC_HEADER_SIZE + size)`.
/// Both bounds are load-bearing and each was wrong at some point:
///
/// - `size` covers header + payload while `user_offset` already skips the
///   header, so bounding at `user_offset + size` overshoots by `GC_HEADER_SIZE`
///   and attributes an address in the NEXT object's header to this one — the
///   report then confidently names the wrong last-known object.
/// - Bounding at the payload end instead (`user_offset + size - GC_HEADER_SIZE`)
///   leaves *header* addresses attributed to nobody, and a raw header address is
///   exactly what this family of bugs hands you: #7192's shape publishes
///   `%inst`, the pre-header allocation pointer, so the faulting address lands
///   on an object's header rather than inside its payload. Measured on the
///   reverted-#7192 reproducer, that turned a correct attribution into
///   "(no census entry covers this offset)".
///
/// Addresses in the inter-object padding left by the `(size + 7) & !7` stride
/// belong to no object and correctly return `None`.
fn census_lookup(census: &[CensusEntry], offset: usize) -> Option<(u32, u8, u32)> {
    census
        .iter()
        .rev()
        .find(|entry| {
            let start = (entry.user_offset as usize).saturating_sub(crate::gc::GC_HEADER_SIZE);
            offset >= start && offset < start + entry.size as usize
        })
        .map(|entry| (entry.user_offset, entry.obj_type, entry.size))
}

/// Fill `[data, data + used)` with the poison word.
///
/// # Safety
/// `data` must point at `used` writable bytes of a retired arena block.
unsafe fn poison(data: *mut u8, used: usize) {
    let words = used / 8;
    let ptr = data as *mut u64;
    for i in 0..words {
        ptr.add(i).write(QUARANTINE_POISON_WORD);
    }
    let tail_start = words * 8;
    for i in tail_start..used {
        data.add(i)
            .write((QUARANTINE_POISON_WORD >> (8 * (i % 8))) as u8);
    }
}

/// The page-aligned interior of `[base, base + size)`, or `None` when the block
/// is too small / too badly aligned to contain a whole page.
fn page_interior(base: usize, size: usize) -> Option<(usize, usize)> {
    let page = page_size();
    let start = base.checked_add(page - 1)? & !(page - 1);
    let end = (base.checked_add(size)?) & !(page - 1);
    if end > start {
        Some((start, end - start))
    } else {
        None
    }
}

#[cfg(unix)]
fn mprotect_range(base: usize, len: usize, prot: libc::c_int) -> bool {
    // SAFETY: `base`/`len` are page-aligned and describe memory this process
    // owns (an arena block detached from the arena and held by the quarantine).
    unsafe { libc::mprotect(base as *mut libc::c_void, len, prot) == 0 }
}

/// Non-Unix: there is no `mprotect`. Reporting failure is what makes the
/// degradation to poison-only visible in `bytes_protected` rather than silent.
#[cfg(not(unix))]
fn mprotect_range(_base: usize, _len: usize, _prot: i32) -> bool {
    false
}

fn unprotect(block: &QuarantinedBlock) {
    if let Some((base, len)) = block.protected {
        #[cfg(unix)]
        mprotect_range(base, len, libc::PROT_READ | libc::PROT_WRITE);
        // Unreachable on non-Unix: `protected` is only ever `Some` when
        // `mprotect_range` succeeded, and there it always fails.
        #[cfg(not(unix))]
        let _ = (base, len);
    }
}

/// Retire one detached from-space block into the pending set.
///
/// # Safety
/// `data` must be an arena block of `size` bytes that has been removed from its
/// arena (tombstoned) and unregistered from the page metadata, with `used`
/// bytes of retired object data at its base.
unsafe fn retire_block(
    data: *mut u8,
    size: usize,
    used: usize,
    mode: FromSpaceProtection,
) -> QuarantinedBlock {
    let census = build_census(data, used);
    poison(data, used.min(size));
    let mut protected = None;
    if mode == FromSpaceProtection::ProtectPages {
        #[cfg(unix)]
        let prot_none = libc::PROT_NONE;
        #[cfg(not(unix))]
        let prot_none = 0i32;
        if let Some((base, len)) = page_interior(data as usize, size) {
            if mprotect_range(base, len, prot_none) {
                protected = Some((base, len));
                BYTES_PROTECTED.fetch_add(len as u64, AtomicOrdering::Relaxed);
            }
        }
    }
    // Poisoned-but-unprotected bytes: the whole retired range when in
    // poison-only mode, otherwise the sub-page edge fragments mprotect could
    // not cover. Counted separately so a run can never claim page protection it
    // did not get.
    let poisoned_uncovered = match protected {
        Some((base, len)) => {
            let head = base.saturating_sub(data as usize);
            let tail = size.saturating_sub(head + len);
            (head + tail).min(used)
        }
        None => used,
    };
    BYTES_POISONED.fetch_add(poisoned_uncovered as u64, AtomicOrdering::Relaxed);
    BLOCKS_QUARANTINED.fetch_add(1, AtomicOrdering::Relaxed);
    QuarantinedBlock {
        data,
        size,
        used,
        protected,
        census,
    }
}

/// Push a freshly-retired page-set and evict past the depth bound. Returns the
/// blocks whose quarantine expired, restored to read/write and ready to go back
/// into Eden.
fn push_set_and_evict(blocks: Vec<QuarantinedBlock>) -> Vec<QuarantinedBlock> {
    let seq = QUARANTINE_SEQ.fetch_add(1, AtomicOrdering::Relaxed);
    let depth = quarantine_depth();
    let mut evicted = Vec::new();
    if let Ok(mut sets) = REGISTRY.lock() {
        // Counted only once the set is actually registered. `sets_retired` is
        // the live-subject evidence for every protected run (CLAUDE.md, "a gate
        // must assert its subject was live"), so it must never claim a
        // retirement that `sets_held` cannot account for.
        SETS_RETIRED.fetch_add(1, AtomicOrdering::Relaxed);
        sets.push(QuarantinedSet { seq, blocks });
        while sets.len() > depth {
            let oldest = sets.remove(0);
            for block in oldest.blocks {
                unprotect(&block);
                evicted.push(block);
            }
        }
    } else {
        // Poisoned registry. These blocks are already detached from the arena,
        // unregistered, and subtracted from `ARENA_TOTAL_BYTES`, and
        // `QuarantinedBlock` has no `Drop` — dropping them here would leak the
        // whole from-space. Recycle immediately instead.
        for block in blocks {
            unprotect(&block);
            evicted.push(block);
        }
    }
    BLOCKS_RECYCLED.fetch_add(evicted.len() as u64, AtomicOrdering::Relaxed);
    evicted
}

/// Quarantining variant of [`super::reset::copying_reset_from_spaces_and_flip`].
///
/// Detaches every from-space block that holds retired bytes, leaving the
/// arena's `blocks` vector the same LENGTH (a `data = null` tombstone in each
/// vacated slot) so block-index semantics — `active_survivor_block_index_range`,
/// `block_has_live`, the walkers — are unchanged. Recycled blocks from an
/// expired quarantine set are reinstalled into Eden.
pub(crate) fn copying_quarantine_from_spaces_and_flip() -> ArenaResetStats {
    let mode = fromspace_protection_mode();
    debug_assert_ne!(mode, FromSpaceProtection::Off);
    install_fault_reporter();
    sync_inline_arena_state();

    let mut retired = Vec::new();
    let mut reset_blocks = 0usize;
    let mut reusable_bytes = 0usize;

    // --- Eden -------------------------------------------------------------
    let eden_detached = ARENA.with(|arena| unsafe {
        let arena = &mut *arena.get();
        detach_used_blocks(arena)
    });
    for (data, size, used) in eden_detached {
        reset_blocks += 1;
        // SAFETY: `detach_used_blocks` tombstoned the slot and unregistered the
        // block's page metadata, so nothing else can reach these bytes.
        retired.push(unsafe { retire_block(data, size, used, mode) });
    }

    // --- active survivor from-space ---------------------------------------
    let active = ACTIVE_SURVIVOR.with(|active| active.get());
    let survivor_detached =
        with_survivor_arena_mut(active, |arena| unsafe { detach_used_blocks(arena) });
    for (data, size, used) in survivor_detached {
        reset_blocks += 1;
        // SAFETY: as above.
        retired.push(unsafe { retire_block(data, size, used, mode) });
    }

    let recycled = push_set_and_evict(retired);

    if std::env::var_os("PERRY_GC_DIAG").is_some() {
        let stats = quarantine_stats();
        eprintln!(
            "[gc-fromspace-protect] mode={:?} retired_set=#{} blocks={} sets_held={}/{} bytes_protected={} bytes_poisoned={} blocks_recycled={}",
            mode,
            stats.sets_retired.saturating_sub(1),
            reset_blocks,
            stats.sets_held,
            quarantine_depth(),
            stats.bytes_protected,
            stats.bytes_poisoned,
            stats.blocks_recycled
        );
    }

    // Hand expired blocks back to Eden rather than `dealloc`ing them: nothing
    // that was ever `mprotect`ed is returned to the system allocator.
    ARENA.with(|arena| unsafe {
        let arena = &mut *arena.get();
        for block in recycled {
            reusable_bytes = reusable_bytes.saturating_add(block.used);
            arena.install_reserved_block(ArenaBlock {
                data: block.data,
                size: block.size,
                offset: 0,
                dead_cycles: 0,
            });
        }
        ensure_usable_current_block(arena);
        crate::gc::ARENA_FREE_LIST.with(|fl| fl.borrow_mut().clear());
        crate::gc::ARENA_FREE_LIST_NONEMPTY.with(|c| c.set(false));
        INLINE_STATE.with(|s| {
            let inline = &mut *s.get();
            if !inline.data.is_null() {
                let block = &arena.blocks[arena.current];
                inline.data = block.data;
                inline.offset = block.offset;
                inline.size = block.size;
            }
        });
    });

    with_survivor_arena_mut(active, |arena| {
        arena.current = arena
            .blocks
            .iter()
            .position(|block| !block.data.is_null())
            .unwrap_or(0);
    });
    ACTIVE_SURVIVOR.with(|active_cell| active_cell.set(1 - active));

    ArenaResetStats {
        reset_blocks,
        reusable_bytes,
        deallocated_blocks: 0,
        deallocated_bytes: 0,
    }
}

/// Remove every block with `offset != 0` from `arena`, leaving a tombstone in
/// its slot. Returns `(data, size, used)` for each. Blocks that were already
/// empty hold nothing retired and are left alone.
///
/// # Safety
/// Caller must hold exclusive access to `arena`.
unsafe fn detach_used_blocks(arena: &mut Arena) -> Vec<(*mut u8, usize, usize)> {
    let mut detached = Vec::new();
    for block in arena.blocks.iter_mut() {
        if block.data.is_null() || block.offset == 0 {
            continue;
        }
        let base = block.data as usize;
        let size = block.size;
        let used = block.offset;
        unregister_block_generation(base, size);
        ARENA_TOTAL_BYTES.with(|total| total.set(total.get().saturating_sub(size)));
        detached.push((block.data, size, used));
        block.data = std::ptr::null_mut();
        block.size = 0;
        block.offset = 0;
        block.dead_cycles = 0;
    }
    arena.current = 0;
    detached
}

/// Point Eden's `current` at a live slot after detaching, installing a fresh
/// block if the whole region was retired.
///
/// This is for `INLINE_STATE`, not for `Arena::alloc`. Allocation itself is
/// already tombstone-safe on every path: a tombstone has `size == 0`, so
/// `ArenaBlock::alloc` fails its `bumped > self.size` test and returns `None`
/// without ever dereferencing `data`; `try_alloc_after_gc` then forward-scans
/// the other blocks and `install_reserved_block` reuses the tombstone slot.
/// (`copying_quarantine_from_spaces_and_flip` leaves the *survivor* arena's
/// `current` on a tombstone in exactly that situation and relies on this — see
/// `survivor_current_on_tombstone_still_allocates_correctly`.)
///
/// Eden is the one that cannot tolerate it, because the caller copies
/// `blocks[current]` straight into `INLINE_STATE` for codegen's inline bump
/// allocator, and a tombstone would publish `data = null, size = 0`.
///
/// # Safety
/// Caller must hold exclusive access to `arena`.
unsafe fn ensure_usable_current_block(arena: &mut Arena) {
    if arena
        .blocks
        .get(arena.current)
        .is_some_and(|block| !block.data.is_null())
    {
        return;
    }
    if let Some(idx) = arena
        .blocks
        .iter()
        .position(|block| !block.data.is_null() && block.offset == 0)
    {
        arena.current = idx;
        return;
    }
    arena.install_fresh_block(BLOCK_SIZE);
}

// ---------------------------------------------------------------------------
// Fault reporter
// ---------------------------------------------------------------------------

static REPORTER_INSTALLED: AtomicBool = AtomicBool::new(false);

/// Non-Unix: no `sigaction`, so there is no reporter to install. `ProtectPages`
/// has already degraded to poison-only (see `mprotect_range`), so there is also
/// nothing that could fault.
#[cfg(not(unix))]
fn install_fault_reporter() {}

/// Install the SIGSEGV/SIGBUS reporter, once, the first time a page-set is
/// retired. Only installed when the knob is on, so a normal build never touches
/// signal disposition.
#[cfg(unix)]
fn install_fault_reporter() {
    if fromspace_protection_mode() != FromSpaceProtection::ProtectPages {
        return;
    }
    if REPORTER_INSTALLED.swap(true, AtomicOrdering::SeqCst) {
        return;
    }
    // SAFETY: standard `sigaction` install with a `SA_SIGINFO` handler.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = fromspace_fault_handler as *const () as usize;
        action.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(libc::SIGSEGV, &action, std::ptr::null_mut());
        libc::sigaction(libc::SIGBUS, &action, std::ptr::null_mut());
    }
}

/// Minimal `write(2)`-based formatter. Deliberately avoids `format!`/`eprintln!`
/// so the common path through the handler does not allocate.
#[cfg(unix)]
struct FaultWriter {
    buf: [u8; 1024],
    len: usize,
}

#[cfg(unix)]
impl FaultWriter {
    fn new() -> Self {
        Self {
            buf: [0; 1024],
            len: 0,
        }
    }
    fn str(&mut self, s: &str) {
        for &byte in s.as_bytes() {
            if self.len < self.buf.len() {
                self.buf[self.len] = byte;
                self.len += 1;
            }
        }
    }
    fn hex(&mut self, mut value: usize) {
        self.str("0x");
        let mut digits = [0u8; 16];
        let mut n = 0;
        if value == 0 {
            digits[0] = b'0';
            n = 1;
        }
        while value != 0 {
            let nibble = (value & 0xf) as u8;
            digits[n] = if nibble < 10 {
                b'0' + nibble
            } else {
                b'a' + nibble - 10
            };
            n += 1;
            value >>= 4;
        }
        for i in (0..n).rev() {
            if self.len < self.buf.len() {
                self.buf[self.len] = digits[i];
                self.len += 1;
            }
        }
    }
    fn dec(&mut self, value: u64) {
        let mut digits = [0u8; 20];
        let mut n = 0;
        let mut value = value;
        if value == 0 {
            digits[0] = b'0';
            n = 1;
        }
        while value != 0 {
            digits[n] = b'0' + (value % 10) as u8;
            n += 1;
            value /= 10;
        }
        for i in (0..n).rev() {
            if self.len < self.buf.len() {
                self.buf[self.len] = digits[i];
                self.len += 1;
            }
        }
    }
    fn flush(&self) {
        // SAFETY: writing `self.len` initialized bytes to stderr.
        unsafe {
            libc::write(2, self.buf.as_ptr() as *const libc::c_void, self.len);
        }
    }
}

#[cfg(unix)]
extern "C" fn fromspace_fault_handler(
    signum: libc::c_int,
    info: *mut libc::siginfo_t,
    _ctx: *mut libc::c_void,
) {
    let addr = if info.is_null() {
        0usize
    } else {
        // SAFETY: `info` is the kernel-provided siginfo for a SIGSEGV/SIGBUS.
        unsafe { (*info).si_addr() as usize }
    };

    let mut out = FaultWriter::new();
    // `try_lock`: never block inside a signal handler. A contended registry
    // costs the census line, not the report.
    let described = REGISTRY.try_lock().ok().and_then(|sets| {
        sets.iter().find_map(|set| {
            set.blocks.iter().find_map(|block| {
                let base = block.data as usize;
                if addr < base || addr >= base + block.size {
                    return None;
                }
                let offset = addr - base;
                let entry = census_lookup(&block.census, offset);
                Some((set.seq, base, block.used, entry))
            })
        })
    });

    match described {
        Some((seq, base, used, entry)) => {
            out.str("\n[gc-fromspace-protect] FAULT: signal ");
            out.dec(signum as u64);
            out.str(" at ");
            out.hex(addr);
            out.str("\n  This address is RETIRED FROM-SPACE. The evacuating minor moved or\n  freed the object here and the holder kept the pre-collection address.\n  block=");
            out.hex(base);
            out.str(" +");
            out.dec((addr - base) as u64);
            out.str(" retired_bytes=");
            out.dec(used as u64);
            out.str(" retired_by_minor=#");
            out.dec(seq);
            match entry {
                Some((user_offset, obj_type, size)) => {
                    out.str("\n  last-known object: user_ptr=");
                    out.hex(base + user_offset as usize);
                    out.str(" obj_type=");
                    out.dec(obj_type as u64);
                    out.str(" size=");
                    out.dec(size as u64);
                }
                None => {
                    out.str("\n  last-known object: (no census entry covers this offset)");
                }
            }
            out.str("\n  The faulting instruction IS the stale use. Backtrace:\n");
            out.flush();
            emit_native_backtrace();
        }
        None => {
            out.str("\n[gc-fromspace-protect] signal ");
            out.dec(signum as u64);
            out.str(" at ");
            out.hex(addr);
            out.str(" is NOT in quarantined from-space (unrelated fault)\n");
            out.flush();
        }
    }

    // Restore the default disposition and return, so the instruction re-faults
    // and the process dies exactly where it should — core file, debugger,
    // crash reporter all see the real site.
    // SAFETY: standard handler teardown.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = libc::SIG_DFL;
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(signum, &action, std::ptr::null_mut());
    }
}

#[cfg(all(unix, any(target_os = "macos", target_os = "linux")))]
fn emit_native_backtrace() {
    const MAX_FRAMES: usize = 64;
    let mut frames = [std::ptr::null_mut::<libc::c_void>(); MAX_FRAMES];
    // SAFETY: `backtrace`/`backtrace_symbols_fd` are the async-signal-safe pair
    // (`_fd` writes directly and does not call `malloc`).
    unsafe {
        let n = libc::backtrace(frames.as_mut_ptr(), MAX_FRAMES as libc::c_int);
        if n > 0 {
            libc::backtrace_symbols_fd(frames.as_ptr(), n, 2);
        }
    }
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
fn emit_native_backtrace() {}

#[cfg(test)]
mod census_tests {
    use super::*;
    use crate::gc::GC_HEADER_SIZE;

    /// Two adjacent 48-byte objects at block offsets 0 and 48.
    fn two_objects() -> Vec<CensusEntry> {
        vec![
            CensusEntry {
                user_offset: GC_HEADER_SIZE as u32,
                size: 48,
                obj_type: 1,
            },
            CensusEntry {
                user_offset: (48 + GC_HEADER_SIZE) as u32,
                size: 48,
                obj_type: 4,
            },
        ]
    }

    /// The fault report's "last-known object" line is the field an investigator
    /// reads to tell a dead closure (`obj_type = 4`) from a dead object (`2`).
    /// An instrument that names the WRONG object there is worse than one that
    /// says nothing, so both bounds are pinned.
    #[test]
    fn census_attributes_headers_and_payloads_to_the_right_object() {
        let census = two_objects();

        // A payload address belongs to its own object.
        assert_eq!(
            census_lookup(&census, GC_HEADER_SIZE + 8).map(|e| e.1),
            Some(1),
            "a payload address must name the object that owns it"
        );

        // The LAST byte of object 0's payload is still object 0.
        assert_eq!(
            census_lookup(&census, 47).map(|e| e.1),
            Some(1),
            "the final payload byte must not spill into the next object"
        );

        // Offset 48 is object 1's HEADER. Before the fix this was attributed to
        // object 0 (the `user_offset + size` overshoot); bounding at the payload
        // end instead made it unattributed. It is object 1.
        assert_eq!(
            census_lookup(&census, 48).map(|e| (e.0, e.1)),
            Some(((48 + GC_HEADER_SIZE) as u32, 4)),
            "an object's header address must name THAT object, not its neighbour"
        );

        // Past the end of the last object: nobody.
        assert_eq!(
            census_lookup(&census, 96),
            None,
            "an offset beyond every censused object must not be attributed"
        );

        // Padding between objects belongs to nobody rather than to the object
        // before it.
        let padded = vec![CensusEntry {
            user_offset: GC_HEADER_SIZE as u32,
            size: 44, // 44 -> stride rounds to 48, leaving 4 pad bytes
            obj_type: 2,
        }];
        assert_eq!(
            census_lookup(&padded, 43).map(|e| e.1),
            Some(2),
            "last real byte is still the object"
        );
        assert_eq!(
            census_lookup(&padded, 45),
            None,
            "stride padding must report no covering entry, not the previous object"
        );
    }
}

#[cfg(test)]
mod tombstone_tests {
    use super::*;

    fn tombstone() -> ArenaBlock {
        ArenaBlock {
            data: std::ptr::null_mut(),
            size: 0,
            offset: 0,
            dead_cycles: 0,
        }
    }

    /// `copying_quarantine_from_spaces_and_flip` can leave the survivor arena's
    /// `current` pointing at a tombstone: `detach_used_blocks` tombstones every
    /// block that held retired bytes, and when that is *all* of them the
    /// `position(..).unwrap_or(0)` fixup lands on slot 0, which is itself a
    /// tombstone (`data = null`, `size = 0`).
    ///
    /// Review flagged that as a Critical fault risk. It is not one, and this
    /// pins why so a future change to the bump allocator cannot quietly make it
    /// true: a tombstone has `size == 0`, so `ArenaBlock::alloc` fails its
    /// `bumped > self.size` test and returns `None` **without dereferencing
    /// `data`**; `try_alloc_after_gc` then forward-scans the remaining blocks,
    /// and `install_reserved_block` reuses the tombstone slot when none has
    /// room. Note the ordinary non-quarantine path reaches the same state —
    /// `reset_region_to_zero` sets `current = 0` unconditionally — so this is a
    /// property the allocator has always had to have.
    #[test]
    fn alloc_is_correct_when_current_points_at_a_tombstone() {
        // A standalone arena over memory this test owns: no page-meta
        // registration, no GC coupling, nothing another test can observe.
        const SIZE: usize = 4096;
        let layout = std::alloc::Layout::from_size_align(SIZE, 16).unwrap();
        // SAFETY: non-zero size, valid alignment.
        let backing = unsafe { std::alloc::alloc(layout) };
        assert!(!backing.is_null());

        let mut arena = Arena {
            blocks: vec![ArenaBlock {
                data: backing,
                size: SIZE,
                offset: 0,
                dead_cycles: 0,
            }],
            current: 0,
            generation: HeapGeneration::Nursery,
            space: HeapSpace::Survivor0,
        };
        let live_base = backing as usize;
        let live_size = SIZE;

        // Shape it exactly as a full-survivor retirement leaves it.
        arena.blocks.push(tombstone());
        arena.current = arena.blocks.len() - 1;

        // (1) A tombstone `current` must not fault, and must not be allocated
        // from. The forward scan has to find the real block instead.
        let ptr = arena.alloc(64, 8);
        assert!(
            !ptr.is_null(),
            "alloc through a tombstone current returned null"
        );
        let addr = ptr as usize;
        assert!(
            addr >= live_base && addr + 64 <= live_base + live_size,
            "alloc must come from the live block, not the tombstone \
             (ptr={addr:#x}, live=[{live_base:#x}, {:#x}))",
            live_base + live_size
        );
        assert_ne!(
            arena.current,
            arena.blocks.len() - 1,
            "the forward scan must repoint `current` off the tombstone"
        );

        // (2) The memory is genuinely usable — a tombstone-derived pointer
        // would fault or alias here.
        unsafe {
            std::ptr::write_bytes(ptr, 0xA5, 64);
            assert!((0..64).all(|i| *ptr.add(i) == 0xA5));
        }

        // (3) A tombstone `current` with NO other block able to satisfy the
        // request must still fail cleanly (return `None` from every block)
        // rather than dereferencing the null `data`. `try_alloc_after_gc` is
        // the part `Arena::alloc` runs before reaching for a fresh block; a
        // tombstone must simply not answer.
        arena.current = arena.blocks.len() - 1;
        for block in arena.blocks.iter_mut() {
            if !block.data.is_null() {
                block.offset = block.size;
            }
        }
        assert!(
            arena.try_alloc_after_gc(64, 8).is_none(),
            "a full arena whose `current` is a tombstone must report no room, \
             not hand back a pointer derived from `data = null`"
        );

        // SAFETY: `backing` came from this layout and nothing else owns it.
        unsafe { std::alloc::dealloc(backing, layout) };
    }
}
