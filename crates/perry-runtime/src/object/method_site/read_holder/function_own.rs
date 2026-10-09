//! Own function properties use the read site's existing holder words.
//!
//! A FunctionDictionary's outer shape states its cell kind, while its owned
//! bag's shape states the key and data slot. The holder is receiver-relative:
//! every hit loads THIS function's bag, checks its ShapeId, and loads the
//! recorded slot. No function or bag address is retained or rooted here.
//! Descriptor installation, deletion and shadowing move the bag's shape.
use super::*;

pub(super) const HOLDER_FUNCTION_BAG: u64 = 1 << 59;

#[inline]
unsafe fn bag_slot_value(bag: usize, slot: u32) -> Option<u64> {
    if slot & HOLDER_SLOT_SPILL == 0 {
        Some(slot_bits(bag, slot))
    } else {
        // An own function key containing undefined is present. Inherited
        // holder reads use spill_get_inline's older absent interpretation.
        crate::object::spill::spill_get_present(bag, (slot & !HOLDER_SLOT_SPILL) as usize)
    }
}

#[inline]
pub(super) fn same_answer(a: &HolderEntry, b: &HolderEntry) -> bool {
    a[HOLDER_RECV] == b[HOLDER_RECV]
        && (a[HOLDER_KIND] as u64 & HOLDER_FUNCTION_BAG == 0 || a[HOLDER_SHAPE] == b[HOLDER_SHAPE])
}

#[inline]
pub(super) unsafe fn entry_answer(
    entry: &HolderEntry,
    recv: *const ObjectHeader,
    token: i64,
) -> Option<u64> {
    if entry[HOLDER_RECV] != token || entry[HOLDER_KIND] as u64 & HOLDER_FUNCTION_BAG == 0 {
        return None;
    }
    // Only a proven closure's ShapeId can prime this entry. Its own child
    // edge is traced and rewritten by the ordinary closure GC layout.
    let bag = (*(recv as *const crate::closure::ClosureHeader)).props;
    if bag.is_null() || object_shape_stamp(bag) != entry[HOLDER_SHAPE] as u32 {
        return None;
    }
    bag_slot_value(bag as usize, entry[HOLDER_KIND] as u32)
}

#[inline]
pub(super) unsafe fn answer(c: &PicCache, recv: *const ObjectHeader, token: i64) -> Option<u64> {
    if (token as u32) < crate::object::shapes::EXOTIC_SHAPE_ID_BASE {
        return None;
    }
    entry_answer(holder_words(c), recv, token)
        .or_else(|| class_read::function_bag_answer(c, recv, token))
}

