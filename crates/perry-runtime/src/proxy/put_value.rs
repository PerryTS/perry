//! `PutValue` for property references (`obj.k = v` / `obj[k] = v` runtime
//! dispatch), split out of `proxy.rs` to keep it under the file-size gate.
//! Routes Proxy traps, integer-indexed exotics, exotic expando cells, and
//! the ordinary receiver-aware `[[Set]]` walk.

use super::*;

/// `proxy[key] = value` — if handler.set exists, call it with
/// (target, key, value) and return TAG_TRUE (the trap's return value is
/// ignored by the default test semantics since we echo `value`). Otherwise
/// forward to the target directly.
#[no_mangle]
pub extern "C" fn js_proxy_set(proxy_boxed: f64, key: f64, value: f64) -> f64 {
    proxy_set_with_receiver(proxy_boxed, key, value, proxy_boxed)
}

/// Proxy `[[Set]]` (ECMA-262 §10.5.9) with an explicit `Receiver`, distinct
/// from `proxy_boxed` itself — reached when a Proxy sits partway up another
/// object's `[[Prototype]]` chain (`OrdinarySetWithOwnDescriptor` forwards to
/// `parent.[[Set]](P, V, Receiver)` with the ORIGINAL receiver, not `parent`).
pub(crate) fn proxy_set_with_receiver(
    proxy_boxed: f64,
    key: f64,
    value: f64,
    receiver: f64,
) -> f64 {
    let id = match lookup(proxy_boxed) {
        Some(id) => id,
        None => return f64::from_bits(TAG_FALSE),
    };
    let (target, handler, revoked) = PROXIES.with(|p| {
        p.borrow()
            .get(id as usize)
            .and_then(|o| o.as_ref())
            .map(|e| (e.target, e.handler, e.revoked))
            .unwrap_or((
                f64::from_bits(TAG_UNDEFINED),
                f64::from_bits(TAG_UNDEFINED),
                false,
            ))
    });
    if revoked {
        return revoked_return();
    }
    let trap = handler_trap(handler, "set");
    if is_callable(trap) {
        // #2756: the `set` trap's boolean result is observable through
        // `Reflect.set(proxy, …)` (and strict-mode assignment). Coerce and
        // return it rather than discarding it. The trap receives the spec
        // argument list `(target, key, value, receiver)` with `this` bound to
        // the handler.
        let scope = crate::gc::RuntimeHandleScope::new();
        let target_h = scope.root_nanbox_f64(target);
        let key_h = scope.root_nanbox_f64(key);
        let value_h = scope.root_nanbox_f64(value);
        let receiver_h = scope.root_nanbox_f64(receiver);
        let trap_result = call_trap(
            handler,
            trap,
            &[
                target_h.get_nanbox_f64(),
                key_h.get_nanbox_f64(),
                value_h.get_nanbox_f64(),
                receiver_h.get_nanbox_f64(),
            ],
        );
        // A falsy trap result means the assignment failed; no invariant check.
        if crate::value::js_is_truthy(trap_result) == 0 {
            return nanbox_bool(false);
        }
        invariants::enforce_set_invariant(
            target_h.get_nanbox_f64(),
            key_h.get_nanbox_f64(),
            value_h.get_nanbox_f64(),
        );
        return nanbox_bool(true);
    }
    // No set trap — forward to the target's `[[Set]]`. When the target is
    // itself a Proxy, recurse through the proxy dispatch (its own trap or
    // target) rather than `ordinary_set`, which would deref the fake pointer.
    if lookup(target).is_some() {
        return proxy_set_with_receiver(target, key, value, receiver);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_nanbox_f64(target);
    let key_handle = scope.root_nanbox_f64(key);
    let value_handle = scope.root_nanbox_f64(value);
    let receiver_handle = scope.root_nanbox_f64(receiver);
    let property_key_handle = scope
        .root_nanbox_f64(unsafe { crate::object::js_to_property_key(key_handle.get_nanbox_f64()) });
    reflect_ordinary_set_with_receiver(
        target_handle.get_nanbox_f64(),
        property_key_handle.get_nanbox_f64(),
        value_handle.get_nanbox_f64(),
        receiver_handle.get_nanbox_f64(),
    )
}

/// Assignment PutValue for a property reference. Returns the assigned RHS value
/// on success or sloppy failure, and throws TypeError when strict code attempts
/// a failed [[Set]].
#[no_mangle]
pub extern "C" fn js_put_value_set(
    target: f64,
    key: f64,
    value: f64,
    receiver: f64,
    strict: i32,
) -> f64 {
    // Sloppy script assignment lowers to PutValue rather than the named-field
    // setter.  Existing own data fields need none of PutValue's rooting,
    // ToPropertyKey, Proxy, typed-array, or receiver-aware prototype work.
    // Keep this before the handle scope: the helper validates both heap
    // headers and only performs a barriered overwrite when the target and
    // receiver are the same ordinary object and codegen supplied an interned
    // heap-string key.
    let target_bits = target.to_bits();
    let key_bits = key.to_bits();
    if target_bits == receiver.to_bits()
        && (target_bits & !POINTER_MASK) == POINTER_TAG
        && (key_bits & !POINTER_MASK) == crate::value::STRING_TAG
    {
        let obj = (target_bits & POINTER_MASK) as *mut crate::ObjectHeader;
        let key_ptr = (key_bits & POINTER_MASK) as *const crate::StringHeader;
        if unsafe { crate::object::try_existing_own_data_overwrite(obj, key_ptr, value) } {
            return value;
        }
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_nanbox_f64(target);
    let key_handle = scope.root_nanbox_f64(key);
    let value_handle = scope.root_nanbox_f64(value);
    let receiver_handle = scope.root_nanbox_f64(receiver);
    let target = target_handle.get_nanbox_f64();
    let key = key_handle.get_nanbox_f64();
    let value = value_handle.get_nanbox_f64();
    let receiver = receiver_handle.get_nanbox_f64();
    let property_key_handle =
        scope.root_nanbox_f64(unsafe { crate::object::js_to_property_key(key) });
    let property_key = property_key_handle.get_nanbox_f64();

    if lookup(target).is_none() {
        if set_integer_indexed_exotic(target, property_key, value) {
            return value;
        }
        // Integer-Indexed exotic objects: a key that is *not* a CanonicalNumeric
        // index does OrdinarySet, creating/looking-up a normal own property on
        // the typed array (ECMA-262 §10.4.5.5). The generic
        // `ordinary_set_with_receiver` path below mis-reads the typed-array
        // header as an `ObjectHeader` and segfaults, so route typed-array
        // targets to the TA-aware setters (mirroring `js_object_set_field_by_name`).
        // A CanonicalNumeric-but-out-of-bounds key (`"1.5"`, `"NaN"`, `"-0"`)
        // is classified `IntegerIndex` inside `typed_array_set_property_by_name`
        // and silently ignored — never materialized as an ordinary property.
        if let Some(addr) = crate::typedarray_props::typed_array_addr_from_value(target) {
            if unsafe { crate::symbol::js_is_symbol(property_key) } != 0 {
                unsafe {
                    crate::symbol::js_object_set_symbol_property(target, property_key, value);
                }
                return value;
            }
            if let Some(name) = key_to_rust_string(property_key) {
                unsafe {
                    crate::typedarray_props::typed_array_set_property_by_name(addr, &name, value);
                }
                return value;
            }
        }
        // Date / RegExp / Error exotic cells: route to the expando-aware
        // setter — the ordinary path below would bit-cast them. Throws on a
        // rejected strict write. (See `object::exotic_expando`.)
        if let Some(v) = crate::object::exotic_expando::exotic_put_value_set(
            target,
            property_key,
            value,
            receiver,
            strict,
        ) {
            return v;
        }
        // #5437: a live Web Stream handle (raw finite f64 id in the stream
        // band). React's `renderToReadableStream` attaches its shell-ready
        // promise as an expando (`stream.allReady = ...`); without a store the
        // write was dropped (sloppy) or threw read-only (strict), which killed
        // the Next.js dynamic-SSR render. Route the write to the stdlib
        // per-stream expando table (GC-traced there).
        if target.is_finite() && target > 0.0 && target.fract() == 0.0 {
            let id = target as usize;
            if crate::value::addr_class::is_stream_id_band(id) {
                if let (Some(probe), Some(setter)) = (
                    crate::object::stream_handle_probe(),
                    crate::object::stream_expando_set(),
                ) {
                    if unsafe { probe(id) } {
                        if let Some(name) = key_to_rust_string(property_key) {
                            unsafe { setter(id, name.as_ptr(), name.len(), value) };
                        }
                    }
                }
                // A stream-band id is a reserved handle, never a settable
                // object — stop here rather than falling through to the
                // ordinary `[[Set]]` walk, even when the expando write was a
                // no-op (dead handle / hooks absent / non-UTF-8 key). Mirrors
                // the `js_object_set_field_by_name` stream guard.
                return value;
            }
        }
        if target.to_bits() == receiver.to_bits() && key_is_length(property_key) {
            if let Some(arr) = array_ptr_from_value(target) {
                // PutValue(`arr.length = v`) is `Set(O, "length", v, Throw)`. In
                // strict mode a frozen array's non-writable `length` makes the
                // write throw a TypeError instead of silently no-oping.
                if strict != 0 {
                    crate::array::js_array_set_length_strict(arr, value);
                } else {
                    crate::array::js_array_set_length(arr, value);
                }
                return value;
            }
        }
    }

    if target_bits == TAG_NULL || target_bits == TAG_UNDEFINED {
        let key_name = key_to_rust_string(property_key).unwrap_or_else(|| "property".to_string());
        let msg = format!("Cannot set properties of null or undefined (setting '{key_name}')");
        return throw_type_error(&msg);
    }
    let ok = if lookup(target).is_some() {
        js_proxy_set(target, property_key, value).to_bits() == TAG_TRUE
    } else {
        ordinary_set_with_receiver(target, property_key, value, receiver)
    };
    if !ok && strict != 0 {
        let key_name = key_to_rust_string(property_key).unwrap_or_else(|| "property".to_string());
        crate::error::throw_immutable_write(0, &key_name);
    }
    value_handle.get_nanbox_f64()
}

/// Miss path for the codegen-emitted monomorphic PutValue store cache.
///
/// The full strict/sloppy `[[Set]]` semantics run first. Only a successful
/// ordinary class-instance own-data overwrite may prime `[shape_token, slot]`;
/// every exotic, descriptor-bearing, frozen, class-object, plain-class-zero,
/// overflow, or typed-layout-intact receiver remains permanently on the miss
/// path. The token mirrors the read PIC: a stamped runtime ShapeId is lifted
/// above the pointer range with bit 62; otherwise the shared keys pointer is
/// used. The generated hit path repeats all mutable per-object guards.
#[no_mangle]
pub extern "C" fn js_put_value_set_ic_miss(
    target: f64,
    key: *const crate::StringHeader,
    value: f64,
    strict: i32,
    cache: *mut [i64; 2],
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_nanbox_f64(target);
    let key_handle = scope.root_string_ptr(key);
    let value_handle = scope.root_nanbox_f64(value);
    let key_value = if key.is_null() {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits())
    };
    let result = js_put_value_set(
        target_handle.get_nanbox_f64(),
        key_value,
        value_handle.get_nanbox_f64(),
        target_handle.get_nanbox_f64(),
        strict,
    );

    if cache.is_null() {
        return result;
    }

    unsafe {
        let target = target_handle.get_nanbox_f64();
        let target_bits = target.to_bits();
        if (target_bits & !POINTER_MASK) != POINTER_TAG {
            return result;
        }
        let obj_addr = (target_bits & POINTER_MASK) as usize;
        let key = key_handle.get_raw_const_ptr::<crate::StringHeader>();
        let Some(gc_header) = crate::value::addr_class::try_read_gc_header(obj_addr) else {
            return result;
        };
        const BLOCKING_FLAGS: u16 = crate::gc::OBJ_FLAG_FROZEN
            | crate::gc::OBJ_FLAG_SEALED
            | crate::gc::OBJ_FLAG_NO_EXTEND
            | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
            | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO
            // A generated hit cannot update/downgrade a typed layout without
            // calling the runtime. The miss store clears this bit; prime only
            // once that per-object downgrade is visible.
            | crate::gc::GC_OBJ_TYPED_LAYOUT_INTACT;
        if gc_header.obj_type != crate::gc::GC_TYPE_OBJECT
            || gc_header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            || gc_header._reserved & BLOCKING_FLAGS != 0
            || key.is_null()
        {
            return result;
        }

        let obj = obj_addr as *mut crate::ObjectHeader;
        let class_id = (*obj).class_id;
        if (*obj).object_type != crate::error::OBJECT_TYPE_REGULAR
            || class_id == 0
            || class_id == crate::object::NATIVE_MODULE_CLASS_ID
        {
            return result;
        }
        let Some(key_gc) = crate::value::addr_class::try_read_gc_header(key as usize) else {
            return result;
        };
        if key_gc.obj_type != crate::gc::GC_TYPE_STRING
            || key_gc.gc_flags & (crate::gc::GC_FLAG_FORWARDED | crate::gc::GC_FLAG_INTERNED)
                != crate::gc::GC_FLAG_INTERNED
        {
            return result;
        }

        let keys = (*obj).keys_array;
        if keys.is_null() || (keys as u64) >> 48 != 0 {
            return result;
        }
        let Some(keys_gc) = crate::value::addr_class::try_read_gc_header(keys as usize) else {
            return result;
        };
        if keys_gc.obj_type != crate::gc::GC_TYPE_ARRAY
            || keys_gc.gc_flags & (crate::gc::GC_FLAG_FORWARDED | crate::gc::GC_FLAG_SHAPE_SHARED)
                != crate::gc::GC_FLAG_SHAPE_SHARED
        {
            return result;
        }

        let mut own_idx = crate::object::prop_plan::read_plan_lookup(keys as usize, key as usize);
        if own_idx.is_none() {
            let key_count = crate::array::keys_array_len_capped_to_capacity(keys);
            if key_count > 4096 {
                return result;
            }
            for i in 0..key_count {
                let candidate = crate::array::js_array_get(keys, i as u32);
                if crate::string::js_string_key_matches(candidate, key) {
                    crate::object::prop_plan::read_plan_record(
                        keys as usize,
                        key as usize,
                        i as u32,
                    );
                    own_idx = Some(i as u32);
                    break;
                }
            }
        }
        let Some(idx) = own_idx else {
            return result;
        };
        let alloc_limit =
            std::cmp::max((*obj).field_count, crate::object::INLINE_SLOT_FLOOR as u32) as usize;
        if idx as usize >= alloc_limit {
            return result;
        }

        let parent_class_id = (*obj).parent_class_id;
        let shape_token = if crate::object::shapes::is_shape_id(parent_class_id) {
            crate::object::shapes::PIC_ID_TOKEN_BIT | parent_class_id as u64
        } else {
            keys as u64
        };

        // Publish the token last conceptually: a zero-initialized or stale
        // token cannot hit this slot until it matches this receiver's current
        // discriminated shape token. Perry's read PIC uses the same format.
        (*cache)[1] = idx as i64;
        (*cache)[0] = shape_token as i64;
    }

    result
}

#[cold]
fn trace_object_array_numeric_write_rejection(reason: &'static str) {
    if std::env::var_os("PERRY_TRACE_OBJECT_ARRAY_WRITE_GUARD").is_some() {
        eprintln!("PERRY_OBJECT_ARRAY_WRITE_GUARD_REJECT: {reason}");
    }
}

#[inline]
fn trace_object_array_numeric_write_stage<T>(value: Option<T>, reason: &'static str) -> Option<T> {
    if value.is_none() {
        trace_object_array_numeric_write_rejection(reason);
    }
    value
}

/// Prove all receivers and return up to four inline numeric slot indexes.
///
/// The caller performs one bounded scan, then holds raw array/object pointers
/// for a finite, call-free loop nest. Consequently this helper must reject any
/// receiver, key, or layout state that could require ordinary `[[Set]]`
/// semantics. The fixed array is stack-only; no descriptor allocation is
/// introduced on the preflight path.
fn object_array_numeric_write_slots(array: f64, keys: &[f64], count: u32) -> Option<[u16; 4]> {
    // Reuse the process gate js_gc_init disables for typed-feedback tracing,
    // typed-layout verification, and the explicit inline-field escape hatch.
    // This loop bypasses the same observations/checks as the class-field
    // inline clone and therefore must honor the identical gate.
    if count == 0
        || keys.is_empty()
        || keys.len() > 4
        || !crate::object::class_field_inline_guard_enabled()
    {
        trace_object_array_numeric_write_rejection("disabled gate or invalid field/count bound");
        return None;
    }

    let array_bits = array.to_bits();
    if (array_bits & !POINTER_MASK) != POINTER_TAG {
        trace_object_array_numeric_write_rejection("receiver container is not an array pointer");
        return None;
    }
    let array_addr = (array_bits & POINTER_MASK) as usize;
    let array_gc = trace_object_array_numeric_write_stage(
        unsafe { crate::value::addr_class::try_read_gc_header(array_addr) },
        "array header is unavailable",
    )?;
    if array_gc.obj_type != crate::gc::GC_TYPE_ARRAY
        || array_gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || array_gc._reserved & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0
        || crate::array::PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED
            .load(std::sync::atomic::Ordering::Relaxed)
            != 0
    {
        trace_object_array_numeric_write_rejection(
            "array kind, forwarding, descriptor, or index-fast-path state",
        );
        return None;
    }

    let arr = array_addr as *const crate::array::ArrayHeader;
    let (length, capacity) = unsafe { ((*arr).length, (*arr).capacity) };
    if length > 16_000_000 || capacity > 16_000_000 || length > capacity || count > length {
        trace_object_array_numeric_write_rejection("array length/capacity/prefix bound");
        return None;
    }

    // Unlike the per-site PIC, this proof does not retain key identity after
    // the call or index a pointer-keyed cache. `find_slot` performs only
    // allocation-free byte comparisons, so a live non-forwarded string-pool
    // handle is sufficient; requiring INTERNED here made eligibility depend
    // accidentally on whether some unrelated site had interned the same name.
    let decode_key = |boxed: f64| -> Option<*const crate::StringHeader> {
        let bits = boxed.to_bits();
        if (bits & !POINTER_MASK) != crate::value::STRING_TAG {
            return None;
        }
        let ptr = (bits & POINTER_MASK) as *const crate::StringHeader;
        let gc = unsafe { crate::value::addr_class::try_read_gc_header(ptr as usize) }?;
        (gc.obj_type == crate::gc::GC_TYPE_STRING
            && gc.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0)
            .then_some(ptr)
    };
    let mut decoded_keys = [std::ptr::null(); 4];
    for (index, boxed) in keys.iter().enumerate() {
        decoded_keys[index] = trace_object_array_numeric_write_stage(
            decode_key(*boxed),
            "target key is not a live heap string",
        )?;
    }

    const BLOCKING_FLAGS: u16 = crate::gc::OBJ_FLAG_FROZEN
        | crate::gc::OBJ_FLAG_SEALED
        | crate::gc::OBJ_FLAG_NO_EXTEND
        | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
        | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO;

    unsafe fn validated_object(
        bits: u64,
    ) -> Option<(
        *mut crate::ObjectHeader,
        *mut crate::array::ArrayHeader,
        u16,
    )> {
        if (bits & !POINTER_MASK) != POINTER_TAG {
            return None;
        }
        let addr = (bits & POINTER_MASK) as usize;
        let gc = crate::value::addr_class::try_read_gc_header(addr)?;
        if gc.obj_type != crate::gc::GC_TYPE_OBJECT
            || gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            || gc._reserved & BLOCKING_FLAGS != 0
        {
            return None;
        }
        let obj = addr as *mut crate::ObjectHeader;
        if (*obj).object_type != crate::error::OBJECT_TYPE_REGULAR
            || (*obj).class_id == 0
            || (*obj).class_id == crate::object::NATIVE_MODULE_CLASS_ID
        {
            return None;
        }
        let keys = (*obj).keys_array;
        if keys.is_null() || (keys as u64) >> 48 != 0 {
            return None;
        }
        let keys_gc = crate::value::addr_class::try_read_gc_header(keys as usize)?;
        if keys_gc.obj_type != crate::gc::GC_TYPE_ARRAY
            || keys_gc.gc_flags & (crate::gc::GC_FLAG_FORWARDED | crate::gc::GC_FLAG_SHAPE_SHARED)
                != crate::gc::GC_FLAG_SHAPE_SHARED
        {
            return None;
        }
        Some((obj, keys, gc._reserved))
    }

    unsafe fn find_slot(
        keys: *mut crate::array::ArrayHeader,
        key: *const crate::StringHeader,
    ) -> Option<u32> {
        let key_count = crate::array::keys_array_len_capped_to_capacity(keys);
        if key_count > 4096 {
            return None;
        }
        for i in 0..key_count {
            let candidate = crate::array::js_array_get(keys, i as u32);
            if crate::string::js_string_key_matches(candidate, key) {
                return Some(i as u32);
            }
        }
        None
    }

    let elements = unsafe {
        (arr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64
    };
    let first_bits = unsafe { (*elements).to_bits() };
    if first_bits == crate::value::TAG_HOLE {
        trace_object_array_numeric_write_rejection("first receiver is a hole");
        return None;
    }
    let (first, shared_keys, first_flags) = trace_object_array_numeric_write_stage(
        unsafe { validated_object(first_bits) },
        "first receiver is not an eligible regular shared-shape object",
    )?;
    let mut slots = [0u16; 4];
    for index in 0..keys.len() {
        // `find_slot` caps the shared keys array at 4096 entries, so every
        // non-zero-encoded index fits comfortably in one 16-bit result lane.
        let slot = trace_object_array_numeric_write_stage(
            unsafe { find_slot(shared_keys, decoded_keys[index]) },
            "target key is absent from the shared shape",
        )?;
        slots[index] = trace_object_array_numeric_write_stage(
            u16::try_from(slot).ok(),
            "target slot cannot be encoded",
        )?;
    }

    let first_limit = unsafe {
        std::cmp::max(
            (*first).field_count,
            crate::object::INLINE_SLOT_FLOOR as u32,
        )
    };
    if slots[..keys.len()]
        .iter()
        .any(|slot| u32::from(*slot) >= first_limit)
    {
        trace_object_array_numeric_write_rejection("first receiver target slot is out of bounds");
        return None;
    }
    if first_flags & crate::gc::GC_OBJ_TYPED_LAYOUT_INTACT != 0
        && slots[..keys.len()].iter().any(|slot| {
            !crate::gc::layout_typed_accepts_finite_number_slot_for_user(
                first as usize,
                usize::from(*slot),
            )
        })
    {
        trace_object_array_numeric_write_rejection(
            "first receiver typed descriptor does not contain every target slot",
        );
        return None;
    }

    for i in 1..count as usize {
        let bits = unsafe { (*elements.add(i)).to_bits() };
        if bits == crate::value::TAG_HOLE {
            trace_object_array_numeric_write_rejection("receiver prefix contains a hole");
            return None;
        }
        let (obj, object_keys, flags) = trace_object_array_numeric_write_stage(
            unsafe { validated_object(bits) },
            "receiver prefix contains an ineligible object",
        )?;
        if object_keys != shared_keys {
            trace_object_array_numeric_write_rejection(
                "receiver prefix does not share one keys array",
            );
            return None;
        }
        let limit =
            unsafe { std::cmp::max((*obj).field_count, crate::object::INLINE_SLOT_FLOOR as u32) };
        if slots[..keys.len()]
            .iter()
            .any(|slot| u32::from(*slot) >= limit)
        {
            trace_object_array_numeric_write_rejection(
                "receiver prefix contains an out-of-bounds target slot",
            );
            return None;
        }
        if flags & crate::gc::GC_OBJ_TYPED_LAYOUT_INTACT != 0
            && slots[..keys.len()].iter().any(|slot| {
                !crate::gc::layout_typed_accepts_finite_number_slot_for_user(
                    obj as usize,
                    usize::from(*slot),
                )
            })
        {
            trace_object_array_numeric_write_rejection(
                "receiver typed descriptor does not contain every target slot",
            );
            return None;
        }
    }

    Some(slots)
}

/// Preflight for codegen's bounded call-free nested object-write loop.
///
/// Each active slot is encoded as `slot + 1` in a 16-bit lane, with the first
/// key in the least-significant lane. Zero means that the generated raw loop
/// must not run. The caller scans once, then performs only finite numeric
/// stores, whose bits are valid in both raw-f64 and ordinary numeric JSValue
/// fields, until both loops finish. That call-free interval is load-bearing:
/// no GC can move the array, its elements, their shared keys array, or their
/// typed-layout records after this function validates them.
#[no_mangle]
pub extern "C" fn js_object_array_numeric_write_guard(
    array: f64,
    key_1: f64,
    key_2: f64,
    key_3: f64,
    key_4: f64,
    field_count: u32,
    receiver_count: u32,
) -> u64 {
    if !(1..=4).contains(&field_count) {
        return 0;
    }
    let keys = [key_1, key_2, key_3, key_4];
    let Some(slots) =
        object_array_numeric_write_slots(array, &keys[..field_count as usize], receiver_count)
    else {
        return 0;
    };
    slots[..field_count as usize]
        .iter()
        .enumerate()
        .fold(0, |packed, (index, slot)| {
            packed | ((u64::from(*slot) + 1) << (index * 16))
        })
}

/// Preserve the #6811 internal ABI for cached generated objects. New codegen
/// uses [`js_object_array_numeric_write_guard`], but an object cache entry
/// produced before the runtime rebuild may still reference this symbol.
#[no_mangle]
pub extern "C" fn js_object_array_numeric_write2_guard(
    array: f64,
    key_1: f64,
    key_2: f64,
    receiver_count: u32,
) -> u64 {
    let Some(slots) = object_array_numeric_write_slots(array, &[key_1, key_2], receiver_count)
    else {
        return 0;
    };
    (u64::from(slots[1]) + 1) << 32 | (u64::from(slots[0]) + 1)
}
