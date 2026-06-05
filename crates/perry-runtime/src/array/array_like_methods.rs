//! Generic `Array.prototype` methods for array-like receivers.
use super::*;
use crate::closure::{js_closure_call3, js_closure_call4, ClosureHeader};
use crate::value::{JSValue, TAG_FALSE, TAG_TRUE, TAG_UNDEFINED};
use std::ptr;

#[inline(always)]
fn undefined_value() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

#[inline(always)]
fn bool_value(value: bool) -> f64 {
    f64::from_bits(if value { TAG_TRUE } else { TAG_FALSE })
}

#[inline(always)]
fn string_result_value(value: *mut crate::string::StringHeader) -> f64 {
    f64::from_bits(JSValue::string_ptr(value).bits())
}

#[inline(always)]
fn array_result_value(value: *mut ArrayHeader) -> f64 {
    f64::from_bits(JSValue::pointer(value as *const u8).bits())
}

#[cold]
fn throw_nullish_receiver() -> ! {
    crate::object::throw_object_type_error(b"Array.prototype method called on null or undefined")
}

#[inline]
fn raw_pointer_addr(value: f64) -> Option<usize> {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_pointer() {
        return Some(jv.as_pointer::<u8>() as usize);
    }
    let bits = value.to_bits();
    if bits >> 48 == 0 && bits > 0x10000 {
        Some(bits as usize)
    } else {
        None
    }
}

#[inline]
fn is_nullish(value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    jv.is_null() || jv.is_undefined()
}

#[inline]
unsafe fn is_array_or_lazy_addr(addr: usize) -> bool {
    if addr < crate::gc::GC_HEADER_SIZE + 0x1000
        || crate::buffer::is_registered_buffer(addr)
        || crate::symbol::is_registered_symbol(addr)
    {
        return false;
    }
    let gc_header =
        (addr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY
        || (*gc_header).obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
}

#[inline]
fn receiver_array_ptr(value: f64) -> Option<*const ArrayHeader> {
    let addr = raw_pointer_addr(value)?;
    unsafe {
        if is_array_or_lazy_addr(addr) || crate::typedarray::lookup_typed_array_kind(addr).is_some()
        {
            Some(addr as *const ArrayHeader)
        } else {
            None
        }
    }
}

#[inline]
fn receiver_string_array(value: f64) -> Option<*mut ArrayHeader> {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_any_string() {
        return None;
    }
    let ptr =
        crate::value::js_get_string_pointer_unified(value) as *const crate::string::StringHeader;
    if ptr.is_null() {
        return Some(js_array_alloc(0));
    }
    Some(unsafe { js_array_from_string_codepoints(ptr) })
}

#[inline]
fn receiver_object_ptr(value: f64) -> Option<*const crate::object::ObjectHeader> {
    let addr = raw_pointer_addr(value)?;
    unsafe {
        if is_array_or_lazy_addr(addr)
            || crate::typedarray::lookup_typed_array_kind(addr).is_some()
            || crate::buffer::is_registered_buffer(addr)
            || crate::symbol::is_registered_symbol(addr)
        {
            None
        } else {
            Some(addr as *const crate::object::ObjectHeader)
        }
    }
}

fn array_like_length(receiver: *const crate::object::ObjectHeader) -> u32 {
    if receiver.is_null() {
        return 0;
    }
    let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    let value = crate::object::js_object_get_field_by_name_f64(receiver, key);
    let number = crate::builtins::js_number_coerce(value);
    if !number.is_finite() || number <= 0.0 {
        0
    } else if number >= u32::MAX as f64 {
        u32::MAX
    } else {
        number.trunc() as u32
    }
}

fn index_key(index: u32) -> *mut crate::string::StringHeader {
    let key = index.to_string();
    crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32)
}

