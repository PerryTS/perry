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

// Cycle-detection state for `js_structured_clone` (#1512). Tracks the source
// pointers currently mid-clone on this thread. On re-entry for a pointer
// already in the set, we return the original value rather than recursing
// — that breaks the spec's "preserve reference identity" guarantee but
// keeps cycles from infinite-recursing into a stack overflow, which is
// what previously caused `performance.mark("n", { detail: o.self = o })`
// to crash. Full reference-identity preservation would need a src→dst
// map; deferred until a real user-facing need surfaces.
thread_local! {
    static STRUCTURED_CLONE_IN_PROGRESS: std::cell::RefCell<std::collections::HashSet<usize>>
        = std::cell::RefCell::new(std::collections::HashSet::new());
}

fn structured_clone_seen(ptr: usize) -> bool {
    STRUCTURED_CLONE_IN_PROGRESS.with(|set| set.borrow().contains(&ptr))
}

fn structured_clone_mark(ptr: usize) {
    STRUCTURED_CLONE_IN_PROGRESS.with(|set| {
        set.borrow_mut().insert(ptr);
    });
}

fn structured_clone_unmark(ptr: usize) {
    STRUCTURED_CLONE_IN_PROGRESS.with(|set| {
        set.borrow_mut().remove(&ptr);
    });
}

