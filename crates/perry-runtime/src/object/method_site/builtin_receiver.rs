//! Builtin receiver facts in the existing method-site memo. A canonical kind
//! or an Array's existing property-bag ShapeId proves absence of own overrides
//! and of a custom prototype. Legacy arrays recheck own-key absence per use
//! without migrating storage. The inherited holder still supplies its shape,
//! slot and declared function identity; no method-name classification occurs.
//!
//! These words have a nonzero low half and a reserved high half. Ordinary
//! receiver words have allocated ShapeIds below the reserved range; emitted exotic words have a zero
//! low half. Consequently an emitted ordinary/function hit cannot mistake a
//! builtin entry for its own layout. Builtin hits use the same memo's runtime
//! entrance, before the generic tower constructs its frame.
use super::*;

// Below the allocated ShapeId band: legacy named-property storage has no
// immutable key list, so this shape needs a key-absence read on every hit.
const LEGACY_ARRAY_SHAPE: u32 = 1;

/// Kind plus canonical receiver-local shape. Array growth can leave the
/// caller holding a forwarding alias; validate the live head, whose header
/// carries the receiver facts, just as the builtin body resolves its input.
pub(super) unsafe fn receiver_word(bits: u64) -> Option<u64> {
    receiver_facts(bits).map(|(word, _)| word)
}

#[inline(always)]
unsafe fn receiver_facts(bits: u64) -> Option<(u64, u64)> {
    if !miss_entry::is_site_receiver(f64::from_bits(bits)) {
        return None;
    }
    let mut addr = (bits & crate::value::POINTER_MASK) as usize;
    let mut h = crate::value::addr_class::try_read_gc_header(addr)?;
    if (*h).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        if (*h).obj_type != crate::gc::GC_TYPE_ARRAY {
            return None;
        }
        addr = crate::array::clean_arr_ptr(addr as *const crate::array::ArrayHeader) as usize;
        if addr == 0 {
            return None;
        }
        h = &*crate::gc::header_from_trusted_user_ptr(addr as *const u8);
    }
    let shape = match (*h).obj_type {
        crate::gc::GC_TYPE_ARRAY => {
            let array = addr as *const crate::array::ArrayHeader;
            if crate::array::array_has_plain_shape_resolved(array) {
                0
            } else {
                if (*h)._reserved & crate::gc::GC_ARRAY_CUSTOM_PROTO != 0 {
                    return None;
                }
                let bag = crate::array::array_property_bag(array);
                if bag.is_null() {
                    LEGACY_ARRAY_SHAPE
                } else {
                    let shape = super::super::shapes::object_shape_stamp(bag);
                    // Dictionaries do not carry an immutable key-absence proof.
                    if !(super::super::shapes::SHAPE_ID_BASE
                        ..super::super::shapes::DICTIONARY_SHAPE_ID_BASE)
                        .contains(&shape)
                    {
                        return None;
                    }
                    shape
                }
            }
        }
        crate::gc::GC_TYPE_MAP if (*(addr as *const crate::map::MapHeader)).meta.is_null() => 0,
        crate::gc::GC_TYPE_SET if (*(addr as *const crate::set::SetHeader)).meta.is_null() => 0,
        _ => return None,
    };
    Some((
        (u64::from(u32::MAX - u32::from((*h).obj_type)) << 32) | (u64::from(shape) + 1),
        crate::value::POINTER_TAG | addr as u64,
    ))
}

fn word_kind(word: u64) -> u8 {
    (u32::MAX - (word >> 32) as u32) as u8
}

