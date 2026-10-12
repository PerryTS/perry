//! Array and dictionary-function reads share the existing holder entry.
//! Tokens distinguish receiver layouts; their low word is the existing
//! property bag's ShapeId. No receiver or bag address is retained.
use super::*;

const ARRAY_TOKEN: u64 = PIC_ID_TOKEN_BIT | (1 << 33);
const FUNCTION_TOKEN: u64 = PIC_ID_TOKEN_BIT | (1 << 34);

/// Canonical array header facts prove no own named override or custom link.
/// Array.prototype's bag supplies both the token and the holder shape.
unsafe fn array_facts(recv: *const ObjectHeader) -> Option<(i64, *const ObjectHeader)> {
    let header = crate::value::addr_class::try_read_gc_header(recv as usize)?;
    if header.obj_type != crate::gc::GC_TYPE_ARRAY
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    // The header is the layout authority. A lazy JSON header or a growth
    // forwarding stub must take the collecting continuation, never the
    // general array resolver (which can materialize JSON). The live ordinary
    // array needs only its shape flags; no resolution or allocation is needed.
    let arr = recv.cast();
    if !crate::array::array_has_plain_shape_resolved(arr) {
        return None;
    }
    let proto = crate::array::array_prototype_addr_if_resolved();
    if proto == 0 {
        return None;
    }
    let proto = proto as *const crate::array::ArrayHeader;
    if crate::array::array_object_flags_resolved(proto) & crate::gc::GC_ARRAY_CUSTOM_PROTO != 0 {
        return None;
    }
    // The intrinsic cache heals forwarding before returning its Array head.
    // Reading its bag is a load, not another general array resolution.
    let bag = crate::array::array_property_bag_resolved(proto);
    if bag.is_null() {
        return None;
    }
    let stamp = object_shape_stamp(bag);
    crate::object::shapes::is_site_matchable_shape_id(stamp)
        .then_some(((ARRAY_TOKEN | u64::from(stamp)) as i64, bag))
}

#[inline(never)]
pub(crate) unsafe fn array_answer(c: &PicCache, recv: *const ObjectHeader) -> Option<u64> {
    if WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0 {
        return None;
    }
    let (token, _) = array_facts(recv)?;
    // These layout tokens name holder proofs only. Class receiver sets
    // retain u32 ShapeIds and must never truncate a layout discriminator.
    primary_entry_answer(c, token, recv).or_else(|| class_read::holder_answer(c, token))
}

/// Read starting ON an ordinary prototype, including its own slot. The
/// ordinary holder walk starts AFTER its receiver, so prepend this hop.
pub(super) unsafe fn on_prototype(proto: *const ObjectHeader, name: &[u8]) -> Option<Walk> {
    if !hop_admitted(proto as usize) {
        return None;
    }
    let shape = object_shape_descriptor(proto)?;
    if !shape.object_kind.is_ordinary_layout() {
        return None;
    }
    let stamp = object_shape_stamp(proto);
    let keys = shape.keys as usize as *const crate::array::ArrayHeader;
    if !keys.is_null() {
        if let Some(s) =
            crate::object::keys_find_slot_by_bytes_resolved(keys, shape.logical_key_count, name)
        {
            if crate::object::key_attrs::key_is_accessor_at(keys, s) {
                return None;
            }
            return Some(Walk {
                holder: proto as usize,
                holder_shape: stamp,
                slot: Some(holder_slot_word(
                    proto as usize,
                    s,
                    shape.live_inline_slot_count,
                )?),
                hops: NO_HOPS,
                depth: 1,
                getter: 0,
            });
        }
    }
    if proto as usize == crate::array::object_prototype_addr_if_resolved() {
        return Some(Walk {
            holder: proto as usize,
            holder_shape: stamp,
            slot: None,
            hops: NO_HOPS,
            depth: 1,
            getter: 0,
        });
    }
    let mut w = walk(proto, name, false)?;
    if w.depth >= HOLDER_MAX_DEPTH {
        return None;
    }
    w.hops.copy_within(0..HOLDER_MAX_DEPTH - 2, 1);
    w.hops[0] = (proto as usize, stamp);
    w.depth += 1;
    Some(w)
}

