//! `Object.assign`: copy every own enumerable string- and symbol-keyed
//! property of a source onto a target through the ordinary `[[Set]]`, with
//! the spec's throw on a rejected set.
//!
//! Split out of `object/alloc.rs` (pure relocation). Shared state and helpers
//! remain in the parent `object` module and are reached via `use super::*;`.

use super::*;

/// `Object.assign(target, source)` for a single source: mutate `target` by
/// copying every own enumerable string-keyed AND symbol-keyed property from
/// `source`, returning `target`. Both args are NaN-boxed JSValues; the return
/// is `target` unchanged so the caller can chain successive sources and the
/// final returned value is the same pointer the user passed in (preserving
/// object identity, class_id, and the existing entries in the SYMBOL_PROPERTIES
/// side table — the bug from #590 was that the previous lowering allocated a
/// fresh object, breaking `result === target` and orphaning target's
/// symbol-keyed properties since the side table is keyed by raw pointer).
///
fn throw_object_assign_nullish_target() -> ! {
    let message = "Cannot convert undefined or null to object";
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[no_mangle]
pub unsafe extern "C" fn js_object_assign_validate_target(target_f64: f64) -> f64 {
    let target = JSValue::from_bits(target_f64.to_bits());
    if target.is_undefined() || target.is_null() {
        throw_object_assign_nullish_target();
    }
    js_object_coerce(target_f64)
}

unsafe fn object_assign_set_string_key(
    define: bool,
    target: *mut ObjectHeader,
    target_is_array: bool,
    key_ptr: *const crate::StringHeader,
    value_f64: f64,
) {
    // `Object.assign(process.env, parsed)` — how `@next/env` loads `.env` files.
    // `process.env.X` READS lower to `js_getenv` (the real environment), so a
    // field stored on the cached env object leaves every read `undefined`: a
    // Next.js standalone server saw NONE of its `.env` config (myairank's
    // `DATABASE_URL` vanished, mysql2 then connected with an empty user and the
    // MySQL handshake timed out). Route the write through the env setter so it
    // lands where the reads look.
    //
    // This hook lives at the single write funnel rather than as an early exit in
    // `js_object_assign_one`, so every source shape still flows through the
    // decoding below: a primitive/array/proxy source is enumerated correctly,
    // and a nullish source is skipped per spec instead of throwing.
    if !target_is_array && crate::process::is_process_env_ptr(target as usize) {
        crate::process::js_setenv(key_ptr, value_f64);
        return;
    }
    if !define {
        // Set(to, key, value, true) must walk inherited descriptors and invoke
        // setters with the target as receiver, including on non-extensible
        // targets. Use the same strict [[Set]] as property assignment.
        let target_value = crate::value::js_nanbox_pointer(target as i64);
        let key_value = f64::from_bits(JSValue::string_ptr(key_ptr as *mut _).bits());
        crate::proxy::js_put_value_set(target_value, key_value, value_f64, target_value, 1);
    } else if target_is_array {
        crate::array::js_array_set_string_key(
            target as *mut crate::array::ArrayHeader,
            key_ptr,
            value_f64,
        );
    } else {
        object_define_string_key(target, key_ptr, value_f64);
    }
}

/// CopyDataProperties' store: `CreateDataPropertyOrThrow(target, key, v)` on
/// the fresh object of a spread literal. The prototype chain is never asked;
/// the receiver shape answers the common case (`define_own_data`). When
/// nothing can intercept a write for the key, `[[Set]]` performs this exact
/// definition and is the path that teaches the lattice its key-add edge;
/// otherwise the general definition runs.
unsafe fn object_define_string_key(
    target: *mut ObjectHeader,
    key_ptr: *const crate::StringHeader,
    value_f64: f64,
) {
    let target_value = crate::value::js_nanbox_pointer(target as i64);
    let key_value = f64::from_bits(JSValue::string_ptr(key_ptr as *mut _).bits());
    if super::define_own_data::define_own_data_from_shape(target_value, key_value, value_f64)
        .is_some()
    {
        return;
    }
    if !super::descriptor_state::plain_data_write_may_intercept(target as usize, 0, key_value) {
        js_object_set_field_by_name(target, key_ptr, value_f64);
        return;
    }
    if !crate::proxy::create_data_property(target_value, key_value, value_f64) {
        crate::collection_iter::throw_type_error("Cannot define property on object literal");
    }
}

/// Copy a plain source whose SHAPE answers CopyDataProperties' questions:
/// its key list is the own string keys in order, its attribute summary proves
/// every one an enumerable data property (no accessor, so `[[Get]]` is the
/// slot and runs no code), and no per-object descriptor exists. Each value is
/// then read by its POSITION, and the key word the list holds is handed to the
/// store as is: no key is spelled, decoded or looked up by name on the source.
///
/// `false` (nothing copied) for anything else: a class instance or a
/// prototype object, a string wrapper, a dictionary, a URL view, an object
/// with descriptors or non-default attributes.
///
/// # Safety
/// `tgt_h` roots the target object and `src_h` the source object.
unsafe fn copy_positional_source(
    define: bool,
    strict_set_is_shape_equivalent: bool,
    plan: PositionalPlan,
    tgt_h: &crate::gc::RuntimeHandle<'_>,
    src_h: &crate::gc::RuntimeHandle<'_>,
    target_is_array: bool,
) -> bool {
    let (keys, key_count, hide_private) = match plan {
        PositionalPlan::Refused => return false,
        PositionalPlan::NoKeys => return true,
        PositionalPlan::Keys(keys, count, hide) => (keys, count, hide),
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let keys_h = scope.root_raw_mut_ptr(keys);
    for i in 0..key_count {
        // The previous store may have allocated: every address is re-derived
        // from its root, and the read allocates nothing.
        let Some((key_val, value)) = src_h.with_const_ptr::<ObjectHeader, _>(|src| {
            keys_h.with_const_ptr::<crate::array::ArrayHeader, _>(|keys| {
                positional_entry(src, keys, key_count, i, hide_private)
            })
        }) else {
            continue;
        };
        let iter_scope = crate::gc::RuntimeHandleScope::new();
        let val_h = iter_scope.root_nanbox_f64(value);
        let key_f64 = f64::from_bits(key_val.bits());
        // The definition roots its own operands before it can allocate.
        if define
            && !target_is_array
            && tgt_h.with_mut_ptr::<ObjectHeader, _>(|target| {
                super::define_own_data::define_own_data_from_shape(
                    crate::value::js_nanbox_pointer(target as i64),
                    key_f64,
                    val_h.get_nanbox_f64(),
                )
                .is_some()
            })
        {
            continue;
        }
        // The general store wants a heap key; an SSO key is materialized
        // (an allocation, so the value is read back through its root).
        let key_ptr =
            crate::value::js_get_string_pointer_unified(key_f64) as *const crate::StringHeader;
        if key_ptr.is_null() {
            continue;
        }
        let key_h = iter_scope.root_string_ptr(key_ptr);
        tgt_h.with_mut_ptr::<ObjectHeader, _>(|t| {
            key_h.with_const_ptr::<crate::StringHeader, _>(|k| {
                if strict_set_is_shape_equivalent {
                    // The all-keys preflight proved that strict [[Set]] is an
                    // own-data overwrite/add for every source key. Preserve
                    // the positional copy's direct shape-transition store.
                    js_object_set_field_by_name(t, k, val_h.get_nanbox_f64());
                } else {
                    object_assign_set_string_key(
                        define,
                        t,
                        target_is_array,
                        k,
                        val_h.get_nanbox_f64(),
                    );
                }
            })
        });
    }
    true
}

/// What [`positional_source_plan`] proves about a source.
#[derive(Clone, Copy)]
enum PositionalPlan {
    /// Not a source the shape answers for.
    Refused,
    /// A plain source with no keys: nothing to copy.
    NoKeys,
    /// Its key list, the count to copy, and whether any key can be hidden.
    Keys(*mut crate::array::ArrayHeader, usize, bool),
}

/// Can `Object.assign` use the positional source copy without changing the
/// result of strict `Set(target, key, value, true)` for any string key in the
/// plan?
///
/// The proof is entirely shape-owned. The target is an extensible ordinary
/// object. For every source key, an own target property must be writable data;
/// otherwise every ordinary object on the actual prototype chain is checked
/// until the key is found (writable data permits the receiver add; an accessor
/// or read-only data property refuses the fast path). An unshaped/exotic/proxy
/// hop is uncertainty and therefore a refusal. No store occurs until all keys
/// pass, so falling back cannot observe a partially copied target.
unsafe fn positional_assign_target_is_safe(
    target: *mut ObjectHeader,
    target_is_array: bool,
    plan: PositionalPlan,
) -> bool {
    if target_is_array {
        return false;
    }
    let Some(header) = crate::value::addr_class::try_read_gc_header(target as usize) else {
        return false;
    };
    let Some(target_shape) = super::shapes::object_shape_descriptor(target) else {
        return false;
    };
    const BLOCKING: u16 = crate::gc::OBJ_FLAG_FROZEN
        | crate::gc::OBJ_FLAG_SEALED
        | crate::gc::OBJ_FLAG_NO_EXTEND
        | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved & BLOCKING != 0
        || target_shape.object_kind != super::shapes::ShapeObjectKind::Ordinary
    {
        return false;
    }

    let (source_keys, key_count) = match plan {
        PositionalPlan::Refused => return false,
        PositionalPlan::NoKeys => return true,
        PositionalPlan::Keys(keys, count, _) => (keys, count),
    };
    let target_keys = super::object_keys(target);
    let mut prototype_keys = [std::mem::MaybeUninit::uninit(); 64];
    let Some(prototype_count) =
        positional_prototype_shape_keys(target as usize, &mut prototype_keys)
    else {
        return false;
    };
    for i in 0..key_count {
        let key = crate::object::ObjectKeys::new(source_keys, key_count as u32).get(i as u32);
        if !key.is_any_string() {
            return false;
        }
        let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let Some(key_bytes) = crate::string::js_string_key_bytes(key, &mut sso) else {
            return false;
        };

        if !target_keys.is_null() {
            if let Some(pos) = crate::object::keys_find_slot_by_bytes(
                target_keys.arr(),
                target_keys.count(),
                key_bytes,
            ) {
                let entry = super::key_attrs::keys_entry(target_keys.arr(), pos as u32);
                if !super::key_attrs::entry_is_plain_writable_data(entry) {
                    return false;
                }
                continue;
            }
        }
        for keys in &prototype_keys[..prototype_count] {
            let keys = *keys.assume_init_ref();
            if keys.is_null() {
                continue;
            }
            if let Some(pos) =
                crate::object::keys_find_slot_by_bytes(keys.arr(), keys.count(), key_bytes)
            {
                let entry = super::key_attrs::keys_entry(keys.arr(), pos as u32);
                if !super::key_attrs::entry_is_plain_writable_data(entry) {
                    return false;
                }
                break;
            }
        }
    }
    true
}

/// Snapshot the receiver's ordinary prototype chain as shape-owned key lists.
/// A non-ordinary hop may carry behavior not represented by such a list, so it
/// refuses the positional proof. Resolving an unmaterialized realm-default
/// prototype may allocate its shape-only sentinel; it runs no user code and
/// suppresses moving collection while the caller's raw operands are live.
unsafe fn positional_prototype_shape_keys(
    receiver: usize,
    out: &mut [std::mem::MaybeUninit<crate::object::ObjectKeys>; 64],
) -> Option<usize> {
    let mut proto = positional_shape_prototype(receiver as *const ObjectHeader)?;
    for (depth, slot) in out.iter_mut().enumerate() {
        let Some(addr) = proto else {
            return Some(depth);
        };
        let Some(header) = crate::value::addr_class::try_read_gc_header(addr) else {
            return None;
        };
        if header.obj_type != crate::gc::GC_TYPE_OBJECT
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            || header._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
        {
            return None;
        }
        let obj = addr as *const ObjectHeader;
        let Some(shape) = super::shapes::object_shape_descriptor(obj) else {
            return None;
        };
        // Only store-admissible ordinary shapes own the whole string-keyed
        // answer. The intrinsic Object.prototype is deliberately unmarked,
        // but its reflected accessors/data attributes still live in its
        // shape, so it is the one ordinary-layout root admitted explicitly.
        let canonical_object_proto = crate::array::object_prototype_addr_matches(addr);
        if shape.object_kind != super::shapes::ShapeObjectKind::Ordinary
            && !(canonical_object_proto && shape.object_kind.is_ordinary_layout())
        {
            return None;
        }
        slot.write(super::object_keys(obj));
        proto = positional_shape_prototype(obj)?;
    }
    None
}

/// Resolve one ordinary object's next prototype directly from the prototype
/// identity carried by its shape. `None` is uncertainty; `Some(None)` is the
/// end of chain. This deliberately does not call the general reflective
/// `getPrototypeOf` machinery: class-implied/per-object identities have no
/// shape-owned pointer word and therefore refuse this fast path.
unsafe fn positional_shape_prototype(obj: *const ObjectHeader) -> Option<Option<usize>> {
    let header = crate::value::addr_class::try_read_gc_header(obj as usize)?;
    if header._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0 {
        return Some(None);
    }
    let shape = super::shapes::object_shape_descriptor(obj)?;
    if shape.proto_id == super::shapes::PROTO_ID_NULL {
        return Some(None);
    }
    if shape.proto_id == super::shapes::PROTO_ID_DEFAULT {
        let intrinsic = crate::object::ensure_object_prototype_shape() as usize;
        if intrinsic == 0 {
            return None;
        }
        return Some((intrinsic != obj as usize).then_some(intrinsic));
    }
    let bits = super::shapes::object_prototype_word(obj);
    if bits == 0 {
        return None;
    }
    if bits == crate::value::TAG_NULL {
        return Some(None);
    }
    let value = JSValue::from_bits(bits);
    if !value.is_pointer() {
        return None;
    }
    let addr = value.as_pointer::<u8>() as usize;
    crate::value::addr_class::is_above_handle_band(addr).then_some(Some(addr))
}

/// The checks [`copy_positional_source`] makes once, on the source as it is
/// before any store. Allocates nothing.
///
/// # Safety
/// `src` is a live heap object.
unsafe fn positional_source_plan(src: *const ObjectHeader) -> PositionalPlan {
    let src_raw = src as usize;
    let Some(header) = crate::value::addr_class::try_read_gc_header(src_raw) else {
        return PositionalPlan::Refused;
    };
    if header._reserved
        & (crate::gc::OBJ_FLAG_HAS_DESCRIPTORS | crate::gc::OBJ_FLAG_STABLE_TOMBSTONES)
        != 0
    {
        return PositionalPlan::Refused;
    }
    let class_id = (*src).class_id;
    if (class_id != 0 && !super::class_registry::is_anon_shape_class_id(class_id))
        || !super::object_is_regular(src)
        || super::dictionary::is_dictionary(src)
        || super::string_wrapper::length(src_raw).is_some()
        || (class_id == 0 && crate::url::is_url_object_shape(src as *mut ObjectHeader))
    {
        return PositionalPlan::Refused;
    }
    let meta = (*src).meta;
    if !meta.is_null() && (*meta).flags & super::OBJECT_META_FLAG_IS_PROTOTYPE != 0 {
        return PositionalPlan::Refused;
    }
    let Some(shape) = super::shapes::object_shape_descriptor(src) else {
        return PositionalPlan::Refused;
    };
    // A private field is a non-enumerable entry (#11791), so the summary
    // gate refuses a source carrying one; it is named here all the same.
    if !shape.object_kind.is_ordinary_layout()
        || super::key_attrs::object_summary(src)
            & (super::key_attrs::SUMMARY_ACCESSOR
                | super::key_attrs::SUMMARY_NON_ENUMERABLE
                | super::key_attrs::SUMMARY_PRIVATE)
            != 0
    {
        return PositionalPlan::Refused;
    }
    let keys = super::object_keys(src);
    if keys.is_null() {
        return PositionalPlan::NoKeys;
    }
    let key_count =
        (keys.count() as usize).min(crate::array::keys_array_len_capped_to_capacity(keys.arr()));
    // The shape answers for every key at once whether any can be hidden
    // (#11791), as for the general enumeration below; no store into the
    // target changes the source's shape.
    let hide_private = crate::object::field_get_set::own_keys_may_hide(src);
    PositionalPlan::Keys(keys.arr(), key_count, hide_private)
}

/// The `i`th own string key of a positional source and its value, read by
/// position (`None` for a key that is not a string or is hidden). Allocates
/// nothing.
///
/// # Safety
/// `src` is a live heap object and `keys` its key list of at least
/// `key_count` entries.
unsafe fn positional_entry(
    src: *const ObjectHeader,
    keys: *const crate::array::ArrayHeader,
    key_count: usize,
    i: usize,
    hide_private: bool,
) -> Option<(crate::JSValue, f64)> {
    let key_val = crate::object::ObjectKeys::new(keys as *mut _, key_count as u32).get(i as u32);
    if !key_val.is_any_string()
        || (hide_private && crate::object::instance_private_key_hidden(src, key_val))
    {
        return None;
    }
    let live = crate::object::object_live_slot_count(src) as usize;
    let alloc_limit = std::cmp::max(live, crate::object::INLINE_SLOT_FLOOR);
    let value = if i < alloc_limit {
        std::ptr::read(
            (src as *const u8).add(std::mem::size_of::<ObjectHeader>() + i * 8) as *const f64,
        )
    } else {
        f64::from_bits(super::js_object_get_field(src, i as u32).bits())
    };
    Some((key_val, value))
}

unsafe fn object_assign_string_source(
    define: bool,
    target: *mut ObjectHeader,
    target_is_array: bool,
    source_f64: f64,
) {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some((ptr, blen)) = crate::string::str_bytes_from_jsvalue(source_f64, &mut scratch) else {
        return;
    };
    if ptr.is_null() {
        return;
    }
    let bytes = std::slice::from_raw_parts(ptr, blen as usize);
    let Ok(s) = std::str::from_utf8(bytes) else {
        return;
    };
    // #7214: SNAPSHOT the source before allocating anything.
    //
    // `str_bytes_from_jsvalue` returns a pointer INTO the source
    // `StringHeader`'s data region for any string past the SSO limit (header
    // and payload are one contiguous `arena_alloc_gc` block), and its own
    // safety note says so: "Callers must not hold this pointer past a
    // subsequent `scratch` modification or a GC cycle that could sweep the
    // heap-backed `StringHeader`." The loop below holds it across three
    // allocation points per character.
    //
    // MEASURED, because the size argument that makes this survivable is not one
    // to rely on. An instrumented build rooted both the source and the target
    // and counted relocations across the loop: on a 26 001-character source,
    // `src_moves=0 tgt_moves=1` — collections DO happen inside this function
    // (which is what makes the #7200 target rooting above load-bearing), but
    // that source could not move because at 26 KB it is over
    // `LARGE_OBJECT_THRESHOLD_BYTES` and `arena_alloc_gc` births it TENURED in
    // the non-moving old generation. Shrink it under the threshold and it
    // becomes a movable nursery string — but then one call allocates too little
    // to reliably span a collection, and none was observed.
    //
    // So the exposure is real and narrow: a source in the band just under
    // 16 KiB is both movable and long enough to allocate ~32 000 times. There
    // is NO runtime witness for it and I am not implying otherwise; what there
    // is, is a documented callee contract this violated and a safety margin
    // that rests entirely on a tunable constant. One owned copy on a path that
    // is already O(n) removes the dependence.
    let owned: String = s.to_string();

    // #7200: three allocations per iteration (`key_ptr`, `value_ptr`, and the
    // write funnel's interning / keys-array growth) with `target` and `key_ptr`
    // live across them. The probe above measured `tgt_moves=1`, so the target
    // half of this is not hypothetical.
    let scope = crate::gc::RuntimeHandleScope::new();
    let tgt_h = scope.root_raw_mut_ptr(target);
    for (idx, ch) in owned.chars().enumerate() {
        let iter_scope = crate::gc::RuntimeHandleScope::new();
        let key = idx.to_string();
        let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
        let key_h = iter_scope.root_string_ptr(key_ptr);
        let mut buf = [0u8; 4];
        let ch_str = ch.encode_utf8(&mut buf);
        let value_ptr = crate::string::js_string_from_bytes(ch_str.as_ptr(), ch_str.len() as u32);
        let value_h = iter_scope.root_string_ptr(value_ptr);
        tgt_h.with_mut_ptr::<ObjectHeader, _>(|t| {
            key_h.with_const_ptr::<crate::StringHeader, _>(|k| {
                object_assign_set_string_key(
                    define,
                    t,
                    target_is_array,
                    k,
                    f64::from_bits(
                        value_h.with_mut_ptr::<crate::StringHeader, _>(|v| {
                            JSValue::string_ptr(v).bits()
                        }),
                    ),
                )
            })
        });
    }
}

/// Copy own enumerable properties via [[OwnPropertyKeys]], [[GetOwnProperty]]
/// and [[Get]], in spec order. This handles ordinary and Proxy sources alike;
/// getters, target setters and traps can change the source between keys.
unsafe fn object_assign_enumerated_source(
    define: bool,
    target: *mut ObjectHeader,
    target_is_array: bool,
    source_f64: f64,
) {
    // Snapshot all own keys before any getter or target setter runs. Recheck
    // each descriptor below: earlier user code may delete or redefine a key.
    let scope = crate::gc::RuntimeHandleScope::new();
    let tgt_h = scope.root_raw_mut_ptr(target);
    let source_h = scope.root_nanbox_f64(source_f64);
    let keys_arr = crate::proxy::js_reflect_own_keys(source_h.get_nanbox_f64());
    let keys_val = JSValue::from_bits(keys_arr.to_bits());
    if !keys_val.is_pointer() {
        return;
    }
    let arr = keys_val.as_pointer::<crate::array::ArrayHeader>();
    if arr.is_null() {
        return;
    }
    let n = crate::array::js_array_length(arr);
    // #7200: the widest window in the file. TWO trap invocations per key —
    // `getOwnPropertyDescriptor` and `get` — each arbitrary user code, with the
    // `ownKeys` result array and the target held across both and used after.
    let keys_h = scope.root_raw_const_ptr(arr);
    for i in 0..n {
        let arr = keys_h.get_raw_const_ptr::<crate::array::ArrayHeader>();
        let source_f64 = source_h.get_nanbox_f64();
        let key = crate::array::js_array_get(arr, i);
        let iter_scope = crate::gc::RuntimeHandleScope::new();
        let key_h = iter_scope.root_nanbox_u64(key.bits());
        let key_f64 = f64::from_bits(key.bits());
        // `[[GetOwnProperty]]` — fires the getOwnPropertyDescriptor trap.
        let desc = crate::proxy::js_reflect_get_own_property_descriptor(source_f64, key_f64);
        let desc_h = iter_scope.root_nanbox_f64(desc);
        let desc_ptr =
            (desc_h.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK) as *const ObjectHeader;
        if desc.to_bits() == JSValue::undefined().bits() || desc_ptr.is_null() {
            continue;
        }
        let ek = crate::string::js_string_from_bytes(b"enumerable".as_ptr(), 10);
        if crate::value::js_is_truthy(crate::object::js_object_get_field_by_name_f64(
            (desc_h.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK) as *const ObjectHeader,
            ek,
        )) == 0
        {
            continue;
        }
        // `[[Get]]` — fires the get trap.
        let key_f64 = f64::from_bits(key_h.get_nanbox_u64());
        let source_value = source_h.get_nanbox_f64();
        let value_f64 = crate::proxy::js_reflect_get(source_value, key_f64, source_value);
        let value_h = iter_scope.root_nanbox_f64(value_f64);
        let key_f64 = f64::from_bits(key_h.get_nanbox_u64());
        if key.is_any_string() {
            let key_ptr =
                crate::value::js_get_string_pointer_unified(key_f64) as *const crate::StringHeader;
            if !key_ptr.is_null() {
                tgt_h.with_mut_ptr::<ObjectHeader, _>(|t| {
                    object_assign_set_string_key(
                        define,
                        t,
                        target_is_array,
                        key_ptr,
                        value_h.get_nanbox_f64(),
                    )
                });
            }
        } else if key.is_pointer() {
            let target_value = tgt_h
                .with_mut_ptr::<ObjectHeader, _>(|t| crate::value::js_nanbox_pointer(t as i64));
            if define {
                crate::symbol::js_object_set_symbol_property(
                    target_value,
                    key_f64,
                    value_h.get_nanbox_f64(),
                );
            } else {
                crate::proxy::js_put_value_set(
                    target_value,
                    key_f64,
                    value_h.get_nanbox_f64(),
                    target_value,
                    1,
                );
            }
        }
    }
}

/// Per spec, undefined/null target throws TypeError. Non-object sources
/// are skipped except string primitives, which expose enumerable index
/// properties (`Object.assign({}, "ab") -> {0:"a",1:"b"}`).
#[no_mangle]
pub unsafe extern "C" fn js_object_assign_one(target_f64: f64, source_f64: f64) -> f64 {
    object_assign_one(target_f64, source_f64, false)
}

/// `{ ...src }` inside a source-ordered object literal: CopyDataProperties
/// into the literal's own fresh object. The same enumeration of `src` as
/// `Object.assign`, but every property is DEFINED on the target
/// (`CreateDataPropertyOrThrow`), never assigned: an `Object.prototype`
/// accessor or read-only property of the same name neither runs nor rejects.
#[no_mangle]
pub unsafe extern "C" fn js_object_literal_spread(target_f64: f64, source_f64: f64) -> f64 {
    object_assign_one(target_f64, source_f64, true)
}

/// `Object.assign(target, source)` for one source (`define == false`), or the
/// spread literal's CopyDataProperties (`define == true`).
unsafe fn object_assign_one(target_f64: f64, source_f64: f64, define: bool) -> f64 {
    let target_f64 = js_object_assign_validate_target(target_f64);

    // NOTE: a `process.env` target is handled in `object_assign_set_string_key`
    // (the single write funnel) rather than here. An early exit at this point
    // would have to re-implement source decoding, and the version that did got
    // all three edge cases wrong: it cast any source pointer to `ObjectHeader`
    // (type confusion on a string/array source) and it enumerated the source
    // with `js_object_keys_value`, which *throws* on `null`/`undefined` instead
    // of skipping it as the spec requires.
    let target_value = JSValue::from_bits(target_f64.to_bits());
    if !target_value.is_pointer() {
        return target_f64;
    }
    let tgt_raw = target_value.as_pointer::<u8>() as usize;
    // A real `ObjectHeader` is heap-allocated and #[repr(C)] with u64 /
    // pointer fields, so a valid object pointer is always 8-byte aligned.
    // If a non-object target reaches here after nullish validation, skip
    // mutation rather than dereferencing an invalid pointer.
    if tgt_raw < 0x10000 || !tgt_raw.is_multiple_of(8) {
        return target_f64;
    }

    let target = tgt_raw as *mut ObjectHeader;

    // #2439: When the target is an array, an integer-keyed source property
    // (e.g. `Object.assign([1,2], {2:3})`) must grow the array's length, not
    // land as an inert object expando. `js_array_set_string_key` parses the
    // key as a canonical array index and routes through `js_array_set_f64_extend`
    // (which extends length + fills holes); non-numeric keys fall back to the
    // object-property path on the array's expando map. Detect array-ness once.
    let target_is_array = {
        let gc_header =
            (target as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY
    };

    let source = JSValue::from_bits(source_f64.to_bits());
    if source.is_undefined() || source.is_null() {
        return target_f64;
    }
    if source.is_any_string() {
        // #7200: the callee allocates per character, so the target it returns
        // through must be the post-collection one.
        let scope = crate::gc::RuntimeHandleScope::new();
        let tgt_h = scope.root_raw_mut_ptr(target);
        object_assign_string_source(define, target, target_is_array, source_f64);
        return tgt_h
            .with_mut_ptr::<ObjectHeader, _>(|t| crate::value::js_nanbox_pointer(t as i64));
    }

    // A Proxy source isn't an `ObjectHeader` (its NaN-box payload is a small
    // registry id, not a heap pointer), so the raw `keys_array` walk below
    // would skip it silently. Spec requires enumerating it through its traps —
    // `[[OwnPropertyKeys]]` (ownKeys), `[[GetOwnProperty]]`
    // (getOwnPropertyDescriptor) for the enumerable test, then `[[Get]]` for
    // each value — with every trap's abrupt completion propagating out (test262
    // Object/assign/source-own-prop-error + source-own-prop-keys-error).
    if crate::proxy::js_proxy_is_proxy(source_f64) != 0 {
        // #7200: every proxy trap is user code; the target can be anywhere by
        // the time the last one returns.
        let scope = crate::gc::RuntimeHandleScope::new();
        let tgt_h = scope.root_raw_mut_ptr(target);
        object_assign_enumerated_source(define, target, target_is_array, source_f64);
        return tgt_h
            .with_mut_ptr::<ObjectHeader, _>(|t| crate::value::js_nanbox_pointer(t as i64));
    }

    // Decode source pointer. Skip null/undefined/non-pointer sources.
    if !source.is_pointer() {
        return target_f64;
    }
    let src_raw = source.as_pointer::<u8>() as usize;
    // Same alignment guard as the target above — `src` is dereferenced at
    // `crate::object::object_keys_array(src)` just below; an unaligned non-object source must
    // be skipped, not dereferenced. Reject the WHOLE handle band, not just a
    // `< 0x10000` floor: a common-band registry id (crypto `Hash`, `Blob`, …)
    // can sit above 0x10000 and be 8-aligned, so the old floor let it through
    // and `crate::object::object_keys_array(src)` read unmapped memory (SIGSEGV). A native handle
    // has no own enumerable properties to spread, so skipping it yields `{}`,
    // matching Node (`{...new Blob([])}` === `{}`). test_gap_handle_band_object_ops
    // `{...blob}`/`{...hash}`.
    if !crate::value::addr_class::is_above_handle_band(src_raw)
        || !src_raw.is_multiple_of(8)
        || crate::symbol::is_registered_symbol(src_raw)
    {
        return target_f64;
    }

    // #8149: a registered BUFFER source — a node `Buffer` / `Uint8Array` (whose
    // own enumerable properties ARE its byte indices, so
    // `{...Buffer.from([1,2,3])}` is `{"0":1,"1":2,"2":3}` in node), or an
    // `ArrayBuffer` / `DataView` (which own only whatever the user assigned).
    // A `BufferHeader` is not an `ObjectHeader`; the walk below reached the
    // `try_read_gc_header` triage and answered `{}` for an arena-backed buffer,
    // and an EXTERNAL one has no `GcHeader` at all, so the byte it reads there
    // is allocator bookkeeping that can classify as anything. Enumerate through
    // the shared buffer own-key helper instead.
    if let Some(keys) =
        crate::object::field_get_set::enumeration::registered_buffer_own_keys(src_raw)
    {
        // The key string and the write funnel both allocate, so the target can
        // move on every iteration: read it through the handle AT the call
        // (`with_mut_ptr`) rather than binding a pre-loop copy.
        let scope = crate::gc::RuntimeHandleScope::new();
        let tgt_h = scope.root_raw_mut_ptr(target);
        for name in keys {
            let value = crate::object::field_get_set::enumeration::registered_buffer_own_value(
                src_raw, &name,
            );
            let value_h = scope.root_nanbox_f64(value);
            let key_ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            tgt_h.with_mut_ptr::<ObjectHeader, _>(|tgt| {
                object_assign_set_string_key(
                    define,
                    tgt,
                    target_is_array,
                    key_ptr,
                    value_h.get_nanbox_f64(),
                )
            });
        }
        return tgt_h
            .with_mut_ptr::<ObjectHeader, _>(|tgt| crate::value::js_nanbox_pointer(tgt as i64));
    }

    // A function/closure source is NOT an `ObjectHeader`: reading `keys_array`
    // off it dereferences a bogus field, yielding a garbage `key_count` and a
    // runaway copy loop. Enumerate the closure's own *enumerable* dynamic props
    // instead — the built-in `length`/`name`/`prototype` slots are
    // non-enumerable and excluded, matching `Object.keys`/`getOwnPropertyNames`.
    // (Stripe's `protoExtend` does `Object.assign(Constructor, Super)` to copy a
    // resource class's enumerable statics like `.extend`/`.method`; without this
    // the call hung at `import 'stripe'`.)
    // An `Error` source. Like the buffer and closure arms around it, an
    // `ErrorHeader` is not the JSObject keys/values layout, so it has no
    // `keys_array` for the generic path below to walk — `{...err}` and
    // `Object.assign({}, err)` therefore copied NOTHING and produced `{}`.
    //
    // Node treats an error as an ordinary property bearer here: its own
    // ENUMERABLE properties are copied, which for a caught fs error means
    // `code`/`errno`/`syscall`/`path`, and for any error means whatever the
    // program assigned. `message`/`name`/`stack` stay behind because they are
    // non-enumerable — `exotic_own_keys(.., enumerable_only = true)` encodes
    // exactly that rule, and is the same enumeration `Object.keys` and
    // `JSON.stringify` use, so the three cannot disagree.
    if src_raw >= 0x10000 && src_raw.is_multiple_of(8) && {
        let src_gc =
            (src_raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        (*src_gc).obj_type == crate::gc::GC_TYPE_ERROR
    } {
        use crate::object::exotic_expando::{exotic_get_own_property, exotic_own_keys, ExoticKind};
        let scope = crate::gc::RuntimeHandleScope::new();
        let tgt_h = scope.root_raw_mut_ptr(target);
        let receiver = crate::value::js_nanbox_pointer(src_raw as i64);
        for name in exotic_own_keys(ExoticKind::Error, src_raw, true) {
            let Some(value) = exotic_get_own_property(src_raw, ExoticKind::Error, &name, receiver)
            else {
                continue;
            };
            let value_h = scope.root_nanbox_f64(value);
            let key_ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            tgt_h.with_mut_ptr::<ObjectHeader, _>(|tgt| {
                object_assign_set_string_key(
                    define,
                    tgt,
                    target_is_array,
                    key_ptr,
                    value_h.get_nanbox_f64(),
                )
            });
        }
        return tgt_h
            .with_mut_ptr::<ObjectHeader, _>(|tgt| crate::value::js_nanbox_pointer(tgt as i64));
    }

    if crate::closure::is_closure_ptr(src_raw) {
        // #7200: `js_string_from_bytes` and the write funnel both allocate, and
        // the snapshot's VALUES are heap references held in a plain `Vec` for
        // the whole loop. `src_raw` keys the closure side tables, so it has to
        // survive too. No accessor runs here (the snapshot is raw), so this is
        // the allocation-only form of the same window rather than user-code
        // re-entry — it is fixed for symmetry, and because a snapshot Vec of
        // unrooted heap words is a liveness hole as well as a staleness one.
        let scope = crate::gc::RuntimeHandleScope::new();
        let tgt_h = scope.root_raw_mut_ptr(target);
        let src_h = scope.root_raw_const_ptr(src_raw as *const u8);
        let snapshot = crate::closure::closure_dynamic_props_snapshot(src_raw);
        let value_handles: Vec<_> = snapshot
            .iter()
            .map(|(_name, value)| scope.root_nanbox_f64(*value))
            .collect();
        for ((name, _), value_h) in snapshot.iter().zip(value_handles.iter()) {
            let src_raw = src_h.get_raw_const_ptr::<u8>() as usize;
            if matches!(name.as_str(), "length" | "name" | "prototype") {
                continue;
            }
            if crate::closure::closure_is_key_deleted(src_raw, name) {
                continue;
            }
            if let Some(attrs) = get_property_attrs(src_raw, name) {
                if !attrs.enumerable() {
                    continue;
                }
            }
            let key_ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            tgt_h.with_mut_ptr::<ObjectHeader, _>(|t| {
                object_assign_set_string_key(
                    define,
                    t,
                    target_is_array,
                    key_ptr,
                    value_h.get_nanbox_f64(),
                )
            });
        }
        return tgt_h
            .with_mut_ptr::<ObjectHeader, _>(|t| crate::value::js_nanbox_pointer(t as i64));
    }

    let src = src_raw as *const ObjectHeader;

    // #6667: native-module namespace source (`Object.assign(t, require("crypto"))`).
    // Its exports resolve lazily through the vtable, so the raw keys_array walk
    // below sees only `__module__`. Enumerate + resolve the export surface, then
    // return — native-module namespaces carry no own symbol-keyed properties, so
    // the symbol-copy tail below would be a no-op.
    {
        let scope = crate::gc::RuntimeHandleScope::new();
        let tgt_h = scope.root_raw_mut_ptr(target);
        if super::native_module::copy_native_module_exports(src, |key_ptr, value| {
            tgt_h.with_mut_ptr::<ObjectHeader, _>(|t| {
                object_assign_set_string_key(define, t, target_is_array, key_ptr, value)
            });
        }) {
            // `copy_native_module_exports` allocates (fresh export closures +
            // key strings); a minor GC there can evacuate `target`. Return the
            // handle-reloaded pointer so the caller threads the post-GC
            // location, not the stale `target_f64`.
            return tgt_h
                .with_mut_ptr::<ObjectHeader, _>(|t| crate::value::js_nanbox_pointer(t as i64));
        }
    }

    // An array source (`Object.assign(t, [1,2])`, `{ ...[1,2] }`) stores its
    // indexed elements in the `ArrayHeader` element buffer, NOT in an
    // `ObjectHeader.keys_array`. ArrayHeader has no such field, so the
    // keys_array read below would deref a garbage pointer and crash (the prior
    // behavior — a hard SIGSEGV on a common operation). Enumerate the dense
    // index range directly through the array API instead. (#5347 Object/assign)
    // Classify the source's GC type once. A genuine plain/class object keeps
    // its own string-keyed props in `ObjectHeader.keys_array`; an array keeps
    // indexed elements in its `ArrayHeader` buffer (handled below). Anything
    // else (Map/Set/Promise/Date/WeakMap/…) has its OWN header layout — reading
    // its bytes as `ObjectHeader.keys_array` yields a garbage pointer that the
    // key-copy loop then walks as an array (a memory-layout-dependent SIGBUS on
    // `Object.assign({}, new Map())`, #6070). Per CopyDataProperties such
    // exotics expose no own enumerable string keys through this path, so they
    // contribute nothing — skip them (mirrors `js_object_copy_own_fields`).
    // Probe the GcHeader without deref-faulting — a handle-band id that passed
    // the guards above would otherwise deref a non-heap address; mirrors the
    // sibling `js_object_copy_own_fields`.
    let source_obj_type = match crate::value::addr_class::try_read_gc_header(src_raw) {
        Some(h) => h.obj_type,
        None => return target_f64,
    };
    let source_is_array = source_obj_type == crate::gc::GC_TYPE_ARRAY;

    // #7341: a RegExp source has no ObjectHeader keys array and must not enter
    // the plain-object copy arm. Its dedicated GC kind makes that decision
    // without reading any native payload word.
    //
    // Per CopyDataProperties a RegExp exposes no own enumerable string keys
    // through this path (`source`/`flags`/`lastIndex` are prototype accessors
    // or non-enumerable), so skipping contributes nothing and matches Node:
    // `Object.assign({}, /x/g)` is `{}`. Any own expandos a user attached live
    // in the exotic-expando side table, which this raw walk never read anyway.
    //
    // Repro: `Object.assign({}, /x/g)` under
    // PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_HEAP_LIMIT=8.
    if source_obj_type == crate::gc::GC_TYPE_REGEXP {
        return target_f64;
    }

    let positional_plan = if source_obj_type == crate::gc::GC_TYPE_OBJECT {
        positional_source_plan(src)
    } else {
        PositionalPlan::Refused
    };
    let strict_set_is_shape_equivalent =
        !define && positional_assign_target_is_safe(target, target_is_array, positional_plan);
    if !define
        && !strict_set_is_shape_equivalent
        && matches!(
            source_obj_type,
            crate::gc::GC_TYPE_OBJECT | crate::gc::GC_TYPE_ARRAY
        )
    {
        // Assign's [[Set]] can run a setter even for a plain data source, so
        // neither positional slots nor a prefiltered enumerable-key list stay
        // valid across stores. The descriptor-driven copy also snapshots the
        // symbol keys before any string-keyed setter can change the source.
        let scope = crate::gc::RuntimeHandleScope::new();
        let tgt_h = scope.root_raw_mut_ptr(target);
        object_assign_enumerated_source(false, target, target_is_array, source_f64);
        return tgt_h
            .with_mut_ptr::<ObjectHeader, _>(|t| crate::value::js_nanbox_pointer(t as i64));
    }

    // #7200: EVERYTHING BELOW RUNS WITH USER CODE IN THE WINDOW.
    //
    // Both copy loops reach a `[[Get]]` that short-circuits into
    // `invoke_accessor_getter` when the source carries an accessor descriptor —
    // i.e. they run ARBITRARY USER CODE inside this runtime helper. User code
    // reaches a loop back-edge poll, and under `PERRY_GC_MOVING_LOOP_POLLS=1`
    // that is an evacuating minor running with this Rust frame live.
    //
    // `target`, `src`, `src_keys`, `arr` and each `key_ptr` are raw addresses
    // in Rust locals. The collector rewrites ROOTS; a local is not one. Every
    // one of them is used *after* the getter returns — `target` and `key_ptr`
    // by the write funnel on the very next line, `src_keys`/`src`/`arr` by the
    // next iteration — so each is a from-space address for the rest of the
    // copy. That is the SIGSEGV in `{ ...src, tail: 7 }` with an accessor
    // source, and the silently-dropped value in its lighter variant.
    //
    // The function already models the fix one branch up: the native-module arm
    // opens a scope, roots `target`, and returns the handle-reloaded pointer.
    // This is that treatment applied to the arms the syntax actually takes, and
    // it spans BOTH numbered sections because the symbol tail's `[[Get]]` is a
    // symbol-keyed getter with exactly the same reach.
    let scope = crate::gc::RuntimeHandleScope::new();
    let tgt_h = scope.root_raw_mut_ptr(target);
    let src_h = scope.root_raw_const_ptr(src);
    let source_h = scope.root_nanbox_f64(source_f64);

    // 1) Copy own string-keyed enumerable properties from source to target,
    //    in source insertion order. Mirrors `js_object_copy_own_fields`.
    if source_is_array {
        let arr_h = scope.root_raw_const_ptr(src_raw as *const crate::array::ArrayHeader);
        let arr = arr_h.get_raw_const_ptr::<crate::array::ArrayHeader>();
        let n = crate::array::js_array_length(arr);
        // Snapshot string expandos (`arr.foo = …`, kept in the named-property
        // side table) BEFORE the index loop: that loop allocates, which can
        // trigger a GC that rekeys the side table to the moved array's new
        // address — after which a lookup by this (stale) address would miss
        // them. They sort AFTER the integer indices in [[OwnPropertyKeys]] order.
        let expandos: Vec<(String, f64)> = crate::array::array_named_property_names(arr, true)
            .into_iter()
            .filter_map(|name| {
                crate::array::array_named_property_get_by_name(arr, &name).map(|v| (name, v))
            })
            .collect();
        for i in 0..n {
            // Re-derive from the handle: `js_string_from_bytes` and the write
            // funnel both allocate, so the previous iteration may have moved the
            // source array and the target.
            let arr = arr_h.get_raw_const_ptr::<crate::array::ArrayHeader>();
            // Holes (absent indices) in a sparse array are NOT own enumerable
            // properties and must be skipped — Object.assign only copies own
            // enumerable properties (test262 assign/target-Array.js).
            if !crate::array::array_spec_has_index(arr, i) {
                continue;
            }
            let value = crate::array::js_array_get(arr, i);
            let iter_scope = crate::gc::RuntimeHandleScope::new();
            let val_h = iter_scope.root_nanbox_u64(value.bits());
            let key = i.to_string();
            let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
            let key_h = iter_scope.root_string_ptr(key_ptr);
            tgt_h.with_mut_ptr::<ObjectHeader, _>(|t| {
                key_h.with_const_ptr::<crate::StringHeader, _>(|k| {
                    object_assign_set_string_key(
                        define,
                        t,
                        target_is_array,
                        k,
                        f64::from_bits(val_h.get_nanbox_u64()),
                    )
                })
            });
        }
        for (name, value) in expandos {
            let iter_scope = crate::gc::RuntimeHandleScope::new();
            let val_h = iter_scope.root_nanbox_f64(value);
            let key_ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            let key_h = iter_scope.root_string_ptr(key_ptr);
            tgt_h.with_mut_ptr::<ObjectHeader, _>(|t| {
                key_h.with_const_ptr::<crate::StringHeader, _>(|k| {
                    object_assign_set_string_key(
                        define,
                        t,
                        target_is_array,
                        k,
                        val_h.get_nanbox_f64(),
                    )
                })
            });
        }
    } else if source_obj_type == crate::gc::GC_TYPE_OBJECT
        && copy_positional_source(
            define,
            strict_set_is_shape_equivalent,
            positional_plan,
            &tgt_h,
            &src_h,
            target_is_array,
        )
    {
        // Every own string key was an enumerable data property of the
        // source shape, copied by position (`copy_positional_source`).
    } else if source_obj_type == crate::gc::GC_TYPE_OBJECT {
        let src_keys = if super::string_wrapper::length(src as usize).is_some() {
            let names = src_h.with_const_ptr(|src: *const ObjectHeader| {
                js_object_get_own_property_names(crate::value::js_nanbox_pointer(src as i64))
            });
            // A fresh result array: exclusively owned.
            crate::object::ObjectKeys::owned(
                crate::value::js_nanbox_get_pointer(names) as *mut crate::ArrayHeader
            )
        } else {
            crate::object::object_keys(src)
        };
        let keys_h = scope.root_raw_mut_ptr(src_keys.arr());
        if !src_keys.is_null() && (src_keys.arr() as usize) >= 0x10000 {
            // The receiver's count, snapshotted before any getter runs; the
            // view caps it at the array's capacity (a malformed keys array can
            // report a bogus, pointer-sized length, and an unclamped
            // `0..key_count` copy loop turns Object.assign / object spread into
            // a minutes-long spin).
            let key_count = (src_keys.count() as usize).min(
                crate::array::keys_array_len_capped_to_capacity(src_keys.arr()),
            );
            // Use the public [[Get]] path, not raw field slots, so accessors run
            // and abrupt completions propagate the way Object.assign requires.
            // A class instance's runtime-internal keys are hidden by name. A
            // private field (#11791) is a non-enumerable entry, so the
            // enumerability check below drops it with the shape's own
            // attributes; no other lookup is needed for it.
            let hide_internal = (*src).class_id != 0;
            for i in 0..key_count {
                // Re-derive every raw address from its handle at the top of the
                // iteration: the PREVIOUS iteration's getter may have moved all
                // of them.
                let src_keys = keys_h.get_raw_mut_ptr::<crate::array::ArrayHeader>();
                let src = src_h.get_raw_const_ptr::<ObjectHeader>();
                let src_raw = src as usize;
                let key_val = crate::array::js_array_get(src_keys, i as u32);
                if !key_val.is_any_string() {
                    continue;
                }
                if hide_internal {
                    let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                    if crate::string::js_string_key_bytes(key_val, &mut buf)
                        .is_some_and(crate::object::field_get_set::is_internal_runtime_key_bytes)
                    {
                        continue;
                    }
                }
                let key_f64 = f64::from_bits(key_val.bits());
                let key_ptr = crate::value::js_get_string_pointer_unified(key_f64)
                    as *const crate::StringHeader;
                if key_ptr.is_null() {
                    continue;
                }
                let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                if let Some(name_bytes) = crate::string::js_string_key_bytes(key_val, &mut sso_buf)
                {
                    if let Ok(name) = std::str::from_utf8(name_bytes) {
                        if let Some(attrs) = get_property_attrs(src_raw, name) {
                            if !attrs.enumerable() {
                                continue;
                            }
                        }
                    }
                }
                // Per-iteration scope so the key/value roots are cut each time
                // round rather than growing the handle stack by 2 per key.
                let iter_scope = crate::gc::RuntimeHandleScope::new();
                let key_h = iter_scope.root_string_ptr(key_ptr);
                let field_f64 = f64::from_bits(js_object_get_field_by_name(src, key_ptr).bits());
                // The getter's RETURN VALUE is a fresh heap reference reachable
                // from nothing else, and the write funnel below allocates (key
                // interning, keys-array growth, shape transition). Root it and
                // read it back, exactly like the pointers.
                let val_h = iter_scope.root_nanbox_f64(field_f64);
                tgt_h.with_mut_ptr::<ObjectHeader, _>(|t| {
                    key_h.with_const_ptr::<crate::StringHeader, _>(|k| {
                        object_assign_set_string_key(
                            define,
                            t,
                            target_is_array,
                            k,
                            val_h.get_nanbox_f64(),
                        )
                    })
                });
            }
        }
    }

    // 2) Copy own symbol-keyed enumerable properties from source to target,
    //    in `[[OwnPropertyKeys]]` symbol order (after the string keys). Use the
    //    full own-symbol-key list — `clone_symbol_entries_for_obj_ptr` only
    //    surfaces symbols with a stored *value*, missing accessor-only symbols
    //    (`Object.defineProperty(o, sym, { get })`), so a symbol getter never
    //    ran during assign (test262 assign/strings-and-symbol-order). Snapshot
    //    the symbol pointers first: the inner `[[Get]]` / set re-acquire
    //    SYMBOL_PROPERTIES, so iterating a held snapshot avoids re-entrancy.
    let sym_keys: Vec<usize> = {
        let arr_raw = crate::symbol::js_object_get_own_property_symbols(source_h.get_nanbox_f64());
        let mut v = Vec::new();
        if arr_raw != 0 {
            let arr = arr_raw as *const crate::array::ArrayHeader;
            if !arr.is_null() {
                let n = crate::array::js_array_length(arr);
                for i in 0..n {
                    let sv = crate::array::js_array_get(arr, i);
                    let p = (sv.bits() & crate::value::POINTER_MASK) as usize;
                    if p != 0 {
                        v.push(p);
                    }
                }
            }
        }
        v
    };
    for sym_ptr in sym_keys {
        // #7200: `js_object_get_symbol_property` below is a symbol-keyed
        // `[[Get]]` — an accessor there runs user code with the same reach as
        // the string-key loop's. `src_raw` keys the attribute side tables and
        // `target`/`target_f64` are the write destination, so all three are
        // re-derived from their handles each time round.
        let src_raw = src_h.get_raw_const_ptr::<ObjectHeader>() as usize;
        if !crate::symbol::symbol_property_is_enumerable(src_raw, sym_ptr) {
            continue;
        }
        let sym_f64 = f64::from_bits(JSValue::pointer(sym_ptr as *const u8).bits());
        let iter_scope = crate::gc::RuntimeHandleScope::new();
        let sym_h = iter_scope.root_nanbox_f64(sym_f64);
        // Read the source value through `[[Get]]`, not the raw side-table bits,
        // so a symbol-keyed accessor's getter runs during `Object.assign`
        // (test262 assign/strings-and-symbol-order). The earlier string-key
        // copy already uses `[[Get]]` via `js_object_get_field_by_name`.
        let value_f64 =
            crate::symbol::js_object_get_symbol_property(source_h.get_nanbox_f64(), sym_f64);
        let value_h = iter_scope.root_nanbox_f64(value_f64);
        let target_value =
            tgt_h.with_mut_ptr::<ObjectHeader, _>(|t| crate::value::js_nanbox_pointer(t as i64));
        if define {
            crate::symbol::js_object_set_symbol_property(
                target_value,
                sym_h.get_nanbox_f64(),
                value_h.get_nanbox_f64(),
            );
        } else {
            crate::proxy::js_put_value_set(
                target_value,
                sym_h.get_nanbox_f64(),
                value_h.get_nanbox_f64(),
                target_value,
                1,
            );
        }
    }

    // The target may have moved under any of the getters above; hand the caller
    // the post-collection address, not the `target_f64` captured on entry. The
    // native-module arm already does this; the main path did not, so `acc` in a
    // chained `Object.assign(t, a, b)` threaded a from-space pointer into the
    // next link.
    tgt_h.with_mut_ptr::<ObjectHeader, _>(|t| crate::value::js_nanbox_pointer(t as i64))
}
