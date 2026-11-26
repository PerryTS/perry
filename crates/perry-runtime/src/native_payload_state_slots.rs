//! Fixed-slot hidden state (Web Streams as GC objects, design 2026-11-25
//! §2.3): an object's JS state is **one dense JS array born at its final
//! length**, stored under the hidden JS-state key ([`JS_STATE_KEY`]). The
//! slot indices are constants of the family that owns the object, so the
//! array is the object's layout, not a table: reads are `array[i]`, writes go
//! through the barriered array store (`js_array_set`), and the collector
//! traces and moves the array like any other field of the object.
//!
//! The C ABI here serves families implemented in perry-stdlib (which reaches
//! the runtime only through it):
//! - [`js_perry_intrinsic_prototype`]: the realm's intrinsic prototype of a
//!   builtin interface (born on first use);
//! - [`js_perry_slots_object_alloc`]: an ordinary object of a class id and
//!   prototype with its slot array (families without native scalars);
//! - [`js_perry_slots_init`]: the slot array on an existing object (a payload
//!   instance right after its birth, a subclass `this`);
//! - [`js_perry_slots`]: the array, without allocating;
//! - [`js_perry_object_class_id`]: the receiver check of payload-less members.

use super::*;
use crate::array::ArrayHeader;

/// The realm's intrinsic prototype of the builtin named `name`
/// (`ReadableStream`, ...), born on first use; `undefined` for a name that
/// is no builtin.
///
/// # Safety
/// `name_ptr..name_ptr + name_len` is readable UTF-8.
#[no_mangle]
pub unsafe extern "C" fn js_perry_intrinsic_prototype(name_ptr: *const u8, name_len: usize) -> f64 {
    match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
        Ok(name) => crate::object::builtin_prototype_value(name),
        Err(_) => undefined(),
    }
}

/// Store a fresh `slots`-long array of `undefined` under `obj`'s hidden
/// JS-state key and return the object's value.
fn init_slots(
    scope: &crate::gc::RuntimeHandleScope,
    obj: &crate::gc::RuntimeHandle<'_>,
    slots: u32,
) -> f64 {
    let array = crate::array::js_array_alloc(slots);
    if array.is_null() {
        return undefined();
    }
    let mut array = array;
    for _ in 0..slots {
        // Within capacity: no reallocation, no GC.
        array = crate::array::js_array_push(array, crate::value::JSValue::undefined());
    }
    let state = crate::value::js_nanbox_pointer(array as i64);
    set_state_field(scope, obj, JS_STATE_KEY, state);
    obj.with_mut_ptr::<ObjectHeader, _>(|obj| crate::value::js_nanbox_pointer(obj as i64))
}

/// An ordinary object of `class_id` whose prototype is `proto`, with a
/// `slots`-long hidden slot array and no own properties. `undefined` when
/// `proto` is no object.
///
/// # Safety
/// `proto` is a live value.
#[no_mangle]
pub unsafe extern "C" fn js_perry_slots_object_alloc(class_id: u32, proto: f64, slots: u32) -> f64 {
    let Some(proto) = any_object(proto) else {
        return undefined();
    };
    // The intrinsic prototype is held by its realm cell: no root needed.
    let obj = born_instance(class_id, proto, 1);
    if obj.is_null() {
        return undefined();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    init_slots(&scope, &obj, slots)
}

/// Give the existing object `value` a `slots`-long hidden slot array.
/// Returns 0 on success, 1 when `value` is no object, 2 when it already has
/// a JS state.
///
/// # Safety
/// `value` is a live value.
#[no_mangle]
pub unsafe extern "C" fn js_perry_slots_init(value: f64, slots: u32) -> i32 {
    let Some(obj) = any_object(value) else {
        return 1;
    };
    if raw_js_state(obj).is_some() {
        return 2;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    if crate::value::JSValue::from_bits(init_slots(&scope, &obj, slots).to_bits()).is_undefined() {
        return 1;
    }
    0
}

/// The hidden slot array of `value` (null when it has none). Never
/// allocates.
///
/// # Safety
/// `value` is a live value.
#[no_mangle]
pub unsafe extern "C" fn js_perry_slots(value: f64) -> *mut ArrayHeader {
    match any_object(value).and_then(|obj| raw_js_state(obj)) {
        Some(state) => state as *mut ArrayHeader,
        None => std::ptr::null_mut(),
    }
}

/// The class id of the ordinary object `value` (0 for any other value): the
/// receiver check of a family whose members carry no payload.
///
/// # Safety
/// `value` is a live value.
#[no_mangle]
pub unsafe extern "C" fn js_perry_object_class_id(value: f64) -> u32 {
    any_object(value).map_or(0, |obj| (*obj).class_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proto() -> f64 {
        crate::value::js_nanbox_pointer(crate::object::js_object_alloc(0, 0) as i64)
    }

    #[test]
    fn slot_object_is_born_with_its_class_prototype_and_fixed_length() {
        let p = proto();
        let v = unsafe {
            js_perry_slots_object_alloc(crate::native_class_ids::WEB_RS_DEFAULT_READER, p, 3)
        };
        let obj = any_object(v).expect("an object");
        unsafe {
            assert_eq!(
                (*obj).class_id,
                crate::native_class_ids::WEB_RS_DEFAULT_READER
            );
            let slots = js_perry_slots(v);
            assert!(!slots.is_null());
            assert_eq!(crate::array::js_array_length(slots), 3);
            for i in 0..3 {
                assert!(crate::array::js_array_get(slots, i).is_undefined());
            }
            // The hidden state is no own enumerable key.
            let keys = crate::object::js_object_keys(obj);
            assert_eq!(crate::array::js_array_length(keys), 0);
            // A second init is refused; the array stays.
            assert_eq!(js_perry_slots_init(v, 3), 2);
            assert_eq!(js_perry_slots(v), slots);
            assert_eq!(
                js_perry_object_class_id(v),
                crate::native_class_ids::WEB_RS_DEFAULT_READER
            );
            assert_eq!(js_perry_object_class_id(1.5), 0);
        }
    }

    #[test]
    fn slots_init_on_existing_object_and_store() {
        let v = proto();
        unsafe {
            assert!(js_perry_slots(v).is_null());
            assert_eq!(js_perry_slots_init(v, 2), 0);
            let slots = js_perry_slots(v);
            assert_eq!(crate::array::js_array_length(slots), 2);
            crate::array::js_array_set(slots, 1, crate::value::JSValue::number(7.0));
            assert_eq!(
                crate::array::js_array_get(js_perry_slots(v), 1).as_number(),
                7.0
            );
            assert_eq!(
                js_perry_slots_init(f64::from_bits(crate::value::TAG_UNDEFINED), 2),
                1
            );
        }
    }
}
