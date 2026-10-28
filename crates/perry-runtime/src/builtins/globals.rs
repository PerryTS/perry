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
    let str_ptr = crate::text::text_encoder_string_ptr(value);
    let buf = js_buffer_from_string(str_ptr, 0); // 0 = UTF-8
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

fn percent_encode(input: &[u8], safe_chars: &[u8]) -> String {
    let mut result = String::with_capacity(input.len() * 3);
    for byte in input {
        if safe_chars.contains(byte) {
            result.push(*byte as char);
        } else {
            result.push('%');
            result.push_str(&format!("{:02X}", byte));
        }
    }
    result
}

/// Read a `%XX` escape at byte offset `i`, returning the decoded octet.
/// Fails (URIError) when there is no `%`, the string is too short, or the two
/// following code units are not hexadecimal digits.
fn read_pct_octet(bytes: &[u8], i: usize) -> Result<u8, ()> {
    if i + 2 >= bytes.len() || bytes[i] != b'%' {
        return Err(());
    }
    match (hex_digit(bytes[i + 1]), hex_digit(bytes[i + 2])) {
        (Some(h), Some(l)) => Ok(h * 16 + l),
        _ => Err(()),
    }
}

/// ECMAScript `Decode` (sec-decode), operating on the input's WTF-8 bytes so
/// lone surrogates pass through unchanged. `reserved` is the set whose
/// single-octet members are left as their original `%XX` escape (decodeURI);
/// decodeURIComponent passes `None`.
///
/// Unlike a naive "decode every `%XX`, then validate the whole buffer", this
/// validates each multi-octet UTF-8 run at the point it is decoded: a lead
/// octet must be followed by the correct number of `%`-escaped continuation
/// octets, and the assembled code point must be a non-overlong, non-surrogate
/// scalar value — otherwise URIError. Non-`%` code units (including multi-byte
/// and lone-surrogate WTF-8 bytes) are copied through verbatim.
fn percent_decode(bytes: &[u8], reserved: Option<&[u8]>) -> Result<Vec<u8>, ()> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        let b = read_pct_octet(bytes, i)?;
        if b < 0x80 {
            if reserved.is_some_and(|set| set.contains(&b)) {
                out.extend_from_slice(&bytes[i..i + 3]);
            } else {
                out.push(b);
            }
            i += 3;
            continue;
        }
        // Multi-octet sequence: derive the length from the leading 1-bits.
        let n = if b & 0xE0 == 0xC0 {
            2
        } else if b & 0xF0 == 0xE0 {
            3
        } else if b & 0xF8 == 0xF0 {
            4
        } else {
            return Err(()); // lone continuation (10xxxxxx) or 5+-byte lead
        };
        let mut cp: u32 = (b as u32) & (0x7F >> n);
        let mut j = i + 3;
        for _ in 1..n {
            let cont = read_pct_octet(bytes, j)?;
            if cont & 0xC0 != 0x80 {
                return Err(());
            }
            cp = (cp << 6) | (cont as u32 & 0x3F);
            j += 3;
        }
        let min = match n {
            2 => 0x80,
            3 => 0x800,
            _ => 0x1_0000,
        };
        if cp < min || cp > 0x10_FFFF || (0xD800..=0xDFFF).contains(&cp) {
            return Err(());
        }
        let ch = char::from_u32(cp).ok_or(())?;
        let mut buf = [0u8; 4];
        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        i = j;
    }
    Ok(out)
}

/// Build a result string from decoded bytes, choosing the WTF-8 constructor
/// when lone surrogates survived the round-trip so `.length`/`isWellFormed`
/// stay correct.
fn string_from_decoded(out: &[u8]) -> i64 {
    if std::str::from_utf8(out).is_ok() {
        js_string_from_bytes(out.as_ptr(), out.len() as u32) as i64
    } else {
        js_string_from_wtf8_bytes(out.as_ptr(), out.len() as u32) as i64
    }
}

