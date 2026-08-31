//! `PutValue` for property references (`obj.k = v` / `obj[k] = v` runtime
//! dispatch), split out of `proxy.rs` to keep it under the file-size gate.
//! Routes Proxy traps, integer-indexed exotics, exotic expando cells, and
//! the ordinary receiver-aware `[[Set]]` walk.

use super::*;

/// Receiver-kind test shared by every object-write fast path (#8098).
///
/// A class instance qualifies, and so does a plain object the runtime
/// birth-marked `OBJ_FLAG_PLAIN_ORDINARY` — today that is `JSON.parse` output
/// (`json/parser.rs`, `json_tape.rs`), which carries an authoritative ShapeId
/// since #8067/#8086 but no class.
///
/// This replaces a blanket `class_id != 0`. That clause was standing in for
/// three per-object exclusions the generic path still applies verbatim
/// (`object/field_set_by_name/fast_paths.rs::try_existing_own_data_overwrite`):
/// `NATIVE_MODULE_CLASS_ID`, `Object.prototype`, and a `URL` instance, whose
/// `pathname`/`search`/… own slots are live views whose setters rebuild `href`
/// (`field_set_by_name/tail.rs`). None of those is derivable from the ShapeId —
/// two objects share one iff they share a keys-array ALLOCATION, and the
/// shape-transition cache deliberately converges distinct objects onto one
/// shared array — so the discriminator has to be per-object and re-tested on
/// every generated cache hit. An opt-in mark is exactly that, and it fails
/// safe: an unmarked class-less receiver keeps the full `[[Set]]` walk.
#[inline]
unsafe fn write_fast_path_receiver_kind_ok(
    obj: *const crate::ObjectHeader,
    obj_flags: u16,
) -> bool {
    let class_id = (*obj).class_id;
    if class_id == crate::object::NATIVE_MODULE_CLASS_ID {
        return false;
    }
    if crate::object::is_class_object_ptr(obj.cast()) {
        return false;
    }
    class_id != 0 || obj_flags & crate::gc::OBJ_FLAG_PLAIN_ORDINARY != 0
}

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
    let _proxy_pin = pin_proxy_for_native_call(proxy_boxed);
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
    let key = key_handle.get_nanbox_f64();
    let property_key_handle =
        scope.root_nanbox_f64(unsafe { crate::object::js_to_property_key(key) });
    let property_key = property_key_handle.get_nanbox_f64();
    // #8495: read the operands AFTER `js_to_property_key`, never before it.
    // ToPropertyKey runs user code (`toString`/`Symbol.toPrimitive`), which
    // allocates and can evacuate. The handles above keep these values ALIVE,
    // but binding them to plain `f64` locals first captures pre-collection
    // addresses that the evacuation then leaves stale — rooting that the
    // rest of the function never benefits from. The observable symptom is a
    // stored value that reads back `undefined` one collection later
    // (`o[heavyKey("k")] = payload()` in
    // `gc_property_key_operand_rooting_6935`), i.e. exactly the #6935
    // contract, re-broken by reading through the handles too early.
    let target = target_handle.get_nanbox_f64();
    let value = value_handle.get_nanbox_f64();
    let receiver = receiver_handle.get_nanbox_f64();

    if lookup(target).is_none() {
        if set_integer_indexed_exotic(target, property_key, value) {
            return value;
        }
        if target.to_bits() == receiver.to_bits() {
            if let Some(stored) = set_nonindexed_buffer_named_self(target, property_key, value) {
                if !stored && strict != 0 {
                    let key_name =
                        key_to_rust_string(property_key).unwrap_or_else(|| "property".to_string());
                    crate::error::throw_immutable_write(0, &key_name);
                }
                return value;
            }
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
            // #7574: `sub.length = n` on a `class X extends Array` instance.
            // `array_ptr_from_value` rightly answers `None` (the instance is an
            // `ObjectHeader`, not an `ArrayHeader`), so this fell through to the
            // ordinary set, which recorded the new `length` but left every
            // element at or above it in place — `sub[1]` still read its old
            // value after `sub.length = 1`. Run the Array-exotic
            // `Set(O, "length", n, true)`, which deletes the truncated indices.
            if crate::array::is_array_subclass_value(target) {
                crate::array::array_object_set_length(
                    target,
                    crate::builtins::js_number_coerce(value),
                );
                return value;
            }
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
        // #8082: `ordinary_set_with_receiver` can run user setters (and
        // allocates), so it is a collection point — the raw `receiver` and
        // `property_key` locals go stale across it. The forced-moving gate
        // faulted on exactly this pair inside the subclass-length note's
        // header read. Root both and re-read after the set.
        let scope = crate::gc::RuntimeHandleScope::new();
        let receiver_h = scope.root_nanbox_f64(receiver);
        let key_h = scope.root_nanbox_f64(property_key);
        let stored = ordinary_set_with_receiver(target, property_key, value, receiver);
        // #7574: `sub[3] = v` on a `class X extends Array` instance is an
        // Array-exotic `[[DefineOwnProperty]]` and must leave `length == 4`.
        // Perry models the instance as a plain object, so the ordinary set
        // above records the index but never touches `length` — pre-fix
        // `sub[0] = 10; sub.length` read back `0` (node: `1`), on the
        // `T[]`-annotated and the unannotated path alike (both land here
        // through `js_put_value_set_ic_miss`). Cheap no-op for every other
        // receiver: an object literal short-circuits on `class_id == 0`.
        if stored {
            crate::array::note_array_subclass_index_write(
                receiver_h.get_nanbox_f64(),
                key_h.get_nanbox_f64(),
            );
        }
        stored
    };
    if !ok && strict != 0 {
        let key_name = key_to_rust_string(property_key).unwrap_or_else(|| "property".to_string());
        crate::error::throw_immutable_write(0, &key_name);
    }
    value_handle.get_nanbox_f64()
}
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
    // #7341: pair the allocating call with the re-read, so the pre-call `key`
    // address is never nameable.
    let (result, key) = key_handle.across_const::<crate::StringHeader, _>(|| {
        js_put_value_set(
            target_handle.get_nanbox_f64(),
            key_value,
            value_handle.get_nanbox_f64(),
            target_handle.get_nanbox_f64(),
            strict,
        )
    });

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
        if !crate::object::object_is_regular(obj)
            || !write_fast_path_receiver_kind_ok(obj, gc_header._reserved)
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

        let Some(shape) = crate::object::shapes::object_shape_descriptor(obj) else {
            return result;
        };
        let keys = shape.keys as usize as *mut crate::array::ArrayHeader;
        if keys.is_null() || (keys as u64) >> 48 != 0 {
            return result;
        }
        let Some(keys_gc) = crate::value::addr_class::try_read_gc_header(keys as usize) else {
            return result;
        };
        if keys_gc.obj_type != crate::gc::GC_TYPE_ARRAY
            || keys_gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return result;
        }

        let mut own_idx = crate::object::prop_plan::read_plan_lookup(keys as usize, key as usize);
        if own_idx.is_none() {
            let key_count = shape.logical_key_count as usize;
            if key_count > 4096 {
                return result;
            }
            // #6759: shape-index + dense-slot scan; the old per-element
            // js_array_get walk was ~60 accessor calls per property op.
            if let Some(i) = crate::object::keys_find_slot_by_key_ptr(keys, key_count as u32, key) {
                crate::object::prop_plan::read_plan_record(keys as usize, key as usize, i);
                own_idx = Some(i);
            }
        }
        let Some(idx) = own_idx else {
            return result;
        };
        // #9287: a slot past the inline region primes too, carrying
        // IC_SLOT_OVERFLOW_BIT exactly like the dynamic-key IC's stub entries.
        // The emitted hit path routes such slots through
        // `js_put_value_set_ic_overflow_store`, which is `dyn_ic_try_store` —
        // the same validate-and-store the dynamic IC has always used for
        // overflow properties (spill store, tombstone check, barriers). Before
        // this, `idx >= alloc_limit` returned without priming, so a property
        // in overflow storage missed this cache on EVERY access: 3 ms vs
        // 27 ms for the identical loop with the identical object, decided by
        // whether the property sat at index 1 or index 2.
        let alloc_limit = shape.live_inline_slot_count as usize;
        let slot_word: u32 = if (idx as usize) < alloc_limit {
            idx
        } else if (idx as usize) < shape.logical_key_count as usize && idx < IC_SLOT_OVERFLOW_BIT {
            idx | IC_SLOT_OVERFLOW_BIT
        } else {
            return result;
        };

        // The descriptor above already proves this stamp is live, so the
        // token comes from the header word rather than from a second full
        // lookup-and-copy of the same id (see `dyn_ic_try_store`).
        let shape_token = crate::object::shapes::PIC_ID_TOKEN_BIT
            | crate::object::shapes::object_shape_stamp(obj) as u64;

        // Publish the token last conceptually: a zero-initialized or stale
        // token cannot hit this slot until it matches this receiver's current
        // discriminated shape token. Perry's read PIC uses the same format.
        (*cache)[1] = slot_word as i64;
        (*cache)[0] = shape_token as i64;
        // Rotating-key sites overflow the single-slot site cache immediately;
        // the global stub is what lets them hit. Key bits from the rooted key.
        // `key_handle` here roots a raw STRING pointer (this entry's key arrives
        // as *const StringHeader) — re-box it the way `key_value` above did
        // rather than asking the handle for a NaN-boxed read it does not hold.
        // Scoped read: nothing inside the closure allocates or polls the
        // collector, so the pointer cannot go stale within it (#7341).
        key_handle.with_const_ptr::<crate::StringHeader, _>(|key_now| {
            if !key_now.is_null() {
                let boxed =
                    f64::from_bits(crate::value::js_nanbox_string(key_now as i64).to_bits());
                if let Some(kb) = stub_key_bits(boxed) {
                    write_stub_insert(shape_token, kb, slot_word);
                }
            }
        });
    }

    result
}