unsafe fn builtin_holder(kind: u8) -> *const ObjectHeader {
    if kind == crate::gc::GC_TYPE_ARRAY {
        let mut proto = crate::array::array_prototype_addr();
        if proto == 0 {
            // The ordinary dispatcher can run array intrinsics before the
            // realm global exists. A holder proof needs the actual intrinsic
            // slot, so materialize it once on this suppressed cold prime.
            crate::object::js_get_global_this();
            proto = crate::array::array_prototype_addr();
        }
        if proto == 0 {
            return std::ptr::null();
        }
        return crate::array::array_property_bag(proto as *const crate::array::ArrayHeader);
    }
    let prototype = match kind {
        crate::gc::GC_TYPE_MAP => super::super::global_this::builtin_prototype_value("Map"),
        crate::gc::GC_TYPE_SET => super::super::global_this::builtin_prototype_value("Set"),
        _ => return std::ptr::null(),
    };
    crate::value::js_nanbox_get_pointer(prototype) as *const ObjectHeader
}

unsafe fn holder_bits(holder: *const ObjectHeader, index: u32, spill: bool) -> Option<u64> {
    if spill {
        spill_bits(holder, index)
    } else {
        Some(field_bits(holder as usize, index))
    }
}

pub(super) unsafe fn prime(slot: *mut MethodSiteSlot, recv: f64, name: &[u8]) {
    if slot.is_null() || name_refused(name) {
        return;
    }
    let Some((word, live)) = receiver_facts(recv.to_bits()) else {
        return;
    };
    if word as u32 == LEGACY_ARRAY_SHAPE + 1 && !legacy_lacks_name(live, name) {
        return;
    }
    if word as u32 > LEGACY_ARRAY_SHAPE + 1 {
        // The receiver's immutable bag shape must lack this site's key.
        let array = (live & crate::value::POINTER_MASK) as *const crate::array::ArrayHeader;
        let bag = crate::array::array_property_bag(array);
        let Some(shape) = super::super::shapes::object_shape_descriptor(bag) else {
            return;
        };
        if super::super::keys_find_slot_by_bytes_resolved(
            shape.keys as usize as *const crate::array::ArrayHeader,
            shape.logical_key_count,
            name,
        )
        .is_some()
        {
            return;
        }
    }
    let next = builtin_holder(word_kind(word));
    if next.is_null() || !address_is_prime_stable(next as usize) {
        return;
    }
    let Some(shape) = super::super::shapes::object_shape_descriptor(next) else {
        return;
    };
    if !shape.object_kind.is_ordinary_layout() {
        return;
    }
    let keys = shape.keys as usize as *const crate::array::ArrayHeader;
    let Some(index) =
        super::super::keys_find_slot_by_bytes_resolved(keys, shape.logical_key_count, name)
    else {
        return;
    };
    if super::super::key_attrs::key_is_accessor_at(keys, index) {
        return;
    }
    let spill = index
        >= shape
            .live_inline_slot_count
            .max(super::super::INLINE_SLOT_FLOOR as u32);
    if index >= shape.live_inline_slot_count && !spill {
        return;
    }
    let Some(value) = holder_bits(next, index, spill) else {
        return;
    };
    // Reuse the ordinary site's callable proof: it excludes bound bodies,
    // constructors, captured-this rebinding and bodiless builtin placeholders.
    let Some(info) = direct_callable(value, 8) else {
        return;
    };
    // Identity and ABI are declared by the installed body, never its key.
    if info.flags & crate::closure::FN_BUILTIN == 0
        || (native_args_tag(info) == 0 && info.params > 8)
    {
        return;
    }
    if publish(
        slot,
        MethodEntry {
            word,
            slot: METHOD_SITE_INHERITED
                | if spill { METHOD_SITE_SPILL } else { 0 }
                | u64::from(index)
                | native_args_tag(info),
            info: info as *const _ as u64,
            closure: next as usize,
            gen: std::ptr::read(next as *const u64),
            code: info.code as u64,
        },
    ) {
        PRIMES_INHERITED.fetch_add(1, Ordering::Relaxed);
        note_builtin_prime(info);
    }
}

