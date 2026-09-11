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
    // Low 32 bits: exact nonzero ShapeId. High 32: slot, including the
    // overflow flag. Generated code rejects the zero-initialized word.
    // Relaxed suffices: this publishes a numeric layout fact, not an object.
    (*packed).store((slot as u64) << 32 | stamp as u64, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_pair_preserves_identity_overflow_and_empty_site() {
        let packed = AtomicU64::new(0);
        let mut cache = [0; super::super::PIC_CACHE_WORDS];
        let bit = crate::object::shapes::PIC_ID_TOKEN_BIT;
        unsafe {
            prime_get(&mut cache, bit as i64, 0, &packed);
        }
        assert_eq!(packed.load(Ordering::Relaxed), 0);
        for stamp in [
            crate::object::shapes::SHAPE_ID_BASE,
            crate::object::shapes::SHAPE_ID_END - 1,
        ] {
            for slot in [0, 1, 1 << 30, 0x7fff_ffff] {
                unsafe {
                    prime_get(&mut cache, (bit | stamp as u64) as i64, slot, &packed);
                }
                let word = packed.load(Ordering::Relaxed);
                assert_eq!(word & 0xffff_ffff, stamp as u64);
                assert_eq!(word >> 32, slot as u64);
                let before = word;
                unsafe {
                    prime_get(&mut cache, bit as i64, 0, &packed);
                }
                assert_eq!(packed.load(Ordering::Relaxed), before);
            }
        }
    }
}
