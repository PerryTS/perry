//! #7510 item 1 — the construction-side shape-install memo.
//!
//! # What this replaces
//!
//! `js_gc_init_typed_shape_layout` runs on **every** construction of a typed
//! object literal and every `new` of a class with a typed field layout. Since
//! #6893 the descriptor it installs is per-*shape*, not per-object: all
//! same-shape objects share one canonical `TypedLayoutDescriptor` in
//! `SHAPE_LAYOUTS`, keyed by their immutable runtime ShapeId. The per-object
//! work that remains is two header bits — `GC_OBJ_TYPED_LAYOUT_INTACT`, and
//! `GC_LAYOUT_POINTER_FREE`/`GC_LAYOUT_SIDE_MASK`.
//!
//! The call did not know that. For the 20-millionth `{v, w}` literal it still
//! built a `TypedLayoutDescriptor` (72 bytes, two `Vec`-carrying enums, cloned
//! once and dropped), took a `RefCell` borrow on the thread-local
//! `SHAPE_LAYOUTS`, hashed the shape key, and compared the freshly
//! built descriptor field-by-field against the one already stored — to conclude
//! what the first construction had already concluded. Measured after #7525,
//! `js_gc_init_typed_shape_layout` + `shape_install_shared` were ~13% of self
//! time on `churn_alloc`.
//!
//! # What the memo asserts
//!
//! One thing, and nothing else:
//!
//! > `SHAPE_LAYOUTS[shape_id]` currently holds `Some(D)`, where `D` is exactly
//! > the descriptor that `(slot_count, raw_words, pointer_words)` describes.
//!
//! That is the *entire* content of a successful `shape_install_shared` on a
//! shape that is already installed — its `Some(Some(existing)) if existing ==
//! descriptor` arm writes nothing to the map. So a memo hit lets the caller
//! skip the descriptor build and the map round-trip and go straight to the
//! header bits.
//!
//! # What the memo deliberately does NOT assert
//!
//! **Anything mutable about the object.** For the validating entry point the
//! caller still checks, per instance, that each raw-f64 slot holds a plain
//! double and no pointer-bearing slot sits outside the pointer mask. The
//! immutable `field_count == slot_count` fact is re-used from the ShapeId: an
//! entry is recorded only after the authoritative descriptor established it.
//!
//! This split is the soundness bar. A wrong `POINTER_FREE` is a
//! use-after-free factory: `heap_payload_slot_selection` skips the whole
//! payload without consulting any mask. Keeping every object-dependent
//! decision on data the caller re-derives per object means a stale memo can
//! only cost work, never correctness.
//!
//! # What the memo DOES replay, and why that is not the same thing (#7578)
//!
//! Two predicates over the mask words alone: their disjointness, and whether
//! the pointer mask is empty (the `POINTER_FREE`/`SIDE_MASK` choice). Both used
//! to be recomputed on every construction of every shape.
//!
//! Replaying them is sound for a reason that does not extend to anything about
//! the object. An entry matches on the mask globals' **addresses and lengths**,
//! and those globals are codegen-emitted `private unnamed_addr constant`s: they
//! live in the read-only image, are never written and are never freed. So a
//! matching address *is* a matching byte string, for the life of the process,
//! and a pure function of those bytes has one answer forever. Contrast a header
//! bit, which belongs to an object whose contents change under the mutator —
//! which is why every object-dependent check above stays per-instance.
//!
//! Disjointness needs no stored bit at all: a shape whose masks intersect is
//! downgraded before it can reach [`record`], so no intersecting tuple can be
//! in the table to hit. Emptiness is stored, in `dims` bit 62.
//!
//! The failure mode this leaves is a *miss*, not a wrong hit: two globals
//! holding equal words that LLVM declined to merge get separate entries, and
//! anything that fails the address, length or slot-count comparison falls
//! through to the install, which re-derives everything.
//!
//! # Self-healing
//!
//! A memo entry is falsified by exactly one transition:
//! `SHAPE_LAYOUTS[shape_id]` going from `Some(D)` to something else.
//! `shape_install_shared` is the only writer of that map, and its only such
//! transition is the ambiguity poison (`Some(Some(_)) => insert(None)` — two
//! live layouts sharing one key set).
//! That branch calls [`invalidate`], which drops every entry. Entries are
//! never removed from `SHAPE_LAYOUTS` and never overwritten with a *different*
//! `Some`, so there is no other way to go stale.
//!
//! Everything else already degrades safely without help:
//!
//! - A moving GC relocates the `keys_array` ⟹ the ShapeId and its map entry do
//!   not change; neither stores a heap address.
//! - A structural or semantic shape transition mints a new process-unique ID
//!   ⟹ the old entry cannot match the changed object.
//! - `PERRY_SHAPE_LAYOUT_KEYED=0` ⟹ [`record`] is unreachable (it is only
//!   called after a successful `shape_install_shared`, which that knob gates)
//!   ⟹ the table stays empty and every lookup misses.
//!
//! # Keying
//!
//! Entries are keyed by `(ShapeId, raw_words, pointer_words, slot_count,
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
//! It contains a ShapeId plus addresses of immutable code-image constants, but
//! no heap pointer. ShapeIds are process-global, monotonic, and never reused;
//! mask globals are never written or freed. Nothing here needs marking or
//! rewriting by a collector.

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
#[cfg(test)]
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
/// count (bits 24..43), the pointer mask word count (bits 43..62) and, in bit
/// 62, whether the pointer mask is empty. [`record`] refuses to store a tuple
/// that does not fit, so a packed value is never ambiguous.
#[derive(Clone, Copy)]
struct Entry {
    shape_id: u32,
    raw_words: *const u64,
    pointer_words: *const u64,
    dims: u64,
}