unsafe fn array_walk(bag: *const ObjectHeader, name: &[u8]) -> Option<Walk> {
    // The bag describes Array.prototype's own keys. Its null link is
    // internal storage; the actual implicit link is Object.prototype.
    let shape = object_shape_descriptor(bag)?;
    let keys = shape.keys as usize as *const crate::array::ArrayHeader;
    if !keys.is_null()
        && crate::object::keys_find_slot_by_bytes_resolved(keys, shape.logical_key_count, name)
            .is_some()
    {
        return on_prototype(bag, name);
    }
    let w = on_prototype(
        crate::array::object_prototype_addr_if_resolved() as *const ObjectHeader,
        name,
    );
    let mut w = w?;
    w.hops.copy_within(0..HOLDER_MAX_DEPTH - 2, 1);
    w.hops[0] = (bag as usize, object_shape_stamp(bag));
    w.depth += 1;
    (w.depth <= HOLDER_MAX_DEPTH).then_some(w)
}

pub(crate) unsafe fn prime_array_read(
    recv: *const ObjectHeader,
    key: *const crate::StringHeader,
    cache_slot: *mut PicCacheSlot,
) -> Option<f64> {
    if cache_slot.is_null()
        || WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0
        || crate::agent::current_agent() != crate::agent::PRIMARY_AGENT
    {
        return None;
    }
    let name = crate::string::header_str_checked(key)?.as_bytes();
    if name != b"constructor" && !holder_name_admitted(name) || name == b"length" {
        return None;
    }
    // The first generic read materializes the intrinsic. Afterwards a
    // pre-walk refuses unsupported descriptors without running their getter.
    if let Some((_, bag)) = array_facts(recv) {
        array_walk(bag, name)?;
    } else if crate::array::array_prototype_addr_if_resolved() != 0 {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_raw_mut_ptr(recv as *mut ObjectHeader);
    let key_handle = scope.root_string_ptr(key);
    let (value, recv) = receiver.across_mut::<ObjectHeader, _>(|| {
        crate::object::field_get_set::get_field_by_name_after_site_miss(recv, key)
    });
    key_handle
        .with_const_ptr::<crate::StringHeader, _>(|key| {
            let name = crate::string::header_str_checked(key)?.as_bytes();
            let (token, bag) = array_facts(recv)?;
            let w = array_walk(bag, name)?;
            let confirmed = match w.slot {
                None => value.bits() == crate::value::TAG_UNDEFINED,
                Some(s) => {
                    holder_slot_value(w.holder, s) == Some(value.bits())
                        && value.bits() != crate::value::TAG_HOLE
                }
            };
            if confirmed && WORKER_AGENTS_EXIST.load(Ordering::SeqCst) == 0 {
                let cache = crate::object::field_get_set::pic_slot_resolve::<PicCache>(cache_slot);
                if !cache.is_null() {
                    publish_token(cache, recv, &w, false, token);
                }
            }
            Some(f64::from_bits(value.bits()))
        })
        .or(Some(f64::from_bits(value.bits())))
}

/// A dictionary function's bag proves own absence, including descriptor
/// keys. Its internal state edge must be empty: recorded prototypes and
/// intrinsic deletion markers otherwise require the generic read.
pub(super) unsafe fn function_token(closure: usize) -> Option<i64> {
    let c = closure as *const crate::closure::ClosureHeader;
    // Fixed at agent creation: reading the intrinsic id never mints metadata.
    let dictionary = crate::closure::shape::function_dictionary_shape();
    if (*c).shape_id != dictionary
        || (*c).info.is_null()
        || (*(*c).info).code == crate::closure::BOUND_METHOD_FUNC_PTR
        || crate::closure::shape::FunctionProtoKind::of_body(&*(*c).info)
            != crate::closure::shape::FunctionProtoKind::Function
    {
        return None;
    }
    let bag = (*c).props;
    if bag.is_null() || !(*bag).meta.is_null() && (*(*bag).meta).expando != 0 {
        return None;
    }
    let stamp = object_shape_stamp(bag);
    crate::object::shapes::is_site_matchable_shape_id(stamp)
        .then_some((FUNCTION_TOKEN | u64::from(stamp)) as i64)
}

pub(super) unsafe fn function_inherits(closure: usize, name: &[u8]) -> bool {
    let c = closure as *const crate::closure::ClosureHeader;
    // Native-module bound exports may synthesize properties outside their
    // bag; their body declares that exotic read contract.
    if !(*c).info.is_null() && (*(*c).info).code == crate::closure::BOUND_METHOD_FUNC_PTR {
        return false;
    }
    if crate::closure::shape::closure_on_base_shape(c) {
        return crate::closure::shape::function_shape_inherits_from_function_prototype(
            (*c).shape_id,
            name,
        );
    }
    if function_token(closure).is_none() {
        return false;
    }
    let bag = (*c).props;
    let shape = object_shape_descriptor(bag).unwrap();
    crate::object::keys_find_slot_by_bytes_resolved(
        shape.keys as usize as *const crate::array::ArrayHeader,
        shape.logical_key_count,
        name,
    )
    .is_none()
}

pub(super) unsafe fn function_answer(c: &PicCache, recv: *const ObjectHeader) -> Option<u64> {
    let token = function_token(recv as usize)?;
    primary_entry_answer(c, token, recv).or_else(|| class_read::holder_answer(c, token))
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn front(recv: *const ObjectHeader, slot: *mut PicCacheSlot) -> u64 {
        let packed = std::sync::atomic::AtomicU64::new(0xFFFF_FFFF);
        crate::object::js_object_get_field_ic_front(
            crate::object::shapes::ordinary_dir_addr(),
            (recv as i64).wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR as i64),
            0,
            slot,
            &packed,
        )
        .to_bits()
    }

    extern "C" fn body(
        _c: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
        0.0
    }

    #[test]
    fn array_read_entry_hits_and_rejects_shadowing_and_prototype_changes() {
        if !super::super::super::run_with_fresh_worker_gate(
            "array_read_entry_hits_and_rejects_shadowing_and_prototype_changes",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        crate::object::js_get_global_this();
        unsafe {
            let recv = crate::array::js_array_alloc(1);
            let key = crate::string::canonical_key(b"arrayic_absent");
            let mut cache: PicCache = [0; crate::object::PIC_CACHE_WORDS];
            cache[HOLDER_STATE] = STATE_REGISTERED;
            let mut slot = &mut cache as *mut PicCache;
            assert_eq!(
                prime_array_read(recv.cast(), key, &mut slot).map(f64::to_bits),
                Some(crate::value::TAG_UNDEFINED)
            );
            assert_eq!(
                front(recv.cast(), &mut slot),
                crate::value::TAG_UNDEFINED,
                "the leaf must hit, not merely fall back"
            );
            let function = crate::closure::js_closure_alloc(crate::fn_info!(body, 0), 0);
            assert_eq!(
                prime_function_read(function as usize, key, &mut slot).map(f64::to_bits),
                Some(crate::value::TAG_UNDEFINED)
            );
            assert_eq!(
                front(recv.cast(), &mut slot),
                crate::value::TAG_UNDEFINED,
                "an array proof survives in the shared saved-holder entries"
            );
            assert_eq!(front(function.cast(), &mut slot), crate::value::TAG_UNDEFINED);
            let proto = crate::array::array_prototype_addr() as *mut ObjectHeader;
            crate::object::js_object_set_field_by_name(proto, key, 17.0);
            assert_eq!(
                front(recv.cast(), &mut slot),
                crate::value::TAG_HOLE,
                "negative control: the old absent entry must miss"
            );
            assert_eq!(prime_array_read(recv.cast(), key, &mut slot), Some(17.0));
            assert_eq!(front(recv.cast(), &mut slot), 17.0f64.to_bits());
            let mut grown = recv;
            for value in 0..32 {
                grown = crate::array::js_array_push_f64(grown, value as f64);
            }
            assert_ne!(grown, recv, "the control must create a forwarding alias");
            assert_eq!(front(grown.cast(), &mut slot), 17.0f64.to_bits());
            assert_eq!(
                front(recv.cast(), &mut slot),
                crate::value::TAG_HOLE,
                "growth aliases decline to the collecting continuation"
            );
            assert_eq!(
                crate::object::js_object_get_field_ic_slow(
                    recv as i64,
                    key,
                    &mut slot,
                    &std::sync::atomic::AtomicU64::new(0xFFFF_FFFF),
                ),
                17.0,
                "the shared slow path resolves the growth alias"
            );
            crate::object::js_object_set_field_by_name(grown.cast(), key, 23.0);
            assert_eq!(
                front(grown.cast(), &mut slot),
                crate::value::TAG_HOLE,
                "own array shadowing must decline"
            );
            let fresh = crate::array::js_array_alloc(1);
            assert_eq!(front(fresh.cast(), &mut slot), 17.0f64.to_bits());
            let replacement = crate::object::js_object_alloc(0, 0);
            crate::object::js_object_set_prototype_of(
                crate::value::js_nanbox_pointer(proto as i64),
                crate::value::js_nanbox_pointer(replacement as i64),
            );
            assert_eq!(
                front(fresh.cast(), &mut slot),
                crate::value::TAG_HOLE,
                "Array.prototype link changes must decline"
            );
        }
    }

    #[test]
    fn function_absence_uses_bag_shape_and_refuses_recorded_links() {
        if !super::super::super::run_with_fresh_worker_gate(
            "function_absence_uses_bag_shape_and_refuses_recorded_links",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        crate::object::js_get_global_this();
        unsafe {
            let ctor = crate::object::js_get_global_this_builtin_value(b"Array".as_ptr(), 5);
            let recv = crate::value::js_nanbox_get_pointer(ctor) as *const ObjectHeader;
            let key = crate::string::canonical_key(b"isBuffer");
            let mut cache: PicCache = [0; crate::object::PIC_CACHE_WORDS];
            cache[HOLDER_STATE] = STATE_REGISTERED;
            let mut slot = &mut cache as *mut PicCache;
            assert_eq!(
                prime_function_read(recv as usize, key, &mut slot).map(f64::to_bits),
                Some(crate::value::TAG_UNDEFINED)
            );
            assert_eq!(front(recv, &mut slot), crate::value::TAG_UNDEFINED);
            crate::closure::closure_set_dynamic_prop(recv as usize, "isBuffer", 31.0);
            assert_eq!(
                front(recv, &mut slot),
                crate::value::TAG_HOLE,
                "negative control: a new own key invalidates absence"
            );
            assert_eq!(
                prime_function_read(recv as usize, key, &mut slot),
                Some(31.0)
            );
            assert_eq!(front(recv, &mut slot), 31.0f64.to_bits());
            let f = crate::closure::js_closure_alloc(crate::fn_info!(body, 0), 0);
            assert_eq!(
                prime_function_read(f as usize, key, &mut slot).map(f64::to_bits),
                Some(crate::value::TAG_UNDEFINED)
            );
            assert_eq!(front(f.cast(), &mut slot), crate::value::TAG_UNDEFINED);
            let fp = crate::array::function_prototype_addr() as *mut ObjectHeader;
            crate::object::js_object_set_field_by_name(fp, key, 41.0);
            assert_eq!(front(f.cast(), &mut slot), crate::value::TAG_HOLE);
            assert_eq!(prime_function_read(f as usize, key, &mut slot), Some(41.0));
            assert_eq!(front(f.cast(), &mut slot), 41.0f64.to_bits());
            crate::closure::closure_set_static_prototype(f as usize, crate::value::TAG_NULL);
            assert_eq!(front(f.cast(), &mut slot), crate::value::TAG_HOLE);
        }
    }
}