fn throw_uri_malformed() -> ! {
    let message = b"URI malformed";
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_urierror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
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
    // Spec: escape/unescape/decodeURI apply `ToString` to the argument, so
    // `undefined`/`null`/booleans/objects coerce ("undefined", "null", …)
    // rather than yielding the empty string (`js_get_string_pointer_unified`
    // only coerces numbers). Strings pass through unchanged.
    let str_ptr = crate::builtins::js_string_coerce(value);
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

/// ToString-coerce `value` and return its raw WTF-8 bytes (lone surrogates
/// preserved). Used by the decode family, which must operate byte-wise per the
/// Decode algorithm rather than dropping non-UTF-8 input.
fn extract_coerced_bytes(value: f64) -> Vec<u8> {
    let str_ptr = crate::builtins::js_string_coerce(value);
    if (str_ptr as usize) < 0x1000 {
        return Vec::new();
    }
    unsafe {
        let header = str_ptr as *const StringHeader;
        let len = (*header).byte_len as usize;
        let data = (header as *const u8).add(std::mem::size_of::<StringHeader>());
        std::slice::from_raw_parts(data, len).to_vec()
    }
}

struct ExtractedStringBytes {
    bytes: Vec<u8>,
    flags: u32,
}

fn extract_string_bytes_from_nanbox(value: f64) -> ExtractedStringBytes {
    // encodeURI/encodeURIComponent apply ToString to the argument (sec-encodeuri
    // step 1), so objects/numbers coerce via toString/valueOf rather than
    // yielding the empty string (test262 encodeURI/encodeURIComponent A6_T1).
    let str_ptr = crate::builtins::js_string_coerce(value) as *const StringHeader;
    if (str_ptr as usize) < 0x1000 {
        return ExtractedStringBytes {
            bytes: Vec::new(),
            flags: 0,
        };
    }
    unsafe {
        let header = str_ptr as *const StringHeader;
        let len = (*header).byte_len as usize;
        let flags = (*header).flags;
        let data = (header as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len).to_vec();
        ExtractedStringBytes { bytes, flags }
    }
}

fn throw_if_lone_surrogate(input: &ExtractedStringBytes) {
    if input.flags & crate::string::STRING_FLAG_HAS_LONE_SURROGATES != 0 {
        throw_uri_malformed();
    }
}

/// encodeURI(string) -> string
#[no_mangle]
pub extern "C" fn js_encode_uri(value: f64) -> i64 {
    let input = extract_string_bytes_from_nanbox(value);
    throw_if_lone_surrogate(&input);
    let mut safe = Vec::with_capacity(URI_UNESCAPED.len() + URI_RESERVED.len());
    safe.extend_from_slice(URI_UNESCAPED);
    safe.extend_from_slice(URI_RESERVED);
    let encoded = percent_encode(&input.bytes, &safe);
    let ptr = js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32);
    ptr as i64
}

/// decodeURI(string) -> string
#[no_mangle]
pub extern "C" fn js_decode_uri(value: f64) -> i64 {
    let input = extract_coerced_bytes(value);
    let decoded =
        percent_decode(&input, Some(URI_RESERVED)).unwrap_or_else(|_| throw_uri_malformed());
    string_from_decoded(&decoded)
}

/// encodeURIComponent(string) -> string
#[no_mangle]
pub extern "C" fn js_encode_uri_component(value: f64) -> i64 {
    let input = extract_string_bytes_from_nanbox(value);
    throw_if_lone_surrogate(&input);
    let encoded = percent_encode(&input.bytes, URI_COMPONENT_UNESCAPED);
    let ptr = js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32);
    ptr as i64
}

/// decodeURIComponent(string) -> string
#[no_mangle]
pub extern "C" fn js_decode_uri_component(value: f64) -> i64 {
    let input = extract_coerced_bytes(value);
    let decoded = percent_decode(&input, None).unwrap_or_else(|_| throw_uri_malformed());
    string_from_decoded(&decoded)
}

// ============================================================
// escape / unescape (ECMAScript Annex B B.2.1)
// ============================================================

