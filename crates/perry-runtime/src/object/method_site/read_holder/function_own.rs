//! Own function properties use the read site's existing holder words.
//!
//! A FunctionDictionary's outer shape states its cell kind, while its owned
//! bag's shape states the key and data slot. The holder is receiver-relative:
//! every hit loads THIS function's bag, checks its ShapeId, and loads the
//! recorded slot. No function or bag address is retained or rooted here.
//! Descriptor installation, deletion and shadowing move the bag's shape.
use super::*;

pub(super) const HOLDER_FUNCTION_BAG: u64 = 1 << 59;
const HOLDER_ALIAS_OWN: u64 = 1 << 58;

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
    if entry[HOLDER_KIND] as u64 & HOLDER_ALIAS_OWN != 0 {
        let bits = bag_slot_value(recv as usize, entry[HOLDER_KIND] as u32)?;
        return (bits != crate::value::TAG_UNDEFINED && bits != crate::value::TAG_HOLE)
            .then_some(bits);
    }
    // Only a proven closure's ShapeId can prime this entry. Its own child
    // edge is traced and rewritten by the ordinary closure GC layout.
    let bag = (*(recv as *const crate::closure::ClosureHeader)).props;
    if bag.is_null() || object_shape_stamp(bag) != entry[HOLDER_SHAPE] as u32 {
        return None;
    }
    bag_slot_value(bag as usize, entry[HOLDER_KIND] as u32)
}

// Ordinary receivers need only the shape-band test, without a query call.
#[inline(always)]
pub(super) unsafe fn answer(c: &PicCache, recv: *const ObjectHeader, token: i64) -> Option<u64> {
    if (token as u32) < crate::object::shapes::EXOTIC_SHAPE_ID_BASE {
        return None;
    }
    scan_answer(c, recv, token)
}

// Keep the bounded function-property scan out of the ordinary read front.
#[inline(never)]
unsafe fn scan_answer(c: &PicCache, recv: *const ObjectHeader, token: i64) -> Option<u64> {
    entry_answer(holder_words(c), recv, token)
        .or_else(|| class_read::function_bag_answer(c, recv, token))
}

// The read front returns through one exotic-receiver query. It need not
// preserve receiver/cache/token registers across a function query and then
// repeat the class query; both answers retain their existing shape proofs.
#[cold]
#[inline(never)]
pub(super) unsafe fn answer_or_class(c: &PicCache, recv: *const ObjectHeader, token: i64) -> f64 {
    // Match the read front's return ABI so this branch can return directly.
    f64::from_bits(
        scan_answer(c, recv, token).unwrap_or_else(|| class_read::leaf_bits(c, recv, token)),
    )
}

/// The native fallback cannot override a non-undefined ordinary own value.
/// Retain a receiver-relative slot in the existing holder words; value
/// overwrites are loaded each time and undefined declines to native dispatch.
pub(crate) unsafe fn prime_alias(
    recv: *const ObjectHeader,
    key: *const crate::StringHeader,
    cache_slot: *mut PicCacheSlot,
) -> Option<f64> {
    let name = crate::string::header_str_checked(key)?.as_bytes();
    let recv = ordinary_receiver(recv as usize)?;
    let shape = object_shape_descriptor(recv)?;
    if shape.object_kind != crate::object::shapes::ShapeObjectKind::OrdinaryNativeAlias {
        return None;
    }
    let keys = shape.keys as usize as *const crate::array::ArrayHeader;
    let own = crate::object::keys_find_slot_by_bytes_resolved(keys, shape.logical_key_count, name);
    if let Some(slot) = own {
        if crate::object::key_attrs::key_is_accessor_at(keys, slot) {
            return prime_alias_accessor(recv, key, cache_slot, name);
        }
        let slot = if slot < shape.live_inline_slot_count {
            slot
        } else {
            slot | HOLDER_SLOT_SPILL
        };
        let bits = bag_slot_value(recv as usize, slot)?;
        if bits == crate::value::TAG_HOLE {
            return None;
        }
        let cache = crate::object::field_get_set::pic_slot_resolve::<PicCache>(cache_slot);
        if cache.is_null() {
            return None;
        }
        let w = Walk {
            holder: 0,
            holder_shape: object_shape_stamp(recv),
            slot: Some(slot),
            hops: NO_HOPS,
            depth: 1,
            getter: 0,
        };
        publish(cache, recv, &w, false);
        (*cache)[HOLDER_KIND] |= (HOLDER_FUNCTION_BAG | HOLDER_ALIAS_OWN) as i64;
        return alias_accessor_result(recv, key, || Some(crate::value::JSValue::from_bits(bits)));
    }
    alias_accessor_result(recv, key, || class_read::prime(recv, key, cache_slot, name))
        .or_else(|| prime_alias_accessor(recv, key, cache_slot, name))
}

