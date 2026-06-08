use regex::Regex;

use super::{
    byte_index_to_char_index, char_index_to_byte, is_valid_ptr, is_valid_regex_ptr, js_regexp_new,
    js_string_from_str, set_exec_array_metadata, string_as_str, throw_match_all_non_global_regex,
    RegExpHeader,
};
use crate::array::ArrayHeader;
use crate::object::ObjectHeader;
use crate::string::StringHeader;
use crate::value::{
    js_nanbox_get_pointer, js_nanbox_pointer, js_nanbox_string, JSValue, TAG_UNDEFINED,
};

/// Class id for `String.prototype.matchAll`'s RegExp String Iterator object.
pub const REGEXP_STRING_ITERATOR_CLASS_ID: u32 = 0xFFFF_000A;

fn build_match_all_groups(
    regex: &Regex,
    caps: &regex::Captures<'_>,
    scope: &crate::gc::RuntimeHandleScope,
) -> f64 {
    let group_names: Vec<(&str, Option<regex::Match<'_>>)> = regex
        .capture_names()
        .enumerate()
        .filter_map(|(i, name)| name.map(|n| (n, caps.get(i))))
        .collect();

    if group_names.is_empty() {
        return f64::from_bits(TAG_UNDEFINED);
    }

    let groups_obj = crate::object::js_object_alloc(0, 0);
    let groups_handle = scope.root_raw_mut_ptr(groups_obj);
    for (name, m) in &group_names {
        let val = if let Some(m) = m {
            let str_ptr = js_string_from_str(m.as_str());
            js_nanbox_string(str_ptr as i64)
        } else {
            f64::from_bits(TAG_UNDEFINED)
        };
        let key_ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let groups_obj = groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
        crate::object::js_object_set_field_by_name(groups_obj, key_ptr, val);
    }
    let groups_obj = groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
    js_nanbox_pointer(groups_obj as i64)
}

fn set_match_all_groups(arr: *mut ArrayHeader, groups_value: f64) {
    let groups_key = js_string_from_str("groups");
    crate::array::js_array_set_string_key(arr, groups_key, groups_value);
}

unsafe fn materialize_match_all_results(
    s: *const StringHeader,
    re: *const RegExpHeader,
    start_char_index: usize,
) -> *mut ArrayHeader {
    if !is_valid_ptr(s) || !is_valid_regex_ptr(re) {
        return crate::array::js_array_alloc(0);
    }

    let str_data = string_as_str(s);
    let search_start = char_index_to_byte(str_data, start_char_index);
    let search_str = &str_data[search_start..];

    // Fancy-regex fallback (lookbehind/backreferences): the never-match
    // placeholder in `regex_ptr` would yield an empty iterator otherwise.
    // Mirrors the standard-engine loop below, including named-group `groups`.
    if let Some(fre) = super::lookup_fancy_regex(re) {
        let mut all_caps: Vec<fancy_regex::Captures> = Vec::new();
        let mut it = fre.captures_iter(search_str);
        while let Some(Ok(c)) = it.next() {
            all_caps.push(c);
        }
        let outer = crate::array::js_array_alloc(all_caps.len() as u32);
        let scope = crate::gc::RuntimeHandleScope::new();
        let outer_handle = scope.root_raw_mut_ptr(outer);
        (*outer_handle.get_raw_mut_ptr::<ArrayHeader>()).length = all_caps.len() as u32;

        for (i, caps) in all_caps.iter().enumerate() {
            let inner = crate::array::js_array_alloc(caps.len() as u32);
            let inner_handle = scope.root_raw_mut_ptr(inner);
            (*inner_handle.get_raw_mut_ptr::<ArrayHeader>()).length = caps.len() as u32;

            for j in 0..caps.len() {
                let value = if let Some(m) = caps.get(j) {
                    let str_ptr = js_string_from_str(m.as_str());
                    js_nanbox_string(str_ptr as i64)
                } else {
                    f64::from_bits(TAG_UNDEFINED)
                };
                let inner = inner_handle.get_raw_mut_ptr::<ArrayHeader>();
                crate::array::store_array_slot(inner, j, value.to_bits());
            }

            let match_index = caps
                .get(0)
                .map(|m| byte_index_to_char_index(str_data, search_start + m.start()))
                .unwrap_or_else(|| start_char_index as f64);
            let inner = inner_handle.get_raw_mut_ptr::<ArrayHeader>();
            set_exec_array_metadata(inner, str_data, match_index);
            let groups_ptr = super::build_fancy_groups(&fre, caps, &scope);
            let groups_value = if groups_ptr.is_null() {
                f64::from_bits(TAG_UNDEFINED)
            } else {
                js_nanbox_pointer(groups_ptr as i64)
            };
            set_match_all_groups(inner, groups_value);

            let inner_boxed = js_nanbox_pointer(inner as i64);
            let outer = outer_handle.get_raw_mut_ptr::<ArrayHeader>();
            crate::array::store_array_slot(outer, i, inner_boxed.to_bits());
        }

        return outer_handle.get_raw_mut_ptr::<ArrayHeader>();
    }

    let regex = &*(*re).regex_ptr;
    let all_caps: Vec<regex::Captures<'_>> = regex.captures_iter(search_str).collect();
    let outer = crate::array::js_array_alloc(all_caps.len() as u32);
    let scope = crate::gc::RuntimeHandleScope::new();
    let outer_handle = scope.root_raw_mut_ptr(outer);
    (*outer_handle.get_raw_mut_ptr::<ArrayHeader>()).length = all_caps.len() as u32;

    for (i, caps) in all_caps.iter().enumerate() {
        let inner = crate::array::js_array_alloc(caps.len() as u32);
        let inner_handle = scope.root_raw_mut_ptr(inner);
        (*inner_handle.get_raw_mut_ptr::<ArrayHeader>()).length = caps.len() as u32;

        for (j, cap) in caps.iter().enumerate() {
            let value = if let Some(m) = cap {
                let str_ptr = js_string_from_str(m.as_str());
                js_nanbox_string(str_ptr as i64)
            } else {
                f64::from_bits(TAG_UNDEFINED)
            };
            let inner = inner_handle.get_raw_mut_ptr::<ArrayHeader>();
            crate::array::store_array_slot(inner, j, value.to_bits());
        }

        let match_index = caps
            .get(0)
            .map(|m| byte_index_to_char_index(str_data, search_start + m.start()))
            .unwrap_or_else(|| start_char_index as f64);
        let inner = inner_handle.get_raw_mut_ptr::<ArrayHeader>();
        set_exec_array_metadata(inner, str_data, match_index);
        let groups_value = build_match_all_groups(regex, caps, &scope);
        set_match_all_groups(inner, groups_value);

        let inner_boxed = js_nanbox_pointer(inner as i64);
        let outer = outer_handle.get_raw_mut_ptr::<ArrayHeader>();
        crate::array::store_array_slot(outer, i, inner_boxed.to_bits());
    }

    outer_handle.get_raw_mut_ptr::<ArrayHeader>()
}

unsafe fn alloc_regexp_string_iterator(matches: *mut ArrayHeader) -> *mut ObjectHeader {
    let obj = crate::object::js_object_alloc(REGEXP_STRING_ITERATOR_CLASS_ID, 2);
    crate::object::js_object_set_field(
        obj,
        0,
        JSValue::from_bits(js_nanbox_pointer(matches as i64).to_bits()),
    );
    crate::object::js_object_set_field(obj, 1, JSValue::number(0.0));
    crate::object::attach_iterator_prototype(obj, REGEXP_STRING_ITERATOR_CLASS_ID);
    obj
}

fn match_all_pattern_to_regex(pattern_value: f64) -> *mut RegExpHeader {
    let pattern_jsval = JSValue::from_bits(pattern_value.to_bits());
    let pattern_ptr = if pattern_jsval.is_undefined() {
        js_string_from_str("")
    } else {
        crate::value::js_jsvalue_to_string(pattern_value)
    };
    let flags_ptr = js_string_from_str("g");
    js_regexp_new(
        pattern_ptr as *const StringHeader,
        flags_ptr as *const StringHeader,
    )
}

/// `String.prototype.matchAll` returns a RegExp String Iterator object.
#[no_mangle]
pub extern "C" fn js_string_match_all_value(
    s: *const StringHeader,
    pattern_value: f64,
) -> *mut ObjectHeader {
    if !is_valid_ptr(s) {
        let empty = crate::array::js_array_alloc(0);
        return unsafe { alloc_regexp_string_iterator(empty) };
    }

    let pattern_jsval = JSValue::from_bits(pattern_value.to_bits());
    let raw = if pattern_jsval.is_pointer() {
        js_nanbox_get_pointer(pattern_value)
    } else {
        0
    };
    let (re, start_index) = if raw != 0 && is_valid_regex_ptr(raw as *const RegExpHeader) {
        let re = raw as *const RegExpHeader;
        unsafe {
            if !(*re).global {
                throw_match_all_non_global_regex();
            }
            (re, crate::regex::regex_last_index_offset(re))
        }
    } else {
        (
            match_all_pattern_to_regex(pattern_value) as *const RegExpHeader,
            0,
        )
    };

    let matches = unsafe { materialize_match_all_results(s, re, start_index) };
    unsafe { alloc_regexp_string_iterator(matches) }
}

/// Compatibility entry point for older call sites that already hold a RegExp.
#[no_mangle]
pub extern "C" fn js_string_match_all(
    s: *const StringHeader,
    re: *const RegExpHeader,
) -> *mut ObjectHeader {
    if !is_valid_regex_ptr(re) {
        let empty = crate::array::js_array_alloc(0);
        return unsafe { alloc_regexp_string_iterator(empty) };
    }
    unsafe {
        if !(*re).global {
            throw_match_all_non_global_regex();
        }
        let matches =
            materialize_match_all_results(s, re, crate::regex::regex_last_index_offset(re));
        alloc_regexp_string_iterator(matches)
    }
}

unsafe fn regexp_string_iter_result(value: JSValue, done: bool) -> f64 {
    let obj = crate::object::js_object_alloc(0, 2);
    let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
    let done_key = crate::string::js_string_from_bytes(b"done".as_ptr(), 4);
    let keys = crate::array::js_array_alloc(2);
    crate::array::js_array_push(keys, JSValue::string_ptr(value_key));
    crate::array::js_array_push(keys, JSValue::string_ptr(done_key));
    crate::object::js_object_set_keys(obj, keys);
    crate::object::js_object_set_field(obj, 0, value);
    crate::object::js_object_set_field(obj, 1, JSValue::bool(done));
    js_nanbox_pointer(obj as i64)
}

pub unsafe fn dispatch_regexp_string_iterator_method(
    iter_obj: *mut ObjectHeader,
    method_name: &str,
) -> f64 {
    match method_name {
        "next" => {
            let backing = f64::from_bits(crate::object::js_object_get_field(iter_obj, 0).bits());
            let arr = js_nanbox_get_pointer(backing) as *const ArrayHeader;
            let idx = f64::from_bits(crate::object::js_object_get_field(iter_obj, 1).bits()) as u32;
            let len = if arr.is_null() {
                0
            } else {
                crate::array::js_array_length(arr)
            };
            if idx >= len {
                return regexp_string_iter_result(JSValue::undefined(), true);
            }
            crate::object::js_object_set_field(iter_obj, 1, JSValue::number((idx + 1) as f64));
            let elem = crate::array::js_array_get_f64(arr, idx);
            regexp_string_iter_result(JSValue::from_bits(elem.to_bits()), false)
        }
        "Symbol.iterator" | "@@iterator" => js_nanbox_pointer(iter_obj as i64),
        "return" | "throw" => regexp_string_iter_result(JSValue::undefined(), true),
        _ => f64::from_bits(TAG_UNDEFINED),
    }
}