/// `shape_id == 0` is the empty marker: runtime ShapeIds occupy a disjoint,
/// nonzero range, so an empty slot can never be mistaken for a hit.
const EMPTY: Entry = Entry {
    shape_id: 0,
    raw_words: std::ptr::null(),
    pointer_words: std::ptr::null(),
    dims: 0,
};

const SLOT_COUNT_BITS: u32 = 24;
const WORD_COUNT_BITS: u32 = 19;
/// Bit 62 of `dims`: this shape's pointer mask is empty, so its install
/// selected `GC_LAYOUT_POINTER_FREE` rather than `GC_LAYOUT_SIDE_MASK` (#7578).
///
/// Payload, not key: [`hit`] probes with it clear and reads the stored one back
/// out of the matched entry.
const POINTER_MASK_EMPTY_BIT: u64 = 1 << 62;

/// Pack the dimension triple plus the pointer-mask-empty bit, or `None` when it
/// does not fit (a pathological shape: > 16M slots, or > 512K mask words —
/// 33.5M slots, twice the ceiling the entry point already enforces). An
/// unpackable tuple is simply never memoised.
#[inline]
fn pack_dims(
    slot_count: usize,
    raw_len: usize,
    pointer_len: usize,
    pointer_mask_empty: bool,
) -> Option<u64> {
    if slot_count >= 1 << SLOT_COUNT_BITS
        || raw_len >= 1 << WORD_COUNT_BITS
        || pointer_len >= 1 << WORD_COUNT_BITS
    {
        return None;
    }
    Some(
        slot_count as u64
            | (raw_len as u64) << SLOT_COUNT_BITS
            | (pointer_len as u64) << (SLOT_COUNT_BITS + WORD_COUNT_BITS)
            | if pointer_mask_empty {
                POINTER_MASK_EMPTY_BIT
            } else {
                0
            },
    )
}