pub(super) unsafe fn prime(
    closure: usize,
    name: &[u8],
    cache_slot: *mut PicCacheSlot,
) -> Option<f64> {
    if super::super::name_refused(name) {
        return None;
    }
    let recv = closure as *const ObjectHeader;
    let token = (u64::from(object_shape_stamp(recv)) | PIC_ID_TOKEN_BIT) as i64;
    let existing = crate::object::field_get_set::pic_slot_peek::<PicCache>(cache_slot);
    if !existing.is_null() {
        if let Some(bits) = answer(&*existing, recv, token) {
            return Some(f64::from_bits(bits));
        }
    }
    let bag = (*(closure as *const crate::closure::ClosureHeader)).props;
    if bag.is_null() {
        return None;
    }
    let shape = object_shape_descriptor(bag)?;
    if !shape.object_kind.is_ordinary_layout() || shape.keys == 0 {
        return None;
    }
    let keys = shape.keys as usize as *const crate::array::ArrayHeader;
    let slot = crate::object::keys_lookup::keys_find_property_slot_by_bytes_resolved_hashed(
        keys,
        shape.logical_key_count,
        name,
        crate::object::key_bytes_hash(name.as_ptr(), name.len()),
    )?;
    if crate::object::key_attrs::key_is_accessor_at(keys, slot) {
        return None;
    }
    let slot = if slot < shape.live_inline_slot_count {
        slot
    } else if (slot as usize) < crate::object::spill::SPILL_MAX_FIELD_INDEX
        && crate::object::spill::object_spill_enabled()
    {
        slot | HOLDER_SLOT_SPILL
    } else {
        return None;
    };
    let bits = bag_slot_value(bag as usize, slot)?;
    if bits == crate::value::TAG_HOLE {
        return None;
    }
    let cache = crate::object::field_get_set::pic_slot_resolve::<PicCache>(cache_slot);
    if cache.is_null() {
        return None;
    }
    let bag_shape = object_shape_stamp(bag);
    if (*cache)[HOLDER_RECV] == token
        && (*cache)[HOLDER_KIND] as u64 & HOLDER_FUNCTION_BAG != 0
        && (*cache)[HOLDER_SHAPE] as u32 != bag_shape
    {
        class_read::retain_holder(cache);
    }
    let w = Walk {
        holder: 0,
        holder_shape: bag_shape,
        slot: Some(slot),
        hops: NO_HOPS,
        depth: 1,
        getter: 0,
    };
    publish(cache, recv, &w, false);
    (*cache)[HOLDER_KIND] |= HOLDER_FUNCTION_BAG as i64;
    Some(f64::from_bits(bits))
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn body(
        _closure: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
        0.0
    }

    #[test]
    fn own_function_bags_load_the_current_receiver_and_guard_every_shape() {
        if !super::super::super::run_with_fresh_worker_gate(
            "own_function_bags_load_the_current_receiver_and_guard_every_shape",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        let a = crate::closure::js_closure_alloc(crate::fn_info!(body, 0), 0);
        let b = crate::closure::js_closure_alloc(crate::fn_info!(body, 0), 0);
        crate::closure::closure_set_dynamic_prop(a as usize, "tag", 7.0);
        crate::closure::closure_set_dynamic_prop(b as usize, "tag", 11.0);
        let bag = unsafe { (*a).props };
        assert!(!bag.is_null(), "own data has physical bag storage");
        let descriptor = unsafe { object_shape_descriptor(bag) }.unwrap();
        assert!(descriptor.object_kind.is_ordinary_layout());
        assert_eq!(crate::agent::current_agent(), crate::agent::PRIMARY_AGENT);
        let key = crate::string::canonical_key(b"tag");
        let mut cache: PicCache = [0; crate::object::PIC_CACHE_WORDS];
        cache[HOLDER_STATE] = STATE_REGISTERED;
        let mut slot = &mut cache as *mut PicCache;
        assert_eq!(
            unsafe { prime_function_read(a as usize, key, &mut slot) },
            Some(7.0)
        );
        assert_ne!(
            cache[HOLDER_KIND] as u64 & HOLDER_FUNCTION_BAG,
            0,
            "a live own-bag entry"
        );
        assert_eq!(cache[HOLDER_OBJ], 0, "no retained function/bag pointer");
        let token = unsafe { u64::from((*a).shape_id) | PIC_ID_TOKEN_BIT } as i64;
        assert_eq!(unsafe { (*a).shape_id }, unsafe { (*b).shape_id });
        assert_eq!(
            unsafe { answer(&cache, a.cast(), token) },
            Some(7.0f64.to_bits())
        );
        assert_eq!(
            unsafe { answer(&cache, b.cast(), token) },
            Some(11.0f64.to_bits())
        );
        crate::closure::closure_set_dynamic_prop(a as usize, "tag", 13.0);
        assert_eq!(
            unsafe { answer(&cache, a.cast(), token) },
            Some(13.0f64.to_bits())
        );
        // Sabotage the saved bag-shape proof. The hit must be unable to
        // answer until the legitimate shape word is restored.
        let shape = cache[HOLDER_SHAPE];
        cache[HOLDER_SHAPE] ^= 1;
        assert_eq!(unsafe { answer(&cache, a.cast(), token) }, None);
        cache[HOLDER_SHAPE] = shape;
        assert_eq!(
            unsafe { answer(&cache, a.cast(), token) },
            Some(13.0f64.to_bits())
        );
        let bag = unsafe { (*a).props };
        assert_eq!(crate::object::js_object_delete_field(bag, key), 1);
        assert_eq!(
            unsafe { answer(&cache, a.cast(), token) },
            None,
            "a delete moves the bag"
        );
        assert_eq!(
            unsafe { answer(&cache, b.cast(), token) },
            Some(11.0f64.to_bits())
        );
    }
}
