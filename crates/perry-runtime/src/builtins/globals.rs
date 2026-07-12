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
    static STRUCTURED_CLONE_TRANSFER_STATE: std::cell::RefCell<Option<StructuredCloneTransferState>>
        = const { std::cell::RefCell::new(None) };
}

#[derive(Default)]
struct StructuredCloneTransferState {
    transferables: std::collections::HashSet<usize>,
    clones: std::collections::HashMap<usize, usize>,
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

struct CloneTransferStateGuard(Option<StructuredCloneTransferState>);
impl Drop for CloneTransferStateGuard {
    fn drop(&mut self) {
        STRUCTURED_CLONE_TRANSFER_STATE.with(|state| {
            *state.borrow_mut() = self.0.take();
        });
    }
}

fn throw_structured_clone_type_error(message: &str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn throw_data_clone_error(message: &str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_error_new_with_name_message(b"DataCloneError", msg);
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

fn transfer_existing_clone(addr: usize) -> Option<usize> {
    STRUCTURED_CLONE_TRANSFER_STATE.with(|state| {
        state
            .borrow()
            .as_ref()
            .and_then(|state| state.clones.get(&addr).copied())
    })
}

fn transfer_requested(addr: usize) -> bool {
    STRUCTURED_CLONE_TRANSFER_STATE.with(|state| {
        state
            .borrow()
            .as_ref()
            .is_some_and(|state| state.transferables.contains(&addr))
    })
}

fn record_transfer_clone(src: usize, cloned: usize) {
    STRUCTURED_CLONE_TRANSFER_STATE.with(|state| {
        if let Some(state) = state.borrow_mut().as_mut() {
            state.clones.insert(src, cloned);
        }
    });
}

/// Detach every transfer-list buffer. Runs only AFTER the whole clone
/// succeeded (HTML structured-clone semantics): a `DataCloneError` thrown
/// mid-walk must leave all source buffers attached, so no detachment happens
/// during cloning itself. Idempotent per buffer.
fn detach_transferables_after_success() {
    STRUCTURED_CLONE_TRANSFER_STATE.with(|state| {
        if let Some(state) = state.borrow().as_ref() {
            for addr in &state.transferables {
                crate::buffer::detach_array_buffer(*addr);
            }
        }
    });
}

fn clone_buffer_header(addr: usize, detach_source: bool) -> f64 {
    // An already-detached ArrayBuffer cannot be serialized — transferring it
    // is rejected earlier by `collect_transfer_list`, and plain cloning must
    // throw rather than produce a fresh empty buffer.
    if crate::buffer::is_detached_buffer(addr) {
        throw_data_clone_error("An ArrayBuffer is detached and could not be cloned");
    }
    if detach_source {
        if let Some(existing) = transfer_existing_clone(addr) {
            return crate::value::js_nanbox_pointer(existing as i64);
        }
    }

    let src = addr as *mut crate::buffer::BufferHeader;
    let src_len = unsafe { (*src).length };
    let dst = crate::buffer::buffer_alloc(src_len);
    unsafe {
        (*dst).length = src_len;
        if src_len > 0 {
            std::ptr::copy_nonoverlapping(
                crate::buffer::buffer_data(src),
                crate::buffer::buffer_data_mut(dst),
                src_len as usize,
            );
        }
    }

    let dst_addr = dst as usize;
    if crate::buffer::is_array_buffer(addr) {
        crate::buffer::mark_as_array_buffer(dst_addr);
    } else if crate::buffer::is_shared_array_buffer(addr) {
        crate::buffer::mark_as_shared_array_buffer(dst_addr);
    } else if crate::buffer::is_data_view(addr) {
        crate::buffer::mark_as_data_view(dst_addr);
        crate::buffer::set_buffer_ab_alias(dst_addr, crate::buffer::resolve_buffer_ab_alias(addr));
    } else if crate::buffer::is_uint8array_buffer(addr) {
        crate::buffer::mark_as_uint8array(dst_addr);
        crate::buffer::set_buffer_ab_alias(dst_addr, crate::buffer::resolve_buffer_ab_alias(addr));
    } else {
        crate::buffer::set_buffer_ab_alias(dst_addr, crate::buffer::resolve_buffer_ab_alias(addr));
    }

    if detach_source {
        // Record only — detachment is deferred to
        // `detach_transferables_after_success` so a clone that fails later in
        // the walk leaves the source attached.
        record_transfer_clone(addr, dst_addr);
    }

    crate::value::js_nanbox_pointer(dst_addr as i64)
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

/// structuredClone(value) -> deep-cloned value
/// Handles numbers (pass-through), strings (copy), arrays/objects (shallow for now)
#[no_mangle]
pub extern "C" fn js_structured_clone(value: f64) -> f64 {
    js_structured_clone_inner(value)
}

/// structuredClone(value, options) -> deep-cloned value with supported transfers.
#[no_mangle]
pub extern "C" fn js_structured_clone_with_options(value: f64, options: f64) -> f64 {
    let transferables = collect_transfer_list(options);
    let previous = STRUCTURED_CLONE_TRANSFER_STATE.with(|state| {
        state.borrow_mut().replace(StructuredCloneTransferState {
            transferables,
            clones: std::collections::HashMap::new(),
        })
    });
    let _guard = CloneTransferStateGuard(previous);
    let cloned = js_structured_clone_inner(value);
    detach_transferables_after_success();
    cloned
}

fn js_structured_clone_inner(value: f64) -> f64 {
    if crate::value::is_js_handle(value) && crate::value::js_handle_is_function(value) {
        throw_data_clone_error("Function could not be cloned");
    }

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
            let addr = ptr as usize;
            // Full handle band, not the legacy `< 0x10000` (2026-07-02
            // audit): a POINTER-tagged fetch/zlib registry id (0x40000+)
            // reached the raw GcHeader byte read below and deref'd the id
            // as memory. Registry handles aren't structured-cloneable heap
            // graphs — return them as-is (matching the unknown-type
            // fallback; the spec's DataCloneError refinement is tracked
            // separately).
            if crate::value::addr_class::is_small_handle(addr) || addr < 0x10000 {
                return value;
            }
            if crate::symbol::is_registered_symbol(addr) {
                throw_data_clone_error("Symbol could not be cloned");
            }
            if crate::value::is_js_handle(value) && crate::value::js_handle_is_function(value) {
                throw_data_clone_error("Function could not be cloned");
            }
            if crate::closure::is_closure_ptr(addr) {
                throw_data_clone_error("Function could not be cloned");
            }
            if crate::buffer::is_registered_buffer(addr) {
                return clone_buffer_header(addr, transfer_requested(addr));
            }
            // #1512: short-circuit on cycle so `o.self = o` doesn't infinite-
            // recurse. The cycle edge resolves to the original value, not
            // the clone — that breaks full reference-identity preservation
            // but keeps cycles from stack-overflowing the runtime.
            if structured_clone_seen(addr) {
                return value;
            }
            structured_clone_mark(addr);
            let _guard = CloneCycleGuard(addr);
            // Set is tracked in SET_REGISTRY (not GC_TYPE_SET since it has
            // no GC header). Check the registry BEFORE touching the GC
            // header bytes — they'd be garbage for raw-allocated sets.
            if crate::set::is_registered_set(addr) {
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
                // Validated probe (plausibility + band + slab) before reading
                // header bytes; non-probeable pointers pass through unchanged.
                let gc_type = match crate::value::addr_class::try_read_gc_header(addr) {
                    Some(h) => h.obj_type,
                    None => return value,
                };
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
                        // GC_STORE_AUDIT(BARRIERED): note_array_slot below re-stores this slot with the barrier.
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
                    #[cfg(feature = "regex-engine")]
                    if crate::regex::is_regex_pointer(ptr as *const u8) {
                        let re_ptr = ptr as *const crate::regex::RegExpHeader;
                        let src = crate::regex::js_regexp_get_source(re_ptr);
                        let flg = crate::regex::js_regexp_get_flags(re_ptr);
                        let new_re = crate::regex::js_regexp_new(src, flg);
                        let new_bits =
                            0x7FFD_0000_0000_0000u64 | (new_re as u64 & 0x0000_FFFF_FFFF_FFFF);
                        return f64::from_bits(new_bits);
                    }
                    // #4879: properties that live outside the inline field
                    // region (OVERFLOW_FIELDS of a dict-grown object, or every
                    // prop of a `{}`-born object with no inline capacity) are
                    // invisible to the clone_with_extra fast path below — it
                    // copies only the inline `field_count` slots and truncates
                    // the keys array to match. When the keys array is longer
                    // than the inline region, rebuild the clone key-by-key via
                    // js_object_get_field (which resolves inline vs overflow
                    // per index) + js_object_set_field_by_name.
                    let src_obj = ptr as *const crate::object::ObjectHeader;
                    let src_keys = (*src_obj).keys_array;
                    let key_count = if !src_keys.is_null() && (src_keys as usize) >= 0x10000 {
                        crate::array::js_array_length(src_keys) as usize
                    } else {
                        0
                    };
                    if key_count > (*src_obj).field_count as usize {
                        let scope = crate::gc::RuntimeHandleScope::new();
                        let src_handle = scope.root_raw_const_ptr(src_obj);
                        let new_obj = crate::object::js_object_alloc(0, key_count as u32);
                        let new_handle = scope.root_raw_mut_ptr(new_obj);
                        let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                        for i in 0..key_count {
                            let src_now =
                                src_handle.get_raw_const_ptr::<crate::object::ObjectHeader>();
                            let keys_now = (*src_now).keys_array;
                            if keys_now.is_null()
                                || i >= crate::array::js_array_length(keys_now) as usize
                            {
                                break;
                            }
                            let key_val = crate::array::js_array_get(keys_now, i as u32);
                            // Own the key bytes before the recursive clone —
                            // it can run a GC cycle.
                            let key_bytes =
                                match crate::string::js_string_key_bytes(key_val, &mut sso_buf) {
                                    Some(b) => b.to_vec(),
                                    None => continue,
                                };
                            let field = crate::object::js_object_get_field(src_now, i as u32);
                            let cloned = js_structured_clone(f64::from_bits(field.bits()));
                            let key_ptr = crate::string::js_string_from_bytes(
                                key_bytes.as_ptr(),
                                key_bytes.len() as u32,
                            );
                            let new_now =
                                new_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
                            crate::object::js_object_set_field_by_name(new_now, key_ptr, cloned);
                        }
                        let new_now = new_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
                        let new_bits =
                            0x7FFD_0000_0000_0000u64 | (new_now as u64 & 0x0000_FFFF_FFFF_FFFF);
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
                            let cloned = js_structured_clone(field);
                            // GC_STORE_AUDIT(BARRIERED): cloned field uses the shared object slot-store helper.
                            // The recursive clone above can run minor GCs that tenure `cloned_obj`
                            // mid-loop, so this store must be barriered like the array branch.
                            crate::object::store_object_field_slot(cloned_obj, i, cloned.to_bits());
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
    let context = crate::async_context::capture_context();
    let ids = crate::async_hooks::init_resource(
        type_name,
        f64::from_bits(crate::value::TAG_UNDEFINED),
        false,
    );
    QUEUED_MICROTASKS.with(|q| {
        q.borrow_mut().push_back(QueuedMicrotask {
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
        js_closure_call5, js_closure_call6, js_closure_call7, js_closure_call8, js_closure_call9,
    };
    let mut ran = 0;
    loop {
        let task = QUEUED_MICROTASKS.with(|q| q.borrow_mut().pop_front());
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
                ran += 1;
            }
            None => break,
        }
    }
    ran
}

pub(crate) fn queued_microtasks_pending() -> bool {
    QUEUED_MICROTASKS.with(|q| !q.borrow().is_empty())
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
        q.push_back(QueuedMicrotask {
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

    /// #4879: properties past the inline field region (overflow side table /
    /// `{}`-born objects with no inline capacity) must survive structuredClone.
    #[test]
    fn structured_clone_keeps_overflow_properties() {
        unsafe {
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
}
