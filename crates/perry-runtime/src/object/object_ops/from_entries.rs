//! `Object.fromEntries` and its iterable-materialization helpers.
use super::super::*;
use super::*;

fn throw_from_entries_type_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn throw_from_entries_not_iterable() -> ! {
    throw_from_entries_type_error(b"undefined is not iterable")
}

fn throw_from_entries_non_object_entry() -> ! {
    throw_from_entries_type_error(b"Iterator value is not an entry object")
}

unsafe fn object_from_entries_gc_type(raw_ptr: i64) -> Option<u8> {
    if raw_ptr < crate::gc::GC_HEADER_SIZE as i64 + 0x1000 {
        return None;
    }
    let addr = raw_ptr as usize;
    if crate::symbol::is_registered_symbol(addr) {
        return None;
    }
    if crate::set::is_registered_set(addr) {
        return Some(crate::gc::GC_TYPE_SET);
    }
    if crate::map::is_registered_map(addr) {
        return Some(crate::gc::GC_TYPE_MAP);
    }
    let ptr = raw_ptr as *const u8;
    if !crate::object::is_valid_obj_ptr(ptr) {
        return None;
    }
    let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    Some((*gc_header).obj_type)
}

unsafe fn object_from_entries_array_ptr(value: f64) -> *mut ArrayHeader {
    let raw = crate::value::js_nanbox_get_pointer(value);
    let gc_type = object_from_entries_gc_type(raw);
    if gc_type != Some(crate::gc::GC_TYPE_ARRAY) && gc_type != Some(crate::gc::GC_TYPE_LAZY_ARRAY) {
        throw_from_entries_not_iterable();
    }
    raw as *mut ArrayHeader
}

unsafe fn object_from_entries_has_iterator(value: f64, raw: i64, gc_type: Option<u8>) -> bool {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_any_string() {
        return true;
    }
    match gc_type {
        Some(crate::gc::GC_TYPE_ARRAY)
        | Some(crate::gc::GC_TYPE_LAZY_ARRAY)
        | Some(crate::gc::GC_TYPE_MAP)
        | Some(crate::gc::GC_TYPE_SET) => return true,
        Some(crate::gc::GC_TYPE_OBJECT) => {
            let obj = raw as *mut ObjectHeader;
            if crate::url::try_read_as_search_params(obj).is_some() {
                return true;
            }
            if !obj.is_null() && (*obj).class_id == crate::array::ARRAY_ITERATOR_CLASS_ID {
                return true;
            }
        }
        _ => {}
    }

    let iter_sym = crate::symbol::well_known_symbol("iterator");
    if !iter_sym.is_null() {
        let sym_value =
            f64::from_bits(crate::value::JSValue::pointer(iter_sym as *const u8).bits());
        let iter_fn = crate::symbol::js_object_get_symbol_property(value, sym_value);
        let iter_fn_ptr = crate::value::js_nanbox_get_pointer(iter_fn);
        if iter_fn_ptr != 0 && crate::closure::is_closure_ptr(iter_fn_ptr as usize) {
            return true;
        }
    }

    crate::array::has_iterator_next(value)
}

unsafe fn object_from_entries_materialize_entries(entries_value: f64) -> *mut ArrayHeader {
    let jv = crate::value::JSValue::from_bits(entries_value.to_bits());
    if jv.is_null() || jv.is_undefined() || jv.is_bool() || jv.is_number() || jv.is_int32() {
        throw_from_entries_not_iterable();
    }
    if jv.is_bigint() {
        throw_from_entries_not_iterable();
    }

    let raw = crate::value::js_nanbox_get_pointer(entries_value);
    let gc_type = object_from_entries_gc_type(raw);

    if !jv.is_any_string() && raw == 0 {
        throw_from_entries_not_iterable();
    }

    if !object_from_entries_has_iterator(entries_value, raw, gc_type) {
        throw_from_entries_not_iterable();
    }

    if gc_type == Some(crate::gc::GC_TYPE_MAP) {
        return crate::map::js_map_entries(raw as *const crate::map::MapHeader);
    }

    if gc_type == Some(crate::gc::GC_TYPE_OBJECT) {
        let obj = raw as *mut ObjectHeader;
        if crate::url::try_read_as_search_params(obj).is_some() {
            let boxed = crate::url::js_url_search_params_entries_arr(obj);
            return object_from_entries_array_ptr(boxed);
        }
    }

    let boxed = crate::array::js_for_of_to_array(entries_value);
    object_from_entries_array_ptr(boxed)
}

unsafe fn object_from_entries_entry_values(entry_val: f64) -> (f64, f64) {
    let jv = crate::value::JSValue::from_bits(entry_val.to_bits());
    if jv.is_null()
        || jv.is_undefined()
        || jv.is_bool()
        || jv.is_number()
        || jv.is_int32()
        || jv.is_any_string()
        || jv.is_bigint()
    {
        throw_from_entries_non_object_entry();
    }

    let raw = crate::value::js_nanbox_get_pointer(entry_val);
    let gc_type = object_from_entries_gc_type(raw);
    if raw == 0 {
        throw_from_entries_non_object_entry();
    }

    if gc_type == Some(crate::gc::GC_TYPE_ARRAY) || gc_type == Some(crate::gc::GC_TYPE_LAZY_ARRAY) {
        let arr = raw as *const ArrayHeader;
        return (
            crate::array::js_array_get_f64(arr, 0),
            crate::array::js_array_get_f64(arr, 1),
        );
    }

    let obj = raw as *const ObjectHeader;
    if obj.is_null() {
        throw_from_entries_non_object_entry();
    }
    let key0 = crate::string::js_string_from_bytes(b"0".as_ptr(), 1);
    let key1 = crate::string::js_string_from_bytes(b"1".as_ptr(), 1);
    (
        js_object_get_field_by_name_f64(obj, key0),
        js_object_get_field_by_name_f64(obj, key1),
    )
}

/// Object.fromEntries(entries) — build an object from iterable [key, value] entries.
#[no_mangle]
pub extern "C" fn js_object_from_entries(entries_value: f64) -> f64 {
    unsafe {
        let arr_ptr = object_from_entries_materialize_entries(entries_value);
        let length = crate::array::js_array_length(arr_ptr) as usize;

        // Allocate empty object — class_id 0 = generic object
        let obj = js_object_alloc(0, length as u32);
        if obj.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }

        for i in 0..length {
            let entry_val = crate::array::js_array_get_f64(arr_ptr, i as u32);
            let (key_val, val_val) = object_from_entries_entry_values(entry_val);
            let key_str = crate::builtins::js_string_coerce(key_val);
            if key_str.is_null() {
                continue;
            }
            js_object_set_field_by_name(obj, key_str, val_val);
        }

        crate::value::js_nanbox_pointer(obj as i64)
    }
}