/// Own data presence only: this reads storage, never invokes an accessor or
/// migrates it. The method body still declares its identity independently of
/// the property key. `live` is the Array head proved by `receiver_facts`.
unsafe fn legacy_lacks_name(live: u64, name: &[u8]) -> bool {
    let Ok(name) = std::str::from_utf8(name) else {
        return false;
    };
    crate::array::array_named_property_get_by_name(
        (live & crate::value::POINTER_MASK) as *const crate::array::ArrayHeader,
        name,
    )
    .is_none()
}

#[inline(always)]
pub(super) unsafe fn lookup_hit(
    slot: *mut MethodSiteSlot,
    bits: u64,
) -> Option<(u64, u64, u64, u64)> {
    if slot.is_null() || WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0 {
        return None;
    }
    let site = crate::object::pic_slot_peek(slot);
    if site.is_null() {
        return None;
    }
    let (word, live) = receiver_facts(bits)?;
    for e in &(*site).entries {
        if e.word != word {
            continue;
        }
        let holder = e.closure as *const ObjectHeader;
        if std::ptr::read(holder as *const u64) != e.gen {
            return None;
        }
        if word as u32 == LEGACY_ARRAY_SHAPE + 1 {
            // The holder shape pins this site's key as well as its slot.
            // Legacy receiver keys can change without a ShapeId transition.
            let keys = super::super::object_keys(holder);
            let index = (e.slot & METHOD_SITE_INDEX_MASK) as u32;
            if index >= keys.count() {
                return None;
            }
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let name = crate::string::js_string_key_bytes(keys.get(index), &mut scratch)?;
            if !legacy_lacks_name(live, name) {
                return None;
            }
        }
        let value = holder_bits(
            holder,
            (e.slot & METHOD_SITE_INDEX_MASK) as u32,
            e.slot & METHOD_SITE_SPILL != 0,
        )?;
        if !closure_of_body(value, e.info) {
            return None;
        }
        return Some((value, e.info, e.code, live));
    }
    None
}

/// Snapshot the selected method before effectful arguments run. Fixed-arity
/// bodies can enter the emitted direct call; native argument-list bodies use
/// the existing value-call bridge with this same snapshotted closure.
pub(super) unsafe fn lookup(
    slot: *mut MethodSiteSlot,
    recv: f64,
    method_id: i64,
    argc: usize,
    code_out: *mut u64,
) -> Option<f64> {
    if let Some((value, info, code, _)) = lookup_hit(slot, recv.to_bits()) {
        let info = &*(info as *const crate::closure::JsFunctionInfo);
        if native_args_tag(info) == 0
            && usize::from(info.params) <= crate::codegen_abi::method_site_padded_argc(argc)
        {
            *code_out = code;
        }
        return Some(f64::from_bits(value));
    }
    // On a canonical builtin receiver the lookup can prime the same holder
    // before arguments run. The returned closure snapshots that lookup;
    // native argument-list bodies use the existing value-call bridge.
    if let Some((word, live)) = receiver_facts(recv.to_bits()) {
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        if let Some(name) =
            crate::string::perry_string_ref_from_dispatch_id(method_id, &mut scratch)
        {
            let name = std::slice::from_raw_parts(name.ptr, name.len);
            let scope = crate::gc::RuntimeHandleScope::new();
            let recv_h = scope.root_nanbox_f64(recv);
            let accessor = {
                let _stable = crate::gc::GcSuppressScope::new();
                if WORKER_AGENTS_EXIST.load(Ordering::SeqCst) == 0 && !site_is_megamorphic(slot) {
                    prime(slot, recv, name);
                    if let Some((value, _, _, _)) = lookup_hit(slot, recv.to_bits()) {
                        return Some(f64::from_bits(value));
                    }
                }
                let next = builtin_holder(word_kind(word));
                super::super::descriptor_state::owner_key_is_accessor(
                    (live & crate::value::POINTER_MASK) as usize,
                    name,
                ) || (!next.is_null()
                    && super::super::key_attrs::object_key_is_accessor(next, name))
            };
            let value = spec_get(recv_h.get_nanbox_f64(), name);
            let bits = value.to_bits();
            let addr = (bits & crate::value::POINTER_MASK) as usize;
            let builtin = bits & !crate::value::POINTER_MASK == crate::value::POINTER_TAG
                && crate::closure::is_closure_ptr(addr)
                && crate::closure::closure_info(addr as *const crate::closure::ClosureHeader)
                    .is_some_and(|info| info.flags & crate::closure::FN_BUILTIN != 0);
            // An overridden slot is called as read. The old by-name tower
            // ignores such runtime prototype patches. Unsupported declared
            // builtin bodies keep its intrinsic algorithms.
            if accessor || !builtin {
                return Some(value);
            }
        }
    }
    None
}

