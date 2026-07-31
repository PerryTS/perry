//! Polymorphic numeric-key index accessors: `obj[idx]` reads and writes
//! where `idx` is a number and the receiver type isn't statically narrowed.
//!
//! Dispatches by GC type (array / object / closure / buffer / typed-array)
//! and routes through the appropriate per-kind getter or setter. Closes
//! issue #471 (both read and write sides) — see per-function docs.
//!
//! Split out of `field_get_set.rs` (issue #1103 follow-up). Pure
//! relocation — no logic changes.

use super::*;

/// `obj[key]` READ through a non-canonical (object / exotic) key, with the
/// receiver rooted across the coercion (#6935).
///
/// Both entry points below reach the key coercion only on their
/// NON-canonical-key arms — which is exactly where an object key lands.
/// `ToPropertyKey` then runs a user `Symbol.toPrimitive` / `toString` /
/// `valueOf`, allocates, and can trigger a GC that **evacuates** the receiver.
/// `raw` is a bare `u64` address in a Rust local — not a GC root and not a
/// shadow slot — so it has to be re-read through a handle afterwards.
///
/// #6945 / CodeRabbit: if ToPropertyKey yields a Symbol (e.g. `@@toPrimitive`
/// returns one), route through the symbol side-table — never stringify the
/// Symbol and never treat the Symbol case as "no key" (the old
/// `property_key_string_ptr` null-out silently returned `undefined`).
unsafe fn rooted_property_key_get(raw: u64, idx: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv = scope.root_raw_mut_ptr(raw as *mut ObjectHeader);
    let key = crate::object::js_to_property_key(idx);
    let key_h = scope.root_nanbox_f64(key);
    let key = key_h.get_nanbox_f64();
    let recv_bits =
        crate::value::js_nanbox_pointer(recv.get_raw_const_ptr::<ObjectHeader>() as i64);
    if crate::symbol::js_is_symbol(key) != 0 {
        return crate::symbol::js_object_get_symbol_property(recv_bits, key);
    }
    let key_ptr = crate::value::js_get_string_pointer_unified(key) as *const crate::StringHeader;
    if key_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let key_handle = scope.root_string_ptr(key_ptr);
    let v = js_object_get_field_by_name(
        recv.get_raw_mut_ptr::<ObjectHeader>(),
        key_handle.get_raw_const_ptr::<crate::StringHeader>(),
    );
    f64::from_bits(v.bits())
}

/// `obj[key] = value` WRITE counterpart of [`rooted_property_key_get`].
///
/// This is the corruption half: the coercion sits between the receiver/value
/// arriving and the store, so pre-fix a stale receiver dropped the write onto a
/// forwarding stub and a stale `value` planted a dangling pointer *inside* a
/// live object, outliving the call. Symbol-yielding ToPropertyKey routes to
/// the symbol store (#6945 / CodeRabbit).
unsafe fn rooted_property_key_set(raw: u64, idx: f64, value: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv = scope.root_raw_mut_ptr(raw as *mut ObjectHeader);
    let value_handle = scope.root_nanbox_f64(value);
    let key = crate::object::js_to_property_key(idx);
    let key_h = scope.root_nanbox_f64(key);
    let key = key_h.get_nanbox_f64();
    let value = value_handle.get_nanbox_f64();
    let recv_bits =
        crate::value::js_nanbox_pointer(recv.get_raw_const_ptr::<ObjectHeader>() as i64);
    if crate::symbol::js_is_symbol(key) != 0 {
        crate::symbol::js_object_set_symbol_property(recv_bits, key, value);
        return;
    }
    let key_ptr = crate::value::js_get_string_pointer_unified(key) as *const crate::StringHeader;
    if key_ptr.is_null() {
        return;
    }
    let key_handle = scope.root_string_ptr(key_ptr);
    js_object_set_field_by_name(
        recv.get_raw_mut_ptr::<ObjectHeader>(),
        key_handle.get_raw_const_ptr::<crate::StringHeader>(),
        value,
    );
}

