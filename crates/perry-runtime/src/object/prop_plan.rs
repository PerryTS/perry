//! Per-(class_id, interned-key) property STORE-PLAN cache.
//!
//! `js_object_set_field_by_name` pays a long interception vet on every store
//! to a class instance (`class_id != 0`): the `CLASS_VTABLE_REGISTRY` RwLock +
//! per-level string-hash setter walk, then
//! `plain_data_write_may_intercept` → `class_instance_set_may_intercept`,
//! which allocates a Rust `String` for the key and probes the address-keyed
//! descriptor tables (each probe re-allocating a `(usize, String)` map key)
//! for every prototype level. Profiled on a fiber-shaped workload this vet is
//! the dominant per-store cost — and its verdict is a pure function of
//! (class chain, key) plus per-object bits the caller checks separately.
//!
//! This cache memoizes the FAST verdict only: "a store of `key` to an
//! instance of `class_id` whose per-object flags are clear cannot be
//! intercepted" — no vtable setter in the chain, no class-prototype
//! accessor/non-writable descriptor, no `Object.prototype` own-key trap.
//! Misses simply take today's slow path; a recorded verdict lets the next
//! store of the same (class, key) skip straight to the shape-transition
//! cache.
//!
//! ## Invalidation
//! A verdict goes stale only when one of its inputs changes:
//!   * vtable mutation (setter/getter/method registration, parent linking) —
//!     tracked by the existing [`VTABLE_GEN`] generation counter, captured in
//!     the entry and compared on lookup;
//!   * descriptor installs/clears anywhere (a class prototype object may be
//!     the target), `setPrototypeOf` recording, `Object.prototype` index
//!     notes — these call [`prop_plan_epoch_bump`];
//!   * GC — interned key pointers can move or die across a collection, so
//!     the epoch is also bumped from the intern-table root scan and the
//!     dead-owner prune fan-out (both run in every collection flavor).
//!     Between collections an interned string cannot be freed or relocated,
//!     so pointer identity holds for the lifetime of an epoch window.
//!
//! Per-OBJECT conditions (frozen/sealed/no-extend, own descriptors,
//! per-instance `setPrototypeOf` override, null-proto) are NOT part of the
//! verdict — the caller checks header flags plus the ObjectMeta prototype-
//! override bit before honoring a hit.

use std::sync::atomic::{AtomicU64, Ordering};

/// Monotonic invalidation epoch for cached store plans. Bumped by descriptor
/// installs/clears, prototype recording, and every GC collection (intern-table
/// scan + dead-owner prune). Entries recorded under an older epoch never
/// match.
static PROP_PLAN_EPOCH: AtomicU64 = AtomicU64::new(1);

/// Invalidate every cached store plan. Cheap (one relaxed add); callers are
/// rare, cold paths by construction.
#[inline]
pub(crate) fn prop_plan_epoch_bump() {
    PROP_PLAN_EPOCH.fetch_add(1, Ordering::Relaxed);
}

#[derive(Clone, Copy)]
struct PlanEntry {
    /// Interned key pointer (pointer identity within an epoch window).
    key_ptr: usize,
    /// [`PROP_PLAN_EPOCH`] at record time.
    epoch: u64,
    /// [`super::class_registry::VTABLE_GEN`] at record time.
    vtable_gen: u64,
    /// Receiver class id the verdict was computed for.
    class_id: u32,
}

const PLAN_CACHE_SIZE: usize = 4096;
const PLAN_CACHE_MASK: usize = PLAN_CACHE_SIZE - 1;

thread_local! {
    // Heap-allocate the table (~112KB) — oversized inline TLS overflows the
    // ILP32 TLS layout on arm64_32 (same fix as string/intern.rs).
    static STORE_PLAN_CACHE: std::cell::UnsafeCell<Box<[PlanEntry]>> =
        std::cell::UnsafeCell::new(
            vec![
                PlanEntry {
                    key_ptr: 0,
                    epoch: 0,
                    vtable_gen: 0,
                    class_id: 0,
                };
                PLAN_CACHE_SIZE
            ]
            .into_boxed_slice(),
        );
}

#[inline(always)]
fn plan_slot(class_id: u32, key_ptr: usize) -> usize {
    // Fibonacci mix of the two identities; low bits of interned pointers are
    // alignment zeros, so fold the middle bits down first.
    let h = ((key_ptr >> 4) as u64 ^ ((class_id as u64) << 17)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    (h >> 40) as usize & PLAN_CACHE_MASK
}

fn plan_diag_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("PERRY_PLAN_DIAG").is_some())
}

