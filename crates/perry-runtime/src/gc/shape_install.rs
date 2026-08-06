//! #7510 item 1 — the construction-side shape-install memo.
//!
//! # What this replaces
//!
//! `js_gc_init_typed_shape_layout` runs on **every** construction of a typed
//! object literal and every `new` of a class with a typed field layout. Since
//! #6893 the descriptor it installs is per-*shape*, not per-object: all
//! same-shape objects share one canonical `TypedLayoutDescriptor` in
//! `SHAPE_LAYOUTS`, keyed by the shared `keys_array`. The per-object work that
//! remains is two header bits — `GC_OBJ_TYPED_LAYOUT_INTACT`, and
//! `GC_LAYOUT_POINTER_FREE`/`GC_LAYOUT_SIDE_MASK`.
//!
//! The call did not know that. For the 20-millionth `{v, w}` literal it still
//! built a `TypedLayoutDescriptor` (72 bytes, two `Vec`-carrying enums, cloned
//! once and dropped), took a `RefCell` borrow on the thread-local
//! `SHAPE_LAYOUTS`, hashed the `keys_array` pointer, and compared the freshly
//! built descriptor field-by-field against the one already stored — to conclude
//! what the first construction had already concluded. Measured after #7525,
//! `js_gc_init_typed_shape_layout` + `shape_install_shared` were ~13% of self
//! time on `churn_alloc`.
//!
//! # What the memo asserts
//!
//! One thing, and nothing else:
//!
//! > `SHAPE_LAYOUTS[keys]` currently holds `Some(D)`, where `D` is exactly the
//! > descriptor that `(slot_count, raw_words, pointer_words)` describes.
//!
//! That is the *entire* content of a successful `shape_install_shared` on a
//! shape that is already installed — its `Some(Some(existing)) if existing ==
//! descriptor` arm writes nothing to the map. So a memo hit lets the caller
//! skip the descriptor build and the map round-trip and go straight to the
//! header bits.
//!
//! # What the memo deliberately does NOT assert
//!
//! **Anything about the object.** The caller still re-derives, from the mask
//! words themselves, every fact that its header declaration rests on:
//! `field_count == slot_count`, raw-f64/pointer mask disjointness, and the
//! per-slot validation that each raw-f64 slot holds a plain double and no
//! pointer-bearing slot sits outside the pointer mask. The
//! `POINTER_FREE`/`SIDE_MASK` choice is recomputed from the pointer mask, not
//! read back from the memo — the memo carries no header state at all.
//!
//! This split is the soundness bar. A wrong `POINTER_FREE` is a
//! use-after-free factory: `heap_payload_slot_selection` skips the whole
//! payload without consulting any mask. Keeping that decision on data the
//! caller re-derives per object means a stale memo can only cost work, never
//! correctness.
//!
//! # Self-healing
//!
//! A memo entry is falsified by exactly one transition: `SHAPE_LAYOUTS[keys]`
//! going from `Some(D)` to something else. `shape_install_shared` is the only
//! writer of that map, and its only such transition is the ambiguity poison
//! (`Some(Some(_)) => insert(None)` — two live layouts sharing one key set).
//! That branch calls [`invalidate`], which drops every entry. Entries are
//! never removed from `SHAPE_LAYOUTS` and never overwritten with a *different*
//! `Some`, so there is no other way to go stale.
//!
//! Everything else already degrades safely without help:
//!
//! - A moving GC relocates the `keys_array` ⟹ objects report the new address,
//!   which no entry matches ⟹ miss ⟹ the slow path re-installs and re-records.
//! - The old address is recycled by an unrelated shape ⟹ its constructions
//!   arrive with different mask globals ⟹ miss. If they arrive with the *same*
//!   mask globals and slot count, the memo's claim is still true of them (the
//!   stored descriptor describes them exactly), so the hit is correct.
//! - `PERRY_SHAPE_LAYOUT_KEYED=0` ⟹ [`record`] is unreachable (it is only
//!   called after a successful `shape_install_shared`, which that knob gates)
//!   ⟹ the table stays empty and every lookup misses.
//!
//! # Keying
//!
//! Entries are keyed by `(keys_array, raw_words, pointer_words, slot_count,
//! word counts)`. The two mask pointers are the addresses of codegen-emitted
//! `private unnamed_addr constant` globals — one per class
//! (`mask_global_name_from_keys_global`) or per object-literal site — so
//! pointer identity is a conservative proxy for word equality: two globals
//! holding equal words that LLVM declined to merge cost a miss, never a wrong
//! hit. Nothing dereferences a stored pointer; they are compared as integers
//! only.
//!
//! # This table is NOT a GC root
//!
//! Worth stating explicitly, because a runtime-side cache of a raw heap
//! pointer usually *is* one and the static root-dominance checker cannot see
//! it (`gc_register_mutable_root_scanner`, CLAUDE.md). `keys` holds the
//! address of a live `keys_array`, but it is only ever **compared as an
//! integer** against the `keys_array` the object under construction reports —
//! never dereferenced, never handed to the mutator, never used to keep
//! anything alive.
//!
//! Both ways it can go stale are already safe:
//!
//! - The array **moves**: every live object reports the new address, so the
//!   old entry simply stops matching. It is dead weight until evicted, not a
//!   dangling read.
//! - The old address is **recycled** by a different shape's keys array, and
//!   that shape's constructions happen to arrive with the same mask globals
//!   and slot count. Then the entry's claim is still true — `SHAPE_LAYOUTS`
//!   is keyed by the same address, its entries are never removed, and the
//!   descriptor stored there describes those objects exactly. If instead they
//!   arrive with different masks the entry does not match, and the install
//!   they fall through to poisons the shape and clears the table.

