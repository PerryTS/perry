//! Miscellaneous global built-ins: TextEncoder/Decoder, encodeURI family,
//! `structuredClone`, `queueMicrotask` / `process.nextTick`.
//!
//! Split out of the original monolithic `builtins.rs` (#topic: split-large-files).

use super::*;

// ============================================================
// TextEncoder / TextDecoder
// ============================================================

/// TextEncoder.encode(string) -> Buffer (Uint8Array of UTF-8 bytes)
/// Takes a NaN-boxed string value and returns a raw buffer pointer.
#[no_mangle]
pub extern "C" fn js_text_encoder_encode(value: f64) -> i64 {
    use crate::buffer::js_buffer_from_string;
    let str_ptr = crate::value::js_get_string_pointer_unified(value);
    let buf = js_buffer_from_string(str_ptr as *const StringHeader, 0); // 0 = UTF-8
    buf as i64
}

/// TextDecoder.decode(buffer_ptr) -> string pointer (i64)
/// Takes a raw buffer/Uint8Array pointer (i64) and returns a StringHeader pointer.
#[no_mangle]
pub extern "C" fn js_text_decoder_decode(buf_ptr: i64) -> i64 {
    use crate::buffer::{js_buffer_to_string, BufferHeader};
    if buf_ptr == 0 || (buf_ptr as usize) < 0x1000 {
        return js_string_from_bytes(std::ptr::null(), 0) as i64;
    }
    let ptr = buf_ptr as *const BufferHeader;
    let str_ptr = js_buffer_to_string(ptr, 0); // 0 = UTF-8
    str_ptr as i64
}

// ============================================================
// encodeURI / decodeURI / encodeURIComponent / decodeURIComponent
// ============================================================

/// Characters that encodeURI does NOT encode (RFC 2396 unreserved + reserved)
const URI_UNESCAPED: &[u8] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.!~*'()";
const URI_RESERVED: &[u8] = b";/?:@&=+$,#";

/// Characters that encodeURIComponent does NOT encode (RFC 2396 unreserved only)
const URI_COMPONENT_UNESCAPED: &[u8] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.!~*'()";

fn percent_encode(input: &str, safe_chars: &[u8]) -> String {
    let mut result = String::with_capacity(input.len() * 3);
    for byte in input.as_bytes() {
        if safe_chars.contains(byte) {
            result.push(*byte as char);
        } else {
            result.push('%');
            result.push_str(&format!("{:02X}", byte));
        }
    }
    result
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = hex_digit(bytes[i + 1]);
            let lo = hex_digit(bytes[i + 2]);
            if let (Some(h), Some(l)) = (hi, lo) {
                result.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&result).into_owned()
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn extract_str_from_nanbox(value: f64) -> String {
    let str_ptr = crate::value::js_get_string_pointer_unified(value);
    if (str_ptr as usize) < 0x1000 {
        return String::new();
    }
    unsafe {
        let header = str_ptr as *const StringHeader;
        let len = (*header).byte_len as usize;
        let data = (header as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        std::str::from_utf8(bytes).unwrap_or("").to_string()
    }
}

/// encodeURI(string) -> string
#[no_mangle]
pub extern "C" fn js_encode_uri(value: f64) -> i64 {
    let input = extract_str_from_nanbox(value);
    let mut safe = Vec::with_capacity(URI_UNESCAPED.len() + URI_RESERVED.len());
    safe.extend_from_slice(URI_UNESCAPED);
    safe.extend_from_slice(URI_RESERVED);
    let encoded = percent_encode(&input, &safe);
    let ptr = js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32);
    ptr as i64
}

/// decodeURI(string) -> string
#[no_mangle]
pub extern "C" fn js_decode_uri(value: f64) -> i64 {
    let input = extract_str_from_nanbox(value);
    let decoded = percent_decode(&input);
    let ptr = js_string_from_bytes(decoded.as_ptr(), decoded.len() as u32);
    ptr as i64
}

/// encodeURIComponent(string) -> string
#[no_mangle]
pub extern "C" fn js_encode_uri_component(value: f64) -> i64 {
    let input = extract_str_from_nanbox(value);
    let encoded = percent_encode(&input, URI_COMPONENT_UNESCAPED);
    let ptr = js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32);
    ptr as i64
}