/// Does a valid fast-store verdict exist for (class_id, interned key)?
#[inline]
pub(crate) fn store_plan_check(class_id: u32, key_ptr: usize) -> bool {
    if class_id == 0 || key_ptr == 0 {
        return false;
    }
    let slot = plan_slot(class_id, key_ptr);
    let hit = STORE_PLAN_CACHE.with(|c| unsafe {
        let e = (*c.get())[slot];
        e.key_ptr == key_ptr
            && e.class_id == class_id
            && e.epoch == PROP_PLAN_EPOCH.load(Ordering::Relaxed)
            && e.vtable_gen == super::class_registry::vtable_generation()
    });
    if plan_diag_enabled() {
        static CHECKS: AtomicU64 = AtomicU64::new(0);
        static HITS: AtomicU64 = AtomicU64::new(0);
        let c = CHECKS.fetch_add(1, Ordering::Relaxed) + 1;
        let h = HITS.fetch_add(hit as u64, Ordering::Relaxed) + hit as u64;
        if c % 1_000_000 == 0 {
            eprintln!(
                "PLAN-DIAG checks={} hits={} epoch={} vgen={}",
                c,
                h,
                PROP_PLAN_EPOCH.load(Ordering::Relaxed),
                super::class_registry::vtable_generation()
            );
        }
    }
    hit
}

/// Record a fast-store verdict for (class_id, interned key). Caller has just
/// completed the full interception vet with a negative result.
#[inline]
pub(crate) fn store_plan_record(class_id: u32, key_ptr: usize) {
    if class_id == 0 || key_ptr == 0 {
        return;
    }
    let slot = plan_slot(class_id, key_ptr);
    STORE_PLAN_CACHE.with(|c| unsafe {
        (*c.get())[slot] = PlanEntry {
            key_ptr,
            epoch: PROP_PLAN_EPOCH.load(Ordering::Relaxed),
            vtable_gen: super::class_registry::vtable_generation(),
            class_id,
        };
    });
    if plan_diag_enabled() {
        static RECORDS: AtomicU64 = AtomicU64::new(0);
        let r = RECORDS.fetch_add(1, Ordering::Relaxed) + 1;
        if r <= 8 || r % 1_000_000 == 0 {
            eprintln!(
                "PLAN-DIAG record #{} class={} key={:#x} slot={}",
                r, class_id, key_ptr, slot
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

const READ_PLAN_SIZE: usize = 8192;
const READ_PLAN_MASK: usize = READ_PLAN_SIZE - 1;

thread_local! {
    // Heap-allocated for the same arm64_32 TLS-size reason as the store table.
    static READ_PLAN_CACHE: std::cell::UnsafeCell<Box<[ReadPlanEntry]>> =
        std::cell::UnsafeCell::new(
            vec![
                ReadPlanEntry {
                    keys_id: 0,
                    key_ptr: 0,
                    epoch: 0,
                    field_idx: 0,
                };
                READ_PLAN_SIZE
            ]
            .into_boxed_slice(),
        );
}

#[inline(always)]
fn read_plan_slot(keys_id: usize, key_ptr: usize) -> usize {
    let h = ((keys_id >> 4) as u64 ^ ((key_ptr >> 3) as u64).rotate_left(21))
        .wrapping_mul(0x9E37_79B9_7F4A_7C15);
    (h >> 40) as usize & READ_PLAN_MASK
}

/// Look up the cached own-field index for (keys_array, interned key).
#[inline]
pub(crate) fn read_plan_lookup(keys_id: usize, key_ptr: usize) -> Option<u32> {
    if keys_id == 0 || key_ptr == 0 {
        return None;
    }
    let slot = read_plan_slot(keys_id, key_ptr);
    READ_PLAN_CACHE.with(|c| unsafe {
        let e = (*c.get())[slot];
        if e.keys_id == keys_id
            && e.key_ptr == key_ptr
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
pub(crate) fn read_plan_record(keys_id: usize, key_ptr: usize, field_idx: u32) {
    if keys_id == 0 || key_ptr == 0 {
        return;
    }
    let slot = read_plan_slot(keys_id, key_ptr);
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

    #[test]
    fn record_then_check_hits_and_epoch_bump_invalidates() {
        let key = 0xDEAD_BEE0usize;
        store_plan_record(7, key);
        assert!(store_plan_check(7, key));
        // Different class or key misses.
        assert!(!store_plan_check(8, key));
        assert!(!store_plan_check(7, key + 16));
        // Epoch bump invalidates.
        prop_plan_epoch_bump();
        assert!(!store_plan_check(7, key));
        // Re-record under the new epoch works again.
        store_plan_record(7, key);
        assert!(store_plan_check(7, key));
    }

    #[test]
    fn vtable_generation_bump_invalidates() {
        let key = 0xBEEF_00F0usize;
        store_plan_record(9, key);
        assert!(store_plan_check(9, key));
        crate::object::class_registry::test_bump_vtable_generation();
        assert!(!store_plan_check(9, key));
    }

    #[test]
    fn read_plan_roundtrip_and_epoch_flush() {
        let keys = 0xAAAA_0040usize;
        let key = 0xBBBB_0080usize;
        read_plan_record(keys, key, 21);
        assert_eq!(read_plan_lookup(keys, key), Some(21));
        assert_eq!(read_plan_lookup(keys, key + 8), None);
        prop_plan_epoch_bump();
        assert_eq!(read_plan_lookup(keys, key), None);
    }

    #[test]
    fn class_zero_and_null_key_never_cache() {
        store_plan_record(0, 0x1000);
        assert!(!store_plan_check(0, 0x1000));
        store_plan_record(3, 0);
        assert!(!store_plan_check(3, 0));
    }
}
