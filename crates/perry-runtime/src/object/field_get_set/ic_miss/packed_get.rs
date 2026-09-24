//! Compact MRU for generated generic property reads. The extra eight-byte
//! site word contains only a ShapeId and slot, never a GC address. Publishing
//! the pair atomically prevents readers from mixing two priming operations.
use super::{ObjectHeader, PicCache, PicCacheSlot};
use std::sync::atomic::{AtomicU64, Ordering};

#[no_mangle]
pub extern "C" fn js_object_get_field_ic_miss(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    cache_slot: *mut PicCacheSlot,
) -> f64 {
    super::get_field_ic_miss_impl(obj, key, cache_slot, std::ptr::null())
}

#[no_mangle]
pub extern "C" fn js_object_get_field_ic_miss_packed(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    cache_slot: *mut PicCacheSlot,
    packed: *const AtomicU64,
) -> f64 {
    super::get_field_ic_miss_impl(obj, key, cache_slot, packed)
}

/// Called at the same two authoritative own-slot proofs as the full cache.
/// The pair comes from that lookup, never from separately loaded cache words.
/// `packed` is null or an aligned, live AtomicU64 site word.
pub(super) unsafe fn prime_get(
    cache: *mut PicCache,
    token: i64,
    slot: i64,
    packed: *const AtomicU64,
) {
    super::pic_prime_get(cache, token, slot);
    if packed.is_null() {
        return;
    }
    let stamp = token as u32;
    // Only an ORDINARY-band ShapeId may enter a site word: a dictionary
    // shape's id is outside the matchable band by construction
    // (`shapes::DICTIONARY_SHAPE_ID_BASE`), so no emitted compare can equal a
    // dictionary receiver's word.
    if !crate::object::shapes::is_site_matchable_token(token as u64)
        || !(0..=0x7fff_ffff).contains(&slot)
    {
        return;
    }
    // Low 32 bits: the exact ShapeId for an INLINE slot, or that ShapeId with
    // [`super::PACKED_SPILL_FLIP`] flipped into it for a SPILL-located one.
    // High 32: the slot or spill index, with no flag bit of its own.
    //
    // The flip is what took the overflow-bit test off the emitted hit path.
    // ShapeIds live in [0x8000_0000, 0xC000_0000), so flipping the top two
    // bits lands a spill entry in [0x4000_0000, 0x8000_0000) — the one u32
    // band that is neither a ShapeId nor any class id — and the emitted
    // compare refuses it without asking a question of its own.
    // `pic.token.miss` un-flips the bits and routes the read to the slow
    // entry, which decodes the same word.
    //
    // Relaxed suffices: this publishes a numeric layout fact, not an object.
    let raw = slot as u32;
    let (key32, index) = if raw & crate::proxy::IC_SLOT_OVERFLOW_BIT != 0 {
        (
            stamp ^ super::PACKED_SPILL_FLIP,
            raw & !crate::proxy::IC_SLOT_OVERFLOW_BIT,
        )
    } else {
        (stamp, raw)
    };
    (*packed).store((index as u64) << 32 | key32 as u64, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_pair_preserves_identity_overflow_and_empty_site() {
        let packed = AtomicU64::new(crate::object::field_get_set::ic_miss::PACKED_GET_EMPTY);
        let mut cache = [0; super::super::PIC_CACHE_WORDS];
        let bit = crate::object::shapes::PIC_ID_TOKEN_BIT;
        unsafe {
            prime_get(&mut cache, bit as i64, 0, &packed);
        }
        assert_eq!(
            packed.load(Ordering::Relaxed),
            crate::object::field_get_set::ic_miss::PACKED_GET_EMPTY
        );
        for stamp in [
            crate::object::shapes::SHAPE_ID_BASE,
            crate::object::shapes::DICTIONARY_SHAPE_ID_BASE - 1,
        ] {
            for slot in [0, 1, 1 << 30, 0x7fff_ffff] {
                unsafe {
                    prime_get(&mut cache, (bit | stamp as u64) as i64, slot, &packed);
                }
                let word = packed.load(Ordering::Relaxed);
                let raw = slot as u32;
                let spill = raw & crate::proxy::IC_SLOT_OVERFLOW_BIT != 0;
                let want_key32 = if spill {
                    stamp ^ crate::object::field_get_set::ic_miss::PACKED_SPILL_FLIP
                } else {
                    stamp
                };
                assert_eq!(word & 0xffff_ffff, want_key32 as u64);
                assert_eq!(
                    word >> 32,
                    (raw & !crate::proxy::IC_SLOT_OVERFLOW_BIT) as u64
                );
                // A spill entry must be UNMATCHABLE by the emitted hit path:
                // its low half has to sit outside the ShapeId range so the
                // plain compare declines it with no test of its own, and
                // outside every class-id range so no UNSTAMPED receiver can
                // match it either.
                if spill {
                    assert!(
                        (0x4000_0000..crate::object::shapes::SHAPE_ID_BASE)
                            .contains(&(word as u32)),
                        "a spill entry must land in the one u32 band that is \
                         neither a ShapeId nor any class id"
                    );
                    assert_eq!(
                        super::super::packed_get_decode(word),
                        Some((stamp, raw & !crate::proxy::IC_SLOT_OVERFLOW_BIT, true)),
                        "and the runtime must decode it back to the same pair"
                    );
                } else {
                    assert_eq!(
                        super::super::packed_get_decode(word),
                        Some((stamp, raw, false))
                    );
                }
                let before = word;
                unsafe {
                    prime_get(&mut cache, bit as i64, 0, &packed);
                }
                assert_eq!(packed.load(Ordering::Relaxed), before);
            }
        }
    }

    /// **S6: a dictionary receiver can never hit a site entry.** A dictionary
    /// shape describes no keys and its receiver keeps the id across in-place
    /// appends and deletes, so a `(ShapeId, slot)` memo of it would be a fact
    /// about no shape at all. The guarantee is a SHAPE fact: dictionary shapes
    /// mint in their own id band, and every site-word writer admits only the
    /// ordinary band — so no compare the emitted code makes (the compact word,
    /// the ways, the global read stub) can equal the receiver's word.
    ///
    /// Must-fail controls (both run by hand, both RED): make
    /// `shapes::is_site_matchable_shape_id` accept the whole ShapeId range, or
    /// make `alloc_shape_id_for_generation` mint dictionaries in the ordinary
    /// band — the compact word then holds the receiver's id and the simulated
    /// hit below matches it.
    #[test]
    fn a_dictionary_receiver_can_never_hit_a_site_entry() {
        use crate::object::shapes;
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_gc = crate::gc::GcSuppressScope::new();
        unsafe {
            let obj = crate::object::js_object_alloc(0, 0);
            for i in 0..6 {
                let name = format!("dict_site_{i}");
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                crate::object::js_object_set_field_by_name(obj, key, i as f64);
            }
            let ordinary_id = shapes::object_shape_stamp(obj);
            assert!(
                shapes::is_site_matchable_shape_id(ordinary_id),
                "test premise: an ordinary receiver's id IS matchable"
            );
            assert!(
                crate::object::dictionary::latch_object_to_dictionary(obj),
                "test premise: the receiver latches"
            );
            let id = shapes::object_shape_stamp(obj);
            assert!(
                shapes::is_shape_id(id),
                "every reader still sees a ShapeId: {id:#x}"
            );
            assert!(
                shapes::is_dictionary_shape_id(id) && !shapes::is_site_matchable_shape_id(id),
                "a dictionary shape must be minted in the unmatchable band: {id:#x}"
            );

            // Every writer is offered exactly what a prime would hand it: the
            // receiver's own token and a slot the key really occupies.
            let token = (shapes::PIC_ID_TOKEN_BIT | id as u64) as i64;
            let empty = crate::object::field_get_set::ic_miss::PACKED_GET_EMPTY;
            let packed = AtomicU64::new(empty);
            let mut cache = [0i64; super::super::PIC_CACHE_WORDS];
            prime_get(&mut cache, token, 1, &packed);
            let word = packed.load(Ordering::Relaxed);
            // The emitted hit: `+4` word == the compact word's low half.
            assert_ne!(
                word as u32,
                (*obj).parent_class_id,
                "the compact word must never hold a dictionary receiver's id"
            );
            assert_eq!(word, empty, "nothing may be published for it");
            // The emitted way compare: the receiver's token against each word.
            assert!(
                !cache.contains(&token),
                "no cache word may hold a dictionary token: {cache:x?}"
            );

            let key_bits = 0x6b_7965_6b; // any nonzero content bits
            crate::object::read_stub::read_stub_insert_raw_for_test(token as u64, key_bits, 1);
            assert_eq!(
                crate::object::read_stub::read_stub_probe_raw_for_test(token as u64, key_bits),
                None,
                "the global read stub must never answer for a dictionary shape"
            );
        }
    }
}