/// decodeURIComponent(string) -> string
#[no_mangle]
pub extern "C" fn js_decode_uri_component(value: f64) -> i64 {
    let input = extract_str_from_nanbox(value);
    let decoded = percent_decode(&input);
    let ptr = js_string_from_bytes(decoded.as_ptr(), decoded.len() as u32);
    ptr as i64
}

// ============================================================
// structuredClone
// ============================================================

/// structuredClone(value) -> deep-cloned value
/// Handles numbers (pass-through), strings (copy), arrays/objects (shallow for now)
#[no_mangle]
pub extern "C" fn js_structured_clone(value: f64) -> f64 {
    let bits = value.to_bits();
    // Pass through primitives (undefined, null, true, false)
    if bits == 0x7FFC_0000_0000_0001
        || bits == 0x7FFC_0000_0000_0002
        || bits == 0x7FFC_0000_0000_0003
        || bits == 0x7FFC_0000_0000_0004
    {
        return value;
    }
    // Regular f64 numbers pass through
    let tag = (bits >> 48) as u16;
    if tag < 0x7FF8 {
        return value;
    }

    match tag {
        0x7FFF => {
            // STRING_TAG — copy the string
            let str_ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
            if (str_ptr as usize) < 0x1000 {
                return value;
            }
            unsafe {
                let len = (*str_ptr).byte_len as usize;
                let data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let new_str = js_string_from_bytes(data, len as u32);
                let new_bits = 0x7FFF_0000_0000_0000u64 | (new_str as u64 & 0x0000_FFFF_FFFF_FFFF);
                f64::from_bits(new_bits)
            }
        }
        0x7FFE => {
            // INT32_TAG — pass through
            value
        }
        0x7FFD => {
            // POINTER_TAG — could be array/object/Map/Set/RegExp. Deep clone recursively.
            let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const u8;
            if (ptr as usize) < 0x10000 {
                return value;
            }
            // Set is tracked in SET_REGISTRY (not GC_TYPE_SET since it has
            // no GC header). Check the registry BEFORE touching the GC
            // header bytes — they'd be garbage for raw-allocated sets.
            if crate::set::is_registered_set(ptr as usize) {
                unsafe {
                    let src = ptr as *const crate::set::SetHeader;
                    let size = crate::set::js_set_size(src);
                    let new_set = crate::set::js_set_alloc(size.max(8));
                    let arr = crate::set::js_set_to_array(src);
                    let len = (*arr).length as usize;
                    let data = (arr as *const u8)
                        .add(std::mem::size_of::<crate::array::ArrayHeader>())
                        as *const f64;
                    for i in 0..len {
                        let v = js_structured_clone(*data.add(i));
                        crate::set::js_set_add(new_set, v);
                    }
                    let new_bits =
                        0x7FFD_0000_0000_0000u64 | (new_set as u64 & 0x0000_FFFF_FFFF_FFFF);
                    return f64::from_bits(new_bits);
                }
            }
            unsafe {
                // GcHeader is stored BEFORE the user pointer (at ptr - GC_HEADER_SIZE)
                let gc_header_ptr = (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE);
                let gc_type = *gc_header_ptr;
                if gc_type == crate::gc::GC_TYPE_ARRAY {
                    // Clone array using existing clone, then recursively clone elements
                    let arr = ptr as *const crate::array::ArrayHeader;
                    let new_arr = crate::array::js_array_clone(arr);
                    let len = (*new_arr).length;
                    let elements = (new_arr as *mut u8)
                        .add(std::mem::size_of::<crate::array::ArrayHeader>())
                        as *mut f64;
                    for i in 0..len as usize {
                        let elem = *elements.add(i);
                        let cloned = js_structured_clone(elem);
                        *elements.add(i) = cloned;
                        crate::array::note_array_slot(new_arr, i, cloned.to_bits());
                    }
                    let new_bits =
                        0x7FFD_0000_0000_0000u64 | (new_arr as u64 & 0x0000_FFFF_FFFF_FFFF);
                    f64::from_bits(new_bits)
                } else if gc_type == crate::gc::GC_TYPE_OBJECT {
                    // Check if this is a RegExp (the RegExpHeader lives in an
                    // arena slot with GC_TYPE_OBJECT but tracked in
                    // REGEX_POINTERS). Clone by reading source/flags and
                    // building a fresh one via js_regexp_new.
                    if crate::regex::is_regex_pointer(ptr as *const u8) {
                        let re_ptr = ptr as *const crate::regex::RegExpHeader;
                        let src = crate::regex::js_regexp_get_source(re_ptr);
                        let flg = crate::regex::js_regexp_get_flags(re_ptr);
                        let new_re = crate::regex::js_regexp_new(src, flg);
                        let new_bits =
                            0x7FFD_0000_0000_0000u64 | (new_re as u64 & 0x0000_FFFF_FFFF_FFFF);
                        return f64::from_bits(new_bits);
                    }
                    // Clone object using clone_with_extra (0 extra fields, no static keys)
                    let cloned_obj =
                        crate::object::js_object_clone_with_extra(value, 0, std::ptr::null(), 0);
                    if !cloned_obj.is_null() && (cloned_obj as usize) > 0x10000 {
                        let field_count = (*cloned_obj).field_count;
                        let fields = (cloned_obj as *mut u8)
                            .add(std::mem::size_of::<crate::object::ObjectHeader>())
                            as *mut f64;
                        for i in 0..field_count as usize {
                            let field = *fields.add(i);
                            *fields.add(i) = js_structured_clone(field);
                        }
                    }
                    // NaN-box with POINTER_TAG
                    let new_bits =
                        0x7FFD_0000_0000_0000u64 | (cloned_obj as u64 & 0x0000_FFFF_FFFF_FFFF);
                    f64::from_bits(new_bits)
                } else if gc_type == crate::gc::GC_TYPE_MAP {
                    // Deep-clone a Map by building a fresh one and copying
                    // entries through js_map_set (which handles the hash
                    // bucket + entries array layout).
                    let map = ptr as *const crate::map::MapHeader;
                    let size = crate::map::js_map_size(map);
                    let new_map = crate::map::js_map_alloc(size.max(8));
                    // Walk entries via js_map_entries which returns an
                    // Array<[key, value]> pair array.
                    let entries_arr = crate::map::js_map_entries(map);
                    let entries_len = (*entries_arr).length as usize;
                    let entries_data = (entries_arr as *const u8)
                        .add(std::mem::size_of::<crate::array::ArrayHeader>())
                        as *const f64;
                    for i in 0..entries_len {
                        let pair_box = *entries_data.add(i);
                        let pair_bits = pair_box.to_bits();
                        let pair_ptr =
                            (pair_bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::array::ArrayHeader;
                        if pair_ptr.is_null() {
                            continue;
                        }
                        let pair_data = (pair_ptr as *const u8)
                            .add(std::mem::size_of::<crate::array::ArrayHeader>())
                            as *const f64;
                        let k = js_structured_clone(*pair_data);
                        let v = js_structured_clone(*pair_data.add(1));
                        crate::map::js_map_set(new_map, k, v);
                    }
                    let new_bits =
                        0x7FFD_0000_0000_0000u64 | (new_map as u64 & 0x0000_FFFF_FFFF_FFFF);
                    f64::from_bits(new_bits)
                } else {
                    // Unknown pointer type — pass through
                    value
                }
            }
        }
        _ => value,
    }
}

// ============================================================
// queueMicrotask
// ============================================================

/// queueMicrotask(callback) — schedule a closure on the microtask queue.
/// The closure runs during the next `js_promise_run_microtasks()` drain,
/// AFTER the current synchronous code completes. Previously this called
/// the closure immediately, which broke the JS spec ordering:
///   queueMicrotask(() => log("micro"));
///   log("sync");
/// should print "sync" then "micro", not "micro" then "sync".
#[no_mangle]
pub extern "C" fn js_queue_microtask(callback: i64) {
    queue_microtask_with_type(callback, "Microtask");
}

#[no_mangle]
pub extern "C" fn js_queue_next_tick(callback: i64) {
    queue_microtask_with_type(callback, "TickObject");
}

fn queue_microtask_with_type(callback: i64, type_name: &str) {
    let context = crate::async_context::capture_context();
    let ids = crate::async_hooks::init_resource(
        type_name,
        f64::from_bits(crate::value::TAG_UNDEFINED),
        false,
    );
    QUEUED_MICROTASKS.with(|q| {
        q.borrow_mut()
            .push((callback, context, ids.async_id, ids.trigger_async_id));
    });
}

thread_local! {
    static QUEUED_MICROTASKS: std::cell::RefCell<Vec<(i64, crate::async_context::AsyncContextSnapshot, u64, u64)>> = const { std::cell::RefCell::new(Vec::new()) };
    static QUEUED_MICROTASK_PREV_CONTEXTS: std::cell::RefCell<Vec<crate::async_context::AsyncContextSnapshot>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn restore_queued_microtask_contexts() {
    QUEUED_MICROTASK_PREV_CONTEXTS.with(|stack| {
        let mut stack = stack.borrow_mut();
        while let Some(previous) = stack.pop() {
            crate::async_context::restore_context(previous);
        }
    });
}

/// Drain queued microtasks. Called by `js_promise_run_microtasks`.
#[no_mangle]
pub extern "C" fn js_drain_queued_microtasks() {
    use crate::closure::js_closure_call0;
    loop {
        let task = QUEUED_MICROTASKS.with(|q| {
            let mut queue = q.borrow_mut();
            if queue.is_empty() {
                None
            } else {
                Some(queue.remove(0))
            }
        });
        match task {
            Some((cb, context, async_id, trigger_async_id)) => {
                let previous = crate::async_context::enter_context(&context);
                QUEUED_MICROTASK_PREV_CONTEXTS.with(|stack| {
                    stack.borrow_mut().push(previous);
                });
                crate::async_hooks::before(async_id, trigger_async_id);
                js_closure_call0(cb as *const crate::closure::ClosureHeader);
                crate::async_hooks::after(async_id);
                crate::async_hooks::destroy(async_id);
                QUEUED_MICROTASK_PREV_CONTEXTS.with(|stack| {
                    if let Some(previous) = stack.borrow_mut().pop() {
                        crate::async_context::restore_context(previous);
                    }
                });
            }
            None => break,
        }
    }
}

pub fn scan_queued_microtask_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_queued_microtask_roots_mut(&mut visitor);
}

pub fn scan_queued_microtask_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    QUEUED_MICROTASKS.with(|q| {
        for (callback, context, _, _) in q.borrow_mut().iter_mut() {
            visitor.visit_i64_slot(callback);
            crate::async_context::scan_snapshot_roots_mut(context, visitor);
        }
    });
}

#[cfg(test)]
pub(crate) fn test_seed_queued_microtask(callback: i64, context_store: f64) {
    let context = crate::async_context::test_snapshot_with_store(context_store);
    QUEUED_MICROTASKS.with(|q| {
        let mut q = q.borrow_mut();
        q.clear();
        q.push((callback, context, 0, 0));
    });
}

#[cfg(test)]
pub(crate) fn test_queued_microtask_snapshot() -> (usize, u64) {
    QUEUED_MICROTASKS.with(|q| {
        let q = q.borrow();
        let Some((callback, context, _, _)) = q.first() else {
            return (0, 0);
        };
        let store_bits = crate::async_context::test_snapshot_first_store(context)
            .map(f64::to_bits)
            .unwrap_or(0);
        (*callback as usize, store_bits)
    })
}