/// Characters `escape()` leaves unencoded (ES Annex B B.2.1.1): ASCII
/// letters, digits, and `@ * _ + - . /`. Unlike `encodeURIComponent`, the
/// escape set keeps `+` and `@` and encodes everything else — code units
/// < 256 as `%XX`, the rest as `%uXXXX`.
const ESCAPE_UNESCAPED: &[u8] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789@*_+-./";

/// `ToString(value)` throws a TypeError for a Symbol argument. `escape` /
/// `unescape` (and the URI family) apply ToString to their input, so a Symbol
/// must reject rather than coerce to a `"Symbol(x)"` description string.
fn throw_if_symbol(value: f64) {
    if (value.to_bits() & 0xFFFF_0000_0000_0000) == crate::value::POINTER_TAG
        && crate::symbol::is_registered_symbol(
            (value.to_bits() & crate::value::POINTER_MASK) as usize,
        )
    {
        let msg = b"Cannot convert a Symbol value to a string";
        let msg_str = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err = crate::error::js_typeerror_new(msg_str);
        crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
    }
}

/// escape(string) -> string (legacy, ES Annex B B.2.1.1)
#[no_mangle]
pub extern "C" fn js_escape(value: f64) -> i64 {
    throw_if_symbol(value);
    let input = extract_str_from_nanbox(value);
    let mut result = String::with_capacity(input.len() * 3);
    let mut buf = [0u16; 2];
    for c in input.chars() {
        let cp = c as u32;
        if cp < 0x80 && ESCAPE_UNESCAPED.contains(&(cp as u8)) {
            result.push(c);
        } else {
            // Encode per UTF-16 code unit so astral code points emit the two
            // `%uXXXX` escapes for their surrogate pair, matching Node.
            for unit in c.encode_utf16(&mut buf) {
                if *unit < 0x100 {
                    result.push_str(&format!("%{:02X}", *unit));
                } else {
                    result.push_str(&format!("%u{:04X}", *unit));
                }
            }
        }
    }
    let ptr = js_string_from_bytes(result.as_ptr(), result.len() as u32);
    ptr as i64
}

/// unescape(string) -> string (legacy, ES Annex B B.2.1.2)
#[no_mangle]
pub extern "C" fn js_unescape(value: f64) -> i64 {
    throw_if_symbol(value);
    let input = extract_str_from_nanbox(value);
    let chars: Vec<char> = input.chars().collect();
    // Reassemble into UTF-16 code units, then decode, so `%uD835%uDFD8`-style
    // surrogate pairs recombine into a single astral scalar.
    let mut units: Vec<u16> = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' {
            // %uXXXX
            if i + 5 < chars.len() && chars[i + 1] == 'u' {
                if let Some(u) = hex4_chars(&chars[i + 2..i + 6]) {
                    units.push(u);
                    i += 6;
                    continue;
                }
            }
            // %XX
            if i + 2 < chars.len() {
                if let (Some(h), Some(l)) = (chars[i + 1].to_digit(16), chars[i + 2].to_digit(16)) {
                    units.push((h * 16 + l) as u16);
                    i += 3;
                    continue;
                }
            }
        }
        let mut buf = [0u16; 2];
        for unit in chars[i].encode_utf16(&mut buf) {
            units.push(*unit);
        }
        i += 1;
    }
    let decoded = String::from_utf16_lossy(&units);
    let ptr = js_string_from_bytes(decoded.as_ptr(), decoded.len() as u32);
    ptr as i64
}

fn hex4_chars(cs: &[char]) -> Option<u16> {
    let mut v: u16 = 0;
    for &c in cs {
        v = v.checked_mul(16)?.checked_add(c.to_digit(16)? as u16)?;
    }
    Some(v)
}

// ============================================================
// structuredClone
// ============================================================