const STATIC_PIC_TAIL_WAYS: usize = 4;

/// Outlined ways 5–8 for the static-key write PIC.
///
/// The generated function keeps its first four shape guards inline. Once
/// those are full, this helper validates up to four additional cached
/// `(shape_token, slot)` pairs before falling back to full `[[Set]]`
/// semantics. Empty ways are filled in order and a full cache is never
/// overwritten, so a stable eight-shape site settles instead of continuously
/// replacing its fourth entry.
#[no_mangle]
pub extern "C" fn js_put_value_set_ic_poly_tail(
    cache: *mut [i64; 8],
    target: f64,
    key: *const crate::StringHeader,
    value: f64,
    strict: i32,
) -> f64 {
    if !cache.is_null() {
        unsafe {
            let c = &mut *cache;
            for way in 0..STATIC_PIC_TAIL_WAYS {
                let word = way * 2;
                let token = c[word] as u64;
                if token == 0 {
                    let entry = c.as_mut_ptr().add(word) as *mut [i64; 2];
                    return js_put_value_set_ic_miss(target, key, value, strict, entry);
                }
                if let Some(result) = dyn_ic_try_store(target, token, c[word + 1] as u32, value) {
                    return result;
                }
            }
        }
    }

    // More than eight stable shapes remain bounded and semantically correct:
    // execute the ordinary write without evicting a useful settled entry.
    js_put_value_set_ic_miss(target, key, value, strict, std::ptr::null_mut())
}

// ---------------------------------------------------------------------------
// #6812 (w12): 3-way dynamic-key write IC.
//
// Layout of the per-site `[8 x i64]` cache (same global family as the
// static write PIC): `[shape_token, k0, s0, k1, s1, k2, s2, _spare]`.
// A way's key is the NaN-boxed key VALUE's bits: SSO keys (<= 5 bytes)
// compare by CONTENT — move-immune and hitting across distinct string
// instances — while heap keys compare by identity (a moved or re-built key
// simply misses; identity can never false-hit). The shape token mirrors the
// static PIC (stamped ShapeId lifted with the ID bit, else the shared keys
// pointer) and, like every Perry IC, is only COMPARED, never dereferenced:
// stale entries self-heal by missing. That property is what the discarded
// safepointing-RHS approach lacked — nothing here roots or revalidates
// across safepoints, so the hot path is pure compares before a store.

const DYN_IC_WAYS: usize = 3;