pub(super) unsafe fn call_miss(
    slot: *mut MethodSiteSlot,
    recv: f64,
    method_id: i64,
    args: *const f64,
    argc: usize,
) -> Option<f64> {
    receiver_word(recv.to_bits())?;
    // A cold prototype bootstrap or an overridden getter can collect. Root
    // every operand before resolving the method, then refresh the arguments.
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv_h = scope.root_nanbox_f64(recv);
    let original = if argc != 0 && !args.is_null() {
        std::slice::from_raw_parts(args, argc).to_vec()
    } else {
        Vec::new()
    };
    let handles = scope.root_nanbox_f64_slice(&original);
    let mut code = 0;
    let value = lookup(slot, recv, method_id, argc, &mut code)?;
    let args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&handles);
    Some(js_method_site_call_value(
        value,
        recv_h.get_nanbox_f64(),
        args.as_ptr(),
        args.len(),
    ))
}

pub(super) unsafe fn call_hit(
    slot: *mut MethodSiteSlot,
    recv: f64,
    args: *const f64,
    argc: usize,
) -> Option<f64> {
    let (value, info_ptr, code, live) = lookup_hit(slot, recv.to_bits())?;
    let info = &*(info_ptr as *const crate::closure::JsFunctionInfo);
    // Only bodies that declare this normalization can receive the live head.
    // Callback-bearing methods keep the caller's original receiver value.
    let recv = if info.flags & perry_abi::FN_RESOLVES_ARRAY_THIS != 0 {
        f64::from_bits(live)
    } else {
        recv
    };
    invoke_body(value, info, code, recv, args, argc)
}

/// The lookup half already selected `value`, before the arguments ran.
/// Its slot or receiver may now have changed: only the selected closure's
/// body identity matters. Reuse an existing builtin entry's declared ABI,
/// without looking the method up again or normalizing the original receiver.
pub(super) unsafe fn call_selected(
    slot: *mut MethodSiteSlot,
    value: f64,
    recv: f64,
    args: *const f64,
    argc: usize,
) -> Option<f64> {
    if slot.is_null()
        || !miss_entry::is_site_receiver(recv)
        || WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0
    {
        return None;
    }
    let site = crate::object::pic_slot_peek(slot);
    if site.is_null() {
        return None;
    }
    for e in &(*site).entries {
        if e.word as u32 == 0
            || e.word >> 32 < u64::from(u32::MAX - u32::from(u8::MAX))
            || !matches!(
                word_kind(e.word),
                crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_MAP | crate::gc::GC_TYPE_SET
            )
            || !closure_of_body(value.to_bits(), e.info)
        {
            continue;
        }
        return invoke_body(
            value.to_bits(),
            &*(e.info as *const crate::closure::JsFunctionInfo),
            e.code,
            recv,
            args,
            argc,
        );
    }
    None
}

