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

unsafe fn property_key_string_ptr(value: f64) -> *mut crate::StringHeader {
    let key = crate::object::js_to_property_key(value);
    if crate::symbol::js_is_symbol(key) != 0 {
        return std::ptr::null_mut();
    }
    crate::value::js_jsvalue_to_string(key)
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
        (obj_handle as u64) & 0x0000_FFFF_FFFF_FFFF
    } else {
        obj_handle as u64
    };
    if raw < 0x1000 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if crate::buffer::is_registered_buffer(raw as usize) {
        let idx_i32 = idx as i32;
        let byte_val =
            crate::buffer::js_buffer_get(raw as *const crate::buffer::BufferHeader, idx_i32);
        return byte_val as f64;
    }
    if crate::typedarray::lookup_typed_array_kind(raw as usize).is_some() {
        let idx_i32 = idx as i32;
        return crate::typedarray::js_typed_array_get(
            raw as *const crate::typedarray::TypedArrayHeader,
            idx_i32,
        );
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

    let idx_i32 = idx as i32;
    if idx_i32 < 0 {
        // Negative numeric keys → string keys on the object path.
        let s = idx_i32.to_string();
        let key = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        let v = js_object_get_field_by_name(raw as *mut ObjectHeader, key);
        return f64::from_bits(v.bits());
    }

    if let Some(value) =
        unsafe { arguments_object_get_index(raw as *const ObjectHeader, idx_i32 as u32) }
    {
        return value;
    }

    if gc_type == crate::gc::GC_TYPE_ARRAY || gc_type == crate::gc::GC_TYPE_LAZY_ARRAY {
        if idx_i32 < 0 || idx != (idx_i32 as f64) {
            let key = unsafe { property_key_string_ptr(idx) };
            if key.is_null() {
                return f64::from_bits(crate::value::TAG_UNDEFINED);
            }
            let v = js_object_get_field_by_name(raw as *mut ObjectHeader, key);
            return f64::from_bits(v.bits());
        }
        return crate::array::js_array_get_f64(
            raw as *mut crate::array::ArrayHeader,
            idx_i32 as u32,
        );
    }
    if gc_type == crate::gc::GC_TYPE_OBJECT || gc_type == crate::gc::GC_TYPE_CLOSURE {
        let key = unsafe { property_key_string_ptr(idx) };
        if key.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        let v = js_object_get_field_by_name(raw as *mut ObjectHeader, key);
        return f64::from_bits(v.bits());
    }
    // Buffer / Map / Set / typed-array / unknown — try the array getter
    // (which handles registered buffers + typed arrays via per-kind reads).
    crate::array::js_array_get_f64(raw as *mut crate::array::ArrayHeader, idx_i32 as u32)
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
/// (8-byte header) but `ObjectHeader` is 24 bytes followed by `max(field_count, 8)`
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
    // Strip NaN-box tags defensively. Codegen calls this with the lower-48
    // bits already extracted via `unbox_to_i64`, but match the convention
    // of every other entry-point so a stray un-stripped caller (or a JIT
    // that forgets the mask) still works.
    let raw = if (obj_handle as u64) >> 48 >= 0x7FF8 {
        (obj_handle as u64) & 0x0000_FFFF_FFFF_FFFF
    } else {
        obj_handle as u64
    };
    if raw < 0x1000 {
        return;
    }
    let idx_i32 = idx as i32;

    if unsafe { arguments_object_set_index(raw as *mut ObjectHeader, idx_i32 as u32, value) } {
        return;
    }

    if crate::buffer::is_registered_buffer(raw as usize) {
        crate::buffer::js_buffer_set(
            raw as *mut crate::buffer::BufferHeader,
            idx_i32,
            value as i32,
        );
        return;
    }
    if crate::typedarray::lookup_typed_array_kind(raw as usize).is_some() {
        crate::typedarray::js_typed_array_set(
            raw as *mut crate::typedarray::TypedArrayHeader,
            idx_i32,
            value,
        );
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
        if idx_i32 < 0 || idx != (idx_i32 as f64) {
            let key = unsafe { property_key_string_ptr(idx) };
            if !key.is_null() {
                js_object_set_field_by_name(raw as *mut ObjectHeader, key, value);
            }
            return;
        }
        // Includes lazy/forwarded — js_array_set_f64_extend's clean_arr_ptr_mut
        // walks the forwarding chain and routes buffers/typed-arrays through
        // their per-kind setter.
        crate::array::js_array_set_f64_extend(
            raw as *mut crate::array::ArrayHeader,
            idx_i32 as u32,
            value,
        );
        return;
    }
    if gc_type == crate::gc::GC_TYPE_OBJECT || gc_type == crate::gc::GC_TYPE_CLOSURE {
        // Stringify the index and route through the object field setter,
        // which handles shape transitions, frozen/sealed/extensible checks,
        // overflow into out-of-line storage, and accessor descriptors.
        let key = unsafe { property_key_string_ptr(idx) };
        if !key.is_null() {
            js_object_set_field_by_name(raw as *mut ObjectHeader, key, value);
        }
        return;
    }
    // Buffer / Map / Set / other GC types — fall through to the array
    // setter, which has its own per-kind dispatch (registered buffer →
    // byte write, registered typed-array → typed setter). Anything not
    // recognized is a no-op via clean_arr_ptr_mut returning null.
    crate::array::js_array_set_f64_extend(
        raw as *mut crate::array::ArrayHeader,
        idx_i32 as u32,
        value,
    );
}
