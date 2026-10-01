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

/// Parse a property name as a canonical array index (ECMA-262 CanonicalNumeric
/// IndexString restricted to non-negative integers `< 2^32-1`): no leading
/// zeros, round-trips through `to_string`. Used to recognise the in-range code-
/// unit indices of a boxed-String `Object.assign` target.
fn assign_canonical_index(name: &str) -> Option<u32> {
    if name.is_empty() || (name.len() > 1 && name.as_bytes()[0] == b'0') {
        return None;
    }
    let value = name.parse::<u32>().ok()?;
    if value == u32::MAX || value.to_string() != name {
        return None;
    }
    Some(value)
}

/// Spec `Set(to, key, value, true)` inside `Object.assign` uses the strict
/// receiver, so a write that the ordinary `[[Set]]` would reject throws a
/// `TypeError`. Perry's `js_object_set_field_by_name` silently no-ops those
/// cases, so detect them up front: a non-writable existing own data property,
/// an accessor own property with no setter, or a new property on a
/// non-extensible target. Throws when the write must fail.
unsafe fn object_assign_throw_if_set_rejected(
    target: *mut ObjectHeader,
    key_ptr: *const crate::StringHeader,
    name: &str,
) {
    if target.is_null() || (target as usize) <= 0x10000 {
        return;
    }
    // A boxed String primitive target — `Object.assign('abc', src)` does
    // `ToObject('abc')` — exposes its code units as non-writable, non-
    // configurable own index properties ("0".."len-1"), which aren't stored in
    // `keys_array`. A strict `Set` to an in-range index must throw, so detect it
    // before the keys_array-based checks treat the index as a writable new
    // property (test262 Object/assign/assignment-to-readonly-property-of-target
    // -must-throw-a-typeerror-exception).
    if let Some(idx) = assign_canonical_index(name) {
        let target_f64 = f64::from_bits(JSValue::pointer(target as *mut u8).bits());
        if crate::builtins::boxed_primitive_to_string_tag(target_f64) == Some("String") {
            if let Some((_, payload)) = crate::builtins::boxed_primitive_payload(target_f64) {
                let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                if let Some((ptr, blen)) =
                    crate::string::str_bytes_from_jsvalue(payload, &mut scratch)
                {
                    let len = if ptr.is_null() {
                        0
                    } else {
                        crate::string::compute_utf16_len(ptr, blen)
                    };
                    if idx < len {
                        throw_object_assign_readonly(name);
                    }
                }
            }
        }
    }
    // Accessor own property: a setter must exist, else the write fails. Check
    // this BEFORE `own_key_present`: an accessor-only property (`{ set foo(){} }`)
    // lives in the accessor side table and may have no `keys_array` entry, so
    // `own_key_present` can report it absent — which on a frozen/non-extensible
    // target would mis-classify the setter call as a forbidden new-property add
    // (test262 assign/target-is-frozen-accessor-property-set-succeeds).
    if let Some(acc) = super::get_accessor_descriptor(target as usize, name) {
        if acc.set == 0 {
            throw_object_assign_readonly(name);
        }
        return;
    }
    let exists = own_key_present(target, key_ptr);
    if exists {
        // Data own property: must be writable.
        if let Some(attrs) = super::get_property_attrs(target as usize, name) {
            if !attrs.writable() {
                throw_object_assign_readonly(name);
            }
        }
        return;
    }
    // New property: target must be extensible.
    let gc = gc_header_for(target);
    if (*gc)._reserved & crate::gc::OBJ_FLAG_NO_EXTEND != 0 {
        throw_object_assign_readonly(name);
    }
}

fn throw_object_assign_readonly(name: &str) -> ! {
    throw_object_type_error_with_suffix(
        "Cannot assign to read only property '",
        &format!("{name}' of object '#<Object>'"),
    )
}

/// Strict `Set(to, sym, value, true)` rejection check for a symbol-keyed
/// `Object.assign` write: a non-writable existing symbol data property, an
/// accessor symbol property with no setter, or a new symbol property on a
/// non-extensible target each make the write fail, which under throwing `Set`
/// semantics is a `TypeError`. The string-keyed counterpart is
/// `object_assign_throw_if_set_rejected`.
unsafe fn object_assign_throw_if_symbol_set_rejected(target: *mut ObjectHeader, sym_ptr: usize) {
    let owner = target as usize;
    let existing = crate::symbol::symbol_property_root_bits(owner, sym_ptr).is_some()
        || crate::symbol::symbol_accessor_descriptor_bits(owner, sym_ptr).is_some();
    if existing {
        if let Some((_get, set)) = crate::symbol::symbol_accessor_descriptor_bits(owner, sym_ptr) {
            if set == 0 {
                throw_object_assign_readonly("Symbol()");
            }
        } else if let Some(attrs) = crate::symbol::get_symbol_property_attrs(owner, sym_ptr) {
            if !attrs.writable() {
                throw_object_assign_readonly("Symbol()");
            }
        }
    } else {
        let gc = gc_header_for(target);
        if (*gc)._reserved & crate::gc::OBJ_FLAG_NO_EXTEND != 0 {
            throw_object_assign_readonly("Symbol()");
        }
    }
}

