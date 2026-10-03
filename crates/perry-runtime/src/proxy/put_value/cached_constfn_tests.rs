//! Raw emitted stores may cache other slots of a completed ConstFn shape,
//! but a write to its SPECIAL slot must retain the checked store funnel.
use super::*;
use crate::object::shapes::{object_shape_stamp, shape_descriptor_by_id, ShapeObjectKind};

extern "C" fn body_a(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    11.0
}

extern "C" fn body_b(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    22.0
}

fn key(name: &[u8]) -> *const crate::StringHeader {
    let hash = name.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    let s = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::string::js_string_intern(s, hash)
}

fn birth() -> f64 {
    // The plain-record birth entry with spare inline slots lets the fixture
    // carry Any, ConstFn and F64 lanes together without forging a shape.
    let keys =
        unsafe { crate::object::static_shapes::canonical_keys_for_names(&[b"cached_cf_seed"]) };
    let obj = crate::object::alloc_plain::alloc_plain_record_with_keys(4, keys);
    crate::object::js_object_set_field_by_name(obj, key(b"cached_cf_seed"), 1.0);
    let value = crate::value::js_nanbox_pointer(obj as i64);
    let d = shape_descriptor_by_id(stamp(value)).expect("plain birth descriptor");
    assert_eq!(d.object_kind, ShapeObjectKind::Ordinary);
    assert_eq!(d.logical_key_count, 1);
    assert!(d.live_inline_slot_count >= 4);
    assert_eq!(d.rep, crate::object::field_rep::REP_ANY);
    value
}

fn object(value: f64) -> *mut crate::ObjectHeader {
    (value.to_bits() & POINTER_MASK) as *mut crate::ObjectHeader
}

fn stamp(value: f64) -> u32 {
    unsafe { object_shape_stamp(object(value)) }
}

fn closure(other: bool) -> f64 {
    let info = if other {
        crate::fn_info!(body_b, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE))
    } else {
        crate::fn_info!(body_a, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE))
    };
    crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(info, 0) as i64)
}

fn completed(method: *const crate::StringHeader, value: f64) -> f64 {
    let receiver = birth();
    crate::object::js_object_set_field_by_name(object(receiver), key(b"cached_cf_scalar"), 2.5);
    crate::object::js_object_set_field_by_name(object(receiver), method, value);
    let d = shape_descriptor_by_id(stamp(receiver)).expect("completed descriptor");
    assert_eq!(d.object_kind, ShapeObjectKind::Ordinary);
    assert_eq!(d.special_constfn_mask, 1 << 2, "fixture minted ConstFn");
    assert!(crate::object::field_rep_store::shape_slot_is_f64(
        stamp(receiver),
        1
    ));
    assert_eq!(d.constfn_infos().len(), 1);
    receiver
}

fn assert_stored(receiver: f64, method: *const crate::StringHeader, value: f64) {
    assert_eq!(
        crate::object::js_object_get_field_by_name_f64(object(receiver), method).to_bits(),
        value.to_bits(),
        "the slot keeps this receiver's current closure"
    );
}

#[test]
fn packed_set_refuses_special_but_keeps_other_slots_of_the_completed_shape() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_gc = crate::gc::GcSuppressScope::new();
    let method = key(b"cached_cf_packed_method");
    let a = closure(false);
    let b = closure(false);
    assert_ne!(a.to_bits(), b.to_bits(), "distinct current closures");
    let first = completed(method, a);
    let second = completed(method, b);
    let final_id = stamp(first);
    assert_eq!(stamp(second), final_id, "fresh receivers share final id");
    let site: &'static PackedSetSite = Box::leak(Box::new(PackedSetSite::empty()));
    let mut ways = packed_set_cache_empty();
    let mut slot: PackedSetWaysSlot = &mut ways;
    unsafe { prime_packed_set(first, method, &mut slot, &site.set) };
    assert_eq!(site.set.load(Ordering::Relaxed), PACKED_SET_EMPTY);
    assert!(ways[..PACKED_SET_WAYS]
        .iter()
        .all(|w| *w == PACKED_SET_EMPTY));

    let same = closure(false);
    js_put_value_set_packed_miss(first, method, same, 0, &mut slot, &site.set);
    assert_eq!(
        stamp(first),
        final_id,
        "generic same-body store preserves fact"
    );
    assert_stored(first, method, same);
    assert_eq!(site.set.load(Ordering::Relaxed), PACKED_SET_EMPTY);

    let scalar = key(b"cached_cf_scalar");
    crate::object::js_object_set_field_by_name(object(first), scalar, 2.5);
    let mixed = stamp(first);
    assert_eq!(
        shape_descriptor_by_id(mixed).unwrap().special_constfn_mask,
        1 << 2
    );
    unsafe { prime_packed_set(first, scalar, &mut slot, &site.set) };
    let word = site.set.load(Ordering::Relaxed);
    assert_eq!(word as u32, mixed, "F64 beside ConstFn still primes");
    assert_ne!(word & PACKED_SET_F64_SLOT, 0);
    unsafe { prime_packed_set(first, key(b"cached_cf_seed"), &mut slot, &site.set) };
    assert_eq!(site.set.load(Ordering::Relaxed) as u32, mixed);
    assert_eq!(site.set.load(Ordering::Relaxed) & PACKED_SET_F64_SLOT, 0);

    let replacement = closure(true);
    js_put_value_set_packed_miss(second, method, replacement, 0, &mut slot, &site.set);
    assert_ne!(
        stamp(second),
        final_id,
        "different body cannot retain stale final id"
    );
    assert_eq!(
        shape_descriptor_by_id(stamp(second))
            .unwrap()
            .special_constfn_mask,
        0
    );
    assert_stored(second, method, replacement);
}

