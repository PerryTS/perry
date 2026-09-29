//! First-read D3: everything a generic read site's miss can answer WITHOUT
//! collecting, in one GC-leaf call.
//!
//! Emitted code keeps exactly one thing inline for a read: the receiver's
//! ShapeId compared against the site's compact word, and the slot load. A
//! ShapeId miss makes ONE call to [`js_object_get_field_ic_front`], which
//! answers, in this order and only from shape facts:
//!
//! 1. a polymorphic way (#7753): `(ShapeId token, slot)` pairs in the site's
//!    full cache;
//! 2. a SPILL entry: the compact word holds the receiver's ShapeId flipped by
//!    `PACKED_SPILL_FLIP`; the ShapeId fixes the key's index in the spill
//!    buffer (`packed_get::prime_get`, `spill_reserve_claimed`);
//! 3. a LATCHED megamorphic site (way state negative): its slot guess (the
//!    compact word's high half), confirmed by the receiver's own shape — the
//!    shape record's `POSBOUND` and its canonical key list compared with the
//!    key atom — and, on a wrong guess, one bounded scan of that key list that
//!    re-aims the guess (D3b).
//!
//! Anything else answers `TAG_HOLE`, and only then does the site branch to
//! its cold block and call the collecting `js_object_get_field_ic_slow` with
//! its usual operands (a never-primed site's inherited-read cache is asked
//! there). The front never allocates, collects, enters JS, throws, takes a
//! lock or makes a call — not even a thread-local access: the site passes the
//! agent's shape-directory mirror (`PERRY_AGENT_PTRS` slot 0, never null) as
//! an operand.
//! It is therefore `Leaf` in the generated call-effects table, the site's call
//! is a plain `"gc-leaf-function"` call, and nothing live across it is
//! spilled or relocated: statepoint spills exist only on the cold slow edge.

use super::ic_miss::{PACKED_GET_EMPTY, PACKED_SPILL_FLIP, PIC_WAYS, PIC_WAY_BASE, PIC_WAY_STATE};
use crate::object::shapes::{is_shape_id, positional_key_words, PIC_ID_TOKEN_BIT};
use crate::object::{ObjectHeader, PicCacheSlot};
use std::sync::atomic::{AtomicU64, Ordering};

/// D3b's second chance scans at most this many key positions.
const SECOND_CHANCE_POSITIONS: usize = 32;

#[inline(always)]
fn hole() -> f64 {
    f64::from_bits(crate::value::TAG_HOLE)
}

/// Inline slot `slot` of `obj`.
#[inline(always)]
unsafe fn inline_slot(obj: *const ObjectHeader, slot: usize) -> f64 {
    *((obj as *const u8).add(std::mem::size_of::<ObjectHeader>() + slot * 8) as *const f64)
}