use std::cell::UnsafeCell;

// --- mask words, without a `LayoutSlotMask` ---------------------------------
//
// `LayoutSlotMask` is a 32-byte enum with a `Vec` arm, so building one has a
// destructor: every early return in `init_typed_shape_layout` used to carry
// drop glue for two of them, and a wide shape allocated twice per
// construction. The construction path needs the *predicates*, not the type —
// the descriptor is only built when a shape is actually installed. These three
// answer the same questions straight off the caller's words.
//
// They are the only implementation of those predicates on the construction
// path now, and `tests::mask_word_helpers_agree_with_layout_slot_mask` pins
// each one against the `LayoutSlotMask` method it replaced across every
// interesting mask/`slot_count` combination — including the trailing-zero and
// past-the-end cases where the trimmed enum and the raw words could disagree.

/// `LayoutSlotMask::from_words(words).contains_slot(slot)`.
#[inline(always)]
pub(super) fn words_contain_slot(words: &[u64], slot: usize) -> bool {
    let word = slot / 64;
    word < words.len() && words[word] & (1u64 << (slot % 64)) != 0
}

/// `LayoutSlotMask::from_words(words).is_empty()`.
#[inline(always)]
pub(super) fn words_are_empty(words: &[u64]) -> bool {
    words.iter().all(|&w| w == 0)
}

/// `LayoutSlotMask::from_words(a).intersects(&from_words(b), slot_count)` —
/// any slot below `slot_count` set in both. Bits at or above `slot_count` are
/// ignored, matching `visit_slots`, which never walks past the object's live
/// prefix.
#[inline]
pub(super) fn words_intersect(a: &[u64], b: &[u64], slot_count: usize) -> bool {
    let limit_words = slot_count.div_ceil(64).min(a.len()).min(b.len());
    for word in 0..limit_words {
        let live = slot_count - word * 64;
        let limit_mask = if live >= 64 {
            u64::MAX
        } else {
            (1u64 << live) - 1
        };
        if a[word] & b[word] & limit_mask != 0 {
            return true;
        }
    }
    false
}

/// Direct-mapped entries. Sized for the shape working set of a hot loop, not
/// for a whole program: a monomorphic allocation site needs one, and the spread
/// across interleaved shapes is what the rest buy. Kept a power of two so the
/// index is a mask.
///
/// 32 rather than 8, measured. A direct-mapped table cycled round-robin by more
/// shapes than it has slots hits **zero** percent of the time — each entry is
/// evicted by its partner before it is read again — so it pays the probe and
/// gets nothing. A 16-shape churn loop against 8 slots was a reproducible
/// 0.993× on the pinned host; the same loop against 32 slots fits and wins with
/// the monomorphic case. The whole table is 1 KiB of const-initialised
/// thread-local, so the cost of the headroom is a page that is never touched by
/// a program with one shape.
const MEMO_SLOTS: usize = 32;