/// Outlined dynamic-key PutValue with per-site cache. Fast path: shape token
/// + key-bits match -> validated own-slot overwrite. Everything else falls
/// through to the full `[[Set]]` semantics and re-primes.
#[no_mangle]
pub extern "C" fn js_put_value_set_dyn_ic(
    cache: *mut [i64; 8],
    target: f64,
    key: f64,
    value: f64,
    strict: i32,
) -> f64 {
    if !cache.is_null() {
        let hit = unsafe {
            let c = &*cache;
            let token = c[0] as u64;
            let key_bits = key.to_bits() as i64;
            // `0` is the empty-way sentinel — and also the NaN-box bits of
            // the JS number 0, so a dynamic numeric-0 key must never reach
            // the way compares (it would false-hit an unfilled way).
            if token != 0 && key_bits != 0 {
                let mut found = None;
                for way in 0..DYN_IC_WAYS {
                    if c[1 + way * 2] == key_bits {
                        found = Some(c[2 + way * 2] as u32);
                        break;
                    }
                }
                // Way miss on a rotating-key site: the global stub answers
                // in one probe. `dyn_ic_try_store` still validates everything.
                if found.is_none() {
                    found = stub_key_bits(key).and_then(|kb| write_stub_probe(token, kb));
                }
                found.and_then(|slot| dyn_ic_try_store(target, token, slot, value))
            } else {
                None
            }
        };
        if let Some(ret) = hit {
            return ret;
        }
    }
    js_put_value_set_dyn_ic_miss(cache, target, key, value, strict)
}

/// Megamorphic stub cache for dynamic string-keyed WRITES — V8's answer to a
/// site that rotates more keys than its per-site ways can hold.
///
/// The per-site inline IC has `DYN_IC_WAYS = 3`; a loop writing 500 rotating
/// keys evicts permanently and every write pays the full miss walk (~400 ns
/// measured, vs node's ~28 ns/op on the same host). This global table is keyed
/// on `(shape_token, key_bits)` so capacity scales with the PROGRAM's live
/// (shape, key) pairs instead of one site's ways.
///
/// Correctness leans entirely on [`dyn_ic_try_store`], which re-validates the
/// receiver's CURRENT shape token, blocking flags and slot bound on every hit.
/// A stale entry therefore misses; it cannot corrupt. That is why there is no
/// epoch and no GC hook: entries hold no roots (token is an id, key_bits are
/// SSO immediates or interned-pointer bits used only for equality, slot is an
/// index), and wrong entries are rejected by validation.
///
/// SSO keys (≤ 5 bytes — every `"k" + i`-style computed name) recur with
/// identical bits by construction. Heap-string keys recur once canonicalised:
/// the first write interns the key (write tail) and later concat evaluations
/// return the canonical pointer (intern hit), so the second prime converges.
/// Buckets in the megamorphic write stub, each holding [`WRITE_STUB_ASSOC`]
/// entries. Capacity is `WRITE_STUB_BUCKETS * WRITE_STUB_ASSOC`.
const WRITE_STUB_BUCKETS: usize = 2048;

/// Two ways per bucket, because a DIRECT-MAPPED table cannot hold a colliding
/// pair at all: the two keys evict each other on every rotation through the
/// key set, so both miss forever and neither can ever stabilise. Measured on
/// the computed-key write loop, that was not a rounding error — the pairs the
/// runtime reported (`"k10"`/`"k115"`, `"k11"`/`"k125"`) land on one index,
/// and the writes that never settle were 11.9% of the program, essentially
/// all of it the full `[[Set]]` walk they fall through to.
///
/// A sweep of the 500-key working set puts the worst bucket at two, so two
/// ways absorb the collisions that exist rather than merely reducing them.
const WRITE_STUB_ASSOC: usize = 2;

crate::perry_thread_local! {
    static WRITE_STUB: [[std::cell::Cell<(u64, u64, u64)>; WRITE_STUB_ASSOC]; WRITE_STUB_BUCKETS] =
        std::array::from_fn(|_| std::array::from_fn(|_| std::cell::Cell::new((0, 0, 0))));
}

/// Content-stable cache key for a NaN-boxed property key, or `None` when this
/// key must not be cached in the stub at all.
///
/// Two jobs, and the second one is a safety property.
///
/// **Normalization.** A short key can arrive in EITHER representation — as an
/// SSO immediate, or as a heap `StringHeader*` when some intermediate (a typed
/// local, a call boundary) materialized it. Those are the same JS string with
/// completely different bits, and a fresh heap key minted per loop iteration
/// is what made a raw-bits stub miss 98% of its probes even after the concat
/// itself started returning SSO. Folding a short ASCII heap key back to the
/// SSO bits its content would have had makes the cache key depend on the
/// STRING, not on which representation happened to reach this call.
///
/// **Content-only, never an address.** A key that does not fit the inline form
/// is rejected rather than cached under its pointer bits. Caching an address
/// would be unsound here in a way the per-hit `dyn_ic_try_store` revalidation
/// cannot catch: that check confirms the receiver still has the cached SHAPE,
/// not that the cached SLOT belongs to this KEY. So a long key at address `A`
/// could be primed, evicted from the intern table (which is direct-mapped and
/// evicts on collision), collected, and its address recycled by an unrelated
/// string — whose write would then hit the stale entry and overwrite the wrong
/// slot. Restricting the table to content-derived bits removes that class
/// entirely: an entry names a STRING VALUE, holds no pointer, and so needs
/// neither GC roots nor eviction epochs. Longer keys simply keep the existing
/// (unchanged) miss path.
#[inline(always)]
fn stub_key_bits(key: f64) -> Option<u64> {
    let bits = key.to_bits();
    if crate::value::JSValue::from_bits(bits).is_short_string() {
        // Already an SSO immediate: pure content.
        return Some(bits);
    }
    if (bits & !POINTER_MASK) != crate::value::STRING_TAG {
        return None;
    }
    let ptr = (bits & POINTER_MASK) as *const crate::StringHeader;
    unsafe { crate::string::short_ascii_sso_bits(ptr) }
}

/// Way index for a `(shape_token, key_bits)` pair.
///
/// Multiplicative (Fibonacci) mixing, taking the TOP bits of the product —
/// not the low bits of an XOR. The distinction is the whole ballgame for
/// string keys: an SSO key's bits are its bytes in little-endian order, so a
/// family like `"k0".."k499"` shares its first byte and varies in bytes 2-4.
/// Indexing on the low bits therefore collapses hundreds of distinct keys onto
/// a handful of ways, which evict each other on every write — measured as
/// 1.17M of 1.19M probes missing with the way occupied by a DIFFERENT key at
/// the SAME shape token, while the table sat 99% empty.
#[inline(always)]
fn write_stub_way(token: u64, key_bits: u64) -> usize {
    let h = (token ^ key_bits).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    ((h >> 40) as usize) & (WRITE_STUB_BUCKETS - 1)
}

#[inline(always)]
fn write_stub_probe(token: u64, key_bits: u64) -> Option<u32> {
    WRITE_STUB.with(|t| {
        let bucket = &t[write_stub_way(token, key_bits)];
        for way in bucket.iter() {
            let (tok, kb, slot) = way.get();
            if tok == token && kb == key_bits && tok != 0 {
                return Some(slot as u32);
            }
        }
        None
    })
}