/// Which entry a `(ShapeId, mask globals)` tuple lives in.
///
/// The mask-global addresses are mixed in, not just the shape. Two object
/// literal sites with the same structural ShapeId can get
/// separate mask globals unless LLVM's constant merger folds them (they are
/// `private unnamed_addr constant`, so it may — but the table must not depend
/// on that). Keying the slot on the shape alone would put both sites in one
/// entry and let them evict each other on every iteration of a loop that
/// builds both. Any index function is *correct* — the full tuple is compared
/// on the way out — so this is purely about not colliding.
///
/// The two mask pointers use different shifts so a shape whose masks are
/// both empty (one shared dangling `&[]` address) does not cancel itself out.
#[inline(always)]
fn slot_index(shape_id: u32, raw_words: *const u64, pointer_words: *const u64) -> usize {
    let mixed = shape_id as usize ^ (raw_words as usize >> 4) ^ (pointer_words as usize >> 3);
    mixed & (MEMO_SLOTS - 1)
}

thread_local! {
    /// Per-thread, mirroring `SHAPE_LAYOUTS`'s own thread-locality — an entry
    /// names a process-global ShapeId and a fact about this thread's map. The
    /// latter does not carry across threads.
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

/// `Some(pointer_mask_empty)` when `SHAPE_LAYOUTS[shape_id]` is already known
/// to hold exactly the descriptor that `(slot_count, raw_words, pointer_words)`
/// describes; `None` otherwise.
///
/// `None` is never an error — it just means the caller must take the ordinary
/// `shape_install_shared` path, which is what establishes the entry.
///
/// **Takes the raw `(pointer, word count)` pairs, not slices** (#7578). The
/// caller receives them that way across the FFI boundary, and normalising a
/// null pointer into `NonNull::dangling()` so that `slice::from_raw_parts` is
/// legal cost twelve instructions per construction to build two slices this
/// function only ever compares as integers. [`record`] takes the same pairs, so
/// the two agree on the empty-mask representation by construction.
#[inline(always)]
pub(super) fn hit(
    shape_id: u32,
    slot_count: usize,
    raw_words: *const u64,
    raw_word_count: u32,
    pointer_words: *const u64,
    pointer_word_count: u32,
) -> Option<bool> {
    debug_assert!(shape_id != 0, "zero is the empty ShapeId marker");
    let dims = pack_dims(
        slot_count,
        raw_word_count as usize,
        pointer_word_count as usize,
        false,
    )?;
    // SAFETY: `table()` is this thread's own storage; the reference does not
    // escape and nothing re-enters between the read and its use.
    let entry = unsafe { (*table().get())[slot_index(shape_id, raw_words, pointer_words)] };
    let matched = entry.shape_id == shape_id
        && entry.dims & !POINTER_MASK_EMPTY_BIT == dims
        && std::ptr::eq(entry.raw_words, raw_words)
        && std::ptr::eq(entry.pointer_words, pointer_words);
    if !matched {
        return None;
    }
    Some(entry.dims & POINTER_MASK_EMPTY_BIT != 0)
}

/// Count a memo result only once the caller has completed any per-object slot
/// validation and is about to consume it. Keeping this separate from [`hit`]
/// preserves the testable guarantee that a contradictory object never takes
/// the fast path.
#[inline(always)]
pub(super) fn note_hit() {
    #[cfg(test)]
    counters::note_hit();
}

#[cfg(test)]
#[inline]
pub(super) fn note_descriptor_probe() {
    counters::note_descriptor_probe();
}

/// Record that `shape_install_shared` just confirmed (or established)
/// `SHAPE_LAYOUTS[shape_id] == Some(D)` for the descriptor these mask words
/// describe, together with the `POINTER_FREE`/`SIDE_MASK` choice that
/// descriptor's pointer mask selects.
#[inline]
pub(super) fn record(
    shape_id: u32,
    slot_count: usize,
    raw_words: *const u64,
    raw_word_count: u32,
    pointer_words: *const u64,
    pointer_word_count: u32,
    pointer_mask_empty: bool,
) {
    if shape_id == 0 {
        return;
    }
    let Some(dims) = pack_dims(
        slot_count,
        raw_word_count as usize,
        pointer_word_count as usize,
        pointer_mask_empty,
    ) else {
        return;
    };
    #[cfg(test)]
    counters::note_record();
    // SAFETY: as in `hit` — this thread's own storage, no escaping reference.
    unsafe {
        (*table().get())[slot_index(shape_id, raw_words, pointer_words)] = Entry {
            shape_id,
            raw_words,
            pointer_words,
            dims,
        };
    }
}

/// Drop every entry.
///
/// Called from the one transition that can falsify a memo: `SHAPE_LAYOUTS`
/// poisoning a shape to ambiguous. Clearing the whole table rather than the
/// one affected slot keeps this correct without having to reason about index
/// collisions, and costs one 1 KiB clear on a branch that fires when a
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
        static DESCRIPTOR_PROBES: Cell<u64> = const { Cell::new(0) };
    }

    #[inline]
    pub(in crate::gc) fn note_hit() {
        HITS.with(|c| c.set(c.get() + 1));
    }

    #[inline]
    pub(in crate::gc) fn note_record() {
        RECORDS.with(|c| c.set(c.get() + 1));
    }

    #[inline]
    pub(in crate::gc) fn note_descriptor_probe() {
        DESCRIPTOR_PROBES.with(|c| c.set(c.get() + 1));
    }

    /// `(hits, records)` since [`reset`].
    pub(in crate::gc) fn snapshot() -> (u64, u64) {
        (HITS.with(|c| c.get()), RECORDS.with(|c| c.get()))
    }

    pub(in crate::gc) fn descriptor_probes() -> u64 {
        DESCRIPTOR_PROBES.with(|c| c.get())
    }

    pub(in crate::gc) fn reset() {
        HITS.with(|c| c.set(0));
        RECORDS.with(|c| c.set(0));
        DESCRIPTOR_PROBES.with(|c| c.set(0));
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

    /// `record`/`hit` take raw `(pointer, word count)` pairs (#7578); these two
    /// keep the tests reading like the slice API they replaced.
    fn put(shape_id: u32, slot_count: usize, raw: &[u64], pointers: &[u64], empty: bool) {
        record(
            shape_id,
            slot_count,
            raw.as_ptr(),
            raw.len() as u32,
            pointers.as_ptr(),
            pointers.len() as u32,
            empty,
        );
    }
    fn get(shape_id: u32, slot_count: usize, raw: &[u64], pointers: &[u64]) -> Option<bool> {
        hit(
            shape_id,
            slot_count,
            raw.as_ptr(),
            raw.len() as u32,
            pointers.as_ptr(),
            pointers.len() as u32,
        )
    }

    /// Distinct shapes must not alias into one entry, and a shape whose
    /// dimensions do not fit the packed key must simply never memoise rather
    /// than collide with one that does.
    #[test]
    fn entries_discriminate_on_every_keyed_field() {
        let raw: [u64; 1] = [0b01];
        let other_raw: [u64; 1] = [0b10];
        let pointers: [u64; 1] = [0b10];
        let shape_id = crate::object::shapes::SHAPE_ID_BASE + 1;

        invalidate();
        put(shape_id, 2, &raw, &pointers, false);
        assert!(get(shape_id, 2, &raw, &pointers).is_some());
        assert!(
            get(shape_id + 1, 2, &raw, &pointers).is_none(),
            "a different shape"
        );
        assert!(
            get(shape_id, 3, &raw, &pointers).is_none(),
            "a different slot count"
        );
        assert!(
            get(shape_id, 2, &other_raw, &pointers).is_none(),
            "a different raw mask"
        );
        assert!(
            get(shape_id, 2, &raw, &raw).is_none(),
            "a different pointer mask"
        );
        assert!(
            get(shape_id, 2, &raw, &[]).is_none(),
            "a different pointer word count"
        );

        invalidate();
        assert!(
            get(shape_id, 2, &raw, &pointers).is_none(),
            "invalidate must drop entries"
        );
    }

    /// #7578: the memo now replays the `POINTER_FREE`/`SIDE_MASK` choice, so it
    /// has to carry that bit **per entry** and hand back the one that was
    /// recorded — not a default, and not the neighbouring shape's.
    ///
    /// The dangerous direction is a spurious `true`: `POINTER_FREE` makes
    /// `heap_payload_slot_selection` skip the payload without consulting any
    /// mask, so a shape with live pointer slots would have its children dropped.
    #[test]
    fn the_pointer_mask_empty_bit_round_trips_per_entry() {
        let raw: [u64; 1] = [0b01];
        let pointers: [u64; 1] = [0b10];
        let empty_pointers: [u64; 1] = [0];
        let shape_masked = crate::object::shapes::SHAPE_ID_BASE + 2;
        // The table is deliberately direct-mapped. Stack layout can make two
        // adjacent ShapeIds' complete keys select the same slot, in which case
        // the second `put` correctly evicts the first. Pick a distinct slot so
        // this test isolates the per-entry payload bit it is meant to pin.
        let masked_slot = slot_index(shape_masked, raw.as_ptr(), pointers.as_ptr());
        let shape_free = (shape_masked + 1..=shape_masked + MEMO_SLOTS as u32)
            .find(|&candidate| {
                slot_index(candidate, raw.as_ptr(), empty_pointers.as_ptr()) != masked_slot
            })
            .expect("a full index cycle must contain a distinct memo slot");

        invalidate();
        put(shape_masked, 2, &raw, &pointers, false);
        put(shape_free, 2, &raw, &empty_pointers, true);

        assert_eq!(
            get(shape_masked, 2, &raw, &pointers),
            Some(false),
            "a shape with a live pointer mask must replay SIDE_MASK"
        );
        assert_eq!(
            get(shape_free, 2, &raw, &empty_pointers),
            Some(true),
            "a shape with an empty pointer mask must replay POINTER_FREE"
        );

        // The bit is payload, not key: it must not make a matching tuple miss.
        invalidate();
        put(shape_free, 2, &raw, &empty_pointers, true);
        assert_eq!(get(shape_free, 2, &raw, &empty_pointers), Some(true));
    }

    /// A `slot_count` that overflows the packed key is refused by both halves,
    /// so it can neither be stored nor produce a spurious hit against a
    /// smaller shape that packs to the same bits.
    #[test]
    fn unpackable_dimensions_are_never_memoised() {
        let raw: [u64; 1] = [0b01];
        let shape_id = crate::object::shapes::SHAPE_ID_BASE + 4;
        let huge = 1usize << SLOT_COUNT_BITS;

        invalidate();
        put(shape_id, huge, &raw, &[], true);
        assert!(get(shape_id, huge, &raw, &[]).is_none());
        // …and it did not land in the slot a packable shape would use.
        assert!(get(shape_id, 0, &raw, &[]).is_none());
    }

    /// The word-count fields narrowed from 20 bits to 19 to make room for
    /// [`POINTER_MASK_EMPTY_BIT`] (#7578). Pin the packing so a future widening
    /// of any field cannot silently overlap that bit: an overlap would make a
    /// wide-mask shape read back as `POINTER_FREE`, and the collector would
    /// then skip payload slots that hold live pointers.
    #[test]
    fn packed_dims_fields_do_not_overlap_the_empty_bit() {
        let max_slots = (1usize << SLOT_COUNT_BITS) - 1;
        let max_words = (1usize << WORD_COUNT_BITS) - 1;
        let packed = pack_dims(max_slots, max_words, max_words, false)
            .expect("the maximum packable tuple must pack");
        assert_eq!(
            packed & POINTER_MASK_EMPTY_BIT,
            0,
            "a maximal dimension triple must leave the empty bit clear"
        );
        assert_eq!(
            pack_dims(max_slots, max_words, max_words, true),
            Some(packed | POINTER_MASK_EMPTY_BIT),
            "setting the empty bit must be the only difference"
        );
        assert!(
            pack_dims(max_slots, 1 << WORD_COUNT_BITS, 0, false).is_none(),
            "an unpackable raw word count must be refused, not truncated"
        );
        assert!(
            pack_dims(max_slots, 0, 1 << WORD_COUNT_BITS, false).is_none(),
            "an unpackable pointer word count must be refused, not truncated"
        );
    }
}