/// One memoised "this shape's canonical descriptor is already installed, and
/// equals what these mask globals describe" fact.
///
/// `dims` packs `slot_count` (low 24 bits — `js_gc_init_typed_shape_layout`
/// rejects anything ≥ 16_000_000 before reaching here), the raw-f64 mask word
/// count (bits 24..44) and the pointer mask word count (bits 44..64).
/// [`record`] refuses to store a tuple that does not fit, so a packed value is
/// never ambiguous.
#[derive(Clone, Copy)]
struct Entry {
    keys: usize,
    raw_words: *const u64,
    pointer_words: *const u64,
    dims: u64,
}

/// `keys == 0` is the empty marker: no live object ever reports a null
/// `keys_array` to the lookup (the caller checks first), so an empty slot can
/// never be mistaken for a hit.
const EMPTY: Entry = Entry {
    keys: 0,
    raw_words: std::ptr::null(),
    pointer_words: std::ptr::null(),
    dims: 0,
};

const SLOT_COUNT_BITS: u32 = 24;
const WORD_COUNT_BITS: u32 = 20;

/// Pack the dimension triple, or `None` when it does not fit (a pathological
/// shape: > 16M slots, or > 1M mask words). An unpackable tuple is simply
/// never memoised.
#[inline]
fn pack_dims(slot_count: usize, raw_len: usize, pointer_len: usize) -> Option<u64> {
    if slot_count >= 1 << SLOT_COUNT_BITS
        || raw_len >= 1 << WORD_COUNT_BITS
        || pointer_len >= 1 << WORD_COUNT_BITS
    {
        return None;
    }
    Some(
        slot_count as u64
            | (raw_len as u64) << SLOT_COUNT_BITS
            | (pointer_len as u64) << (SLOT_COUNT_BITS + WORD_COUNT_BITS),
    )
}

/// Which entry a `(shape, mask globals)` tuple lives in.
///
/// The mask-global addresses are mixed in, not just the shape. Two object
/// literal sites with the same key names share one `keys_array` but get
/// separate mask globals unless LLVM's constant merger folds them (they are
/// `private unnamed_addr constant`, so it may — but the table must not depend
/// on that). Keying the slot on the shape alone would put both sites in one
/// entry and let them evict each other on every iteration of a loop that
/// builds both. Any index function is *correct* — the full tuple is compared
/// on the way out — so this is purely about not colliding.
///
/// The low bits of a heap address are allocation-granularity noise, hence the
/// shifts; the two mask pointers use different ones so a shape whose masks are
/// both empty (one shared dangling `&[]` address) does not cancel itself out.
#[inline(always)]
fn slot_index(keys: usize, raw_words: *const u64, pointer_words: *const u64) -> usize {
    let mixed = (keys >> 4) ^ (raw_words as usize >> 4) ^ (pointer_words as usize >> 3);
    mixed & (MEMO_SLOTS - 1)
}

thread_local! {
    /// Per-thread, mirroring `SHAPE_LAYOUTS`'s own thread-locality — an entry
    /// names an address in this thread's arena and a fact about this thread's
    /// map, and neither means anything on another thread.
    ///
    /// `UnsafeCell` rather than `Cell<[Entry; N]>`: a `Cell` get/set would copy
    /// the whole table on every probe. Access is single-threaded by
    /// construction (this is a `thread_local!`), and the module hands out no
    /// references into it.
    static SHAPE_INSTALL_MEMO: UnsafeCell<[Entry; MEMO_SLOTS]> =
        const { UnsafeCell::new([EMPTY; MEMO_SLOTS]) };
}

/// Address of this thread's [`SHAPE_INSTALL_MEMO`] (see `crate::tls_hot`).
pub(crate) fn shape_install_memo_hot_addr() -> *mut u8 {
    SHAPE_INSTALL_MEMO.with(|m| m as *const _ as *mut u8)
}

