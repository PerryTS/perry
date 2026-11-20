//! Existing property store verdicts keyed by (receiver ShapeId, interned key).
//!
//! A shape names the receiver kind, layout, class facts and actual prototype
//! edge. A recorded negative interception vet can therefore serve another
//! eligible receiver of that shape without requiring a synthetic class id.
//! Own descriptors and immutable/reserved receiver conditions stay at the
//! callers. Descriptor/prototype/class mutations bump the existing epoch;
//! every collection also bumps it, preventing moved key or recycled ShapeId
//! identities from meeting an old verdict. No heap edge is owned here.

use crate::codegen_abi::LayoutWord;
use std::sync::atomic::{AtomicU64, Ordering};

// Monotonic invalidation epoch for cached store plans. Bumped by descriptor
// installs/clears, prototype recording, and every GC collection (intern-table
// scan + dead-owner prune). Entries recorded under an older epoch never
// match.
//
// `per_test_global!`: the plan caches below are per-thread, and any test that
// bumps the epoch (every `delete`, every prototype relink, every collection)
// would otherwise invalidate the caches of every OTHER concurrently running
// test, so a test that records a plan and checks it moments later saw a miss.
// In a test build each test thread has its own epoch beside its own caches;
// outside a test build this is the plain `static`.
per_test_global! {
    static PROP_PLAN_EPOCH: AtomicU64 = AtomicU64::new(1);
}

// The SEMANTIC half of [`PROP_PLAN_EPOCH`]: bumped by exactly the events that
// change what a property lookup would ANSWER — descriptor installs and clears,
// `delete`, per-instance prototype recording, class-prototype-object
// registration, parent-static linking. Deliberately NOT bumped by garbage
// collection.
//
// The store/read-plan caches need the GC bump because they memoize a
// (keys_array address, interned key pointer) → slot mapping, and a collection
// can relocate either identity. A cache that instead re-derives its addresses
// from live objects on every lookup — and only COMPARES them against what it
// recorded — does not: a relocation shows up as an address mismatch, which is
// a miss, and GC never adds or removes a property. Keying such a cache on the
// full epoch is not merely wasteful, it is a performance CLIFF: the
// incremental collector's root scan bumps the epoch at loop-poll cadence, so
// the entry is invalid on essentially every lookup and the "cache" degrades
// into an unconditional recompute. Measured on `gc-handoff/apps/asyncpipe.ts`:
// +35 % instructions versus baseline, against −24.6 % with the recompute
// eliminated (#7910).
per_test_global! {
    static PROP_PLAN_SEMANTIC_EPOCH: AtomicU64 = AtomicU64::new(1);
}

/// Invalidate every cached store plan. Cheap (two relaxed adds); callers are
/// rare, cold paths by construction.
#[inline]
pub(crate) fn prop_plan_epoch_bump() {
    PROP_PLAN_EPOCH.fetch_add(1, Ordering::Relaxed);
    PROP_PLAN_SEMANTIC_EPOCH.fetch_add(1, Ordering::Relaxed);
}

/// [`prop_plan_epoch_bump`] for a mutation of ONE object — a descriptor
/// install or clear, or a `delete`.
#[inline]
pub(crate) fn prop_plan_epoch_bump_for_owner(_owner: usize) {
    PROP_PLAN_EPOCH.fetch_add(1, Ordering::Relaxed);
    PROP_PLAN_SEMANTIC_EPOCH.fetch_add(1, Ordering::Relaxed);
}

/// Invalidate cached store plans for a reason that is NOT a semantic property
/// change: a collection moved interned keys or pruned dead owners' side-table
/// entries. Bumps only the pointer-identity epoch.
#[inline]
pub(crate) fn prop_plan_gc_epoch_bump() {
    PROP_PLAN_EPOCH.fetch_add(1, Ordering::Relaxed);
}

/// Current SEMANTIC epoch — see [`PROP_PLAN_SEMANTIC_EPOCH`]. Exposed so other
/// caches whose validity rests on the same set of property-changing events can
/// key on it rather than growing a parallel counter with its own — separately
/// fallible — set of mutation hooks. First consumer: the `Object.prototype`
/// "has no `then`" verdict behind the promise assimilation fast path
/// (`promise::then_probe`, #7910).
#[inline]
pub(crate) fn prop_plan_semantic_epoch() -> u64 {
    PROP_PLAN_SEMANTIC_EPOCH.load(Ordering::Relaxed)
}