#[inline(always)]
fn write_stub_insert(token: u64, key_bits: u64, slot: u32) {
    if token == 0 || key_bits == 0 {
        return;
    }
    WRITE_STUB.with(|t| {
        let bucket = &t[write_stub_way(token, key_bits)];
        let entry = (token, key_bits, slot as u64);
        // Refresh this key's own way if it is already resident, then fill an
        // empty one, and only otherwise evict — shifting way 0 down so the
        // most recently primed key survives.
        for way in bucket.iter() {
            let (tok, kb, _) = way.get();
            if tok == token && kb == key_bits {
                way.set(entry);
                return;
            }
        }
        for way in bucket.iter() {
            if way.get().0 == 0 {
                way.set(entry);
                return;
            }
        }
        for i in (1..WRITE_STUB_ASSOC).rev() {
            bucket[i].set(bucket[i - 1].get());
        }
        bucket[0].set(entry);
    });
}

/// Slot-word bit marking a cached slot as living in the OVERFLOW store.
///
/// Set at prime time, where the full receiver validation runs and the
/// inline-vs-overflow verdict is known. `object_kind`,
/// `live_inline_slot_count` and `logical_key_count` are immutable per ordinary
/// shape id. #9064's private stable-tombstone state is the exception: cached
/// slots remain placed, but a deleted one contains `TAG_HOLE` and must miss.
/// Otherwise a hit whose token matches the receiver's
/// CURRENT stamp has already proved everything the prime proved about kind
/// and bounds. What remains mutable per object is the GC header — type,
/// forwarded, and the blocking flags `Object.freeze`-family operations set —
/// and the hit still checks those on every store.
/// #9287: validate-and-store for a constant-key IC hit whose slot word
/// carries [`IC_SLOT_OVERFLOW_BIT`]. The emitted inline path has already
/// matched the receiver's shape token against the cache, but this helper
/// re-validates everything through [`dyn_ic_try_store`] anyway — the checks
/// are ~10 instructions, and reusing the dynamic IC's audited path means the
/// overflow store (spill buffer, stable-tombstone hole check, barriers,
/// layout notes) has exactly one implementation. Returns 1 and performs the
/// store on success; returns 0 without side effects when validation fails,
/// and the caller falls back to `js_put_value_set_ic_miss`.
#[no_mangle]
pub extern "C" fn js_put_value_set_ic_overflow_store(
    target: f64,
    token: i64,
    slot: i32,
    value: f64,
) -> i32 {
    unsafe {
        match dyn_ic_try_store(target, token as u64, slot as u32, value) {
            Some(_) => 1,
            None => 0,
        }
    }
}

pub(crate) const IC_SLOT_OVERFLOW_BIT: u32 = 1 << 30;

/// Validated fast store for a cached `(token, slot)` hit. Header checks and
/// the token compare read the receiver's LIVE state; everything shape-derived
/// was proved at prime time and is immutable per id (see
/// [`IC_SLOT_OVERFLOW_BIT`]). A stale entry misses; it cannot store wrongly.
#[inline]
unsafe fn dyn_ic_try_store(target: f64, token: u64, slot: u32, value: f64) -> Option<f64> {
    let target_bits = target.to_bits();
    if (target_bits & !POINTER_MASK) != POINTER_TAG {
        return None;
    }
    let obj_addr = (target_bits & POINTER_MASK) as usize;
    let gc_header = crate::value::addr_class::try_read_gc_header(obj_addr)?;
    const BLOCKING_FLAGS: u16 = crate::gc::OBJ_FLAG_FROZEN
        | crate::gc::OBJ_FLAG_SEALED
        | crate::gc::OBJ_FLAG_NO_EXTEND
        | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
        | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO
        | crate::gc::GC_OBJ_TYPED_LAYOUT_INTACT;
    if gc_header.obj_type != crate::gc::GC_TYPE_OBJECT
        || gc_header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || gc_header._reserved & BLOCKING_FLAGS != 0
    {
        return None;
    }
    let obj = obj_addr as *mut crate::ObjectHeader;
    // Token compare against the receiver's CURRENT stamp. On a match, the
    // shape id is the one every prime-time check ran under — receiver kind,
    // class-object exclusion, and the slot's inline/overflow placement (now
    // carried in the slot word) — so none of it is re-derived here. This
    // took `write_fast_path_receiver_kind_ok` (6.8%), `shape_object_kind_by_id`
    // (5.1%) and the descriptor bound fetch (4.4%) off every hit of the
    // combined overwrite loop.
    let stamp = crate::object::shapes::object_shape_stamp(obj);
    if (crate::object::shapes::PIC_ID_TOKEN_BIT | stamp as u64) != token {
        return None;
    }
    if gc_header._reserved & crate::gc::OBJ_FLAG_STABLE_TOMBSTONES != 0 {
        let old_bits = if slot & IC_SLOT_OVERFLOW_BIT != 0 {
            crate::object::overflow_get(obj_addr, (slot & !IC_SLOT_OVERFLOW_BIT) as usize)?
        } else {
            let fields_ptr = (obj as *const u8).add(std::mem::size_of::<crate::ObjectHeader>())
                as *const crate::JSValue;
            (*fields_ptr.add(slot as usize)).bits()
        };
        if old_bits == crate::value::TAG_HOLE {
            return None;
        }
    }
    if slot & IC_SLOT_OVERFLOW_BIT != 0 {
        crate::object::overflow_set(
            obj_addr,
            (slot & !IC_SLOT_OVERFLOW_BIT) as usize,
            value.to_bits(),
        );
        return Some(value);
    }
    crate::object::store_object_field_slot(obj, slot as usize, value.to_bits());
    Some(value)
}

// #6088-style keep: codegen emits the only call; a whole-program bitcode
// link would otherwise dead-strip the IC entry.
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_PUT_VALUE_SET_DYN_IC: extern "C" fn(*mut [i64; 8], f64, f64, f64, i32) -> f64 =
    js_put_value_set_dyn_ic;