/// [`SHAPE_INSTALL_MEMO`] without a TLS resolution.
#[inline(always)]
fn table() -> &'static UnsafeCell<[Entry; MEMO_SLOTS]> {
    // SAFETY: paired with `shape_install_memo_hot_addr` above; asserted by
    // `tls_hot::tests::cached_addresses_match_thread_locals`.
    unsafe {
        &*(crate::tls_hot::hot().shape_install_memo as *const UnsafeCell<[Entry; MEMO_SLOTS]>)
    }
}

/// True when `SHAPE_LAYOUTS[keys]` is already known to hold exactly the
/// descriptor that `(slot_count, raw_words, pointer_words)` describes.
///
/// `false` is never an error — it just means the caller must take the ordinary
/// `shape_install_shared` path, which is what establishes the entry.
#[inline(always)]
pub(super) fn hit(
    keys: usize,
    slot_count: usize,
    raw_words: &[u64],
    pointer_words: &[u64],
) -> bool {
    debug_assert!(keys != 0, "the null keys_array is the empty-slot marker");
    let Some(dims) = pack_dims(slot_count, raw_words.len(), pointer_words.len()) else {
        return false;
    };
    // SAFETY: `table()` is this thread's own storage; the reference does not
    // escape and nothing re-enters between the read and its use.
    let entry =
        unsafe { (*table().get())[slot_index(keys, raw_words.as_ptr(), pointer_words.as_ptr())] };
    let matched = entry.keys == keys
        && entry.dims == dims
        && std::ptr::eq(entry.raw_words, raw_words.as_ptr())
        && std::ptr::eq(entry.pointer_words, pointer_words.as_ptr());
    #[cfg(test)]
    {
        if matched {
            counters::note_hit();
        }
    }
    matched
}

/// Record that `shape_install_shared` just confirmed (or established)
/// `SHAPE_LAYOUTS[keys] == Some(D)` for the descriptor these mask words
/// describe.
#[inline]
pub(super) fn record(keys: usize, slot_count: usize, raw_words: &[u64], pointer_words: &[u64]) {
    if keys == 0 {
        return;
    }
    let Some(dims) = pack_dims(slot_count, raw_words.len(), pointer_words.len()) else {
        return;
    };
    #[cfg(test)]
    counters::note_record();
    // SAFETY: as in `hit` — this thread's own storage, no escaping reference.
    unsafe {
        (*table().get())[slot_index(keys, raw_words.as_ptr(), pointer_words.as_ptr())] = Entry {
            keys,
            raw_words: raw_words.as_ptr(),
            pointer_words: pointer_words.as_ptr(),
            dims,
        };
    }
}

/// Drop every entry.
///
/// Called from the one transition that can falsify a memo: `SHAPE_LAYOUTS`
/// poisoning a shape to ambiguous. Clearing the whole table rather than the
/// one affected slot keeps this correct without having to reason about index
/// collisions, and costs one 256-byte store on a branch that fires when a
/// program's shape *stops* being monomorphic — i.e. rarely, and never twice
/// for the same shape.
#[inline]
pub(super) fn invalidate() {
    // SAFETY: as in `hit` — this thread's own storage, no escaping reference.
    unsafe {
        (*table().get()) = [EMPTY; MEMO_SLOTS];
    }
}

/// Hit/record counters, so a test can assert the fast path was **live** rather
/// than merely that nothing threw — the #7525 failure mode was a fast path
/// that fired once in 40 million calls and was believed anyway.
///
/// Test-only on purpose: this sits on the hottest path in allocation-heavy
/// code, and a counter that ships would be paid 20 million times to answer a
/// question asked once.
#[cfg(test)]
pub(super) mod counters {
    use std::cell::Cell;

    thread_local! {
        static HITS: Cell<u64> = const { Cell::new(0) };
        static RECORDS: Cell<u64> = const { Cell::new(0) };
    }

    #[inline]
    pub(in crate::gc) fn note_hit() {
        HITS.with(|c| c.set(c.get() + 1));
    }