unsafe fn alias_accessor_result(
    recv: *const ObjectHeader,
    key: *const crate::StringHeader,
    invoke: impl FnOnce() -> Option<crate::value::JSValue>,
) -> Option<f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_raw_mut_ptr(recv as *mut ObjectHeader);
    let key = scope.root_string_ptr(key);
    let value = invoke()?;
    if value.bits() != crate::value::TAG_UNDEFINED {
        return Some(f64::from_bits(value.bits()));
    }
    // The getter has already run. Preserve native forwarding without calling
    // it again, and reload the receiver/key after that collecting call.
    let name = key.with_const_ptr::<crate::StringHeader, _>(|key| {
        crate::string::header_str_checked(key).map(str::to_owned)
    })?;
    let recv = receiver.with_mut_ptr(|recv: *mut ObjectHeader| recv);
    Some(
        crate::object::native_this_alias::alias_forward_property_read(
            crate::value::js_nanbox_pointer(recv as i64),
            &name,
        )
        .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED)),
    )
}

unsafe fn prime_alias_accessor(
    recv: *const ObjectHeader,
    key: *const crate::StringHeader,
    cache_slot: *mut PicCacheSlot,
    name: &[u8],
) -> Option<f64> {
    accessor_walk(recv, name)?;
    alias_accessor_result(recv, key, || prime_read_holder(recv, key, cache_slot))
}

/// A collecting alias read may retain an own undefined slot: the caller
/// forwards its answer after this ordinary slot has been proved. Leaf reads
/// still decline undefined through `entry_answer` above.
#[inline]
pub(super) unsafe fn alias_own_answer(
    entry: &HolderEntry,
    recv: *const ObjectHeader,
    token: i64,
) -> Option<u64> {
    if entry[HOLDER_RECV] != token
        || entry[HOLDER_KIND] as u64 & (HOLDER_FUNCTION_BAG | HOLDER_ALIAS_OWN)
            != HOLDER_FUNCTION_BAG | HOLDER_ALIAS_OWN
    {
        return None;
    }
    let bits = bag_slot_value(recv as usize, entry[HOLDER_KIND] as u32)?;
    (bits != crate::value::TAG_HOLE).then_some(bits)
}