/// A generic read site's ShapeId miss (module docs): the answer, or
/// `TAG_HOLE` for the site's collecting slow call.
///
/// * `dir` — this agent's shape-directory mirror
///   (`shapes::ordinary_dir_addr`, published in `PERRY_AGENT_PTRS` slot
///   `AGENT_PTR_SHAPE_DIR`), or `PERRY_EMPTY_SHAPE_DIR`, which confirms
///   nothing; never null. A `length` site passes the empty one on purpose: an
///   Array-subclass receiver serves `length` from its elements store, which no
///   key list names.
/// * `obj_biased` — the receiver's payload minus
///   `perry_abi::RECEIVER_HANDLE_FLOOR`: the value the site's fused receiver
///   test already holds on its pointer edge, so passing it costs the site a
///   register move where the payload cost a 10-byte constant and an add
///   (first-read D4: every way hit pays this edge). The front adds the floor
///   back in its load displacements. The site calls only on the ShapeId
///   compare's false edge, which its small-handle test dominates, so the
///   payload is a real object pointer (its `+4` word was just loaded).
/// * `key_bits` — the site's key exactly as its pool global holds it: the
///   interned key, STRING-tagged, which is also how a canonical key list
///   stores it, so the confirm compares one word.
/// * `cache_slot`, `packed` — the site's full-cache slot (its global, never
///   null) and compact word.
///
/// The ways are asked first (a polymorphic site's common miss), then the
/// spill entry, then a latched site's confirm: each answer is proven on its
/// own, so the order only decides who pays for which test.
///
/// # Safety
/// The operands as a generic read site passes them (above).
#[no_mangle]
pub unsafe extern "C" fn js_object_get_field_ic_front(
    dir: *const u8,
    obj_biased: i64,
    key_bits: u64,
    cache_slot: *mut PicCacheSlot,
    packed: *const AtomicU64,
) -> f64 {
    let obj =
        (obj_biased as usize).wrapping_add(perry_abi::RECEIVER_HANDLE_FLOOR) as *const ObjectHeader;
    let shape_id = (*obj).parent_class_id;
    // `pic_slot_peek` without its null test: the slot is the site's global.
    let cache = (*(cache_slot as *const std::sync::atomic::AtomicPtr<crate::object::PicCache>))
        .load(Ordering::Acquire);
    // A never-primed site (no cache) can still hold a spill entry in its
    // compact word; what else can serve it (the inherited-read cache) is
    // asked on the slow edge.
    let state = if cache.is_null() {
        0
    } else {
        (*cache)[PIC_WAY_STATE]
    };
    if state > 0 {
        // 1. The ways (first-read D4), in order, each hit loading its own
        // way's slot; the ShapeId is the one loaded above. A way token is
        // `PIC_ID_TOKEN_BIT | ShapeId`; an empty way is 0 and cannot match.
        // No spill re-test: a way never holds a spill or overflow entry
        // (`pic_prime_get` publishes a spill entry only to the compact word,
        // and refuses to cascade an overflow-encoded slot into a way).
        let token = (shape_id as u64 | PIC_ID_TOKEN_BIT) as i64;
        for w in 0..PIC_WAYS {
            if (*cache)[PIC_WAY_BASE + 2 * w] == token {
                return inline_slot(obj, (*cache)[PIC_WAY_BASE + 2 * w + 1] as usize);
            }
        }
    }
    let word = (*packed).load(Ordering::Relaxed);
    // 2. Spill. Equality with a real ShapeId proves the receiver is an
    // ordinary object of that shape (#10828 rule 3); the range test keeps an
    // unflipped non-id word (a zeroed word flips to the synthetic-class floor)
    // from matching an unstamped receiver. Nested, not `&&`: the common miss
    // leaves on the first compare.
    let spill_id = (word as u32) ^ PACKED_SPILL_FLIP;
    if shape_id == spill_id {
        if !is_shape_id(spill_id) {
            return hole();
        }
        let meta = (*obj).meta;
        let spill = (*meta).spill as usize as *const u8;
        let index = (word >> 32) as usize;
        return *(spill.add(std::mem::size_of::<crate::array::ArrayHeader>() + index * 8)
            as *const f64);
    }
    if state < 0 {
        // 3. Latched.
        return confirm_in(dir, obj, shape_id, packed, word, key_bits);
    }
    hole()
}

/// The latched site's confirm (module docs, 3.), for tests that drive it
/// with an explicit directory.
#[cfg(test)]
pub(crate) fn read_confirm(
    dir: *const u8,
    obj_handle: i64,
    shape_id: u32,
    packed: *const AtomicU64,
    key_bits: u64,
) -> f64 {
    unsafe {
        let word = (*packed).load(Ordering::Relaxed);
        confirm_in(
            dir,
            obj_handle as usize as *const ObjectHeader,
            shape_id,
            packed,
            word,
            key_bits,
        )
    }
}