fn numeric_key_u32_index(value: f64) -> Option<u32> {
    let bits = value.to_bits();
    if (bits & crate::value::TAG_MASK) == crate::value::INT32_TAG {
        let index = crate::value::JSValue::from_bits(bits).as_int32();
        return (index >= 0).then_some(index as u32);
    }
    if value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value < u32::MAX as f64 {
        Some(value as u32)
    } else {
        None
    }
}

fn numeric_key_i32_index(value: f64) -> Option<i32> {
    let bits = value.to_bits();
    if (bits & crate::value::TAG_MASK) == crate::value::INT32_TAG {
        let index = crate::value::JSValue::from_bits(bits).as_int32();
        return (index >= 0).then_some(index);
    }
    if value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value <= i32::MAX as f64 {
        Some(value as i32)
    } else {
        None
    }
}

/// Polymorphic numeric-key get: companion of `js_object_set_index_polymorphic`.
/// Reads `obj[idx]` where `idx` is a number and the receiver type isn't
/// statically narrowed. Dispatches by GC type:
///
/// - `GC_TYPE_ARRAY` (and forwarded / lazy variants) → `js_array_get_f64`,
///   which routes through `clean_arr_ptr` for forwarding-chain follow.
/// - `GC_TYPE_OBJECT` / `GC_TYPE_CLOSURE`            → stringify `idx` and
///   delegate to `js_object_get_field_by_name_f64`. JS treats `obj[0]` as
///   `obj["0"]`, so the stringification matches spec semantics.
///
/// Closes #471 (read side): paired with the IndexSet polymorphic fix so
/// `Record<number, T>` stores and reads through the same path. Without
/// this, `constMap[i] = v; constMap[i]` would set via the object setter
/// but read from `obj+8+i*8` (stale ObjectHeader fields), returning
/// garbage f64 values.
#[no_mangle]
pub extern "C" fn js_object_get_index_polymorphic(obj_handle: i64, idx: f64) -> f64 {
    let raw = if (obj_handle as u64) >> 48 >= 0x7FF8 {
        // NaN-boxed: only POINTER_TAG (0x7FFD) and STRING_TAG (0x7FFF) carry a
        // heap pointer in the low 48 bits. INT32 (0x7FFE) is usually a primitive
        // number — BUT a registered **class-ref** is also INT32-tagged, and
        // `C[k]` with a non-string key (object ToPropertyKey, numeric static
        // name, …) reaches this helper from codegen's IndexGet last-resort
        // path. Class refs must NOT be rejected as primitives: route them
        // through `js_dyn_index_get`, which has the dedicated class-ref arm
        // (static methods / CLASS_DYNAMIC_PROPS / ToString key). (#6945)
        // BIGINT (0x7FFA) and the undefined/null/bool tags (0x7FFC) remain
        // primitives — indexing them yields `undefined` per JS. Treating an
        // INT32 *number* payload as a pointer would SIGSEGV (Next.js
        // app-page-turbo: 0xf000f indexed inside a class `get`); only a
        // *registered* class-id takes the class-ref arm.
        match (obj_handle as u64) >> 48 {
            0x7FFD | 0x7FFF => (obj_handle as u64) & 0x0000_FFFF_FFFF_FFFF,
            0x7FFE => {
                let class_id = (obj_handle as u64 & 0xFFFF_FFFF) as u32;
                if class_id != 0 && crate::object::class_registry::is_class_id_registered(class_id)
                {
                    return crate::value::js_dyn_index_get(f64::from_bits(obj_handle as u64), idx);
                }
                return f64::from_bits(crate::value::TAG_UNDEFINED);
            }
            _ => return f64::from_bits(crate::value::TAG_UNDEFINED),
        }
    } else {
        obj_handle as u64
    };
    if raw < 0x1000 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // #5525 fast path: cached typed-array kind lookup + inline load, ahead of
    // the thread-local `typed_array_get_numeric_index` registry dispatch.
    // `typed_array_fast_index_get` returns `Some(value)` for an in-bounds read
    // or `Some(undefined)` for an in-range OOB index, `None` for the BigInt /
    // non-canonical-key cases the slow path still owns.
    if let Some(kind) = crate::typedarray::lookup_typed_array_kind(raw as usize) {
        if let Some(value) = crate::typedarray::typed_array_fast_index_get(raw as usize, kind, idx)
        {
            return value;
        }
    }
    if let Some(value) =
        unsafe { crate::typedarray_props::typed_array_get_numeric_index(raw as usize, idx) }
    {
        return value;
    }
    if crate::buffer::is_registered_buffer(raw as usize) {
        let Some(index) = numeric_key_i32_index(idx) else {
            // A NON-numeric computed key on a Buffer (`buf[k]` where `k` is a
            // method/expando name — mysql2's `MockBuffer` probes
            // `typeof mock[k] === "function"` over `Packet.prototype`'s names).
            // Node's Buffer is an ordinary Uint8Array, so this reads the own
            // property, else the prototype method. Perry returned `undefined`,
            // so the MockBuffer no-op swap never happened and the packet-sizing
            // pass wrote into a zero-length Buffer (RangeError
            // [ERR_OUT_OF_RANGE] at the MySQL handshake).
            if let Some(name) = buffer_key_name(idx) {
                if let Some(v) = crate::buffer::buffer_get_own_prop(raw as usize, &name) {
                    return v;
                }
                if crate::object::buffer_dispatch::is_buffer_method_name(&name) {
                    let bytes = name.as_bytes();
                    return crate::object::js_class_method_bind(
                        crate::value::js_nanbox_pointer(raw as i64),
                        bytes.as_ptr(),
                        bytes.len(),
                    );
                }
            }
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        };
        let byte_val =
            crate::buffer::js_buffer_get(raw as *const crate::buffer::BufferHeader, index);
        return byte_val as f64;
    }
    if crate::typedarray::lookup_typed_array_kind(raw as usize).is_some() {
        return crate::typedarray::js_typed_array_index_get_dynamic(
            raw as *const crate::typedarray::TypedArrayHeader,
            idx,
        );
    }

    // #wall5-render: `obj[idx]` where `obj` is a mis-boxed / non-heap value
    // (e.g. a small bogus pointer like 0xf000f produced upstream) must NOT
    // dereference the GcHeader at `raw-8` — that's a wild read → SIGSEGV. The
    // `raw < 0x1000` guards above are too weak (0xf000f passes). Typed-array /
    // buffer / string receivers were already handled before this point, so a
    // value reaching here that isn't a valid arena/old-gen object pointer is
    // not indexable → `undefined` (matches JS `(5)[0]` etc.). Mirrors the
    // is_closure_ptr heap-range guard (wall #2). Next.js app-page-turbo's
    // `u_i_24_6.get` indexed such a value during the app render → crash.
    if !crate::value::addr_class::is_valid_obj_ptr(raw as *const u8) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let gc_type = unsafe {
        let gc_header_addr = raw.wrapping_sub(crate::gc::GC_HEADER_SIZE as u64) as usize;
        if gc_header_addr < 0x1000 {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        *(gc_header_addr as *const u8)
    };

    if gc_type == crate::gc::GC_TYPE_STRING {
        return crate::string::js_string_index_get(raw as *const crate::StringHeader, idx);
    }

    if let Some(index) = numeric_key_u32_index(idx) {
        if let Some(value) =
            unsafe { arguments_object_get_index(raw as *const ObjectHeader, index) }
        {
            return value;
        }
    }

    if gc_type == crate::gc::GC_TYPE_ARRAY || gc_type == crate::gc::GC_TYPE_LAZY_ARRAY {
        if let Some(index) = numeric_key_u32_index(idx) {
            return crate::array::js_array_get_f64(raw as *mut crate::array::ArrayHeader, index);
        } else {
            return unsafe { rooted_property_key_get(raw, idx) };
        }
    }
    if gc_type == crate::gc::GC_TYPE_OBJECT || gc_type == crate::gc::GC_TYPE_CLOSURE {
        return unsafe { rooted_property_key_get(raw, idx) };
    }
    if crate::set::is_registered_set(raw as usize) || crate::map::is_registered_map(raw as usize) {
        let Some(index) = numeric_key_u32_index(idx) else {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        };
        return crate::array::js_array_get_f64(raw as *mut crate::array::ArrayHeader, index);
    }
    // Buffer / Map / Set / typed-array / unknown — try the array getter
    // (which handles registered buffers + typed arrays via per-kind reads).
    let Some(index) = numeric_key_u32_index(idx) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    crate::array::js_array_get_f64(raw as *mut crate::array::ArrayHeader, index)
}

/// Polymorphic numeric-key set: `obj[idx] = value` where `idx` is a number
/// and the receiver type isn't statically known. Dispatches by GC type:
///
/// - `GC_TYPE_ARRAY` / buffer / typed-array → `js_array_set_f64_extend`,
///   which preserves the array fast-path (forwarding chain follow + grow).
/// - `GC_TYPE_OBJECT` / `GC_TYPE_CLOSURE`   → stringify `idx` and delegate
///   to `js_object_set_field_by_name`. JS treats `obj[0] = v` as `obj["0"] = v`,
///   so the stringification matches spec semantics.
///
/// Closes #471: codegen's previous IndexSet numeric-key fallback emitted
/// an inline `obj+8+idx*8` store. That layout assumes an `ArrayHeader`
/// (8-byte header) but `ObjectHeader` is `size_of::<ObjectHeader>()` bytes (target-dependent) followed by `max(field_count, 8)`
/// inline slots, so any `idMap[i] = v` on an object with i ≥ 7 wrote past
/// the object's allocation, corrupting whatever heap memory followed.
/// In the @perryts/mongodb repro, that memory happened to be doc[0]'s
/// `keys_array` pointer — Object.keys returned a stale string pointer
/// the BSON encoder read as an empty array, emitting empty BSON docs
/// over the wire.
///
/// Receiver layout other than array/object (e.g. raw pointer below the heap
/// or a small handle) silently no-ops, matching the existing tolerant-on-
/// bad-args contract of `js_array_set_f64` / `js_object_set_field_by_name`.
#[no_mangle]
pub extern "C" fn js_object_set_index_polymorphic(obj_handle: i64, idx: f64, value: f64) {
    // `Object.prototype[i] = v` makes the index visible through every array's
    // hole/OOB reads — flip the global flag (cheap compare; see
    // `note_object_prototype_index_write`).
    crate::array::note_object_prototype_index_write(
        (obj_handle as u64 & 0x0000_FFFF_FFFF_FFFF) as usize,
    );
    // Strip NaN-box tags defensively. Codegen calls this with the lower-48
    // bits already extracted via `unbox_to_i64`, but match the convention
    // of every other entry-point so a stray un-stripped caller (or a JIT
    // that forgets the mask) still works.
    let raw = if (obj_handle as u64) >> 48 >= 0x7FF8 {
        (obj_handle as u64) & 0x0000_FFFF_FFFF_FFFF
    } else {
        obj_handle as u64
    };
    // A proxy is a small tagged handle, not a heap header. Rebuild the tag so
    // unknown-typed computed stores use its [[Set]] path.
    let boxed = f64::from_bits(crate::value::POINTER_TAG | raw);
    if crate::proxy::js_proxy_is_proxy(boxed) != 0 {
        crate::proxy::js_proxy_set(boxed, idx, value);
        return;
    }
    if raw < 0x1000 {
        return;
    }
    // #5525 fast path: a cached typed-array kind lookup + inline store, before
    // the thread-local `typed_array_set_numeric_index` registry dispatch
    // (`typed_array_owner_*` → `_tlv_get_addr`) that dominated the bcrypt
    // profile. `typed_array_fast_index_set` returns `true` when it fully handled
    // the write (in-bounds store or spec-correct OOB-canonical drop), `false`
    // for the BigInt / non-canonical-key cases the slow dispatch still owns.
    if let Some(kind) = crate::typedarray::lookup_typed_array_kind(raw as usize) {
        if crate::typedarray::typed_array_fast_index_set(raw as usize, kind, idx, value) {
            return;
        }
    }
    if unsafe { crate::typedarray_props::typed_array_set_numeric_index(raw as usize, idx, value) } {
        return;
    }

    if let Some(index) = numeric_key_u32_index(idx) {
        if unsafe { arguments_object_set_index(raw as *mut ObjectHeader, index, value) } {
            return;
        }
    }

    if crate::buffer::is_registered_buffer(raw as usize) {
        if let Some(index) = numeric_key_i32_index(idx) {
            crate::buffer::js_buffer_set(
                raw as *mut crate::buffer::BufferHeader,
                index,
                value as i32,
            );
            return;
        }
        // NON-numeric computed key: an expando / method override
        // (`mock[k] = noop` — mysql2's MockBuffer neutralizes the write methods
        // of a zero-length Buffer to MEASURE a packet before allocating it).
        // Node's Buffer is an ordinary object, so the own key shadows the
        // prototype method; Perry used to drop the write entirely.
        if let Some(name) = buffer_key_name(idx) {
            crate::buffer::buffer_set_own_prop(raw as usize, &name, value);
        }
        return;
    }
    if crate::typedarray::lookup_typed_array_kind(raw as usize).is_some() {
        crate::typedarray_props::js_typed_array_index_set_dynamic(
            raw as *mut crate::typedarray::TypedArrayHeader,
            idx,
            value,
        );
        return;
    }
    if crate::set::is_registered_set(raw as usize) || crate::map::is_registered_map(raw as usize) {
        return;
    }

    // Read GC type byte (offset 0 of GcHeader, which lives at obj-8).
    let gc_type = unsafe {
        let gc_header_addr = raw.wrapping_sub(crate::gc::GC_HEADER_SIZE as u64) as usize;
        if gc_header_addr < 0x1000 {
            return;
        }
        *(gc_header_addr as *const u8)
    };

    if gc_type == crate::gc::GC_TYPE_ARRAY {
        if let Some(index) = numeric_key_u32_index(idx) {
            // Includes lazy/forwarded — js_array_set_f64_extend's clean_arr_ptr_mut
            // walks the forwarding chain and routes buffers/typed-arrays through
            // their per-kind setter.
            crate::array::js_array_set_f64_extend(
                raw as *mut crate::array::ArrayHeader,
                index,
                value,
            );
            return;
        } else {
            unsafe { rooted_property_key_set(raw, idx, value) };
            return;
        }
    }
    if gc_type == crate::gc::GC_TYPE_OBJECT || gc_type == crate::gc::GC_TYPE_CLOSURE {
        // Stringify the index and route through the object field setter,
        // which handles shape transitions, frozen/sealed/extensible checks,
        // overflow into out-of-line storage, and accessor descriptors.
        unsafe { rooted_property_key_set(raw, idx, value) };
        return;
    }
    // Buffer / typed-array were handled above. Map / Set are collection
    // objects with external storage, not dense ArrayHeader payloads, so numeric
    // writes are no-ops instead of truncating fractional keys into element
    // offsets.
    if let Some(index) = numeric_key_u32_index(idx) {
        crate::array::js_array_set_f64_extend(raw as *mut crate::array::ArrayHeader, index, value);
    }
}

/// A NON-numeric computed key as a Rust string (`buf["writeInt8"]`), or `None`
/// when the key isn't a string value. Used by the Buffer own-prop / method-value
/// arms above.
fn buffer_key_name(idx: f64) -> Option<String> {
    let jv = crate::value::JSValue::from_bits(idx.to_bits());
    if !jv.is_any_string() {
        return None;
    }
    let ptr = crate::value::js_get_string_pointer_unified(idx) as *const crate::StringHeader;
    if ptr.is_null() {
        return None;
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}