#[derive(Clone, Copy)]
struct PlanEntry {
    /// Interned key pointer (pointer identity within an epoch window).
    key_ptr: usize,
    /// [`PROP_PLAN_EPOCH`] at record time.
    epoch: u64,
    /// Receiver shape whose full interception vet returned negative.
    shape_id: LayoutWord,
}

// SAFETY: integer fields only; `key_ptr == 0` never matches an interned key,
// so a zeroed slot is a miss (#11507).
unsafe impl crate::zeroed_cache::ZeroEmpty for PlanEntry {}

const PLAN_CACHE_SIZE: usize = 4096;
const PLAN_CACHE_MASK: usize = PLAN_CACHE_SIZE - 1;

crate::perry_thread_local! {
    // Heap-allocate the table (~96KB) — oversized inline TLS overflows the
    // ILP32 TLS layout on arm64_32 (same fix as string/intern.rs).
    static STORE_PLAN_CACHE: std::cell::UnsafeCell<Box<[PlanEntry]>> =
        std::cell::UnsafeCell::new(crate::zeroed_cache::new_zeroed_cache(PLAN_CACHE_SIZE));
}

#[inline(always)]
fn plan_slot(shape_id: LayoutWord, key_ptr: usize) -> usize {
    // Slot placement uses content and ShapeId, while the entry retains the
    // canonical pointer and epoch identity checks below.
    let h = (super::interned_key_slot_hash(key_ptr) ^ ((shape_id.0 as u64) << 17))
        .wrapping_mul(0x9E37_79B9_7F4A_7C15);
    (h >> 40) as usize & PLAN_CACHE_MASK
}

fn plan_diag_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *crate::once_init::get_or_init(&ON, || std::env::var_os("PERRY_PLAN_DIAG").is_some())
}

/// A valid negative interception verdict for this shape and interned key.
#[inline]
pub(crate) fn store_plan_check(shape_id: LayoutWord, key_ptr: usize) -> bool {
    if shape_id == LayoutWord(0) || key_ptr == 0 {
        return false;
    }
    let slot = plan_slot(shape_id, key_ptr);
    let hit = STORE_PLAN_CACHE.with(|c| unsafe {
        let e = (*c.get())[slot];
        e.key_ptr == key_ptr
            && e.shape_id == shape_id
            && e.epoch == PROP_PLAN_EPOCH.load(Ordering::Relaxed)
    });
    if plan_diag_enabled() {
        static CHECKS: AtomicU64 = AtomicU64::new(0);
        static HITS: AtomicU64 = AtomicU64::new(0);
        let c = CHECKS.fetch_add(1, Ordering::Relaxed) + 1;
        let h = HITS.fetch_add(hit as u64, Ordering::Relaxed) + hit as u64;
        if c % 1_000_000 == 0 {
            eprintln!(
                "PLAN-DIAG checks={} hits={} epoch={}",
                c,
                h,
                PROP_PLAN_EPOCH.load(Ordering::Relaxed)
            );
        }
    }
    hit
}

/// Record a fast-store verdict for (shape_id, interned key). Caller has just
/// completed the full interception vet with a negative result.
#[inline]
pub(crate) fn store_plan_record(shape_id: LayoutWord, key_ptr: usize) {
    if shape_id == LayoutWord(0) || key_ptr == 0 {
        return;
    }
    let slot = plan_slot(shape_id, key_ptr);
    STORE_PLAN_CACHE.with(|c| unsafe {
        (*c.get())[slot] = PlanEntry {
            key_ptr,
            epoch: PROP_PLAN_EPOCH.load(Ordering::Relaxed),
            shape_id,
        };
    });
    if plan_diag_enabled() {
        static RECORDS: AtomicU64 = AtomicU64::new(0);
        let r = RECORDS.fetch_add(1, Ordering::Relaxed) + 1;
        if r <= 8 || r % 1_000_000 == 0 {
            eprintln!(
                "PLAN-DIAG record #{} shape={} key={:#x} slot={}",
                r, shape_id.0, key_ptr, slot
            );
        }
    }
}