/// RAII guard that unmarks a pointer from the in-progress set when dropped,
/// even on early returns from `js_structured_clone`'s POINTER_TAG branches.
struct CloneCycleGuard(usize);
impl Drop for CloneCycleGuard {
    fn drop(&mut self) {
        structured_clone_unmark(self.0);
    }
}

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
            // #1512: short-circuit on cycle so `o.self = o` doesn't infinite-
            // recurse. The cycle edge resolves to the original value, not
            // the clone — that breaks full reference-identity preservation
            // but keeps cycles from stack-overflowing the runtime.
            if structured_clone_seen(ptr as usize) {
                return value;
            }
            structured_clone_mark(ptr as usize);
            let _guard = CloneCycleGuard(ptr as usize);
            // Set is tracked in SET_REGISTRY (not GC_TYPE_SET since it has
            // no GC header). Check the registry BEFORE touching the GC
            // header bytes — they'd be garbage for raw-allocated sets.
            if crate::set::is_registered_set(ptr as usize) {
                let src = ptr as *const crate::set::SetHeader;
                let size = crate::set::js_set_size(src);
                let scope = crate::gc::RuntimeHandleScope::new();
                let src_handle = scope.root_raw_const_ptr(src);
                let new_set = crate::set::js_set_alloc(size.max(8));
                let new_set_handle = scope.root_raw_mut_ptr(new_set);
                for i in 0..size {
                    let src_now = src_handle.get_raw_const_ptr::<crate::set::SetHeader>();
                    let elem = crate::set::js_set_value_at(src_now, i);
                    let v = js_structured_clone(elem);
                    let new_set_now = new_set_handle.get_raw_mut_ptr::<crate::set::SetHeader>();
                    crate::set::js_set_add(new_set_now, v);
                }
                let new_set = new_set_handle.get_raw_mut_ptr::<crate::set::SetHeader>();
                let new_bits = 0x7FFD_0000_0000_0000u64 | (new_set as u64 & 0x0000_FFFF_FFFF_FFFF);
                return f64::from_bits(new_bits);
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
                    let scope = crate::gc::RuntimeHandleScope::new();
                    let map_handle = scope.root_raw_const_ptr(ptr as *const crate::map::MapHeader);
                    let size = crate::map::js_map_size(
                        map_handle.get_raw_const_ptr::<crate::map::MapHeader>(),
                    );
                    let new_map = crate::map::js_map_alloc(size.max(8));
                    let new_map_handle = scope.root_raw_mut_ptr(new_map);
                    // Walk entries via js_map_entries which returns an
                    // Array<[key, value]> pair array.
                    let entries_arr = crate::map::js_map_entries(
                        map_handle.get_raw_const_ptr::<crate::map::MapHeader>(),
                    );
                    let entries_handle = scope.root_raw_mut_ptr(entries_arr);
                    let entries_len = crate::array::js_array_length(
                        entries_handle.get_raw_const_ptr::<crate::array::ArrayHeader>(),
                    ) as usize;
                    for i in 0..entries_len {
                        let entries_arr =
                            entries_handle.get_raw_const_ptr::<crate::array::ArrayHeader>();
                        let pair_box = crate::array::js_array_get_f64(entries_arr, i as u32);
                        let pair_bits = pair_box.to_bits();
                        let pair_ptr =
                            (pair_bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::array::ArrayHeader;
                        if pair_ptr.is_null() {
                            continue;
                        }
                        let entry_scope = crate::gc::RuntimeHandleScope::new();
                        let pair_handle = entry_scope.root_raw_const_ptr(pair_ptr);
                        let pair_now = pair_handle.get_raw_const_ptr::<crate::array::ArrayHeader>();
                        let key_handle = entry_scope
                            .root_nanbox_f64(crate::array::js_array_get_f64(pair_now, 0));
                        let cloned_key = js_structured_clone(key_handle.get_nanbox_f64());
                        key_handle.set_nanbox_f64(cloned_key);

                        let pair_now = pair_handle.get_raw_const_ptr::<crate::array::ArrayHeader>();
                        let value_handle = entry_scope
                            .root_nanbox_f64(crate::array::js_array_get_f64(pair_now, 1));
                        let cloned_value = js_structured_clone(value_handle.get_nanbox_f64());
                        value_handle.set_nanbox_f64(cloned_value);

                        let new_map = new_map_handle.get_raw_mut_ptr::<crate::map::MapHeader>();
                        crate::map::js_map_set(
                            new_map,
                            key_handle.get_nanbox_f64(),
                            value_handle.get_nanbox_f64(),
                        );
                    }
                    let new_map = new_map_handle.get_raw_mut_ptr::<crate::map::MapHeader>();
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
    queue_microtask_with_type(callback, "Microtask", Vec::new());
}

#[no_mangle]
pub extern "C" fn js_queue_next_tick(callback: i64) {
    queue_microtask_with_type(callback, "TickObject", Vec::new());
}

/// process.nextTick(cb, ...args) — forwards trailing args to `cb` when the
/// tick fires (#1351). `args_ptr`/`n_args` describe a NaN-boxed-f64 buffer
/// allocated on the caller's stack; we copy the slice eagerly because the
/// drain runs after the caller returns.
///
/// # Safety
/// `args_ptr` must point to `n_args` valid `f64` values, or be null if
/// `n_args == 0`.
#[no_mangle]
pub unsafe extern "C" fn js_queue_next_tick_args(callback: i64, args_ptr: *const f64, n_args: i32) {
    let args: Vec<f64> = if args_ptr.is_null() || n_args <= 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(args_ptr, n_args as usize).to_vec()
    };
    queue_microtask_with_type(callback, "TickObject", args);
}

fn queue_microtask_with_type(callback: i64, type_name: &str, args: Vec<f64>) {
    let context = crate::async_context::capture_context();
    let ids = crate::async_hooks::init_resource(
        type_name,
        f64::from_bits(crate::value::TAG_UNDEFINED),
        false,
    );
    QUEUED_MICROTASKS.with(|q| {
        q.borrow_mut().push(QueuedMicrotask {
            callback,
            context,
            async_id: ids.async_id,
            trigger_async_id: ids.trigger_async_id,
            args,
        });
    });
}

pub(crate) struct QueuedMicrotask {
    pub callback: i64,
    pub context: crate::async_context::AsyncContextSnapshot,
    pub async_id: u64,
    pub trigger_async_id: u64,
    pub args: Vec<f64>,
}

