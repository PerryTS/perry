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
    if !(crate::object::shapes::SHAPE_ID_BASE..crate::object::shapes::SHAPE_ID_END).contains(&stamp)
        || token as u64 != (stamp as u64 | crate::object::shapes::PIC_ID_TOKEN_BIT)
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
            crate::object::shapes::SHAPE_ID_END - 1,
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
}