#[inline(always)]
unsafe fn confirm_in(
    dir: *const u8,
    obj: *const ObjectHeader,
    shape_id: u32,
    packed: *const AtomicU64,
    word: u64,
    key_bits: u64,
) -> f64 {
    let Some((keys, bound)) = positional_key_words(dir, shape_id) else {
        return hole();
    };
    let guess = (word >> 32) as usize;
    // `guess < bound` proves POSBOUND nonzero before the keys are touched.
    let slot = if guess < bound && *keys.words().add(guess) == key_bits {
        guess
    } else {
        // D3b: the receiver's own key list, bounded; the found position is
        // published as the new guess unless the word holds a stamp.
        let scan = bound.min(SECOND_CHANCE_POSITIONS);
        if scan == 0 {
            return hole();
        }
        let words = keys.words();
        let Some(found) = (0..scan).find(|&i| *words.add(i) == key_bits) else {
            return hole();
        };
        if word as u32 == PACKED_GET_EMPTY as u32 {
            (*packed).store(
                ((found as u64) << 32) | (PACKED_GET_EMPTY as u32 as u64),
                Ordering::Relaxed,
            );
        }
        found
    };
    #[cfg(test)]
    crate::object::shapes::SHAPE_ANSWERED_READS.fetch_add(1, Ordering::Relaxed);
    inline_slot(obj, slot)
}

/// A generic read site's miss as emitted code performs it: the leaf front
/// with this agent's directory, then the slow entry on `TAG_HOLE`.
#[cfg(test)]
pub(crate) unsafe fn test_site_miss_read(
    obj_handle: i64,
    key: *const crate::StringHeader,
    cache_slot: *mut PicCacheSlot,
    packed: *const AtomicU64,
) -> f64 {
    let dir = if super::ic_slow::key_is_length(key) {
        std::ptr::addr_of!(crate::object::shapes::PERRY_EMPTY_SHAPE_DIR) as *const u8
    } else {
        crate::object::shapes::ordinary_dir_addr()
    };
    let key_bits = key as usize as u64 | crate::value::STRING_TAG;
    let obj_biased = obj_handle.wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR as i64);
    let v = js_object_get_field_ic_front(dir, obj_biased, key_bits, cache_slot, packed);
    if v.to_bits() != crate::value::TAG_HOLE {
        return v;
    }
    super::js_object_get_field_ic_slow(obj_handle, key, cache_slot, packed)
}

#[cfg(test)]
mod tests {
    use super::read_confirm as js_object_read_confirm;
    use crate::gc::RuntimeHandle;
    use crate::object::ObjectHeader;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn atom(text: &[u8]) -> *mut crate::StringHeader {
        let hash = crate::object::key_bytes_hash(text.as_ptr(), text.len());
        crate::string::js_string_pool_atom(text.as_ptr(), text.len() as u32, hash, 0)
    }

    /// The confirm's answer bits and the site word after the call.
    fn confirm(
        obj: &RuntimeHandle<'_>,
        key: &RuntimeHandle<'_>,
        word: u64,
        dir: *const u8,
    ) -> (u64, u64) {
        let packed = AtomicU64::new(word);
        let bits = obj.with_mut_ptr(|o: *mut ObjectHeader| {
            key.with_const_ptr(|k: *const crate::StringHeader| unsafe {
                js_object_read_confirm(
                    dir,
                    o as i64,
                    (*o).parent_class_id,
                    &packed,
                    crate::value::js_nanbox_string(k as i64).to_bits(),
                )
                .to_bits()
            })
        });
        (bits, packed.load(Ordering::Relaxed))
    }

    fn guess(slot: u64) -> u64 {
        (slot << 32) | 0xFFFF_FFFF
    }

