use super::{
    byte_index_to_utf16_index, is_valid_ptr, is_valid_regex_ptr, js_regexp_new, js_string_from_str,
    set_exec_array_metadata_value, string_as_str, throw_match_all_non_global_regex,
    utf16_index_to_byte, RegExpHeader,
};
use crate::array::ArrayHeader;
use crate::object::ObjectHeader;
use crate::string::StringHeader;
use crate::value::{
    js_nanbox_get_pointer, js_nanbox_pointer, js_nanbox_string, JSValue, TAG_UNDEFINED,
};

/// Class id for `String.prototype.matchAll`'s RegExp String Iterator object.
/// Re-exported from the parent (kept ungated there so always-linked iterator
/// dispatch can reference it even when this engine module is gated out).
use super::REGEXP_STRING_ITERATOR_CLASS_ID;

/// Owned, GC-inert snapshot of one matchAll result, copied OUT of the subject
/// string before the allocating phase below. There is no user callback here —
/// sweeping isn't the risk (internal allocations only), MOVING is: an
/// alloc-point minor can be moving under the evacuation policy, and both a
/// cached `&str` and the `Captures` borrowing it would silently read
/// from-space after the subject relocates (2026-07-09 audit, wave 1).
struct OwnedMatchAllData {
    /// Group 0 (full match) + capture groups (None = non-participating).
    groups: Vec<Option<String>>,
    /// Named groups in declaration order: (name, text).
    named: Vec<(String, Option<String>)>,
    /// Char index of the match start in the full subject.
    match_index: f64,
}

/// Build the named-capture `groups` object from an owned snapshot, or return
/// `undefined` when the pattern declares no named groups.
fn build_match_all_groups_owned(
    named: &[(String, Option<String>)],
    scope: &crate::gc::RuntimeHandleScope,
) -> f64 {
    if named.is_empty() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let groups_obj = crate::object::js_object_alloc(0, 0);
    let groups_handle = scope.root_raw_mut_ptr(groups_obj);
    for (name, text) in named {
        let val = match text {
            Some(t) => js_nanbox_string(js_string_from_str(t) as i64),
            None => f64::from_bits(TAG_UNDEFINED),
        };
        // Root the value across the key allocation below.
        let val_handle = scope.root_nanbox_f64(val);
        let key_ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let groups_obj = groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
        crate::object::js_object_set_field_by_name(
            groups_obj,
            key_ptr,
            val_handle.get_nanbox_f64(),
        );
    }
    js_nanbox_pointer(groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>() as i64)
}

fn set_match_all_groups(arr: *mut ArrayHeader, groups_value: f64) {
    // Root both sides across the key-string allocation (a moving minor at
    // that point would leave either raw local stale).
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(arr);
    let groups_handle = scope.root_nanbox_f64(groups_value);
    let groups_key = js_string_from_str("groups");
    crate::array::js_array_set_string_key(
        arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
        groups_key,
        groups_handle.get_nanbox_f64(),
    );
}