#[inline(always)]
unsafe fn invoke_body(
    value: u64,
    info: &crate::closure::JsFunctionInfo,
    code: u64,
    recv: f64,
    args: *const f64,
    argc: usize,
) -> Option<f64> {
    let closure = (value & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
    let this = crate::closure::JsThis::from_f64(recv);
    if native_args_tag(info) != 0 {
        let body: crate::codegen_abi::JsNativeArgsBody<crate::closure::ClosureHeader> =
            std::mem::transmute(code);
        return Some(body(closure, this, args, argc));
    }
    let arg = |i| {
        if i < argc {
            *args.add(i)
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
    };
    macro_rules! call {
            ($ty:ident $(, $i:expr)*) => {{
                let body: crate::codegen_abi::$ty<crate::closure::ClosureHeader> = std::mem::transmute(code);
                body(closure, this $(, arg($i))*)
            }};
        }
    return Some(match info.params {
        0 => call!(JsBody0),
        1 => call!(JsBody1, 0),
        2 => call!(JsBody2, 0, 1),
        3 => call!(JsBody3, 0, 1, 2),
        4 => call!(JsBody4, 0, 1, 2, 3),
        5 => call!(JsBody5, 0, 1, 2, 3, 4),
        6 => call!(JsBody6, 0, 1, 2, 3, 4, 5),
        7 => call!(JsBody7, 0, 1, 2, 3, 4, 5, 6),
        8 => call!(JsBody8, 0, 1, 2, 3, 4, 5, 6, 7),
        _ => return None,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn selected_method_getter(
        closure: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
        crate::closure::js_closure_get_capture_f64(closure, 0)
    }

    extern "C" fn replacement(
        _closure: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
        _arg: f64,
    ) -> f64 {
        901.0
    }

    #[test]
    fn builtin_site_primes_and_rejects_changed_receiver_and_holder() {
        if !run_with_fresh_worker_gate(
            "builtin_site_primes_and_rejects_changed_receiver_and_holder",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let _stable = crate::gc::GcSuppressScope::new();
            let array = crate::array::js_array_alloc(16);
            let recv = crate::value::js_nanbox_pointer(array as i64);
            let mut slot = std::ptr::null_mut();
            prime(&mut slot, recv, b"push");
            assert!(!slot.is_null(), "builtin site must actually prime");
            assert_eq!(call_hit(&mut slot, recv, [17.0].as_ptr(), 1), Some(1.0));
            let (selected, _, _, _) = lookup_hit(&mut slot, recv.to_bits()).unwrap();
            let push_holder = builtin_holder(crate::gc::GC_TYPE_ARRAY);
            let push_key = crate::string::canonical_key(b"push");
            crate::object::js_object_set_field_by_name(push_holder.cast_mut(), push_key, 123.0);
            assert_eq!(call_hit(&mut slot, recv, [18.0].as_ptr(), 1), None);
            assert_eq!(
                call_selected(
                    &mut slot,
                    f64::from_bits(selected),
                    recv,
                    [18.0].as_ptr(),
                    1
                ),
                Some(2.0),
                "a split call must consume its snapshotted body after a slot replacement"
            );
            crate::object::js_object_set_field_by_name(
                push_holder.cast_mut(),
                push_key,
                f64::from_bits(selected),
            );
            // Sites have one key; use another slot for the other method.
            let mut pop_slot = std::ptr::null_mut();
            prime(&mut pop_slot, recv, b"pop");
            assert_eq!(
                call_hit(&mut pop_slot, recv, std::ptr::null(), 0),
                Some(18.0)
            );
            assert_eq!(
                call_hit(&mut pop_slot, recv, std::ptr::null(), 0),
                Some(17.0)
            );
            assert_eq!(call_hit(&mut pop_slot, 7.0, std::ptr::null(), 0), None);
            assert_eq!(
                call_selected(&mut slot, f64::from_bits(selected), 7.0, [1.0].as_ptr(), 1),
                None,
                "a primitive or handle receiver must keep generic dispatch"
            );
            let header = crate::gc::header_from_trusted_user_ptr(array.cast()).cast_mut();
            let saved = (*header)._reserved;
            (*header)._reserved |= crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS;
            assert_eq!(call_hit(&mut pop_slot, recv, std::ptr::null(), 0), None);
            (*header)._reserved = saved | crate::gc::GC_ARRAY_CUSTOM_PROTO;
            assert_eq!(call_hit(&mut pop_slot, recv, std::ptr::null(), 0), None);
            (*header)._reserved = saved;
            let frozen = crate::array::js_array_alloc(4);
            let frozen = crate::array::js_array_push_f64(frozen, 1.0);
            let frozen = crate::array::js_array_push_f64(frozen, 2.0);
            let frozen_recv = crate::value::js_nanbox_pointer(frozen as i64);
            crate::object::js_object_freeze(frozen_recv);
            let thrown = crate::exception::catch_js_throw(|| {
                call_selected(
                    &mut slot,
                    f64::from_bits(selected),
                    frozen_recv,
                    [99.0].as_ptr(),
                    1,
                )
            })
            .expect_err("a frozen push must throw");
            let error =
                (thrown.to_bits() & crate::value::POINTER_MASK) as *mut crate::error::ErrorHeader;
            let message = crate::error::js_error_get_message(error);
            assert_eq!(
                crate::string::js_string_key_bytes(
                    crate::value::JSValue::string_ptr(message),
                    &mut [0u8; crate::value::SHORT_STRING_MAX_LEN]
                ),
                Some(&b"Cannot add property 2, object is not extensible"[..]),
                "push must preserve indexed Set before length Set"
            );
            for (count, expected) in [
                (
                    0,
                    "Cannot assign to read only property 'length' of object '[object Array]'",
                ),
                (2, "Cannot add property 2, object is not extensible"),
            ] {
                let thrown = crate::exception::catch_js_throw(|| {
                    call_selected(
                        &mut slot,
                        f64::from_bits(selected),
                        frozen_recv,
                        [99.0, 100.0].as_ptr(),
                        count,
                    )
                })
                .expect_err("a frozen push must throw for any argument count");
                let error = (thrown.to_bits() & crate::value::POINTER_MASK)
                    as *mut crate::error::ErrorHeader;
                let message = crate::error::js_error_get_message(error);
                assert_eq!(
                    crate::string::js_string_key_bytes(
                        crate::value::JSValue::string_ptr(message),
                        &mut [0u8; crate::value::SHORT_STRING_MAX_LEN]
                    ),
                    Some(expected.as_bytes())
                );
            }
            let entry = (*pop_slot)
                .entries
                .iter()
                .find(|e| e.word != METHOD_SITE_EMPTY)
                .unwrap();
            let holder = entry.closure as *mut ObjectHeader;
            let index = (entry.slot & METHOD_SITE_INDEX_MASK) as u32;
            let old = holder_bits(holder, index, entry.slot & METHOD_SITE_SPILL != 0).unwrap();
            let key = crate::string::canonical_key(b"pop");
            crate::object::js_object_set_field_by_name(holder, key, 123.0);
            assert_eq!(call_hit(&mut pop_slot, recv, std::ptr::null(), 0), None);
            let replacement = crate::closure::js_closure_alloc(crate::fn_info!(replacement, 1), 0);
            crate::object::js_object_set_field_by_name(
                holder,
                key,
                crate::value::js_nanbox_pointer(replacement as i64),
            );
            let pop_id = crate::value::js_nanbox_string(key as i64).to_bits() as i64;
            assert_eq!(
                call_miss(&mut pop_slot, recv, pop_id, std::ptr::null(), 0),
                Some(901.0),
                "a changed prototype must call its current method"
            );
            crate::object::js_object_set_field_by_name(holder, key, f64::from_bits(old));
            // A different key holding the same declared builtin body qualifies.
            let alias_key = crate::string::canonical_key(b"builtin_site_pop_alias");
            crate::object::js_object_set_field_by_name(holder, alias_key, f64::from_bits(old));
            let mut alias_slot = std::ptr::null_mut();
            prime(&mut alias_slot, recv, b"builtin_site_pop_alias");
            assert!(
                !alias_slot.is_null(),
                "identity must not depend on the method name"
            );
            assert_eq!(
                call_hit(&mut alias_slot, recv, std::ptr::null(), 0)
                    .unwrap()
                    .to_bits(),
                crate::value::TAG_UNDEFINED
            );
            let placeholder = crate::closure::js_closure_alloc(
                crate::fn_info!(super::super::super::global_this::global_this_builtin_noop_thunk, 1; with_flags(crate::closure::FN_BUILTIN)),
                0,
            );
            let placeholder_key = crate::string::canonical_key(b"builtin_site_placeholder");
            crate::object::js_object_set_field_by_name(
                holder,
                placeholder_key,
                crate::value::js_nanbox_pointer(placeholder as i64),
            );
            let mut placeholder_slot = std::ptr::null_mut();
            prime(&mut placeholder_slot, recv, b"builtin_site_placeholder");
            assert!(
                placeholder_slot.is_null(),
                "bodiless placeholder must refuse"
            );
            let growing = crate::array::js_array_alloc(0);
            let growing_recv = crate::value::js_nanbox_pointer(growing as i64);
            let mut growing_slot = std::ptr::null_mut();
            prime(&mut growing_slot, growing_recv, b"push");
            assert_eq!(
                call_hit(&mut growing_slot, growing_recv, [1.0].as_ptr(), 1),
                Some(1.0)
            );
            assert_eq!(
                call_hit(&mut growing_slot, growing_recv, [2.0].as_ptr(), 1),
                Some(2.0)
            );
            for _ in 0..(*growing).capacity {
                call_hit(&mut growing_slot, growing_recv, [42.0].as_ptr(), 1).unwrap();
            }
            // The site must resolve a forwarding alias once. A declared
            // push/pop body must consume that proven head, not repeat the
            // tracked forwarding walk on the caller's stale alias.
            let probes = crate::value::addr_class::tracked_header_probe_count_for_tests;
            let before = probes();
            let (_, live) = receiver_facts(growing_recv.to_bits()).unwrap();
            let proof_probes = probes() - before;
            assert_ne!(live, growing_recv.to_bits(), "fixture must have grown");
            assert!(proof_probes > 0, "fixture must exercise forwarding");
            let mut growing_pop_slot = std::ptr::null_mut();
            prime(&mut growing_pop_slot, growing_recv, b"pop");
            let before = probes();
            assert_eq!(
                call_hit(&mut growing_pop_slot, growing_recv, std::ptr::null(), 0),
                Some(42.0)
            );
            assert_eq!(
                probes() - before,
                proof_probes,
                "the native body must not repeat the site's forwarding proof"
            );
            crate::object::js_object_set_field_by_name(
                growing.cast(),
                crate::string::canonical_key(b"push"),
                42.0,
            );
            assert_eq!(
                call_hit(&mut growing_slot, growing_recv, [3.0].as_ptr(), 1),
                None,
                "forwarded alias must validate the live head's override fact"
            );

            let described = crate::array::js_array_alloc(8);
            let described_recv = crate::value::js_nanbox_pointer(described as i64);
            crate::object::js_object_set_field_by_name(
                described.cast(),
                crate::string::canonical_key(b"pos"),
                2.0,
            );
            assert!(crate::array::array_property_bag(described).is_null());
            let mut legacy_slot = std::ptr::null_mut();
            prime(&mut legacy_slot, described_recv, b"push");
            assert!(!legacy_slot.is_null(), "legacy own-key absence can prime");
            assert_eq!(
                call_hit(&mut legacy_slot, described_recv, [3.0].as_ptr(), 1),
                Some(1.0)
            );
            crate::object::js_object_set_field_by_name(
                described.cast(),
                crate::string::canonical_key(b"push"),
                crate::value::js_nanbox_pointer(replacement as i64),
            );
            assert_eq!(
                call_hit(&mut legacy_slot, described_recv, [4.0].as_ptr(), 1),
                None,
                "legacy storage must recheck this key after an own override"
            );
            crate::array::array_named_property_delete_by_name(described, "push");
            assert_eq!(
                call_hit(&mut legacy_slot, described_recv, [4.0].as_ptr(), 1),
                Some(2.0),
                "deleting the override restores the per-use absence proof"
            );
            // Priming must leave the indexed storage representation intact.
            assert!(crate::array::array_property_bag(described).is_null());
            crate::array::js_array_set_length(described, 0.0);
            // This is existing descriptor storage, not a site-induced migration.
            crate::array::array_property_bag_ensure(described);
            let mut described_slot = std::ptr::null_mut();
            prime(&mut described_slot, described_recv, b"push");
            assert!(!described_slot.is_null(), "unrelated own fields can prime");
            assert_eq!(
                call_hit(&mut described_slot, described_recv, [3.0].as_ptr(), 1),
                Some(1.0)
            );
            let mut includes_slot = std::ptr::null_mut();
            prime(&mut includes_slot, described_recv, b"includes");
            assert_eq!(
                call_hit(&mut includes_slot, described_recv, [3.0].as_ptr(), 1)
                    .unwrap()
                    .to_bits(),
                crate::value::TAG_TRUE,
                "native arguments must reach a search body without a rest array"
            );
            crate::object::js_object_set_field_by_name(
                described.cast(),
                crate::string::canonical_key(b"push"),
                crate::value::js_nanbox_pointer(replacement as i64),
            );
            assert_eq!(
                call_hit(&mut described_slot, described_recv, [4.0].as_ptr(), 1),
                None
            );
            let mut overridden_slot = std::ptr::null_mut();
            prime(&mut overridden_slot, described_recv, b"push");
            assert!(
                overridden_slot.is_null(),
                "a bag's own method must not prime"
            );

            let getter_array = crate::array::js_array_alloc(8);
            crate::object::js_object_set_field_by_name(
                getter_array.cast(),
                push_key,
                f64::from_bits(crate::value::TAG_UNDEFINED),
            );
            let getter =
                crate::closure::js_closure_alloc(crate::fn_info!(selected_method_getter, 0), 1);
            crate::closure::js_closure_set_capture_f64(getter, 0, f64::from_bits(selected));
            super::super::super::set_builtin_accessor_descriptor(
                getter_array as usize,
                "push".to_owned(),
                super::super::super::AccessorDescriptor {
                    get: crate::value::js_nanbox_pointer(getter as i64).to_bits(),
                    set: 0,
                },
                super::super::super::PropertyAttrs::new(true, false, true),
            );
            let mut getter_slot = std::ptr::null_mut();
            let push_id = crate::value::js_nanbox_string(push_key as i64).to_bits() as i64;
            assert_eq!(
                call_miss(
                    &mut getter_slot,
                    crate::value::js_nanbox_pointer(getter_array as i64),
                    push_id,
                    [7.0].as_ptr(),
                    1
                ),
                Some(1.0),
                "a getter returning a builtin must retain its one selected value"
            );

            let map = crate::map::js_map_alloc(4);
            let map_recv = crate::value::js_nanbox_pointer(map as i64);
            let mut map_slot = std::ptr::null_mut();
            prime(&mut map_slot, map_recv, b"get");
            assert!(!map_slot.is_null(), "Map get must actually prime");
            let missing =
                crate::value::js_nanbox_string(crate::string::canonical_key(b"missing") as i64);
            assert_eq!(
                call_hit(&mut map_slot, map_recv, [missing].as_ptr(), 1)
                    .unwrap()
                    .to_bits(),
                crate::value::TAG_UNDEFINED
            );
            let ordinary = crate::object::js_object_alloc(0, 4);
            assert_eq!(
                receiver_word(crate::value::js_nanbox_pointer(ordinary as i64).to_bits()),
                None
            );
        }
    }
}