#[inline]
fn object_has_index(
    receiver_value: f64,
    receiver: *const crate::object::ObjectHeader,
    index: u32,
) -> bool {
    let key = index_key(index);
    let key_value = f64::from_bits(JSValue::string_ptr(key).bits());
    if crate::object::js_object_has_property(receiver_value, key_value).to_bits() == TAG_TRUE {
        return true;
    }
    let value = crate::object::js_object_get_field_by_name_f64(receiver, key);
    !JSValue::from_bits(value.to_bits()).is_undefined()
}

#[inline]
fn object_get_index(receiver: *const crate::object::ObjectHeader, index: u32) -> f64 {
    let key = index_key(index);
    crate::object::js_object_get_field_by_name_f64(receiver, key)
}

#[inline]
fn validate_callback(callback: f64, map_form_receiver: i64) -> *const ClosureHeader {
    if map_form_receiver != 0 {
        crate::array::js_validate_array_map_callback(map_form_receiver, callback)
            as *const ClosureHeader
    } else {
        crate::array::js_validate_array_callback(callback) as *const ClosureHeader
    }
}

#[inline]
fn with_callback_this<F, R>(this_arg: f64, f: F) -> R
where
    F: FnOnce() -> R,
{
    let previous = crate::object::js_implicit_this_set(this_arg);
    let result = f();
    crate::object::js_implicit_this_set(previous);
    result
}

#[inline]
fn from_index_forward(length: i64, from_index: f64, has_from: bool) -> Option<i64> {
    if !has_from {
        return Some(0);
    }
    let number = crate::builtins::js_number_coerce(from_index);
    let n = if number.is_nan() { 0.0 } else { number.trunc() };
    if n >= length as f64 {
        None
    } else if n >= 0.0 {
        Some(n as i64)
    } else if n >= -(length as f64) {
        Some(length + n as i64)
    } else {
        Some(0)
    }
}

#[inline]
fn from_index_backward(length: i64, from_index: f64, has_from: bool) -> Option<i64> {
    if length == 0 {
        return None;
    }
    if !has_from {
        return Some(length - 1);
    }
    let number = crate::builtins::js_number_coerce(from_index);
    let n = if number.is_nan() { 0.0 } else { number.trunc() };
    if n >= length as f64 {
        Some(length - 1)
    } else if n >= 0.0 {
        Some(n as i64)
    } else if n >= -(length as f64) {
        Some(length + n as i64)
    } else {
        None
    }
}

fn slice_index(value: f64, default_end: bool) -> i32 {
    if default_end && JSValue::from_bits(value.to_bits()).is_undefined() {
        return i32::MAX;
    }
    let number = crate::builtins::js_number_coerce(value);
    if number.is_nan() {
        0
    } else if number >= i32::MAX as f64 {
        i32::MAX
    } else if number <= i32::MIN as f64 {
        i32::MIN
    } else {
        number.trunc() as i32
    }
}

#[inline]
fn normalize_slice_index(len: u32, index: i32, is_end: bool) -> u32 {
    let len_i32 = len.min(i32::MAX as u32) as i32;
    if is_end && index == i32::MAX {
        len
    } else if index < 0 {
        (len_i32 + index).max(0) as u32
    } else {
        (index as u32).min(len)
    }
}