/// Full `[[Set]]` semantics + prime. Mirrors the static PIC's prime policy
/// (only a successful ordinary own-data overwrite on a class-tagged,
/// unblocked, shape-shared receiver may prime), with the key resolved from
/// its VALUE (SSO or heap) instead of requiring an interned pointer.
#[no_mangle]
pub extern "C" fn js_put_value_set_dyn_ic_miss(
    cache: *mut [i64; 8],
    target: f64,
    key: f64,
    value: f64,
    strict: i32,
) -> f64 {
    // The compiled inline IC (`lower_put_value_dyn_ic_inline`) walks its three
    // ways in GENERATED code and calls straight here on a way miss — it never
    // enters `js_put_value_set_dyn_ic` above. A rotating-key site therefore
    // lands here on every write, so the megamorphic stub probe must sit at
    // THIS entry to be on that path at all. `c[0]` holds the site's primed
    // shape token; `dyn_ic_try_store` validates it against the receiver's
    // live state, so a stale site token or stub entry misses into the full
    // walk below instead of storing wrongly. No allocation precedes the
    // probe — the handle scope opens only after this fails.
    if !cache.is_null() {
        let c = unsafe { &*cache };
        let token = c[0] as u64;
        let key_bits = key.to_bits();
        if token != 0 && key_bits != 0 {
            if let Some(slot) = stub_key_bits(key).and_then(|kb| write_stub_probe(token, kb)) {
                if let Some(ret) = unsafe { dyn_ic_try_store(target, token, slot, value) } {
                    return ret;
                }
            }
        }
    }

    // Stable-tombstone re-adds own their complete rooted transition. Probe
    // before constructing this miss handler's general-purpose handle scope so
    // the hot successful lane does not root target/key/value twice.
    let target_bits = target.to_bits();
    if (target_bits & !POINTER_MASK) == POINTER_TAG {
        let obj = (target_bits & POINTER_MASK) as *mut crate::ObjectHeader;
        let stub_bits = stub_key_bits(key);
        if let Some((slot, obj, value)) = crate::object::try_readd_stable_tombstone(obj, key, value)
        {
            let token = crate::object::shapes::PIC_ID_TOKEN_BIT
                | unsafe { crate::object::shapes::object_shape_stamp(obj) } as u64;
            if let Some(kb) = stub_bits {
                write_stub_insert(token, kb, slot);
                crate::object::read_stub::read_stub_insert(token, kb, slot);
            }
            return value;
        }
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_nanbox_f64(target);
    let key_handle = scope.root_nanbox_f64(key);
    let value_handle = scope.root_nanbox_f64(value);

    // A computed write site that constructs the same plain-object shape over
    // and over changes receiver shape after every append, so the per-site
    // overwrite IC above cannot hit. Reuse the runtime's global transition
    // lattice before entering the full `[[Set]]` walk: the helper accepts
    // only a previously learned transition on a class-id-zero ordinary object
    // and rejects every receiver with exotic, descriptor, frozen/sealed, or
    // prototype-interceptor semantics. It roots internally while interning
    // the key, and these outer handles preserve all three operands across
    // that nested allocation/collection point.
    let target_now = target_handle.get_nanbox_f64();
    let key_now = key_handle.get_nanbox_f64();
    if (target_now.to_bits() & !POINTER_MASK) == POINTER_TAG {
        let obj = (target_now.to_bits() & POINTER_MASK) as *mut crate::ObjectHeader;
        if (key_now.to_bits() & !POINTER_MASK) == crate::value::STRING_TAG {
            let key_ptr = (key_now.to_bits() & POINTER_MASK) as *const crate::StringHeader;
            if crate::object::object_set_field_by_name_transition_only_fast(
                obj,
                key_ptr,
                value_handle.get_nanbox_f64(),
            ) != 0
            {
                return value_handle.get_nanbox_f64();
            }
        }
    }

    let result = js_put_value_set(
        target_handle.get_nanbox_f64(),
        key_handle.get_nanbox_f64(),
        value_handle.get_nanbox_f64(),
        target_handle.get_nanbox_f64(),
        strict,
    );
    if cache.is_null() {
        return result;
    }
    unsafe {
        let target = target_handle.get_nanbox_f64();
        let key = key_handle.get_nanbox_f64();
        let target_bits = target.to_bits();
        if (target_bits & !POINTER_MASK) != POINTER_TAG {
            return result;
        }
        let obj_addr = (target_bits & POINTER_MASK) as usize;
        let Some(gc_header) = crate::value::addr_class::try_read_gc_header(obj_addr) else {
            return result;
        };
        const BLOCKING_FLAGS: u16 = crate::gc::OBJ_FLAG_FROZEN
            | crate::gc::OBJ_FLAG_SEALED
            | crate::gc::OBJ_FLAG_NO_EXTEND
            | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
            | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO
            | crate::gc::GC_OBJ_TYPED_LAYOUT_INTACT;
        if gc_header.obj_type != crate::gc::GC_TYPE_OBJECT
            || gc_header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            || gc_header._reserved & BLOCKING_FLAGS != 0
        {
            return result;
        }
        let obj = obj_addr as *mut crate::ObjectHeader;
        if !crate::object::object_is_regular(obj)
            || !write_fast_path_receiver_kind_ok(obj, gc_header._reserved)
            || crate::array::object_prototype_addr_matches(obj_addr)
        {
            return result;
        }
        let Some(shape) = crate::object::shapes::object_shape_descriptor(obj) else {
            return result;
        };
        let keys = shape.keys as usize as *mut crate::array::ArrayHeader;
        if keys.is_null() || (keys as u64) >> 48 != 0 {
            return result;
        }
        let Some(keys_gc) = crate::value::addr_class::try_read_gc_header(keys as usize) else {
            return result;
        };
        if keys_gc.obj_type != crate::gc::GC_TYPE_ARRAY
            || keys_gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return result;
        }
        // Resolve the key's own-field index by VALUE (SSO or heap bytes).
        let mut key_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let key_jsval = crate::value::JSValue::from_bits(key.to_bits());
        let Some(key_bytes) = crate::string::js_string_key_bytes(key_jsval, &mut key_buf) else {
            return result;
        };
        let key_count = shape.logical_key_count as usize;
        if key_count > 4096 {
            return result;
        }
        let own_idx = crate::object::keys_find_slot_by_bytes(keys, key_count as u32, key_bytes);
        let Some(idx) = own_idx else {
            return result;
        };
        // The descriptor above already proves this stamp is live, so the
        // token comes from the header word rather than from a second full
        // lookup-and-copy of the same id (see `dyn_ic_try_store`).
        let shape_token = crate::object::shapes::PIC_ID_TOKEN_BIT
            | crate::object::shapes::object_shape_stamp(obj) as u64;
        let key_bits = key.to_bits() as i64;
        // Preserve the empty-way sentinel invariant: never prime bits 0
        // (only the JS number 0 has them, and numeric keys cannot prime
        // anyway — the byte resolver above only accepts strings).
        if key_bits == 0 {
            return result;
        }
        // Feed the megamorphic stub BEFORE the inline-slot bail below: a wide
        // object keeps every data property in an overflow slot, so gating the
        // stub on the inline region would starve it for exactly the receivers
        // it exists for. The slot word carries the inline/overflow verdict
        // (IC_SLOT_OVERFLOW_BIT), decided HERE where the descriptor is in
        // hand, so the hit never re-fetches the bound — the verdict is a
        // fact of the shape id the token pins.
        let alloc_limit = shape.live_inline_slot_count;
        if let Some(kb) = stub_key_bits(key) {
            if idx < alloc_limit {
                write_stub_insert(shape_token, kb, idx);
            } else if idx < shape.logical_key_count {
                // Overflow: bound-checked against the key count at prime,
                // which the hit-time token match preserves.
                let slot = idx | IC_SLOT_OVERFLOW_BIT;
                write_stub_insert(shape_token, kb, slot);
            }
        }
        if idx >= alloc_limit {
            return result;
        }
        let c = &mut *cache;
        if c[0] as u64 != shape_token {
            // New shape at this site: restart the way set.
            *c = [0; 8];
        }
        // MRU insert: shift ways down, newest first. A duplicate key way is
        // moved to the front rather than duplicated.
        let mut ways: Vec<(i64, i64)> = (0..DYN_IC_WAYS)
            .map(|w| (c[1 + w * 2], c[2 + w * 2]))
            .filter(|(k, _)| *k != 0 && *k != key_bits)
            .collect();
        ways.insert(0, (key_bits, idx as i64));
        ways.truncate(DYN_IC_WAYS);
        for (w, (k, sl)) in ways.iter().enumerate() {
            c[1 + w * 2] = *k;
            c[2 + w * 2] = *sl;
        }
        // (The megamorphic stub was already fed above, before the inline-slot
        // bail — overflow slots feed it too.)
        // Token last: a zero or stale token cannot hit until it matches the
        // receiver's current discriminated shape.
        c[0] = shape_token as i64;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_ic_miss_reuses_learned_anon_shape_transition_only() {
        let _lock = crate::gc::global_side_table_test_lock();
        crate::object::test_reset_transition_fast_hits();
        crate::object::shapes::test_reset_cached_transition_stamps();

        // Longer than the SSO limit so this exercises the heap-string arm
        // admitted above. A unique name prevents another test's transition
        // edge from turning the first write into a hit.
        let key_bytes = b"__dyn_ic_transition_fast_test_8783";
        let key_ptr =
            crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
        let scope = crate::gc::RuntimeHandleScope::new();
        let key_handle = scope.root_nanbox_f64(crate::value::js_nanbox_string(key_ptr as i64));
        const ANON_CLASS_ID: u32 = 0x8783_1001;
        const USER_CLASS_ID: u32 = 0x8783_1002;
        unsafe { crate::object::js_register_anon_shape_class_id(ANON_CLASS_ID) };
        let first_handle = scope.root_raw_mut_ptr(crate::object::js_object_alloc(ANON_CLASS_ID, 0));
        let second_handle =
            scope.root_raw_mut_ptr(crate::object::js_object_alloc(ANON_CLASS_ID, 0));
        let user_class_handle =
            scope.root_raw_mut_ptr(crate::object::js_object_alloc(USER_CLASS_ID, 0));
        let mut cache = [0i64; 8];

        let first = first_handle.get_raw_mut_ptr::<crate::ObjectHeader>();
        let learned_predecessor = unsafe { crate::object::shapes::object_shape_stamp(first) };
        assert_eq!(
            js_put_value_set_dyn_ic_miss(
                &mut cache,
                crate::value::js_nanbox_pointer(first as i64),
                key_handle.get_nanbox_f64(),
                11.0,
                1,
            ),
            11.0
        );
        assert_eq!(crate::object::test_transition_fast_hits(), 0);
        let first = first_handle.get_raw_mut_ptr::<crate::ObjectHeader>();
        let learned_target = unsafe { crate::object::shapes::object_shape_stamp(first) };
        assert_ne!(learned_target, learned_predecessor);
        let key_ptr =
            (key_handle.get_nanbox_f64().to_bits() & POINTER_MASK) as *const crate::StringHeader;
        assert_eq!(
            crate::object::js_object_get_field_by_name(first, key_ptr).as_number(),
            11.0
        );
        // The first lazy runtime operation can perform unrelated cached
        // transitions while bootstrapping builtins. Isolate the sibling edge
        // below rather than treating that process-wide activity as test state.
        let second = second_handle.get_raw_mut_ptr::<crate::ObjectHeader>();
        crate::object::shapes::test_watch_cached_transition_stamps(second as usize);
        assert_eq!(
            unsafe { crate::object::shapes::object_shape_stamp(second) },
            learned_predecessor,
            "the sibling must begin at the learned predecessor ShapeId"
        );
        assert_eq!(
            js_put_value_set_dyn_ic_miss(
                &mut cache,
                crate::value::js_nanbox_pointer(second as i64),
                key_handle.get_nanbox_f64(),
                29.0,
                1,
            ),
            29.0
        );
        assert_eq!(crate::object::test_transition_fast_hits(), 1);
        assert_eq!(crate::object::shapes::test_cached_transition_stamps(), 1);
        let second = second_handle.get_raw_mut_ptr::<crate::ObjectHeader>();
        assert_eq!(
            unsafe { crate::object::shapes::object_shape_stamp(second) },
            learned_target,
            "the hit must stamp the exact learned successor ShapeId"
        );
        let key_ptr =
            (key_handle.get_nanbox_f64().to_bits() & POINTER_MASK) as *const crate::StringHeader;
        assert_eq!(
            crate::object::js_object_get_field_by_name(second, key_ptr).as_number(),
            29.0
        );

        // A real user class can have inherited setters and must not borrow the
        // anonymous shape's cached edge even though both receivers begin with
        // the same null keys-array identity.
        let user_class = user_class_handle.get_raw_mut_ptr::<crate::ObjectHeader>();
        assert_eq!(
            js_put_value_set_dyn_ic_miss(
                &mut cache,
                crate::value::js_nanbox_pointer(user_class as i64),
                key_handle.get_nanbox_f64(),
                47.0,
                1,
            ),
            47.0
        );
        assert_eq!(crate::object::test_transition_fast_hits(), 1);
        let user_class = user_class_handle.get_raw_mut_ptr::<crate::ObjectHeader>();
        let key_ptr =
            (key_handle.get_nanbox_f64().to_bits() & POINTER_MASK) as *const crate::StringHeader;
        assert_eq!(
            crate::object::js_object_get_field_by_name(user_class, key_ptr).as_number(),
            47.0
        );
    }
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
fn object_array_numeric_write_slots(
    array: f64,
    keys: &[f64],
    receiver_start: u32,
    receiver_end: u32,
) -> Option<[u16; 4]> {
    // Reuse the process gate js_gc_init disables for typed-feedback tracing,
    // typed-layout verification, and the explicit inline-field escape hatch.
    // This loop bypasses the same observations/checks as the class-field
    // inline clone and therefore must honor the identical gate.
    if keys.is_empty() || keys.len() > 4 || !crate::object::class_field_inline_guard_enabled() {
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
    // #6812 dynamic-bound loops (`i < arr.length`): the u32::MAX sentinel
    // resolves to the VALIDATED array's own length — after the array-kind
    // checks above, before the 16M cap below, so the emitter's fast nest
    // (which loads the same header length after guard-ok) can never outrun
    // the proven prefix. An empty array is not worth the fast path.
    let receiver_end = if receiver_end == u32::MAX {
        length
    } else {
        receiver_end
    };
    if receiver_start >= receiver_end {
        trace_object_array_numeric_write_rejection("empty or reversed receiver range");
        return None;
    }
    if length > 16_000_000 || capacity > 16_000_000 || length > capacity || receiver_end > length {
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
        u32,
        *mut crate::array::ArrayHeader,
        u32,
        u32,
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
        if !write_fast_path_receiver_kind_ok(obj, gc._reserved) {
            return None;
        }
        let shape = crate::object::shapes::object_shape_descriptor(obj)?;
        if shape.object_kind != crate::object::shapes::ShapeObjectKind::Ordinary {
            return None;
        }
        let shape_id = crate::object::shapes::object_shape_stamp(obj);
        let keys = shape.keys as usize as *mut crate::array::ArrayHeader;
        if keys.is_null() || (keys as u64) >> 48 != 0 {
            return None;
        }
        let keys_gc = crate::value::addr_class::try_read_gc_header(keys as usize)?;
        if keys_gc.obj_type != crate::gc::GC_TYPE_ARRAY
            || keys_gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return None;
        }
        Some((
            obj,
            shape_id,
            keys,
            shape.logical_key_count,
            shape.live_inline_slot_count,
            gc._reserved,
        ))
    }

    unsafe fn find_slot(
        keys: *mut crate::array::ArrayHeader,
        key_count: u32,
        key: *const crate::StringHeader,
    ) -> Option<u32> {
        if key_count > 4096 {
            return None;
        }
        if let Some(slot) = crate::object::keys_find_slot_by_key_ptr(keys, key_count, key) {
            return Some(slot);
        }
        None
    }

    let elements = unsafe {
        (arr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64
    };
    let first_bits = unsafe { (*elements.add(receiver_start as usize)).to_bits() };
    if first_bits == crate::value::TAG_HOLE {
        trace_object_array_numeric_write_rejection("first receiver is a hole");
        return None;
    }
    let (first, shared_shape_id, shared_keys, shared_key_count, first_limit, first_flags) =
        trace_object_array_numeric_write_stage(
            unsafe { validated_object(first_bits) },
            "first receiver is not an eligible regular shared-shape object",
        )?;
    let mut slots = [0u16; 4];
    for index in 0..keys.len() {
        // `find_slot` caps the shared keys array at 4096 entries, so every
        // non-zero-encoded index fits comfortably in one 16-bit result lane.
        let slot = trace_object_array_numeric_write_stage(
            unsafe { find_slot(shared_keys, shared_key_count, decoded_keys[index]) },
            "target key is absent from the shared shape",
        )?;
        slots[index] = trace_object_array_numeric_write_stage(
            u16::try_from(slot).ok(),
            "target slot cannot be encoded",
        )?;
    }

    // #6812 spill lanes: classify each lane as INLINE (slot within the
    // receiver's inline alloc_limit) or SPILL (slot lives in the
    // object-owned overflow buffer, `meta.spill`). The FIRST receiver fixes
    // each lane's mode; every later receiver must be on the SAME side of its
    // own alloc_limit and pass the same coverage proof, so the emitter's
    // per-lane store sequence (raw inline store vs the meta → spill
    // indirection) is uniform across the whole proven prefix. A spill lane
    // sets bit 15 of its result lane; the caller's `+1` packing cannot carry
    // into it (slot ≤ 4096).
    unsafe fn spill_lane_covers(obj: *const crate::ObjectHeader, slot: u32) -> bool {
        let meta = (*obj).meta;
        if meta.is_null() {
            return false;
        }
        let spill = (*meta).spill as *const crate::array::ArrayHeader;
        if spill.is_null() {
            return false;
        }
        // Mirror the shared-keys validation: a live, non-forwarded plain
        // array whose length high-water covers the slot (a key present in
        // the shape implies its value slot was written, so length > slot —
        // verified rather than assumed).
        let Some(gc) = crate::value::addr_class::try_read_gc_header(spill as usize) else {
            return false;
        };
        if gc.obj_type != crate::gc::GC_TYPE_ARRAY
            || gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return false;
        }
        slot < (*spill).length && slot < (*spill).capacity
    }

    let mut lane_spill = [false; 4];
    for index in 0..keys.len() {
        let slot = u32::from(slots[index]);
        if slot >= first_limit {
            if !unsafe { spill_lane_covers(first, slot) } {
                trace_object_array_numeric_write_rejection(
                    "first receiver target slot is out of bounds",
                );
                return None;
            }
            lane_spill[index] = true;
        }
    }
    if first_flags & crate::gc::GC_OBJ_TYPED_LAYOUT_INTACT != 0 {
        // Typed-intact receivers keep raw-f64 inline invariants; a spill
        // lane on one is unexpected — reject conservatively rather than
        // reason about typed descriptors for out-of-line slots.
        if lane_spill[..keys.len()].iter().any(|s| *s) {
            trace_object_array_numeric_write_rejection(
                "first receiver typed descriptor does not contain every target slot",
            );
            return None;
        }
        if slots[..keys.len()].iter().any(|slot| {
            !crate::gc::layout_typed_accepts_finite_number_slot_for_user(
                first as usize,
                usize::from(*slot),
            )
        }) {
            trace_object_array_numeric_write_rejection(
                "first receiver typed descriptor does not contain every target slot",
            );
            return None;
        }
    }

    for i in (receiver_start as usize + 1)..receiver_end as usize {
        let bits = unsafe { (*elements.add(i)).to_bits() };
        if bits == crate::value::TAG_HOLE {
            trace_object_array_numeric_write_rejection("receiver prefix contains a hole");
            return None;
        }
        let (obj, receiver_shape_id, _object_keys, _object_key_count, limit, flags) =
            trace_object_array_numeric_write_stage(
                unsafe { validated_object(bits) },
                "receiver prefix contains an ineligible object",
            )?;
        if receiver_shape_id != shared_shape_id {
            trace_object_array_numeric_write_rejection(
                "receiver prefix does not share one ShapeId",
            );
            return None;
        }
        for index in 0..keys.len() {
            let slot = u32::from(slots[index]);
            if lane_spill[index] {
                // Mode uniformity: this receiver must ALSO hold the slot in
                // its spill buffer (a wider receiver holding it inline would
                // make the emitter's spill store write the wrong memory).
                if slot < limit || !unsafe { spill_lane_covers(obj, slot) } {
                    trace_object_array_numeric_write_rejection(
                        "receiver prefix contains an out-of-bounds target slot",
                    );
                    return None;
                }
            } else if slot >= limit {
                trace_object_array_numeric_write_rejection(
                    "receiver prefix contains an out-of-bounds target slot",
                );
                return None;
            }
        }
        if flags & crate::gc::GC_OBJ_TYPED_LAYOUT_INTACT != 0 {
            if lane_spill[..keys.len()].iter().any(|s| *s) {
                trace_object_array_numeric_write_rejection(
                    "receiver typed descriptor does not contain every target slot",
                );
                return None;
            }
            if slots[..keys.len()].iter().any(|slot| {
                !crate::gc::layout_typed_accepts_finite_number_slot_for_user(
                    obj as usize,
                    usize::from(*slot),
                )
            }) {
                trace_object_array_numeric_write_rejection(
                    "receiver typed descriptor does not contain every target slot",
                );
                return None;
            }
        }
    }

    for index in 0..keys.len() {
        if lane_spill[index] {
            slots[index] |= 0x8000;
        }
    }
    Some(slots)
}

/// #6812 (w12): preflight for the TABLE-DRIVEN write lane
/// (`o[K[idx]] = v`). Validates the key table (live array, length covers
/// the proven index range, first `required_len` entries are strings),
/// materializes each entry to a stable heap string (SSO elements allocate,
/// so the array/table/keys are rooted across materialization), then runs
/// the SAME receiver-prefix proof as the numeric guard via
/// `object_array_numeric_write_slots` — spill lanes included. On success,
/// writes each resolved lane into `out_lanes` in the emitter's encoding
/// (`(slot + 1) | spill_flag`) and returns 1.
#[no_mangle]
pub extern "C" fn js_object_array_keytable_write_guard(
    array: f64,
    key_table: f64,
    required_len: u32,
    receiver_count: u32,
    out_lanes: *mut i64,
) -> i32 {
    if !(1..=4).contains(&required_len) || out_lanes.is_null() {
        return 0;
    }
    let kt_bits = key_table.to_bits();
    if (kt_bits & !POINTER_MASK) != POINTER_TAG {
        return 0;
    }
    let kt_addr = (kt_bits & POINTER_MASK) as usize;
    let Some(kt_gc) = (unsafe { crate::value::addr_class::try_read_gc_header(kt_addr) }) else {
        return 0;
    };
    if kt_gc.obj_type != crate::gc::GC_TYPE_ARRAY
        || kt_gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return 0;
    }
    if unsafe { (*(kt_addr as *const crate::array::ArrayHeader)).length } < required_len {
        return 0;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let array_handle = scope.root_nanbox_f64(array);
    let table_handle = scope.root_nanbox_f64(key_table);
    let mut key_handles = Vec::with_capacity(required_len as usize);
    for i in 0..required_len {
        let table_now = table_handle.get_nanbox_f64();
        let table_ptr = (table_now.to_bits() & POINTER_MASK) as *const crate::array::ArrayHeader;
        let elem = crate::array::js_array_get(table_ptr, i);
        let elem_f64 = f64::from_bits(elem.bits());
        // Stable heap pointer for SSO/heap strings alike; may allocate, so
        // every prior key stays rooted and the table re-reads per element.
        let heap_ptr = crate::value::js_get_string_pointer_unified(elem_f64);
        if heap_ptr == 0 {
            return 0;
        }
        let boxed = f64::from_bits(crate::value::js_nanbox_string(heap_ptr).to_bits());
        key_handles.push(scope.root_nanbox_f64(boxed));
    }
    let keys: Vec<f64> = key_handles.iter().map(|h| h.get_nanbox_f64()).collect();
    let Some(slots) =
        object_array_numeric_write_slots(array_handle.get_nanbox_f64(), &keys, 0, receiver_count)
    else {
        return 0;
    };
    for i in 0..required_len as usize {
        let lane = slots[i];
        let low = (lane & 0x7FFF) as i64 + 1;
        let flag = (lane & 0x8000) as i64;
        unsafe {
            *out_lanes.add(i) = low | flag;
        }
    }
    1
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
        object_array_numeric_write_slots(array, &keys[..field_count as usize], 0, receiver_count)
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

/// Range variant for a constant non-zero inner loop start. The result has the
/// same packed-lane ABI as [`js_object_array_numeric_write_guard`], but proves
/// exactly `receiver_start..receiver_end`; generated code retains the array
/// base pointer and uses the original absolute element index.
#[no_mangle]
pub extern "C" fn js_object_array_numeric_write_range_guard(
    array: f64,
    key_1: f64,
    key_2: f64,
    key_3: f64,
    key_4: f64,
    field_count: u32,
    receiver_start: u32,
    receiver_end: u32,
) -> u64 {
    if !(1..=4).contains(&field_count) {
        return 0;
    }
    let keys = [key_1, key_2, key_3, key_4];
    let Some(slots) = object_array_numeric_write_slots(
        array,
        &keys[..field_count as usize],
        receiver_start,
        receiver_end,
    ) else {
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
    let Some(slots) = object_array_numeric_write_slots(array, &[key_1, key_2], 0, receiver_count)
    else {
        return 0;
    };
    (u64::from(slots[1]) + 1) << 32 | (u64::from(slots[0]) + 1)
}
