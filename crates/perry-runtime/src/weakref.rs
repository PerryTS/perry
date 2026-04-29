//! WeakRef and FinalizationRegistry runtime support.
//!
//! Pragmatic / stub implementation: WeakRef holds a STRONG reference internally
//! (so `deref()` always returns the wrapped value) and FinalizationRegistry stores
//! registrations but never actually fires the cleanup callbacks. Implementing real
//! weak references would require integrating with `gc.rs`'s mark phase and
//! clearing the slot during sweep — that's a multi-day project, and most user code
//! that uses these APIs only relies on their behaviour for the lifetime of the
//! references (not on actual collection).
//!
//! This implementation matches the Node.js output for `test_gap_weakref_finalization.ts`.

use crate::array::{
    js_array_alloc, js_array_alloc_with_length, js_array_get_f64, js_array_length,
    js_array_push_f64, js_array_set_f64, ArrayHeader,
};
use crate::object::{
    js_object_alloc_with_shape, js_object_get_field_by_name, js_object_set_field, ObjectHeader,
};
use crate::value::{js_nanbox_get_pointer, JSValue};

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

const WEAKREF_SHAPE_ID: u32 = 0x7FFF_FE10;
const FINREG_SHAPE_ID: u32 = 0x7FFF_FE11;

/// Allocate a `WeakRef` wrapper object that strongly holds the target value
/// in a single field named `target`.
#[no_mangle]
pub extern "C" fn js_weakref_new(target: f64) -> *mut ObjectHeader {
    let packed = b"target\0";
    let obj = js_object_alloc_with_shape(WEAKREF_SHAPE_ID, 1, packed.as_ptr(), packed.len() as u32);
    js_object_set_field(obj, 0, JSValue::from_bits(target.to_bits()));
    obj
}

/// Return the wrapped value (or `undefined` if the WeakRef pointer is null).
/// Stub: a real implementation would return undefined once the GC has collected
/// the target — Perry's GC doesn't yet track weak references, so this always
/// returns the strongly-held target.
#[no_mangle]
pub extern "C" fn js_weakref_deref(weakref: f64) -> f64 {
    let ptr = js_nanbox_get_pointer(weakref) as *mut ObjectHeader;
    if ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    unsafe {
        let key_ptr = crate::string::js_string_from_bytes(b"target".as_ptr(), 6);
        let val = js_object_get_field_by_name(ptr, key_ptr);
        if val.is_undefined() {
            f64::from_bits(TAG_UNDEFINED)
        } else {
            f64::from_bits(val.bits())
        }
    }
}

/// Allocate a `FinalizationRegistry` wrapper. The first field stores the cleanup
/// callback, the second field stores a registrations array — each entry is a
/// 2-element `[token, held]` array used by `unregister(token)` to find matches.
#[no_mangle]
pub extern "C" fn js_finreg_new(callback: f64) -> *mut ObjectHeader {
    let packed = b"callback\0entries\0";
    let obj = js_object_alloc_with_shape(FINREG_SHAPE_ID, 2, packed.as_ptr(), packed.len() as u32);
    js_object_set_field(obj, 0, JSValue::from_bits(callback.to_bits()));
    let entries_arr = js_array_alloc(0);
    js_object_set_field(obj, 1, JSValue::array_ptr(entries_arr));
    obj
}