#[no_mangle]
pub extern "C" fn js_array_like_join(
    receiver_value: f64,
    separator_value: f64,
) -> *mut crate::string::StringHeader {
    if is_nullish(receiver_value) {
        throw_nullish_receiver();
    }
    if let Some(arr) = receiver_array_ptr(receiver_value) {
        return crate::array::js_array_join_value(arr, separator_value);
    }
    if let Some(arr) = receiver_string_array(receiver_value) {
        return crate::array::js_array_join_value(arr, separator_value);
    }
    let Some(receiver) = receiver_object_ptr(receiver_value) else {
        return crate::string::js_string_from_bytes(ptr::null(), 0);
    };
    let len = array_like_length(receiver);
    if len == 0 {
        return crate::string::js_string_from_bytes(ptr::null(), 0);
    }
    let separator = if JSValue::from_bits(separator_value.to_bits()).is_undefined() {
        ",".to_string()
    } else {
        let sp = crate::value::js_jsvalue_to_string(separator_value);
        if sp.is_null() {
            String::new()
        } else {
            unsafe {
                let header = &*sp;
                let data =
                    (sp as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
                String::from_utf8_lossy(std::slice::from_raw_parts(data, header.byte_len as usize))
                    .into_owned()
            }
        }
    };
    let mut output = String::new();
    for i in 0..len {
        if i > 0 {
            output.push_str(&separator);
        }
        let value = object_get_index(receiver, i);
        let jv = JSValue::from_bits(value.to_bits());
        if jv.is_null() || jv.is_undefined() {
            continue;
        }
        let sp = crate::value::js_jsvalue_to_string(value);
        if sp.is_null() {
            continue;
        }
        unsafe {
            let header = &*sp;
            let data = (sp as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
            output.push_str(std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                data,
                header.byte_len as usize,
            )));
        }
    }
    crate::string::js_string_from_bytes(output.as_ptr(), output.len() as u32)
}