unsafe fn object_assign_set_string_key(
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
    if target_is_array {
        // Routes integer-index keys to array element-set (extending length);
        // non-numeric keys fall back to the object setter.
        crate::array::js_array_set_string_key(
            target as *mut crate::array::ArrayHeader,
            key_ptr,
            value_f64,
        );
    } else {
        // Strict `Set` semantics: reject (throw) a write the ordinary `[[Set]]`
        // would silently drop.
        let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        if let Some(name_bytes) = crate::string::js_string_key_bytes(
            crate::value::JSValue::string_ptr(key_ptr as *mut _),
            &mut sso,
        ) {
            if let Ok(name) = std::str::from_utf8(name_bytes) {
                object_assign_throw_if_set_rejected(target, key_ptr, name);
            }
        }
        js_object_set_field_by_name(target, key_ptr, value_f64);
    }
}

unsafe fn object_assign_string_source(
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

/// Copy a Proxy source's own enumerable properties onto `target`, driving the
/// proxy's `ownKeys` / `getOwnPropertyDescriptor` / `get` traps in spec order.
/// Any trap that throws longjmps straight past this frame to the caller's
/// `try`/`catch`, which is exactly the abrupt-completion propagation
/// `Object.assign` requires.
unsafe fn object_assign_proxy_source(
    target: *mut ObjectHeader,
    target_is_array: bool,
    source_f64: f64,
) {
    // `[[OwnPropertyKeys]]` — fires the ownKeys trap (throw propagates).
    let keys_arr = crate::proxy::js_proxy_own_keys(source_f64);
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let tgt_h = scope.root_raw_mut_ptr(target);
    let keys_h = scope.root_raw_const_ptr(arr);
    let source_h = scope.root_nanbox_f64(source_f64);
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
        let value_f64 = crate::proxy::js_proxy_get(source_h.get_nanbox_f64(), key_f64);
        let value_h = iter_scope.root_nanbox_f64(value_f64);
        let key_f64 = f64::from_bits(key_h.get_nanbox_u64());
        if key.is_any_string() {
            let key_ptr =
                crate::value::js_get_string_pointer_unified(key_f64) as *const crate::StringHeader;
            if !key_ptr.is_null() {
                tgt_h.with_mut_ptr::<ObjectHeader, _>(|t| {
                    object_assign_set_string_key(
                        t,
                        target_is_array,
                        key_ptr,
                        value_h.get_nanbox_f64(),
                    )
                });
            }
        } else if key.is_pointer() {
            // Strict `Set` semantics for symbol keys, same as the ordinary path.
            let sym_ptr = (key_f64.to_bits() & crate::value::POINTER_MASK) as usize;
            tgt_h.with_mut_ptr::<ObjectHeader, _>(|t| {
                object_assign_throw_if_symbol_set_rejected(t, sym_ptr)
            });
            crate::symbol::js_object_set_symbol_property(
                tgt_h
                    .with_mut_ptr::<ObjectHeader, _>(|t| crate::value::js_nanbox_pointer(t as i64)),
                key_f64,
                value_h.get_nanbox_f64(),
            );
        }
    }
}

/// Per spec, undefined/null target throws TypeError. Non-object sources
/// are skipped except string primitives, which expose enumerable index
/// properties (`Object.assign({}, "ab") -> {0:"a",1:"b"}`).
#[no_mangle]
pub unsafe extern "C" fn js_object_assign_one(target_f64: f64, source_f64: f64) -> f64 {
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
        object_assign_string_source(target, target_is_array, source_f64);
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
        object_assign_proxy_source(target, target_is_array, source_f64);
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
                object_assign_set_string_key(t, target_is_array, key_ptr, value_h.get_nanbox_f64())
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
                object_assign_set_string_key(t, target_is_array, key_ptr, value)
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
                    object_assign_set_string_key(t, target_is_array, k, val_h.get_nanbox_f64())
                })
            });
        }
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
                // Private elements (`#x`) live in a class instance's keys_array
                // but are never copied by Object.assign / object spread.
                if crate::object::instance_private_key_hidden(src, key_val) {
                    continue;
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
                        object_assign_set_string_key(t, target_is_array, k, val_h.get_nanbox_f64())
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
        // Strict `Set` semantics for symbol-keyed writes too.
        tgt_h.with_mut_ptr::<ObjectHeader, _>(|t| {
            object_assign_throw_if_symbol_set_rejected(t, sym_ptr)
        });
        crate::symbol::js_object_set_symbol_property(
            tgt_h.with_mut_ptr::<ObjectHeader, _>(|t| crate::value::js_nanbox_pointer(t as i64)),
            sym_h.get_nanbox_f64(),
            value_h.get_nanbox_f64(),
        );
    }

    // The target may have moved under any of the getters above; hand the caller
    // the post-collection address, not the `target_f64` captured on entry. The
    // native-module arm already does this; the main path did not, so `acc` in a
    // chained `Object.assign(t, a, b)` threaded a from-space pointer into the
    // next link.
    tgt_h.with_mut_ptr::<ObjectHeader, _>(|t| crate::value::js_nanbox_pointer(t as i64))
}