/// Register a (target, held value, optional token) triple. Returns undefined.
/// We append a small `[token, held]` 2-element array to the registry's `entries`
/// array so a later `unregister(token)` can find and remove it. If no token is
/// provided, we still record an `[undefined, held]` pair so the registration count
/// is correct (but it can never be unregistered).
#[no_mangle]
pub extern "C" fn js_finreg_register(registry: f64, _target: f64, held: f64, token: f64) -> f64 {
    let reg_ptr = js_nanbox_get_pointer(registry) as *mut ObjectHeader;
    if reg_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    unsafe {
        let entries_key = crate::string::js_string_from_bytes(b"entries".as_ptr(), 7);
        let entries_val = js_object_get_field_by_name(reg_ptr, entries_key);
        let entries_ptr = (entries_val.bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ArrayHeader;
        if entries_ptr.is_null() {
            return f64::from_bits(TAG_UNDEFINED);
        }
        // Build a 2-element array: [token, held]
        let pair = js_array_alloc_with_length(2);
        js_array_set_f64(pair, 0, token);
        js_array_set_f64(pair, 1, held);
        let pair_val = f64::from_bits(JSValue::array_ptr(pair).bits());
        js_array_push_f64(entries_ptr, pair_val);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// Unregister all entries matching the given token. Returns `true` if at least
/// one entry was found and removed, `false` otherwise. Token comparison uses
/// strict equality (raw NaN-box bit comparison) which is correct for object
/// references — both sides are stored as POINTER_TAG-tagged f64 values.
#[no_mangle]
pub extern "C" fn js_finreg_unregister(registry: f64, token: f64) -> f64 {
    let reg_ptr = js_nanbox_get_pointer(registry) as *mut ObjectHeader;
    if reg_ptr.is_null() {
        return f64::from_bits(TAG_FALSE);
    }
    unsafe {
        let entries_key = crate::string::js_string_from_bytes(b"entries".as_ptr(), 7);
        let entries_val = js_object_get_field_by_name(reg_ptr, entries_key);
        let entries_ptr = (entries_val.bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ArrayHeader;
        if entries_ptr.is_null() {
            return f64::from_bits(TAG_FALSE);
        }
        let len = js_array_length(entries_ptr) as usize;
        let mut found = false;
        // Rebuild the entries array without the matching pairs.
        let new_arr = js_array_alloc(0);
        for i in 0..len {
            let pair_val_f = js_array_get_f64(entries_ptr, i as u32);
            let pair_ptr = (pair_val_f.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ArrayHeader;
            if pair_ptr.is_null() {
                continue;
            }
            let stored_token = js_array_get_f64(pair_ptr, 0);
            if stored_token.to_bits() == token.to_bits() {
                found = true;
                continue;
            }
            js_array_push_f64(new_arr, pair_val_f);
        }
        // Replace entries field with the new array.
        js_object_set_field(reg_ptr, 1, JSValue::array_ptr(new_arr));
        if found {
            f64::from_bits(TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        }
    }
}

// =============================================================================
// WeakMap / WeakSet runtime — implemented separately from `crate::map`/`crate::set`
// because the existing `js_map_set` does *content-based* equality on string-like
// pointer keys, which incorrectly collapses two distinct empty objects (`{}`)
// onto the same slot. WeakMap/WeakSet require *reference* equality, so we use
// our own storage backed by an `entries` array of `[key, value]` pairs (set just
// stores `[key, key]`) with raw NaN-box bit comparison.
// =============================================================================

const WEAKMAP_SHAPE_ID: u32 = 0x7FFF_FE12;
const WEAKSET_SHAPE_ID: u32 = 0x7FFF_FE13;

unsafe fn entries_array(reg: *mut ObjectHeader) -> *mut ArrayHeader {
    let entries_key = crate::string::js_string_from_bytes(b"entries".as_ptr(), 7);
    let entries_val = js_object_get_field_by_name(reg, entries_key);
    (entries_val.bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ArrayHeader
}

#[no_mangle]
pub extern "C" fn js_weakmap_new() -> *mut ObjectHeader {
    let packed = b"entries\0";
    let obj = js_object_alloc_with_shape(WEAKMAP_SHAPE_ID, 1, packed.as_ptr(), packed.len() as u32);
    let entries_arr = js_array_alloc(0);
    js_object_set_field(obj, 0, JSValue::array_ptr(entries_arr));
    obj
}

#[no_mangle]
pub extern "C" fn js_weakmap_set(map: f64, key: f64, value: f64) -> f64 {
    let map_ptr = js_nanbox_get_pointer(map) as *mut ObjectHeader;
    if map_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    unsafe {
        let entries_ptr = entries_array(map_ptr);
        if entries_ptr.is_null() {
            return f64::from_bits(TAG_UNDEFINED);
        }
        let len = js_array_length(entries_ptr) as usize;
        // Update existing pair if key matches.
        for i in 0..len {
            let pair_val_f = js_array_get_f64(entries_ptr, i as u32);
            let pair_ptr = (pair_val_f.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ArrayHeader;
            if pair_ptr.is_null() {
                continue;
            }
            let stored_key = js_array_get_f64(pair_ptr, 0);
            if stored_key.to_bits() == key.to_bits() {
                js_array_set_f64(pair_ptr, 1, value);
                return map;
            }
        }
        // Append new [key, value] pair.
        let pair = js_array_alloc_with_length(2);
        js_array_set_f64(pair, 0, key);
        js_array_set_f64(pair, 1, value);
        let pair_val = f64::from_bits(JSValue::array_ptr(pair).bits());
        js_array_push_f64(entries_ptr, pair_val);
    }
    map
}

#[no_mangle]
pub extern "C" fn js_weakmap_get(map: f64, key: f64) -> f64 {
    let map_ptr = js_nanbox_get_pointer(map) as *mut ObjectHeader;
    if map_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    unsafe {
        let entries_ptr = entries_array(map_ptr);
        if entries_ptr.is_null() {
            return f64::from_bits(TAG_UNDEFINED);
        }
        let len = js_array_length(entries_ptr) as usize;
        for i in 0..len {
            let pair_val_f = js_array_get_f64(entries_ptr, i as u32);
            let pair_ptr = (pair_val_f.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ArrayHeader;
            if pair_ptr.is_null() {
                continue;
            }
            let stored_key = js_array_get_f64(pair_ptr, 0);
            if stored_key.to_bits() == key.to_bits() {
                return js_array_get_f64(pair_ptr, 1);
            }
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_weakmap_has(map: f64, key: f64) -> f64 {
    let map_ptr = js_nanbox_get_pointer(map) as *mut ObjectHeader;
    if map_ptr.is_null() {
        return f64::from_bits(TAG_FALSE);
    }
    unsafe {
        let entries_ptr = entries_array(map_ptr);
        if entries_ptr.is_null() {
            return f64::from_bits(TAG_FALSE);
        }
        let len = js_array_length(entries_ptr) as usize;
        for i in 0..len {
            let pair_val_f = js_array_get_f64(entries_ptr, i as u32);
            let pair_ptr = (pair_val_f.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ArrayHeader;
            if pair_ptr.is_null() {
                continue;
            }
            let stored_key = js_array_get_f64(pair_ptr, 0);
            if stored_key.to_bits() == key.to_bits() {
                return f64::from_bits(TAG_TRUE);
            }
        }
    }
    f64::from_bits(TAG_FALSE)
}

#[no_mangle]
pub extern "C" fn js_weakmap_delete(map: f64, key: f64) -> f64 {
    let map_ptr = js_nanbox_get_pointer(map) as *mut ObjectHeader;
    if map_ptr.is_null() {
        return f64::from_bits(TAG_FALSE);
    }
    unsafe {
        let entries_ptr = entries_array(map_ptr);
        if entries_ptr.is_null() {
            return f64::from_bits(TAG_FALSE);
        }
        let len = js_array_length(entries_ptr) as usize;
        let mut found = false;
        let new_arr = js_array_alloc(0);
        for i in 0..len {
            let pair_val_f = js_array_get_f64(entries_ptr, i as u32);
            let pair_ptr = (pair_val_f.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ArrayHeader;
            if pair_ptr.is_null() {
                continue;
            }
            let stored_key = js_array_get_f64(pair_ptr, 0);
            if stored_key.to_bits() == key.to_bits() {
                found = true;
                continue;
            }
            js_array_push_f64(new_arr, pair_val_f);
        }
        js_object_set_field(map_ptr, 0, JSValue::array_ptr(new_arr));
        if found {
            f64::from_bits(TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        }
    }
}

#[no_mangle]
pub extern "C" fn js_weakset_new() -> *mut ObjectHeader {
    let packed = b"entries\0";
    let obj = js_object_alloc_with_shape(WEAKSET_SHAPE_ID, 1, packed.as_ptr(), packed.len() as u32);
    let entries_arr = js_array_alloc(0);
    js_object_set_field(obj, 0, JSValue::array_ptr(entries_arr));
    obj
}

#[no_mangle]
pub extern "C" fn js_weakset_add(set: f64, value: f64) -> f64 {
    // Reuse js_weakmap_set with value as both key and value (matches JS Set spec).
    js_weakmap_set(set, value, value);
    set
}

#[no_mangle]
pub extern "C" fn js_weakset_has(set: f64, value: f64) -> f64 {
    js_weakmap_has(set, value)
}

#[no_mangle]
pub extern "C" fn js_weakset_delete(set: f64, value: f64) -> f64 {
    js_weakmap_delete(set, value)
}

/// Throw a `TypeError` for `WeakMap.set(primitive, ...)` / `WeakSet.add(primitive)`.
/// Used by codegen when the static AST key/value is a primitive literal so we can
/// match the JS spec which mandates an exception in those cases.
///
/// Marked `-> f64` for the ABI signature even though `js_throw` is `-> !`;
/// the function never actually returns.
#[no_mangle]
pub extern "C" fn js_weak_throw_primitive() -> f64 {
    let msg = "Invalid value used as weak collection key";
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_error_new_with_message(msg_str);
    let err_val = JSValue::pointer(err as *const u8);
    crate::exception::js_throw(f64::from_bits(err_val.bits()))
}