#[no_mangle]
pub extern "C" fn js_array_like_map(
    receiver_value: f64,
    callback_value: f64,
    this_arg: f64,
) -> *mut ArrayHeader {
    if is_nullish(receiver_value) {
        throw_nullish_receiver();
    }
    if let Some(arr) = receiver_array_ptr(receiver_value) {
        let callback = validate_callback(callback_value, arr as i64);
        return with_callback_this(this_arg, || crate::array::js_array_map(arr, callback));
    }
    if let Some(arr) = receiver_string_array(receiver_value) {
        let callback = validate_callback(callback_value, arr as i64);
        return with_callback_this(this_arg, || crate::array::js_array_map(arr, callback));
    }
    let callback = validate_callback(callback_value, 0);
    let Some(receiver) = receiver_object_ptr(receiver_value) else {
        return js_array_alloc(0);
    };
    let len = array_like_length(receiver);
    let result = js_array_alloc_with_length(len);
    let result_elements =
        unsafe { (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64 };
    with_callback_this(this_arg, || {
        for i in 0..len {
            if !object_has_index(receiver_value, receiver, i) {
                continue;
            }
            let element = object_get_index(receiver, i);
            let mapped = js_closure_call3(callback, element, i as f64, receiver_value);
            unsafe {
                ptr::write(result_elements.add(i as usize), mapped);
            }
            let bits = mapped.to_bits();
            unsafe {
                if len <= 64 {
                    note_array_slot_layout_only(result, i as usize, bits);
                } else {
                    note_array_slot(result, i as usize, bits);
                }
            }
        }
    });
    result
}

#[no_mangle]
pub extern "C" fn js_array_like_filter(
    receiver_value: f64,
    callback_value: f64,
    this_arg: f64,
) -> *mut ArrayHeader {
    if is_nullish(receiver_value) {
        throw_nullish_receiver();
    }
    if let Some(arr) = receiver_array_ptr(receiver_value) {
        let callback = validate_callback(callback_value, 0);
        return with_callback_this(this_arg, || crate::array::js_array_filter(arr, callback));
    }
    if let Some(arr) = receiver_string_array(receiver_value) {
        let callback = validate_callback(callback_value, 0);
        return with_callback_this(this_arg, || crate::array::js_array_filter(arr, callback));
    }
    let callback = validate_callback(callback_value, 0);
    let Some(receiver) = receiver_object_ptr(receiver_value) else {
        return js_array_alloc(0);
    };
    let len = array_like_length(receiver);
    let mut result = js_array_alloc(0);
    with_callback_this(this_arg, || {
        for i in 0..len {
            if !object_has_index(receiver_value, receiver, i) {
                continue;
            }
            let element = object_get_index(receiver, i);
            let keep = js_closure_call3(callback, element, i as f64, receiver_value);
            if crate::value::js_is_truthy(keep) != 0 {
                result = js_array_push_f64(result, element);
            }
        }
    });
    result
}

#[no_mangle]
pub extern "C" fn js_array_like_for_each(receiver_value: f64, callback_value: f64, this_arg: f64) {
    if is_nullish(receiver_value) {
        throw_nullish_receiver();
    }
    if let Some(arr) = receiver_array_ptr(receiver_value) {
        let callback = validate_callback(callback_value, 0);
        with_callback_this(this_arg, || crate::array::js_array_forEach(arr, callback));
        return;
    }
    if let Some(arr) = receiver_string_array(receiver_value) {
        let callback = validate_callback(callback_value, 0);
        with_callback_this(this_arg, || crate::array::js_array_forEach(arr, callback));
        return;
    }
    let callback = validate_callback(callback_value, 0);
    let Some(receiver) = receiver_object_ptr(receiver_value) else {
        return;
    };
    let len = array_like_length(receiver);
    with_callback_this(this_arg, || {
        for i in 0..len {
            if !object_has_index(receiver_value, receiver, i) {
                continue;
            }
            let element = object_get_index(receiver, i);
            js_closure_call3(callback, element, i as f64, receiver_value);
        }
    });
}

#[no_mangle]
pub extern "C" fn js_array_like_some(
    receiver_value: f64,
    callback_value: f64,
    this_arg: f64,
) -> f64 {
    if is_nullish(receiver_value) {
        throw_nullish_receiver();
    }
    if let Some(arr) = receiver_array_ptr(receiver_value) {
        let callback = validate_callback(callback_value, 0);
        return with_callback_this(this_arg, || crate::array::js_array_some(arr, callback));
    }
    if let Some(arr) = receiver_string_array(receiver_value) {
        let callback = validate_callback(callback_value, 0);
        return with_callback_this(this_arg, || crate::array::js_array_some(arr, callback));
    }
    let callback = validate_callback(callback_value, 0);
    let Some(receiver) = receiver_object_ptr(receiver_value) else {
        return bool_value(false);
    };
    let len = array_like_length(receiver);
    with_callback_this(this_arg, || {
        for i in 0..len {
            if !object_has_index(receiver_value, receiver, i) {
                continue;
            }
            let element = object_get_index(receiver, i);
            let result = js_closure_call3(callback, element, i as f64, receiver_value);
            if crate::value::js_is_truthy(result) != 0 {
                return bool_value(true);
            }
        }
        bool_value(false)
    })
}

#[no_mangle]
pub extern "C" fn js_array_like_every(
    receiver_value: f64,
    callback_value: f64,
    this_arg: f64,
) -> f64 {
    if is_nullish(receiver_value) {
        throw_nullish_receiver();
    }
    if let Some(arr) = receiver_array_ptr(receiver_value) {
        let callback = validate_callback(callback_value, 0);
        return with_callback_this(this_arg, || crate::array::js_array_every(arr, callback));
    }
    if let Some(arr) = receiver_string_array(receiver_value) {
        let callback = validate_callback(callback_value, 0);
        return with_callback_this(this_arg, || crate::array::js_array_every(arr, callback));
    }
    let callback = validate_callback(callback_value, 0);
    let Some(receiver) = receiver_object_ptr(receiver_value) else {
        return bool_value(true);
    };
    let len = array_like_length(receiver);
    with_callback_this(this_arg, || {
        for i in 0..len {
            if !object_has_index(receiver_value, receiver, i) {
                continue;
            }
            let element = object_get_index(receiver, i);
            let result = js_closure_call3(callback, element, i as f64, receiver_value);
            if crate::value::js_is_truthy(result) == 0 {
                return bool_value(false);
            }
        }
        bool_value(true)
    })
}

#[no_mangle]
pub extern "C" fn js_array_like_find(
    receiver_value: f64,
    callback_value: f64,
    this_arg: f64,
) -> f64 {
    if is_nullish(receiver_value) {
        throw_nullish_receiver();
    }
    if let Some(arr) = receiver_array_ptr(receiver_value) {
        let callback = validate_callback(callback_value, 0);
        return with_callback_this(this_arg, || crate::array::js_array_find(arr, callback));
    }
    if let Some(arr) = receiver_string_array(receiver_value) {
        let callback = validate_callback(callback_value, 0);
        return with_callback_this(this_arg, || crate::array::js_array_find(arr, callback));
    }
    let callback = validate_callback(callback_value, 0);
    let Some(receiver) = receiver_object_ptr(receiver_value) else {
        return undefined_value();
    };
    let len = array_like_length(receiver);
    with_callback_this(this_arg, || {
        for i in 0..len {
            let element = object_get_index(receiver, i);
            let result = js_closure_call3(callback, element, i as f64, receiver_value);
            if crate::value::js_is_truthy(result) != 0 {
                return element;
            }
        }
        undefined_value()
    })
}

#[no_mangle]
pub extern "C" fn js_array_like_find_index(
    receiver_value: f64,
    callback_value: f64,
    this_arg: f64,
) -> i32 {
    if is_nullish(receiver_value) {
        throw_nullish_receiver();
    }
    if let Some(arr) = receiver_array_ptr(receiver_value) {
        let callback = validate_callback(callback_value, 0);
        return with_callback_this(this_arg, || crate::array::js_array_findIndex(arr, callback));
    }
    if let Some(arr) = receiver_string_array(receiver_value) {
        let callback = validate_callback(callback_value, 0);
        return with_callback_this(this_arg, || crate::array::js_array_findIndex(arr, callback));
    }
    let callback = validate_callback(callback_value, 0);
    let Some(receiver) = receiver_object_ptr(receiver_value) else {
        return -1;
    };
    let len = array_like_length(receiver);
    with_callback_this(this_arg, || {
        for i in 0..len {
            let element = object_get_index(receiver, i);
            let result = js_closure_call3(callback, element, i as f64, receiver_value);
            if crate::value::js_is_truthy(result) != 0 {
                return i as i32;
            }
        }
        -1
    })
}

#[no_mangle]
pub extern "C" fn js_array_like_index_of(
    receiver_value: f64,
    search: f64,
    from_index: f64,
    has_from: i32,
) -> i32 {
    if is_nullish(receiver_value) {
        throw_nullish_receiver();
    }
    if let Some(arr) = receiver_array_ptr(receiver_value) {
        return crate::array::js_array_indexOf_jsvalue(arr, search, from_index, has_from);
    }
    if let Some(arr) = receiver_string_array(receiver_value) {
        return crate::array::js_array_indexOf_jsvalue(arr, search, from_index, has_from);
    }
    let Some(receiver) = receiver_object_ptr(receiver_value) else {
        return -1;
    };
    let len = array_like_length(receiver) as i64;
    let Some(start) = from_index_forward(len, from_index, has_from != 0) else {
        return -1;
    };
    for i in start..len {
        if !object_has_index(receiver_value, receiver, i as u32) {
            continue;
        }
        let element = object_get_index(receiver, i as u32);
        if crate::value::js_jsvalue_equals(element, search) == 1 {
            return i as i32;
        }
    }
    -1
}

#[no_mangle]
pub extern "C" fn js_array_like_last_index_of(
    receiver_value: f64,
    search: f64,
    from_index: f64,
    has_from: i32,
) -> i32 {
    if is_nullish(receiver_value) {
        throw_nullish_receiver();
    }
    if let Some(arr) = receiver_array_ptr(receiver_value) {
        return crate::array::js_array_last_index_of_jsvalue(arr, search, from_index, has_from);
    }
    if let Some(arr) = receiver_string_array(receiver_value) {
        return crate::array::js_array_last_index_of_jsvalue(arr, search, from_index, has_from);
    }
    let Some(receiver) = receiver_object_ptr(receiver_value) else {
        return -1;
    };
    let len = array_like_length(receiver) as i64;
    let Some(mut i) = from_index_backward(len, from_index, has_from != 0) else {
        return -1;
    };
    while i >= 0 {
        if object_has_index(receiver_value, receiver, i as u32) {
            let element = object_get_index(receiver, i as u32);
            if crate::value::js_jsvalue_equals(element, search) == 1 {
                return i as i32;
            }
        }
        i -= 1;
    }
    -1
}

#[no_mangle]
pub extern "C" fn js_array_like_includes(
    receiver_value: f64,
    search: f64,
    from_index: f64,
    has_from: i32,
) -> f64 {
    if is_nullish(receiver_value) {
        throw_nullish_receiver();
    }
    if let Some(arr) = receiver_array_ptr(receiver_value) {
        return bool_value(
            crate::array::js_array_includes_jsvalue(arr, search, from_index, has_from) != 0,
        );
    }
    if let Some(arr) = receiver_string_array(receiver_value) {
        return bool_value(
            crate::array::js_array_includes_jsvalue(arr, search, from_index, has_from) != 0,
        );
    }
    let Some(receiver) = receiver_object_ptr(receiver_value) else {
        return bool_value(false);
    };
    let len = array_like_length(receiver) as i64;
    let Some(start) = from_index_forward(len, from_index, has_from != 0) else {
        return bool_value(false);
    };
    for i in start..len {
        let element = object_get_index(receiver, i as u32);
        if crate::value::js_jsvalue_same_value_zero(element, search) == 1 {
            return bool_value(true);
        }
    }
    bool_value(false)
}

#[no_mangle]
pub extern "C" fn js_array_like_reduce(
    receiver_value: f64,
    callback_value: f64,
    has_initial: i32,
    initial: f64,
) -> f64 {
    if is_nullish(receiver_value) {
        throw_nullish_receiver();
    }
    if let Some(arr) = receiver_array_ptr(receiver_value) {
        let callback = validate_callback(callback_value, 0);
        return crate::array::js_array_reduce(arr, callback, has_initial, initial);
    }
    if let Some(arr) = receiver_string_array(receiver_value) {
        let callback = validate_callback(callback_value, 0);
        return crate::array::js_array_reduce(arr, callback, has_initial, initial);
    }
    let callback = validate_callback(callback_value, 0);
    let Some(receiver) = receiver_object_ptr(receiver_value) else {
        if has_initial != 0 {
            return initial;
        }
        throw_reduce_of_empty();
    };
    let len = array_like_length(receiver);
    let (mut accumulator, start) = if has_initial != 0 {
        (initial, 0)
    } else {
        let mut seed = None;
        for i in 0..len {
            if object_has_index(receiver_value, receiver, i) {
                seed = Some((object_get_index(receiver, i), i + 1));
                break;
            }
        }
        match seed {
            Some(seed) => seed,
            None => throw_reduce_of_empty(),
        }
    };
    for i in start..len {
        if !object_has_index(receiver_value, receiver, i) {
            continue;
        }
        let element = object_get_index(receiver, i);
        accumulator = js_closure_call4(callback, accumulator, element, i as f64, receiver_value);
    }
    accumulator
}

#[no_mangle]
pub extern "C" fn js_array_like_reduce_right(
    receiver_value: f64,
    callback_value: f64,
    has_initial: i32,
    initial: f64,
) -> f64 {
    if is_nullish(receiver_value) {
        throw_nullish_receiver();
    }
    if let Some(arr) = receiver_array_ptr(receiver_value) {
        let callback = validate_callback(callback_value, 0);
        return crate::array::js_array_reduce_right(arr, callback, has_initial, initial);
    }
    if let Some(arr) = receiver_string_array(receiver_value) {
        let callback = validate_callback(callback_value, 0);
        return crate::array::js_array_reduce_right(arr, callback, has_initial, initial);
    }
    let callback = validate_callback(callback_value, 0);
    let Some(receiver) = receiver_object_ptr(receiver_value) else {
        if has_initial != 0 {
            return initial;
        }
        throw_reduce_of_empty();
    };
    let len = array_like_length(receiver);
    let (mut accumulator, start_exclusive) = if has_initial != 0 {
        (initial, len)
    } else {
        let mut seed = None;
        for i in (0..len).rev() {
            if object_has_index(receiver_value, receiver, i) {
                seed = Some((object_get_index(receiver, i), i));
                break;
            }
        }
        match seed {
            Some(seed) => seed,
            None => throw_reduce_of_empty(),
        }
    };
    if start_exclusive > 0 {
        for i in (0..start_exclusive).rev() {
            if !object_has_index(receiver_value, receiver, i) {
                continue;
            }
            let element = object_get_index(receiver, i);
            accumulator =
                js_closure_call4(callback, accumulator, element, i as f64, receiver_value);
        }
    }
    accumulator
}

#[no_mangle]
pub extern "C" fn js_array_like_slice(
    receiver_value: f64,
    start_value: f64,
    end_value: f64,
) -> *mut ArrayHeader {
    if is_nullish(receiver_value) {
        throw_nullish_receiver();
    }
    if let Some(arr) = receiver_array_ptr(receiver_value) {
        return crate::array::js_array_slice_values(arr, start_value, end_value);
    }
    if let Some(arr) = receiver_string_array(receiver_value) {
        return crate::array::js_array_slice_values(arr, start_value, end_value);
    }
    let Some(receiver) = receiver_object_ptr(receiver_value) else {
        return js_array_alloc(0);
    };
    let len = array_like_length(receiver);
    let start_idx = normalize_slice_index(len, slice_index(start_value, false), false);
    let end_idx = normalize_slice_index(len, slice_index(end_value, true), true);
    let slice_len = end_idx.saturating_sub(start_idx);
    let result = js_array_alloc_with_length(slice_len);
    let result_elements =
        unsafe { (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64 };
    for offset in 0..slice_len {
        let source_index = start_idx + offset;
        if !object_has_index(receiver_value, receiver, source_index) {
            continue;
        }
        let value = object_get_index(receiver, source_index);
        unsafe {
            ptr::write(result_elements.add(offset as usize), value);
        }
        let bits = value.to_bits();
        unsafe {
            if slice_len <= 64 {
                note_array_slot_layout_only(result, offset as usize, bits);
            } else {
                note_array_slot(result, offset as usize, bits);
            }
        }
    }
    result
}

#[no_mangle]
pub extern "C" fn js_array_like_join_value(receiver_value: f64, separator_value: f64) -> f64 {
    string_result_value(js_array_like_join(receiver_value, separator_value))
}

#[no_mangle]
pub extern "C" fn js_array_like_map_value(
    receiver_value: f64,
    callback_value: f64,
    this_arg: f64,
) -> f64 {
    array_result_value(js_array_like_map(receiver_value, callback_value, this_arg))
}

#[no_mangle]
pub extern "C" fn js_array_like_filter_value(
    receiver_value: f64,
    callback_value: f64,
    this_arg: f64,
) -> f64 {
    array_result_value(js_array_like_filter(
        receiver_value,
        callback_value,
        this_arg,
    ))
}

#[no_mangle]
pub extern "C" fn js_array_like_for_each_value(
    receiver_value: f64,
    callback_value: f64,
    this_arg: f64,
) -> f64 {
    js_array_like_for_each(receiver_value, callback_value, this_arg);
    undefined_value()
}

#[no_mangle]
pub extern "C" fn js_array_like_find_index_value(
    receiver_value: f64,
    callback_value: f64,
    this_arg: f64,
) -> f64 {
    js_array_like_find_index(receiver_value, callback_value, this_arg) as f64
}

#[no_mangle]
pub extern "C" fn js_array_like_index_of_value(
    receiver_value: f64,
    search: f64,
    from_index: f64,
    has_from: f64,
) -> f64 {
    js_array_like_index_of(
        receiver_value,
        search,
        from_index,
        if has_from != 0.0 { 1 } else { 0 },
    ) as f64
}

#[no_mangle]
pub extern "C" fn js_array_like_last_index_of_value(
    receiver_value: f64,
    search: f64,
    from_index: f64,
    has_from: f64,
) -> f64 {
    js_array_like_last_index_of(
        receiver_value,
        search,
        from_index,
        if has_from != 0.0 { 1 } else { 0 },
    ) as f64
}

#[no_mangle]
pub extern "C" fn js_array_like_includes_value(
    receiver_value: f64,
    search: f64,
    from_index: f64,
    has_from: f64,
) -> f64 {
    js_array_like_includes(
        receiver_value,
        search,
        from_index,
        if has_from != 0.0 { 1 } else { 0 },
    )
}

#[no_mangle]
pub extern "C" fn js_array_like_reduce_value(
    receiver_value: f64,
    callback_value: f64,
    has_initial: f64,
    initial: f64,
) -> f64 {
    js_array_like_reduce(
        receiver_value,
        callback_value,
        if has_initial != 0.0 { 1 } else { 0 },
        initial,
    )
}

#[no_mangle]
pub extern "C" fn js_array_like_reduce_right_value(
    receiver_value: f64,
    callback_value: f64,
    has_initial: f64,
    initial: f64,
) -> f64 {
    js_array_like_reduce_right(
        receiver_value,
        callback_value,
        if has_initial != 0.0 { 1 } else { 0 },
        initial,
    )
}

#[no_mangle]
pub extern "C" fn js_array_like_slice_value(
    receiver_value: f64,
    start_value: f64,
    end_value: f64,
) -> f64 {
    array_result_value(js_array_like_slice(receiver_value, start_value, end_value))
}

#[used]
static KEEP_ARRAY_LIKE_JOIN_VALUE: extern "C" fn(f64, f64) -> f64 = js_array_like_join_value;
#[used]
static KEEP_ARRAY_LIKE_MAP_VALUE: extern "C" fn(f64, f64, f64) -> f64 = js_array_like_map_value;
#[used]
static KEEP_ARRAY_LIKE_FILTER_VALUE: extern "C" fn(f64, f64, f64) -> f64 =
    js_array_like_filter_value;
#[used]
static KEEP_ARRAY_LIKE_FOR_EACH_VALUE: extern "C" fn(f64, f64, f64) -> f64 =
    js_array_like_for_each_value;
#[used]
static KEEP_ARRAY_LIKE_SOME: extern "C" fn(f64, f64, f64) -> f64 = js_array_like_some;
#[used]
static KEEP_ARRAY_LIKE_EVERY: extern "C" fn(f64, f64, f64) -> f64 = js_array_like_every;
#[used]
static KEEP_ARRAY_LIKE_FIND: extern "C" fn(f64, f64, f64) -> f64 = js_array_like_find;
#[used]
static KEEP_ARRAY_LIKE_FIND_INDEX_VALUE: extern "C" fn(f64, f64, f64) -> f64 =
    js_array_like_find_index_value;
#[used]
static KEEP_ARRAY_LIKE_INDEX_OF_VALUE: extern "C" fn(f64, f64, f64, f64) -> f64 =
    js_array_like_index_of_value;
#[used]
static KEEP_ARRAY_LIKE_LAST_INDEX_OF_VALUE: extern "C" fn(f64, f64, f64, f64) -> f64 =
    js_array_like_last_index_of_value;
#[used]
static KEEP_ARRAY_LIKE_INCLUDES_VALUE: extern "C" fn(f64, f64, f64, f64) -> f64 =
    js_array_like_includes_value;
#[used]
static KEEP_ARRAY_LIKE_REDUCE_VALUE: extern "C" fn(f64, f64, f64, f64) -> f64 =
    js_array_like_reduce_value;
#[used]
static KEEP_ARRAY_LIKE_REDUCE_RIGHT_VALUE: extern "C" fn(f64, f64, f64, f64) -> f64 =
    js_array_like_reduce_right_value;
#[used]
static KEEP_ARRAY_LIKE_SLICE_VALUE: extern "C" fn(f64, f64, f64) -> f64 = js_array_like_slice_value;