unsafe fn materialize_match_all_results(
    s: *const StringHeader,
    re: *const RegExpHeader,
    start_char_index: usize,
) -> *mut ArrayHeader {
    if !is_valid_ptr(s) || !is_valid_regex_ptr(re) {
        return crate::array::js_array_alloc(0);
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let s_handle = scope.root_string_ptr(s);

    // Phase 1 (borrowing, no JS allocation): snapshot every match into owned
    // Rust data. The fancy-regex fallback (lookbehind/backreferences) is
    // needed because the never-match placeholder in `regex_ptr` would yield
    // an empty iterator otherwise.
    let str_data = string_as_str(s);
    let search_start = utf16_index_to_byte(str_data, start_char_index);
    let search_str = &str_data[search_start..];

    let mut owned: Vec<OwnedMatchAllData> = Vec::new();
    if let Some(fre) = super::lookup_fancy_regex(re) {
        let named_names: Vec<(usize, String)> = fre
            .capture_names()
            .enumerate()
            .filter_map(|(i, name)| name.map(|n| (i, n.to_string())))
            .collect();
        let mut it = fre.captures_iter(search_str);
        while let Some(Ok(caps)) = it.next() {
            owned.push(OwnedMatchAllData {
                groups: (0..caps.len())
                    .map(|j| caps.get(j).map(|m| m.as_str().to_string()))
                    .collect(),
                named: named_names
                    .iter()
                    .map(|(gi, n)| (n.clone(), caps.get(*gi).map(|m| m.as_str().to_string())))
                    .collect(),
                match_index: caps
                    .get(0)
                    .map(|m| byte_index_to_utf16_index(str_data, search_start + m.start()) as f64)
                    .unwrap_or(start_char_index as f64),
            });
        }
    } else {
        let regex = &*(*re).regex_ptr;
        let named_names: Vec<(usize, String)> = regex
            .capture_names()
            .enumerate()
            .filter_map(|(i, name)| name.map(|n| (i, n.to_string())))
            .collect();
        for caps in regex.captures_iter(search_str) {
            owned.push(OwnedMatchAllData {
                groups: (0..caps.len())
                    .map(|j| caps.get(j).map(|m| m.as_str().to_string()))
                    .collect(),
                named: named_names
                    .iter()
                    .map(|(gi, n)| (n.clone(), caps.get(*gi).map(|m| m.as_str().to_string())))
                    .collect(),
                match_index: caps
                    .get(0)
                    .map(|m| byte_index_to_utf16_index(str_data, search_start + m.start()) as f64)
                    .unwrap_or(start_char_index as f64),
            });
        }
    }

    // Phase 2 (allocating, no borrows into the subject): build the result
    // arrays from the owned snapshots. Every heap pointer lives in a rooted
    // handle and is re-derived after each allocation; the `input` metadata
    // is the rooted subject itself (same string value, current address).
    let outer = crate::array::js_array_alloc(owned.len() as u32);
    let outer_handle = scope.root_raw_mut_ptr(outer);
    (*outer_handle.get_raw_mut_ptr::<ArrayHeader>()).length = owned.len() as u32;

    for (i, m) in owned.iter().enumerate() {
        let match_scope = crate::gc::RuntimeHandleScope::new();
        let inner = crate::array::js_array_alloc(m.groups.len() as u32);
        let inner_handle = match_scope.root_raw_mut_ptr(inner);
        (*inner_handle.get_raw_mut_ptr::<ArrayHeader>()).length = m.groups.len() as u32;

        for (j, group) in m.groups.iter().enumerate() {
            let value = match group {
                Some(text) => js_nanbox_string(js_string_from_str(text) as i64),
                None => f64::from_bits(TAG_UNDEFINED),
            };
            let inner = inner_handle.get_raw_mut_ptr::<ArrayHeader>();
            crate::array::store_array_slot(inner, j, value.to_bits());
        }

        set_exec_array_metadata_value(
            inner_handle.get_raw_mut_ptr::<ArrayHeader>(),
            js_nanbox_string(s_handle.get_raw_const_ptr::<StringHeader>() as i64),
            m.match_index,
        );
        let groups_value = build_match_all_groups_owned(&m.named, &match_scope);
        set_match_all_groups(inner_handle.get_raw_mut_ptr::<ArrayHeader>(), groups_value);

        let inner_boxed = js_nanbox_pointer(inner_handle.get_raw_mut_ptr::<ArrayHeader>() as i64);
        let outer = outer_handle.get_raw_mut_ptr::<ArrayHeader>();
        crate::array::store_array_slot(outer, i, inner_boxed.to_bits());
    }

    outer_handle.get_raw_mut_ptr::<ArrayHeader>()
}

unsafe fn alloc_regexp_string_iterator(matches: *mut ArrayHeader) -> *mut ObjectHeader {
    // Root the matches array across the iterator-object allocation.
    let scope = crate::gc::RuntimeHandleScope::new();
    let matches_handle = scope.root_raw_mut_ptr(matches);
    let obj = crate::object::js_object_alloc(REGEXP_STRING_ITERATOR_CLASS_ID, 2);
    crate::object::js_object_set_field(
        obj,
        0,
        JSValue::from_bits(
            js_nanbox_pointer(matches_handle.get_raw_mut_ptr::<ArrayHeader>() as i64).to_bits(),
        ),
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

    // Root the subject across the pattern→RegExp conversion below, which
    // allocates (ToString of the pattern, the "g" flags string, the RegExp
    // registration) before `materialize_match_all_results` roots it again.
    let scope = crate::gc::RuntimeHandleScope::new();
    let s_handle = scope.root_string_ptr(s);

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

    let matches = unsafe {
        materialize_match_all_results(
            s_handle.get_raw_const_ptr::<StringHeader>(),
            re,
            start_index,
        )
    };
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