pub(crate) unsafe fn try_alias_cached_accessor(
    recv: *const ObjectHeader,
    key: *const crate::StringHeader,
    cache_slot: *mut PicCacheSlot,
) -> Option<f64> {
    let cache = crate::object::field_get_set::pic_slot_peek::<PicCache>(cache_slot);
    if cache.is_null() || WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0 {
        return None;
    }
    let token = (u64::from(object_shape_stamp(recv)) | PIC_ID_TOKEN_BIT) as i64;
    let entry = holder_words(&*cache);
    if let Some(bits) = alias_own_answer(entry, recv, token) {
        return alias_accessor_result(recv, key, || Some(crate::value::JSValue::from_bits(bits)));
    }
    if let Some(value) = alias_accessor_result(recv, key, || class_read::try_hit(recv, cache_slot))
    {
        return Some(value);
    }
    if (*cache)[HOLDER_KIND] as u64 & HOLDER_ACCESSOR == 0 {
        return None;
    }
    alias_accessor_result(recv, key, || try_cached_accessor(recv, cache_slot))
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
    fn native_alias_own_slots_are_receiver_relative_and_decline_undefined() {
        if !super::super::super::run_with_fresh_worker_gate(
            "native_alias_own_slots_are_receiver_relative_and_decline_undefined",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            let key = crate::string::canonical_key(b"alias_v");
            let a = crate::object::js_object_alloc(0xC000_2001, 1);
            let b = crate::object::js_object_alloc(0xC000_2001, 1);
            for (object, value) in [(a, 7.0f64), (b, 11.0)] {
                crate::object::native_this_alias::register_this_to_handle_alias(
                    crate::value::js_nanbox_pointer(object as i64),
                    crate::value::js_nanbox_pointer(42),
                    false,
                );
                crate::object::js_object_set_field_by_name(
                    object,
                    key,
                    f64::from_bits(crate::value::TAG_TRUE),
                );
                crate::object::js_object_set_field_by_name(object, key, value);
            }
            assert_eq!(object_shape_stamp(a), object_shape_stamp(b));
            let mut cache: PicCache = [0; crate::object::PIC_CACHE_WORDS];
            cache[HOLDER_STATE] = STATE_REGISTERED;
            let mut slot = &mut cache as *mut PicCache;
            assert_eq!(prime_alias(a, key, &mut slot), Some(7.0));
            let token = (u64::from(object_shape_stamp(a)) | PIC_ID_TOKEN_BIT) as i64;
            assert_eq!(
                super::super::primary_entry_answer(&cache, token, b),
                Some(11.0f64.to_bits())
            );
            let index = cache[HOLDER_KIND] as u32;
            crate::object::js_object_set_field(
                a,
                index,
                crate::value::JSValue::from_bits(crate::value::TAG_UNDEFINED),
            );
            assert_eq!(
                object_shape_stamp(a),
                token as u32,
                "an Any-slot overwrite retains the receiver shape"
            );
            assert_eq!(
                super::super::primary_entry_answer(&cache, token, a),
                None,
                "undefined must retain native fallback"
            );
            assert_eq!(
                super::super::primary_entry_answer(&cache, token, b),
                Some(11.0f64.to_bits())
            );
            assert_eq!(
                try_alias_cached_accessor(a, key, &mut slot).map(f64::to_bits),
                Some(crate::value::TAG_UNDEFINED),
                "the collecting hit forwards without re-resolving the ordinary slot"
            );
            cache[HOLDER_RECV] = 0;
            cache[HOLDER_KIND] = 0;
            assert_eq!(
                prime_alias(a, key, &mut slot).map(f64::to_bits),
                Some(crate::value::TAG_UNDEFINED)
            );
            assert_ne!(
                cache[HOLDER_KIND] as u64 & HOLDER_ALIAS_OWN,
                0,
                "a slot initially holding undefined still primes its ordinary answer"
            );
            assert_eq!(crate::object::js_object_delete_field(b, key), 1);
            let token = (u64::from(object_shape_stamp(b)) | PIC_ID_TOKEN_BIT) as i64;
            assert_eq!(super::super::primary_entry_answer(&cache, token, b), None);
            crate::object::proto_validity::mark_exotic_read_receiver(a as usize);
            assert!(
                prime_alias(a, key, &mut slot).is_none(),
                "a separate exotic contract still refuses ordinary admission"
            );
        }
    }

    #[test]
    fn saved_alias_undefined_slots_keep_their_collecting_answer() {
        if !super::super::super::run_with_fresh_worker_gate(
            "saved_alias_undefined_slots_keep_their_collecting_answer",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            let key = crate::string::canonical_key(b"saved_alias_v");
            let extra = crate::string::canonical_key(b"saved_alias_extra");
            let a = crate::object::js_object_alloc(0xC000_2003, 1);
            let b = crate::object::js_object_alloc(0xC000_2003, 1);
            for object in [a, b] {
                crate::object::native_this_alias::register_this_to_handle_alias(
                    crate::value::js_nanbox_pointer(object as i64),
                    crate::value::js_nanbox_pointer(42),
                    false,
                );
                crate::object::js_object_set_field_by_name(
                    object,
                    key,
                    f64::from_bits(crate::value::TAG_TRUE),
                );
            }
            crate::object::js_object_set_field_by_name(
                a,
                key,
                f64::from_bits(crate::value::TAG_UNDEFINED),
            );
            crate::object::js_object_set_field_by_name(b, key, 11.0);
            crate::object::js_object_set_field_by_name(
                b,
                extra,
                f64::from_bits(crate::value::TAG_TRUE),
            );
            assert_ne!(object_shape_stamp(a), object_shape_stamp(b));
            let mut cache: PicCache = [0; crate::object::PIC_CACHE_WORDS];
            cache[HOLDER_STATE] = STATE_REGISTERED;
            let mut slot = &mut cache as *mut PicCache;
            assert_eq!(
                prime_alias(a, key, &mut slot).map(f64::to_bits),
                Some(crate::value::TAG_UNDEFINED)
            );
            assert_eq!(prime_alias(b, key, &mut slot), Some(11.0));
            for _ in 0..8 {
                let token = (u64::from(object_shape_stamp(a)) | PIC_ID_TOKEN_BIT) as i64;
                assert_eq!(class_read::leaf_answer(&cache, a, token), None);
                assert_eq!(
                    try_alias_cached_accessor(a, key, &mut slot).map(f64::to_bits),
                    Some(crate::value::TAG_UNDEFINED),
                    "a saved own undefined slot must forward without priming again"
                );
                assert_eq!(try_alias_cached_accessor(b, key, &mut slot), Some(11.0));
            }
        }
    }

    #[test]
    fn global_stub_refuses_alias_publication_and_a_pre_alias_entry() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            let object = crate::object::js_object_alloc(0xC000_2002, 1);
            let key = crate::string::canonical_key(b"alias");
            crate::object::js_object_set_field_by_name(object, key, 7.0);
            let bits = crate::object::read_stub::read_stub_key_bits(key).unwrap();
            crate::object::read_stub::read_stub_prime(object, bits, 0);
            assert_eq!(
                crate::object::read_stub::read_stub_lookup(object, bits),
                Some(7.0)
            );
            let before = object_shape_stamp(object);
            crate::object::native_this_alias::register_this_to_handle_alias(
                crate::value::js_nanbox_pointer(object as i64),
                crate::value::js_nanbox_pointer(42),
                false,
            );
            assert_ne!(object_shape_stamp(object), before);
            assert_eq!(
                crate::object::read_stub::read_stub_lookup(object, bits),
                None
            );
            crate::object::read_stub::read_stub_prime(object, bits, 0);
            assert_eq!(
                crate::object::read_stub::read_stub_lookup(object, bits),
                None
            );
        }
    }

    #[test]
    fn native_alias_absence_primes_but_keeps_collecting_forwarding() {
        if !super::super::super::run_with_fresh_worker_gate(
            "native_alias_absence_primes_but_keeps_collecting_forwarding",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        crate::object::builtin_prototype_value("Object");
        unsafe {
            let a = crate::object::js_object_alloc(0xC000_2002, 1);
            crate::object::native_this_alias::register_this_to_handle_alias(
                crate::value::js_nanbox_pointer(a as i64),
                crate::value::js_nanbox_pointer(42),
                false,
            );
            let holder = crate::object::js_object_create(f64::from_bits(crate::value::TAG_NULL));
            crate::object::prototype_chain::object_set_user_prototype(a as usize, holder.to_bits());
            let missing = crate::string::canonical_key(b"native_alias_absent_value");
            let mut cache: PicCache = [0; crate::object::PIC_CACHE_WORDS];
            cache[HOLDER_STATE] = STATE_REGISTERED;
            let mut slot = &mut cache as *mut PicCache;
            assert_eq!(
                prime_alias(a, missing, &mut slot).map(f64::to_bits),
                Some(crate::value::TAG_UNDEFINED)
            );
            assert!(
                class_read::has_site(&cache),
                "ordinary absence keeps its chain proof"
            );
            assert_eq!(
                class_read::leaf_answer(
                    &cache,
                    a,
                    (u64::from(object_shape_stamp(a)) | PIC_ID_TOKEN_BIT) as i64
                ),
                None,
                "the leaf must leave native forwarding to the collecting path"
            );
            assert_eq!(
                try_alias_cached_accessor(a, missing, &mut slot).map(f64::to_bits),
                Some(crate::value::TAG_UNDEFINED)
            );
            crate::object::js_object_set_field_by_name(a, missing, 17.0);
            assert_eq!(
                try_alias_cached_accessor(a, missing, &mut slot),
                None,
                "shadowing invalidates the absence proof"
            );
        }
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