fn throw_structured_clone_type_error(message: &str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn throw_data_clone_error(message: &str) -> ! {
    // The runtime throw uses longjmp, so no Rust guard can release roots from
    // the abandoned traversal. They are no longer needed once cloning fails.
    let scope = crate::gc::RuntimeHandleScope::new();
    let message = scope.root_nanbox_f64(crate::value::js_nanbox_string(js_string_from_bytes(
        message.as_ptr(),
        message.len() as u32,
    ) as i64));
    let name = scope.root_nanbox_f64(crate::value::js_nanbox_string(js_string_from_bytes(
        b"DataCloneError".as_ptr(),
        14,
    ) as i64));
    let err = crate::event_target_hook_slots::js_dom_exception_new(
        message.get_nanbox_f64(),
        name.get_nanbox_f64(),
    );
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn pointer_addr(value: f64) -> Option<usize> {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_pointer() {
        Some((jv.bits() & crate::value::POINTER_MASK) as usize)
    } else {
        None
    }
}

fn gc_type_for_pointer(addr: usize) -> Option<u8> {
    if addr < 0x10000
        || crate::buffer::is_registered_buffer(addr)
        || crate::symbol::is_registered_symbol(addr)
        || crate::set::is_registered_set(addr)
    {
        return None;
    }
    unsafe { Some(*((addr as *const u8).sub(crate::gc::GC_HEADER_SIZE))) }
}

fn is_array_value(value: f64) -> bool {
    pointer_addr(value)
        .is_some_and(|addr| gc_type_for_pointer(addr) == Some(crate::gc::GC_TYPE_ARRAY))
}

fn get_object_property(value: f64, name: &[u8]) -> f64 {
    let Some(addr) = pointer_addr(value) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    if gc_type_for_pointer(addr) != Some(crate::gc::GC_TYPE_OBJECT) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let val =
        crate::object::js_object_get_field_by_name(addr as *mut crate::object::ObjectHeader, key);
    f64::from_bits(val.bits())
}

fn collect_transfer_list(options: f64) -> std::collections::HashSet<usize> {
    let options_value = crate::value::JSValue::from_bits(options.to_bits());
    if options_value.is_undefined() || options_value.is_null() {
        return std::collections::HashSet::new();
    }
    if !options_value.is_pointer() {
        throw_structured_clone_type_error(
            "Failed to execute 'structuredClone': Options cannot be converted to a dictionary",
        );
    }

    let transfer = get_object_property(options, b"transfer");
    if crate::value::JSValue::from_bits(transfer.to_bits()).is_undefined() {
        return std::collections::HashSet::new();
    }
    if !is_array_value(transfer) {
        throw_structured_clone_type_error(
            "Failed to execute 'structuredClone': transfer in Options can not be converted to sequence",
        );
    }

    let transfer_addr = pointer_addr(transfer).unwrap_or(0);
    let transfer_arr = transfer_addr as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(transfer_arr);
    let mut out = std::collections::HashSet::new();
    for i in 0..len {
        let item = crate::array::js_array_get_f64(transfer_arr, i);
        let Some(item_addr) = pointer_addr(item) else {
            throw_data_clone_error("Found invalid value in transferList");
        };
        if !crate::buffer::is_array_buffer(item_addr)
            || crate::buffer::is_shared_array_buffer(item_addr)
        {
            throw_data_clone_error("Found invalid value in transferList");
        }
        if crate::buffer::is_detached_buffer(item_addr) {
            throw_data_clone_error("ArrayBuffer is already detached");
        }
        if !out.insert(item_addr) {
            throw_data_clone_error("Transfer list contains duplicate ArrayBuffer");
        }
    }
    out
}

/// structuredClone and worker messages use one Node-compatible codec.
#[no_mangle]
pub extern "C" fn js_structured_clone(value: f64) -> f64 {
    crate::thread_hook_slots::clone_with_transfers(value, &[])
}

#[no_mangle]
pub extern "C" fn js_structured_clone_with_options(value: f64, options: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let transfer = collect_transfer_list(options)
        .into_iter()
        .collect::<Vec<_>>();
    let outcome = crate::exception::catch_js_throw(|| {
        crate::thread_hook_slots::clone_with_transfers(value.get_nanbox_f64(), &transfer)
    });
    drop(transfer);
    drop(scope);
    match outcome {
        Ok(value) => value,
        Err(error) => crate::exception::js_throw(error),
    }
}

pub(crate) fn clone_with_transfers(value: f64, transfer: &[usize]) -> f64 {
    let message = match unsafe { crate::thread::serialize_message(value.to_bits(), transfer, None) }
    {
        Ok(message) => message,
        Err(error) => throw_data_clone_error(&error),
    };
    f64::from_bits(unsafe { crate::thread::deserialize_nanbox_on_current_thread(&message) })
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
    crate::promise::enqueue_queue_microtask(callback);
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback_handle =
        scope.root_raw_const_ptr(callback as *const crate::closure::ClosureHeader);
    let arg_handles = scope.root_nanbox_f64_slice(&args);
    let resource = crate::object::js_object_alloc(0, 0);
    let resource_handle = scope.root_raw_mut_ptr(resource);
    let mut context = crate::async_context::capture_context();
    let context_roots = crate::async_context::root_snapshot(&scope, &context);
    let resource_value = resource_handle
        .with_mut_ptr::<u8, _>(|resource| crate::value::js_nanbox_pointer(resource as i64));
    let (ids, callback) = callback_handle.across_const::<crate::closure::ClosureHeader, _>(|| {
        crate::async_hook_slots::init_scheduled_resource(type_name, resource_value, None)
    });
    // The queue entry owns the job's state; keep it reachable until queued.
    if let Some(state) = crate::async_hook_slots::resource_state::get(ids.async_id) {
        scope.root_raw_mut_ptr(state);
    }
    crate::async_context::refresh_snapshot_from_roots(&mut context, &context_roots);
    if !context.prepare_job_enqueue() {
        return;
    }
    QUEUED_MICROTASKS.with(|q| {
        q.borrow_mut().push_back(QueuedMicrotask {
            callback: callback as i64,
            context,
            async_id: ids.async_id,
            trigger_async_id: ids.trigger_async_id,
            resource: resource_handle
                .with_mut_ptr::<u8, _>(|resource| crate::value::js_nanbox_pointer(resource as i64)),
            args: crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles),
        });
    });
}

pub(crate) struct QueuedMicrotask {
    pub callback: i64,
    pub context: crate::async_context::AsyncContextSnapshot,
    /// Owns the job's async state (a state token, or a bare serial when no
    /// hook was enabled at queue time).
    pub async_id: u64,
    pub trigger_async_id: u64,
    /// The job's `executionAsyncResource()`.
    pub resource: f64,
    pub args: Vec<f64>,
}

thread_local! {
    // VecDeque so the drain pops from the front in O(1); a `Vec` + `remove(0)`
    // is O(n) per job → O(n²) to drain a burst of n nextTick jobs (#6084).
    static QUEUED_MICROTASKS: std::cell::RefCell<std::collections::VecDeque<QueuedMicrotask>> = const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
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

pub(crate) fn discard_cancelled_microtasks() {
    QUEUED_MICROTASKS.with(|q| q.borrow_mut().retain(|job| !job.context.job_is_cancelled()));
}

/// Drain queued nextTick jobs. Called by `js_promise_run_microtasks` before
/// regular Promise/queueMicrotask jobs so Node's nextTick priority is
/// preserved.
#[no_mangle]
pub extern "C" fn js_drain_queued_microtasks() {
    let _ = drain_queued_microtasks_count();
}

pub(crate) fn drain_queued_microtasks_count() -> i32 {
    use crate::closure::{
        js_closure_call0, js_closure_call1, js_closure_call2, js_closure_call3, js_closure_call4,
        js_closure_call5, js_closure_call6, js_closure_call7, js_closure_call8,
    };
    let mut ran = 0;
    loop {
        let task = QUEUED_MICROTASKS.with(|q| {
            let mut q = q.borrow_mut();
            let index = q.iter().position(|task| task.context.job_is_eligible())?;
            q.remove(index)
        });
        match task {
            Some(QueuedMicrotask {
                callback: cb,
                context,
                async_id,
                trigger_async_id,
                resource,
                args,
            }) => {
                let scope = crate::gc::RuntimeHandleScope::new();
                // The popped entry no longer owns the job's state: this scope
                // does until its destroy.
                if let Some(state) = crate::async_hook_slots::resource_state::get(async_id) {
                    scope.root_raw_mut_ptr(state);
                }
                let resource_handle = scope.root_nanbox_f64(resource);
                let callback_handle =
                    scope.root_raw_const_ptr(cb as *const crate::closure::ClosureHeader);
                let arg_handles = scope.root_nanbox_f64_slice(&args);
                let previous = crate::async_context::enter_context(&context);
                QUEUED_MICROTASK_PREV_CONTEXTS.with(|stack| {
                    stack.borrow_mut().push(previous);
                });
                crate::async_hook_slots::before_scheduled(
                    async_id,
                    trigger_async_id,
                    resource_handle.get_nanbox_f64(),
                );
                let a = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
                let cb_ptr = callback_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>();
                match a.len() {
                    0 => {
                        js_closure_call0(cb_ptr, crate::closure::plain_call_receiver());
                    }
                    1 => {
                        js_closure_call1(cb_ptr, crate::closure::plain_call_receiver(), a[0]);
                    }
                    2 => {
                        js_closure_call2(cb_ptr, crate::closure::plain_call_receiver(), a[0], a[1]);
                    }
                    3 => {
                        js_closure_call3(
                            cb_ptr,
                            crate::closure::plain_call_receiver(),
                            a[0],
                            a[1],
                            a[2],
                        );
                    }
                    4 => {
                        js_closure_call4(
                            cb_ptr,
                            crate::closure::plain_call_receiver(),
                            a[0],
                            a[1],
                            a[2],
                            a[3],
                        );
                    }
                    5 => {
                        js_closure_call5(
                            cb_ptr,
                            crate::closure::plain_call_receiver(),
                            a[0],
                            a[1],
                            a[2],
                            a[3],
                            a[4],
                        );
                    }
                    6 => {
                        js_closure_call6(
                            cb_ptr,
                            crate::closure::plain_call_receiver(),
                            a[0],
                            a[1],
                            a[2],
                            a[3],
                            a[4],
                            a[5],
                        );
                    }
                    7 => {
                        js_closure_call7(
                            cb_ptr,
                            crate::closure::plain_call_receiver(),
                            a[0],
                            a[1],
                            a[2],
                            a[3],
                            a[4],
                            a[5],
                            a[6],
                        );
                    }
                    8 => {
                        js_closure_call8(
                            cb_ptr,
                            crate::closure::plain_call_receiver(),
                            a[0],
                            a[1],
                            a[2],
                            a[3],
                            a[4],
                            a[5],
                            a[6],
                            a[7],
                        );
                    }
                    // #10420: more than 8 trailing args used to clamp to 9.
                    n => unsafe {
                        crate::closure::js_closure_call_array(
                            cb_ptr as i64,
                            crate::closure::plain_call_receiver(),
                            a.as_ptr(),
                            n as i64,
                        );
                    },
                }
                crate::async_hook_slots::after(async_id);
                crate::async_hook_slots::destroy_scheduled(async_id);
                QUEUED_MICROTASK_PREV_CONTEXTS.with(|stack| {
                    if let Some(previous) = stack.borrow_mut().pop() {
                        crate::async_context::restore_context(previous);
                    }
                });
                ran += 1;
            }
            None => break,
        }
    }
    ran
}

pub(crate) fn queued_microtasks_pending() -> bool {
    QUEUED_MICROTASKS.with(|q| q.borrow().iter().any(|task| task.context.job_is_eligible()))
}

pub fn scan_queued_microtask_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_queued_microtask_roots_mut(&mut visitor);
}

pub fn scan_queued_microtask_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    QUEUED_MICROTASKS.with(|q| {
        for task in q.borrow_mut().iter_mut() {
            visitor.visit_i64_slot(&mut task.callback);
            crate::async_hook_slots::resource_state::scan_token(&mut task.async_id, visitor);
            visitor.visit_nanbox_f64_slot(&mut task.resource);
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
        q.push_back(QueuedMicrotask {
            callback,
            context,
            async_id: 0,
            trigger_async_id: 0,
            resource: f64::from_bits(crate::value::TAG_UNDEFINED),
            args: Vec::new(),
        });
    });
    QUEUED_MICROTASK_PREV_CONTEXTS.with(|stack| stack.borrow_mut().clear());
}

/// Queue one job entry that owns `async_id` (no callback: never drained).
#[cfg(test)]
pub(crate) fn test_seed_queued_job_owner(async_id: u64, resource: f64) {
    QUEUED_MICROTASKS.with(|q| {
        let mut q = q.borrow_mut();
        q.clear();
        q.push_back(QueuedMicrotask {
            callback: 0,
            context: Default::default(),
            async_id,
            trigger_async_id: 0,
            resource,
            args: Vec::new(),
        });
    });
}

#[cfg(test)]
pub(crate) fn test_clear_queued_jobs() {
    QUEUED_MICROTASKS.with(|q| q.borrow_mut().clear());
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
            .front()
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

#[cfg(test)]
mod structured_clone_tests {
    use super::*;

    #[test]
    fn structured_clone_date_is_independent_and_preserves_nested_identity() {
        let source = crate::date_hook_slots::alloc_date_cell(0.0);
        let direct = js_structured_clone(source);
        assert_ne!(direct.to_bits(), source.to_bits());
        assert_eq!(crate::date::date_cell_timestamp(direct), 0.0);
        let next_time = [5000.0];
        crate::date_hook_slots::js_date_apply_setter(direct, 0, 7, next_time.as_ptr(), 1);
        assert_eq!(crate::date::date_cell_timestamp(direct), 5000.0);
        assert_eq!(crate::date::date_cell_timestamp(source), 0.0);

        let object = crate::object::js_object_alloc(0, 2);
        let first_key = crate::string::js_string_from_bytes(b"first".as_ptr(), 5);
        let second_key = crate::string::js_string_from_bytes(b"second".as_ptr(), 6);
        crate::object::js_object_set_field_by_name(object, first_key, source);
        crate::object::js_object_set_field_by_name(object, second_key, source);
        let cloned_object = js_structured_clone(crate::value::js_nanbox_pointer(object as i64));
        let cloned_ptr = crate::value::js_nanbox_get_pointer(cloned_object)
            as *const crate::object::ObjectHeader;
        let first = crate::object::js_object_get_field_by_name(cloned_ptr, first_key);
        let second = crate::object::js_object_get_field_by_name(cloned_ptr, second_key);
        assert_eq!(first.bits(), second.bits());
        assert_ne!(first.bits(), source.to_bits());
        assert_eq!(
            crate::date::date_cell_timestamp(f64::from_bits(first.bits())),
            0.0
        );
    }

    #[cfg(feature = "regex-engine")]
    #[test]
    fn structured_clone_regexp_is_independent_and_resets_last_index() {
        let pattern = crate::string::js_string_from_bytes(b"a+".as_ptr(), 2);
        let flags = crate::string::js_string_from_bytes(b"gi".as_ptr(), 2);
        let source = crate::regex::js_regexp_new(pattern, flags);
        crate::regex::set_last_index(source, 3.0);
        let cloned_value = js_structured_clone(crate::value::js_nanbox_pointer(source as i64));
        let cloned =
            crate::value::js_nanbox_get_pointer(cloned_value) as *mut crate::regex::RegExpHeader;
        assert_ne!(cloned, source);
        assert_eq!(crate::regex::get_last_index(cloned), 0.0);
        assert_eq!(crate::regex::get_last_index(source), 3.0);
        crate::regex::set_last_index(cloned, 7.0);
        assert_eq!(crate::regex::get_last_index(source), 3.0);
    }

    /// #8232: a clone memo must resolve a back-edge to the destination object,
    /// not return the original source object as the old recursion guard did.
    #[test]
    fn structured_clone_preserves_cycle_identity() {
        let src = crate::object::js_object_alloc(0, 1);
        let src_v = crate::value::js_nanbox_pointer(src as i64);
        let self_key = crate::string::js_string_from_bytes(b"self".as_ptr(), 4);
        crate::object::js_object_set_field_by_name(src, self_key, src_v);

        let cloned_v = js_structured_clone(src_v);
        let cloned =
            crate::value::js_nanbox_get_pointer(cloned_v) as *const crate::object::ObjectHeader;
        let cloned_self_key = crate::string::js_string_from_bytes(b"self".as_ptr(), 4);
        let cloned_self = crate::object::js_object_get_field_by_name(cloned, cloned_self_key);

        assert_ne!(cloned_v.to_bits(), src_v.to_bits());
        assert_eq!(cloned_self.bits(), cloned_v.to_bits());
    }

    /// #4879: properties past the inline field region (overflow side table /
    /// `{}`-born objects with no inline capacity) must survive structuredClone.
    #[test]
    fn structured_clone_keeps_overflow_properties() {
        {
            let src = crate::object::js_object_alloc(0, 0);
            let mut names = Vec::new();
            for i in 0..50 {
                names.push(format!("f{}", i));
            }
            for (i, name) in names.iter().enumerate() {
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                crate::object::js_object_set_field_by_name(src, key, i as f64);
            }
            let src_v = crate::value::js_nanbox_pointer(src as i64);
            let cloned_v = js_structured_clone(src_v);
            let cloned =
                crate::value::js_nanbox_get_pointer(cloned_v) as *const crate::object::ObjectHeader;
            assert!(!cloned.is_null());
            assert_ne!(cloned as usize, src as usize);
            let cloned_keys = crate::object::js_object_keys(cloned);
            assert_eq!(crate::array::js_array_length(cloned_keys), 50);
            for (i, name) in names.iter().enumerate() {
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                let v = crate::object::js_object_get_field_by_name(cloned, key);
                assert_eq!(
                    f64::from_bits(v.bits()),
                    i as f64,
                    "property {} lost or wrong in clone",
                    name
                );
            }
        }
    }

    #[test]
    fn structured_clone_data_view_uses_the_cached_registered_representation() {
        let backing = crate::buffer::js_array_buffer_new(8);
        let backing_value = crate::value::js_nanbox_pointer(backing as i64);
        let source = crate::buffer::js_data_view_new(backing_value, 0.0, 8.0);
        crate::buffer::js_data_view_set(
            source,
            0.0,
            0x0102_0304u32 as f64,
            crate::buffer::DataViewKind::Uint32,
            false,
        );

        let cloned = js_structured_clone(source);
        let cloned_addr = pointer_addr(cloned).expect("DataView clone must be a pointer");
        assert!(crate::buffer::is_data_view(cloned_addr));
        assert_ne!(
            crate::buffer::buffer_backing_array_buffer(cloned_addr),
            backing as usize,
            "the clone must own an independent ArrayBuffer"
        );

        crate::buffer::js_data_view_set(
            cloned,
            4.0,
            0x0506_0708u32 as f64,
            crate::buffer::DataViewKind::Uint32,
            false,
        );
        assert_eq!(
            crate::buffer::js_data_view_get(
                cloned,
                4.0,
                crate::buffer::DataViewKind::Uint32,
                false,
            ),
            0x0506_0708u32 as f64
        );
        assert_eq!(
            crate::buffer::js_data_view_get(
                source,
                4.0,
                crate::buffer::DataViewKind::Uint32,
                false,
            ),
            0.0,
            "writing the clone must not change its source"
        );
    }
}