// ── Read-plan cache: (keys_array, interned key) → own-field index ──────────
//
// The read fast lane (`js_object_get_field_by_name`) resolves an OWN data
// field on a provably-plain arena class instance without key hashing or a
// keys-array scan: one direct-mapped probe keyed by (keys_array address,
// interned key pointer). Entries are valid for one epoch window — the same
// [`PROP_PLAN_EPOCH`] that guards store plans — which is bumped on every GC
// collection (a keys-array address cannot be freed/reused and an interned
// key cannot move within a window), on descriptor/prototype/vtable
// mutations, and on property deletes (a delete can rewrite key→slot
// mappings in place at the same keys-array address).

#[derive(Clone, Copy)]
struct ReadPlanEntry {
    keys_id: usize,
    key_ptr: usize,
    epoch: u64,
    field_idx: u32,
}

// SAFETY: integer fields only; `keys_id == 0` never matches a keys array.
unsafe impl crate::zeroed_cache::ZeroEmpty for ReadPlanEntry {}

const READ_PLAN_SIZE: usize = 8192;
const READ_PLAN_MASK: usize = READ_PLAN_SIZE - 1;

crate::perry_thread_local! {
    // Heap-allocated for the same arm64_32 TLS-size reason as the store table.
    static READ_PLAN_CACHE: std::cell::UnsafeCell<Box<[ReadPlanEntry]>> =
        std::cell::UnsafeCell::new(crate::zeroed_cache::new_zeroed_cache(READ_PLAN_SIZE));
}

#[inline(always)]
fn read_plan_slot(shape_id: LayoutWord, key_ptr: usize) -> usize {
    let h = (u64::from(shape_id.0) ^ super::interned_key_slot_hash(key_ptr).rotate_left(21))
        .wrapping_mul(0x9E37_79B9_7F4A_7C15);
    (h >> 40) as usize & READ_PLAN_MASK
}

/// Look up the cached own-field index for (keys_array, interned key).
/// Slot placement uses receiver ShapeId and key content; entry identity
/// still compares the keys-array and canonical key pointers.
///
/// `key_count` is the receiver's key count from its shape. A plan is keyed by
/// the keys ARRAY, which the lists on one growth chain share, so a slot that
/// was learned on a longer list answers only when it is below this
/// receiver's count.
#[inline]
pub(crate) fn read_plan_lookup(
    shape_id: LayoutWord,
    keys_id: usize,
    key_ptr: usize,
    key_count: u32,
) -> Option<u32> {
    if keys_id == 0 || key_ptr == 0 {
        return None;
    }
    let slot = read_plan_slot(shape_id, key_ptr);
    READ_PLAN_CACHE.with(|c| unsafe {
        let e = (*c.get())[slot];
        if e.keys_id == keys_id
            && e.key_ptr == key_ptr
            && e.field_idx < key_count
            && e.epoch == PROP_PLAN_EPOCH.load(Ordering::Relaxed)
        {
            Some(e.field_idx)
        } else {
            None
        }
    })
}