    #[inline]
    pub(in crate::gc) fn note_record() {
        RECORDS.with(|c| c.set(c.get() + 1));
    }

    /// `(hits, records)` since [`reset`].
    pub(in crate::gc) fn snapshot() -> (u64, u64) {
        (HITS.with(|c| c.get()), RECORDS.with(|c| c.get()))
    }

    pub(in crate::gc) fn reset() {
        HITS.with(|c| c.set(0));
        RECORDS.with(|c| c.set(0));
    }
}

#[cfg(test)]
pub(in crate::gc) fn test_clear() {
    invalidate();
    counters::reset();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::layout::LayoutSlotMask;

    /// The construction path no longer builds a `LayoutSlotMask` to answer
    /// "is slot K raw-f64 / pointer-bearing / is this mask empty / do these
    /// two overlap". These three helpers are that path's only implementation,
    /// so they have to agree with the type they replaced — including on the
    /// shapes where the trimmed enum and the raw words could plausibly differ:
    /// trailing zero words, bits past `slot_count`, and slots past the end of
    /// the word array.
    #[test]
    fn mask_word_helpers_agree_with_layout_slot_mask() {
        let masks: &[&[u64]] = &[
            &[],
            &[0],
            &[0b1],
            &[0b1011],
            &[u64::MAX],
            &[0, 0],
            &[0b1, 0],
            &[0, 0b1],
            &[u64::MAX, 0b101],
            &[0b1, 0, 0],
            &[0, 0, 1 << 63],
        ];
        for &words in masks {
            let mask = LayoutSlotMask::from_words(words);
            assert_eq!(
                words_are_empty(words),
                mask.is_empty(),
                "is_empty disagreed for {words:?}"
            );
            for slot in 0..200 {
                assert_eq!(
                    words_contain_slot(words, slot),
                    mask.contains_slot(slot),
                    "contains_slot({slot}) disagreed for {words:?}"
                );
            }
            for &other_words in masks {
                let other = LayoutSlotMask::from_words(other_words);
                for slot_count in [0usize, 1, 2, 63, 64, 65, 128, 129, 192] {
                    assert_eq!(
                        words_intersect(words, other_words, slot_count),
                        mask.intersects(&other, slot_count),
                        "intersects(slot_count = {slot_count}) disagreed for \
                         {words:?} vs {other_words:?}"
                    );
                }
            }
        }
    }

    /// Distinct shapes must not alias into one entry, and a shape whose
    /// dimensions do not fit the packed key must simply never memoise rather
    /// than collide with one that does.
    #[test]
    fn entries_discriminate_on_every_keyed_field() {
        let raw: [u64; 1] = [0b01];
        let other_raw: [u64; 1] = [0b10];
        let pointers: [u64; 1] = [0b10];
        let keys = 0x4000usize;

        invalidate();
        record(keys, 2, &raw, &pointers);
        assert!(hit(keys, 2, &raw, &pointers));
        assert!(!hit(keys + 16, 2, &raw, &pointers), "a different shape");
        assert!(!hit(keys, 3, &raw, &pointers), "a different slot count");
        assert!(!hit(keys, 2, &other_raw, &pointers), "a different raw mask");
        assert!(!hit(keys, 2, &raw, &raw), "a different pointer mask");
        assert!(!hit(keys, 2, &raw, &[]), "a different pointer word count");

        invalidate();
        assert!(
            !hit(keys, 2, &raw, &pointers),
            "invalidate must drop entries"
        );
    }

    /// A `slot_count` that overflows the packed key is refused by both halves,
    /// so it can neither be stored nor produce a spurious hit against a
    /// smaller shape that packs to the same bits.
    #[test]
    fn unpackable_dimensions_are_never_memoised() {
        let raw: [u64; 1] = [0b01];
        let keys = 0x8000usize;
        let huge = 1usize << SLOT_COUNT_BITS;

        invalidate();
        record(keys, huge, &raw, &[]);
        assert!(!hit(keys, huge, &raw, &[]));
        // …and it did not land in the slot a packable shape would use.
        assert!(!hit(keys, 0, &raw, &[]));
    }
}