#[test]
fn packed_add_refuses_special_successor_before_any_memo_publication() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_gc = crate::gc::GcSuppressScope::new();
    let method = key(b"cached_cf_append_method");
    let first = birth();
    let second = birth();
    let pre = stamp(first);
    assert_eq!(stamp(second), pre);
    let site: &'static PackedSetSite = Box::leak(Box::new(PackedSetSite::empty()));
    let mut slot: PackedSetWaysSlot = std::ptr::null_mut();
    let a = closure(false);
    js_put_value_set_packed_miss(first, method, a, 0, &mut slot, &site.set);
    let final_id = stamp(first);
    assert_eq!(
        shape_descriptor_by_id(final_id)
            .unwrap()
            .special_constfn_mask,
        1 << 1
    );
    assert_eq!(site.add_shapes.load(Ordering::Relaxed), PACKED_SET_EMPTY);
    assert_eq!(site.add_guard.load(Ordering::Relaxed), 0);
    assert_eq!(site.add_ways.load(Ordering::Relaxed), 0);
    assert_eq!(
        unsafe { super::super::packed_add::packed_add_try(site, second, a) },
        None
    );
    assert_eq!(stamp(second), pre, "declining add changes nothing");

    let b = closure(false);
    js_put_value_set_packed_miss(second, method, b, 0, &mut slot, &site.set);
    assert_eq!(
        stamp(second),
        final_id,
        "same-body generic append shares final id"
    );
    assert_stored(second, method, b);
    let replacement = closure(true);
    js_put_value_set_packed_miss(second, method, replacement, 0, &mut slot, &site.set);
    assert_ne!(stamp(second), final_id);
    assert_eq!(
        shape_descriptor_by_id(stamp(second))
            .unwrap()
            .special_constfn_mask,
        0
    );
    assert_stored(second, method, replacement);
}

#[test]
fn dynamic_own_slot_primer_refuses_special_and_preserves_mixed_slots() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_gc = crate::gc::GcSuppressScope::new();
    let method = key(b"cached_cf_dynamic_method");
    let first = completed(method, closure(false));
    let current = closure(false);
    let second = completed(method, current);
    let final_id = stamp(first);
    assert_eq!(stamp(second), final_id);
    let mut cache: super::super::WritePicCache = [0; super::super::WRITE_PIC_WORDS];
    let mut slot: super::super::WritePicCacheSlot = &mut cache;
    let store = |target,
                 key: *const crate::StringHeader,
                 value,
                 slot: *mut super::super::WritePicCacheSlot| {
        super::super::js_put_value_set_dyn_ic_miss(
            slot,
            target,
            f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits()),
            value,
            0,
        )
    };
    let same = closure(false);
    store(first, method, same, &mut slot);
    assert_eq!(stamp(first), final_id);
    assert_eq!(cache[0], 0, "SPECIAL must not publish an own-slot token");
    assert_stored(first, method, same);

    let scalar = key(b"cached_cf_scalar");
    let mixed = stamp(first);
    store(first, scalar, 4.5, &mut slot);
    assert_eq!(
        shape_descriptor_by_id(mixed).unwrap().special_constfn_mask,
        1 << 2
    );
    assert_eq!(
        cache[0] as u64,
        crate::object::shapes::PIC_ID_TOKEN_BIT | u64::from(mixed),
        "ordinary F64 slot beside ConstFn still primes"
    );
    store(first, key(b"cached_cf_seed"), 5.5, &mut slot);
    assert_eq!(stamp(first), mixed, "unrelated stores keep the method fact");
    assert_stored(first, method, same);

    let replacement = closure(true);
    store(second, method, replacement, &mut slot);
    assert_ne!(stamp(second), final_id);
    assert_eq!(
        shape_descriptor_by_id(stamp(second))
            .unwrap()
            .special_constfn_mask,
        0
    );
    assert_stored(second, method, replacement);
}