/// Record an own-field index resolved by a validated keys-array scan.
#[inline]
pub(crate) fn read_plan_record(
    shape_id: LayoutWord,
    keys_id: usize,
    key_ptr: usize,
    field_idx: u32,
) {
    if keys_id == 0 || key_ptr == 0 {
        return;
    }
    let slot = read_plan_slot(shape_id, key_ptr);
    READ_PLAN_CACHE.with(|c| unsafe {
        (*c.get())[slot] = ReadPlanEntry {
            keys_id,
            key_ptr,
            epoch: PROP_PLAN_EPOCH.load(Ordering::Relaxed),
            field_idx,
        };
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A recorded verdict is invalidated by the PROCESS-global
    /// `PROP_PLAN_EPOCH` (bumped by every GC cycle's dead-owner fan-out and
    /// every descriptor install, on any thread). A bump
    /// landing between record and check legitimately flushes the entry, so a
    /// single-shot `record → assert(check)` is order-dependent under
    /// default-parallel `cargo test` (#6965). Retry instead: a genuine
    /// record/check regression fails every lap, while a concurrent bump only
    /// costs one. The caches themselves are thread-local, so nothing a
    /// parallel thread does can turn a MISS assertion into a spurious hit —
    /// only the positive direction needs this.
    fn store_plan_records_and_hits(shape_id: LayoutWord, key_ptr: usize) -> bool {
        (0..64).any(|_| {
            store_plan_record(shape_id, key_ptr);
            store_plan_check(shape_id, key_ptr)
        })
    }

    #[test]
    fn record_then_check_hits_and_epoch_bump_invalidates() {
        let key = crate::string::canonical_key(b"plan-key") as usize;
        let other = crate::string::canonical_key(b"other-key") as usize;
        assert!(store_plan_records_and_hits(LayoutWord(7), key));
        // Different shape or key misses.
        assert!(!store_plan_check(LayoutWord(8), key));
        assert!(!store_plan_check(LayoutWord(7), other));
        // Epoch bump invalidates.
        prop_plan_epoch_bump();
        assert!(!store_plan_check(LayoutWord(7), key));
        // Re-record under the new epoch works again.
        assert!(store_plan_records_and_hits(LayoutWord(7), key));
    }

    #[test]
    fn a_verdict_for_one_prototype_shape_does_not_serve_another() {
        let key = crate::string::canonical_key(b"prototype-key") as usize;
        assert!(store_plan_records_and_hits(LayoutWord(11), key));
        assert!(!store_plan_check(LayoutWord(12), key));
        assert!(!store_plan_check(LayoutWord(0), key));
    }

    #[test]
    fn read_plan_roundtrip_and_epoch_flush() {
        let keys = 0xAAAA_0040usize;
        let key = crate::string::canonical_key(b"read-key") as usize;
        let other = crate::string::canonical_key(b"other-key") as usize;
        // Same order-dependence as the store-plan tests: a concurrent global
        // epoch bump between record and lookup flushes the thread-local
        // entry, so retry the roundtrip.
        assert!((0..64).any(|_| {
            read_plan_record(LayoutWord(17), keys, key, 21);
            read_plan_lookup(LayoutWord(17), keys, key, u32::MAX) == Some(21)
        }));
        assert_eq!(
            read_plan_lookup(LayoutWord(17), keys, other, u32::MAX),
            None
        );
        // A shorter list on the same array does not see a slot past its count.
        assert!((0..64).any(|_| {
            read_plan_record(LayoutWord(17), keys, key, 21);
            read_plan_lookup(LayoutWord(17), keys, key, 21).is_none()
                && read_plan_lookup(LayoutWord(17), keys, key, 22) == Some(21)
        }));
        prop_plan_epoch_bump();
        assert_eq!(read_plan_lookup(LayoutWord(17), keys, key, u32::MAX), None);
    }

    #[test]
    fn plan_slots_follow_content_when_interned_addresses_change() {
        let (_storage, a, b) = super::super::address_free_slots_tests::relocated_keys(
            b"same-property",
            b"same-property",
        );
        assert_eq!(
            plan_slot(LayoutWord(0x8000_0042), a),
            plan_slot(LayoutWord(0x8000_0042), b)
        );
        assert_eq!(
            read_plan_slot(LayoutWord(0x8000_0042), a),
            read_plan_slot(LayoutWord(0x8000_0042), b)
        );
    }

    #[test]
    fn shape_zero_and_null_key_never_cache() {
        store_plan_record(LayoutWord(0), 0x1000);
        assert!(!store_plan_check(LayoutWord(0), 0x1000));
        store_plan_record(LayoutWord(3), 0);
        assert!(!store_plan_check(LayoutWord(3), 0));
    }
}

#[cfg(test)]
mod zeroed_cache_tests {
    use super::*;

    /// #11507: both plan caches are zero-allocated rather than filled, so a
    /// thread's first view of them must hold no key in any slot.
    #[test]
    fn fresh_thread_plan_caches_read_empty_everywhere() {
        std::thread::spawn(|| {
            STORE_PLAN_CACHE.with(|c| {
                let cache = unsafe { &*c.get() };
                assert_eq!(cache.len(), PLAN_CACHE_SIZE);
                for e in cache.iter() {
                    assert_eq!((e.key_ptr, e.epoch, e.shape_id.0), (0, 0, 0));
                }
            });
            READ_PLAN_CACHE.with(|c| {
                let cache = unsafe { &*c.get() };
                assert_eq!(cache.len(), READ_PLAN_SIZE);
                for e in cache.iter() {
                    assert_eq!((e.keys_id, e.key_ptr, e.epoch, e.field_idx), (0, 0, 0, 0));
                }
            });
        })
        .join()
        .unwrap();
    }
}