    /// The confirm answers exactly one thing: the receiver's own inline slot at
    /// the guessed position, when the receiver's shape names the site's key
    /// atom there. Every other input declines with `TAG_HOLE` — including a
    /// key of the same TEXT that is not the atom (a pointer mismatch proves
    /// nothing, so the slow entry decides), a guess past the position bound,
    /// an id that names no ordinary record, and a null directory.
    #[test]
    fn the_confirm_answers_only_what_the_receivers_shape_names() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let a = scope.root_string_ptr(atom(b"d3_confirm_a"));
        let b = scope.root_string_ptr(atom(b"d3_confirm_b"));
        let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 8));
        for (k, v) in [(&a, 11.0), (&b, 22.0)] {
            obj.with_mut_ptr(|o| {
                k.with_const_ptr(|kp| crate::object::js_object_set_field_by_name(o, kp, v))
            });
        }
        let dir = crate::object::shapes::ordinary_dir_addr();
        let hole = crate::value::TAG_HOLE;
        let (v11, v22) = (11.0f64.to_bits(), 22.0f64.to_bits());
        // The guess, confirmed: answered, the word untouched.
        assert_eq!(confirm(&obj, &a, guess(0), dir), (v11, guess(0)), "a at 0");
        assert_eq!(confirm(&obj, &b, guess(1), dir), (v22, guess(1)), "b at 1");
        // A wrong guess, or one past the position bound: the second chance
        // finds the atom in the receiver's own key list, answers it, and
        // re-aims the guess.
        assert_eq!(
            confirm(&obj, &a, guess(1), dir),
            (v11, guess(0)),
            "a is at 0"
        );
        assert_eq!(
            confirm(&obj, &b, guess(0), dir),
            (v22, guess(1)),
            "b is at 1"
        );
        assert_eq!(
            confirm(&obj, &a, guess(9), dir),
            (v11, guess(0)),
            "past the bound"
        );
        // A word whose low half is a matchable stamp is never re-aimed.
        let stamped = (1u64 << 32) | 0x8000_0001;
        assert_eq!(
            confirm(&obj, &a, stamped, dir),
            (v11, stamped),
            "stamp kept"
        );
        // The empty directory confirms nothing: declined, the slow entry
        // decides.
        let empty = std::ptr::addr_of!(crate::object::shapes::PERRY_EMPTY_SHAPE_DIR) as *const u8;
        assert_eq!(
            confirm(&obj, &a, guess(0), empty),
            (hole, guess(0)),
            "empty dir"
        );
        let copy_text = b"d3_confirm_a";
        let copy = scope.root_string_ptr(crate::string::js_string_from_bytes(
            copy_text.as_ptr(),
            copy_text.len() as u32,
        ));
        assert_ne!(
            copy.with_const_ptr(|p: *const crate::StringHeader| p as usize),
            a.with_const_ptr(|p: *const crate::StringHeader| p as usize),
            "premise: the copy is another string object"
        );
        assert_eq!(
            confirm(&obj, &copy, guess(0), dir),
            (hole, guess(0)),
            "same text, not the atom: declined, the slow entry decides"
        );
        let unrecorded = a.with_const_ptr(|k: *const crate::StringHeader| {
            obj.with_mut_ptr(|o: *mut ObjectHeader| {
                js_object_read_confirm(
                    dir,
                    o as i64,
                    7,
                    &AtomicU64::new(0xFFFF_FFFF),
                    crate::value::js_nanbox_string(k as i64).to_bits(),
                )
                .to_bits()
            })
        });
        assert_eq!(unrecorded, hole, "an id below the ShapeId band");
    }

    /// The front as a site calls it: `(answer bits, word after)`.
    fn front(
        obj: *mut ObjectHeader,
        key_bits: u64,
        cache_slot: *mut crate::object::PicCacheSlot,
        word: u64,
    ) -> (u64, u64) {
        let packed = AtomicU64::new(word);
        let dir = crate::object::shapes::ordinary_dir_addr();
        let bits = unsafe {
            // The operand form a site passes (`obj_biased`, see the front).
            let biased = (obj as i64).wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR as i64);
            super::js_object_get_field_ic_front(dir, biased, key_bits, cache_slot, &packed)
                .to_bits()
        };
        (bits, packed.load(Ordering::Relaxed))
    }

    /// #7753 in the front: a polymorphic site's way — `(PIC_ID_TOKEN_BIT |
    /// ShapeId, slot)` in the full cache — is answered before anything else,
    /// from the receiver's own inline slot; a way naming another shape, an
    /// unprimed site (null cache: never dereferenced) and a primed site whose
    /// ways hold nothing for this shape all decline to the slow call.
    #[test]
    fn the_front_answers_a_way_and_declines_a_null_cache() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let a = scope.root_string_ptr(atom(b"d3_front_way_a"));
        let b = scope.root_string_ptr(atom(b"d3_front_way_b"));
        let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 8));
        for (k, v) in [(&a, 11.0), (&b, 22.0)] {
            obj.with_mut_ptr(|o| {
                k.with_const_ptr(|kp| crate::object::js_object_set_field_by_name(o, kp, v))
            });
        }
        let key_bits = b.with_const_ptr(|k: *const crate::StringHeader| {
            crate::value::js_nanbox_string(k as i64).to_bits()
        });
        let hole = crate::value::TAG_HOLE;
        obj.with_mut_ptr(|o: *mut ObjectHeader| {
            let shape_id = unsafe { (*o).parent_class_id };
            assert!(super::is_shape_id(shape_id), "premise: a shaped receiver");
            let mut cache: crate::object::PicCache = [0; crate::object::PIC_CACHE_WORDS];
            cache[super::PIC_WAY_STATE] = 1;
            cache[super::PIC_WAY_BASE + 2] = (shape_id as u64 | super::PIC_ID_TOKEN_BIT) as i64;
            cache[super::PIC_WAY_BASE + 3] = 1;
            let mut slot: crate::object::PicCacheSlot = &mut cache;
            assert_eq!(
                front(o, key_bits, &mut slot, 0xFFFF_FFFF),
                (22.0f64.to_bits(), 0xFFFF_FFFF),
                "the way names this shape: its slot answers"
            );
            cache[super::PIC_WAY_BASE + 2] =
                ((shape_id + 1) as u64 | super::PIC_ID_TOKEN_BIT) as i64;
            let mut slot: crate::object::PicCacheSlot = &mut cache;
            assert_eq!(
                front(o, key_bits, &mut slot, 0xFFFF_FFFF).0,
                hole,
                "a way for another shape"
            );
            let mut null_slot: crate::object::PicCacheSlot = std::ptr::null_mut();
            assert_eq!(
                front(o, key_bits, &mut null_slot, 0xFFFF_FFFF).0,
                hole,
                "never primed"
            );
        });
    }

    /// S5 in the front: a SPILL entry — the compact word holding the
    /// receiver's ShapeId flipped by `PACKED_SPILL_FLIP` — is served from the
    /// spill buffer at the word's index; and the un-flipped id must be a REAL
    /// ShapeId before it proves anything. A zeroed word un-flips to
    /// `PACKED_SPILL_FLIP` itself, the synthetic-class floor, which an
    /// unstamped receiver's `+4` word can hold: that receiver is not an
    /// ordinary shaped object and its spill buffer answers nothing.
    #[test]
    fn the_front_serves_a_spill_entry_only_for_a_real_shape_id() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let names: [&[u8]; 4] = [b"d3_sp_a", b"d3_sp_b", b"d3_sp_c", b"d3_sp_d"];
        let keys: Vec<_> = names
            .iter()
            .map(|n| scope.root_string_ptr(atom(n)))
            .collect();
        let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        for (i, k) in keys.iter().enumerate() {
            obj.with_mut_ptr(|o| {
                k.with_const_ptr(|kp| {
                    crate::object::js_object_set_field_by_name(o, kp, 10.0 + i as f64)
                })
            });
        }
        let last = &keys[3];
        let key_bits = last.with_const_ptr(|k: *const crate::StringHeader| {
            crate::value::js_nanbox_string(k as i64).to_bits()
        });
        let packed = AtomicU64::new(0xFFFF_FFFF);
        let mut slot: crate::object::PicCacheSlot = std::ptr::null_mut();
        let primed = obj.with_mut_ptr(|o: *mut ObjectHeader| {
            last.with_const_ptr(|k| {
                super::super::js_object_get_field_ic_slow(o as i64, k, &mut slot, &packed)
            })
        });
        assert_eq!(primed, 13.0, "the priming read");
        let word = packed.load(Ordering::Relaxed);
        obj.with_mut_ptr(|o: *mut ObjectHeader| {
            let shape_id = unsafe { (*o).parent_class_id };
            assert_eq!(
                (word as u32) ^ super::PACKED_SPILL_FLIP,
                shape_id,
                "premise: the site published a spill entry (word {word:#x})"
            );
            let mut none: crate::object::PicCacheSlot = std::ptr::null_mut();
            assert_eq!(
                front(o, key_bits, &mut none, word),
                (13.0f64.to_bits(), word),
                "a spill entry for this shape is served by the front"
            );
            // The same receiver under a `+4` word that is no ShapeId, read at a
            // site whose word's low half is 0 (it un-flips to exactly that
            // word) and whose index names the live spill value, so a front
            // that skipped the id check would answer it.
            assert!(!super::is_shape_id(super::PACKED_SPILL_FLIP), "premise");
            unsafe { (*o).parent_class_id = super::PACKED_SPILL_FLIP };
            let unstamped = front(o, key_bits, &mut none, word & !0xFFFF_FFFF).0;
            unsafe { (*o).parent_class_id = shape_id };
            assert_eq!(
                unstamped,
                crate::value::TAG_HOLE,
                "an un-flipped word that is no ShapeId proves nothing"
            );
        });
    }

    /// S6: a `length` site is never confirmed from the receiver's shape — its
    /// front is handed the EMPTY directory (codegen:
    /// `array_length::a_length_read_serves_a_live_plain_array_off_the_shape_compare`
    /// pins the emitted operand; `test_site_miss_read` mirrors it). An
    /// Array-subclass receiver serves `length` from its elements store, so a
    /// latched `length` site gains nothing from a key-list confirm; every such
    /// read goes to the slow entry, which answers `length` for every receiver
    /// kind. Pinned on the receiver where the confirm WOULD answer — a plain
    /// object with an own `length` data key — by the shape-answered counter:
    /// the read is right either way, so the value alone cannot see a `length`
    /// site that started confirming.
    #[test]
    fn a_length_site_is_never_confirmed_from_the_shape() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let len = scope.root_string_ptr(atom(b"length"));
        let other = scope.root_string_ptr(atom(b"d3_len_other"));
        let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 8));
        for (k, v) in [(&len, 3.0), (&other, 4.0)] {
            obj.with_mut_ptr(|o| {
                k.with_const_ptr(|kp| crate::object::js_object_set_field_by_name(o, kp, v))
            });
        }
        let answered = || crate::object::shapes::SHAPE_ANSWERED_READS.load(Ordering::Relaxed);
        let mut cache: crate::object::PicCache = [0; crate::object::PIC_CACHE_WORDS];
        cache[super::PIC_WAY_STATE] = -1_000_000;
        let mut read = |key: &RuntimeHandle<'_>, word: u64| {
            let mut slot: crate::object::PicCacheSlot = &mut cache;
            let packed = AtomicU64::new(word);
            obj.with_mut_ptr(|o: *mut ObjectHeader| {
                key.with_const_ptr(|k| unsafe {
                    super::test_site_miss_read(o as i64, k, &mut slot, &packed)
                })
            })
        };
        // Premise: the same latched site confirms an ordinary key from the
        // shape, so the counter can move.
        let before = answered();
        assert_eq!(read(&other, guess(1)), 4.0);
        assert_eq!(answered(), before + 1, "premise: a latched confirm counts");
        for word in [guess(0), guess(1), 0xFFFF_FFFF] {
            let before = answered();
            assert_eq!(read(&len, word), 3.0, "a latched `length` read ({word:#x})");
            assert_eq!(
                answered(),
                before,
                "a `length` read was confirmed from the shape ({word:#x})"
            );
        }
    }
}