thread_local! {
    static QUEUED_MICROTASKS: std::cell::RefCell<Vec<QueuedMicrotask>> = const { std::cell::RefCell::new(Vec::new()) };
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
    use crate::closure::{
        js_closure_call0, js_closure_call1, js_closure_call2, js_closure_call3, js_closure_call4,
        js_closure_call5, js_closure_call6, js_closure_call7, js_closure_call8, js_closure_call9,
    };
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
            Some(QueuedMicrotask {
                callback: cb,
                context,
                async_id,
                trigger_async_id,
                args,
            }) => {
                let scope = crate::gc::RuntimeHandleScope::new();
                let callback_handle =
                    scope.root_raw_const_ptr(cb as *const crate::closure::ClosureHeader);
                let arg_handles = scope.root_nanbox_f64_slice(&args);
                let previous = crate::async_context::enter_context(&context);
                QUEUED_MICROTASK_PREV_CONTEXTS.with(|stack| {
                    stack.borrow_mut().push(previous);
                });
                crate::async_hooks::before(async_id, trigger_async_id);
                let a = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
                let cb_ptr = callback_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>();
                match a.len() {
                    0 => {
                        js_closure_call0(cb_ptr);
                    }
                    1 => {
                        js_closure_call1(cb_ptr, a[0]);
                    }
                    2 => {
                        js_closure_call2(cb_ptr, a[0], a[1]);
                    }
                    3 => {
                        js_closure_call3(cb_ptr, a[0], a[1], a[2]);
                    }
                    4 => {
                        js_closure_call4(cb_ptr, a[0], a[1], a[2], a[3]);
                    }
                    5 => {
                        js_closure_call5(cb_ptr, a[0], a[1], a[2], a[3], a[4]);
                    }
                    6 => {
                        js_closure_call6(cb_ptr, a[0], a[1], a[2], a[3], a[4], a[5]);
                    }
                    7 => {
                        js_closure_call7(cb_ptr, a[0], a[1], a[2], a[3], a[4], a[5], a[6]);
                    }
                    8 => {
                        js_closure_call8(cb_ptr, a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7]);
                    }
                    _ => {
                        // >= 9 args: clamp to 9. Mirrors the setTimeout
                        // dispatch fallback; real-world nextTick rarely
                        // exceeds 1-2 trailing args.
                        js_closure_call9(
                            cb_ptr, a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8],
                        );
                    }
                }
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
        for task in q.borrow_mut().iter_mut() {
            visitor.visit_i64_slot(&mut task.callback);
            crate::async_context::scan_snapshot_roots_mut(&mut task.context, visitor);
            // #1351: trailing nextTick args may be heap pointers — keep
            // them rooted alongside the callback closure.
            for arg in task.args.iter_mut() {
                visitor.visit_nanbox_f64_slot(arg);
            }
        }
    });
    QUEUED_MICROTASK_PREV_CONTEXTS.with(|stack| {
        for context in stack.borrow_mut().iter_mut() {
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
        q.push(QueuedMicrotask {
            callback,
            context,
            async_id: 0,
            trigger_async_id: 0,
            args: Vec::new(),
        });
    });
    QUEUED_MICROTASK_PREV_CONTEXTS.with(|stack| stack.borrow_mut().clear());
}

#[cfg(test)]
pub(crate) fn test_seed_queued_microtask_previous_context(context_store: f64) {
    let context = crate::async_context::test_snapshot_with_store(context_store);
    QUEUED_MICROTASK_PREV_CONTEXTS.with(|stack| {
        let mut stack = stack.borrow_mut();
        stack.clear();
        stack.push(context);
    });
}

#[cfg(test)]
pub(crate) fn test_queued_microtask_snapshot() -> (usize, u64, u64) {
    QUEUED_MICROTASKS.with(|q| {
        let q = q.borrow();
        let (callback, store_bits) = q
            .first()
            .map(|task| {
                (
                    task.callback as usize,
                    crate::async_context::test_snapshot_first_store(&task.context)
                        .map(f64::to_bits)
                        .unwrap_or(0),
                )
            })
            .unwrap_or((0, 0));
        let previous_store_bits = QUEUED_MICROTASK_PREV_CONTEXTS.with(|stack| {
            stack
                .borrow()
                .first()
                .and_then(crate::async_context::test_snapshot_first_store)
                .map(f64::to_bits)
                .unwrap_or(0)
        });
        (callback, store_bits, previous_store_bits)
    })
}
