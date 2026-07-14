//! TextEncoder / TextDecoder runtime.
//!
//! `js_text_encoder_encode_llvm` returns a `BufferHeader*` (packed u8 bytes,
//! identical layout to `new Uint8Array([...])`) so the inline `bytes[i]`
//! Uint8ArrayGet path (which reads `i8` at `ptr+8+idx`) sees real byte
//! values. Previously this allocated an `ArrayHeader` with f64-per-byte
//! storage, which iteration paths after #578 read as packed u8 — yielding
//! the IEEE-754 byte pattern of the first byte instead of the byte itself
//! (issue #584).
//!
//! `TextEncoder` / `TextDecoder` are stateless wrappers — the encoder is
//! always UTF-8, so we return a small sentinel integer NaN-boxed as a
//! pointer on the codegen side. The runtime doesn't need per-instance state.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::buffer::{buffer_alloc, buffer_data_mut, mark_as_uint8array, BufferHeader};
use crate::object::{js_object_alloc, js_object_set_field_by_name, ObjectHeader};
use crate::string::{js_string_from_bytes, StringHeader};

// WHATWG single-byte index tables + `resolve_single_byte()`, generated in
// build.rs from encoding_rs (only the `[u16; 128]` arrays ship in the binary).
include!(concat!(env!("OUT_DIR"), "/single_byte_encodings.rs"));

/// Supported decode encodings (the WHATWG-canonical name lives in the
/// registry as a `&'static str`; this enum drives the byte-level path).
#[derive(Clone, Copy, PartialEq, Eq)]
enum DecoderEncoding {
    Utf8,
    /// A WHATWG single-byte legacy encoding (ibm866, iso-8859-*, windows-125x,
    /// koi8, macintosh, …). The `[u16; 128]` is the high-half index (byte
    /// 0x80..=0xFF → BMP code point, 0xFFFD for unmapped). `windows-1252` and
    /// its `latin1`/`iso-8859-1`/`ascii` labels route here too — the previous
    /// 1:1 Latin-1 approximation mis-decoded the 0x80–0x9F range.
    SingleByte(&'static [u16; 128]),
    Utf16Le,
}

struct DecoderState {
    encoding: DecoderEncoding,
    /// WHATWG-canonical label reported by `decoder.encoding`.
    label: &'static str,
    fatal: bool,
    ignore_bom: bool,
}

lazy_static::lazy_static! {
    static ref DECODER_REGISTRY: Mutex<HashMap<i64, DecoderState>> = Mutex::new(HashMap::new());
    static ref NEXT_DECODER_ID: Mutex<i64> = Mutex::new(2);
}

/// Map a user-supplied encoding label to (enum, canonical-name).
/// Returns `None` for unsupported labels (caller throws `RangeError`).
fn resolve_decoder_label(raw: &str) -> Option<(DecoderEncoding, &'static str)> {
    // WHATWG label matching: trim ASCII whitespace, case-insensitive.
    let l = raw
        .trim_matches(|c| matches!(c, '\t' | '\n' | '\u{0C}' | '\r' | ' '))
        .to_ascii_lowercase();
    match l.as_str() {
        // NB: an explicit empty/whitespace label is NOT utf-8 (only an omitted
        // arg defaults to utf-8, handled in js_text_decoder_new).
        "utf-8" | "utf8" | "unicode-1-1-utf-8" | "unicode11utf8" | "unicode20utf8"
        | "x-unicode20utf8" => Some((DecoderEncoding::Utf8, "utf-8")),
        // WHATWG utf-16le label set (`utf-16-le` is not a real label).
        "utf-16le" | "utf-16" | "unicode" | "csunicode" | "unicodefeff" | "iso-10646-ucs-2"
        | "ucs-2" => Some((DecoderEncoding::Utf16Le, "utf-16le")),
        // All WHATWG single-byte encodings (incl. windows-1252 and its
        // latin1/iso-8859-1/ascii labels). Multi-byte encodings (gbk, big5,
        // shift_jis, euc-*) are intentionally NOT recognized here — perry can't
        // decode them yet, so constructing one still throws rather than silently
        // mis-decoding.
        other => resolve_single_byte(other)
            .map(|(table, canonical)| (DecoderEncoding::SingleByte(table), canonical)),
    }
}

fn throw_type_error(message: &[u8]) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    let bits = crate::value::JSValue::pointer(err as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(bits))
}

pub(crate) fn text_encoder_string_ptr(value: f64) -> *const StringHeader {
    let jsval = crate::value::JSValue::from_bits(value.to_bits());

    if jsval.is_undefined() {
        return js_string_from_bytes(std::ptr::null(), 0) as *const StringHeader;
    }

    if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        throw_type_error(b"Cannot convert a Symbol value to a string");
    }

    crate::value::js_jsvalue_to_string(value) as *const StringHeader
}

/// `new TextEncoder()` — returns a non-null sentinel integer pointer.
///
/// The returned value is a small integer (`1`) that the codegen NaN-boxes
/// with `POINTER_TAG`. TextEncoder has no state beyond "I encode UTF-8",
/// so any non-null sentinel works. We use a distinct value from the
/// decoder sentinel purely for debuggability.
#[no_mangle]
pub extern "C" fn js_text_encoder_new() -> i64 {
    TEXT_ENCODER_SENTINEL_ID
}

/// The stateless `TextEncoder` sentinel handle returned by
/// `js_text_encoder_new` — see its doc comment.
pub const TEXT_ENCODER_SENTINEL_ID: i64 = 1;

/// Whether `id` is a live `TextDecoder` registry handle. Used by the
/// dynamic method-call / property-GET handle arms
/// (`native_call_method.rs` / `get_field_by_name_tail.rs`) to route
/// `decode`/`encoding`/… on a type-erased receiver to the text natives.
pub fn is_known_text_decoder_id(id: i64) -> bool {
    DECODER_REGISTRY.lock().unwrap().contains_key(&id)
}

/// `new TextDecoder(label?, { fatal?, ignoreBOM? })` — validates the
/// label, stores per-instance decode state in `DECODER_REGISTRY`, and
/// returns a small-int handle that the codegen NaN-boxes with
/// `POINTER_TAG`. An unsupported label throws a `RangeError`
/// (`ERR_ENCODING_NOT_SUPPORTED`).
///
/// `label` arrives as a NaN-boxed f64 (`undefined` for the no-arg form);
/// `fatal` / `ignore_bom` arrive as NaN-boxed booleans (truthy → on).
#[no_mangle]
pub extern "C" fn js_text_decoder_new(label: f64, fatal: f64, ignore_bom: f64) -> i64 {
    let label_jsval = crate::value::JSValue::from_bits(label.to_bits());
    // An OMITTED label defaults to utf-8; an EXPLICIT "" (or all-whitespace) is
    // an invalid label per WHATWG "get an encoding" and must throw RangeError.
    if label_jsval.is_undefined() {
        return register_decoder(DecoderEncoding::Utf8, "utf-8", fatal, ignore_bom);
    }
    let label_str = {
        let ptr = crate::value::js_jsvalue_to_string(label) as *const StringHeader;
        text_string_header_to_string(ptr)
    };

    let (encoding, canonical) = match resolve_decoder_label(&label_str) {
        Some(pair) => pair,
        None => {
            let message = format!("The \"{label_str}\" encoding is not supported");
            // WHATWG (and Node) throw a RangeError for an unknown label, not a
            // TypeError — `RangeError [ERR_ENCODING_NOT_SUPPORTED]`.
            crate::fs::validate::throw_range_error_named(&message, "ERR_ENCODING_NOT_SUPPORTED");
        }
    };

    register_decoder(encoding, canonical, fatal, ignore_bom)
}

fn register_decoder(
    encoding: DecoderEncoding,
    canonical: &'static str,
    fatal: f64,
    ignore_bom: f64,
) -> i64 {
    let state = DecoderState {
        encoding,
        label: canonical,
        fatal: crate::value::js_is_truthy(fatal) != 0,
        ignore_bom: crate::value::js_is_truthy(ignore_bom) != 0,
    };
    let id = {
        let mut next = NEXT_DECODER_ID.lock().unwrap();
        let id = *next;
        *next += 1;
        id
    };
    DECODER_REGISTRY.lock().unwrap().insert(id, state);
    id
}

fn decoder_handle_id(handle: f64) -> i64 {
    let bits = handle.to_bits();
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
    const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
    if (bits & TAG_MASK) == POINTER_TAG {
        (bits & POINTER_MASK) as i64
    } else if !handle.is_nan() && bits != 0 && bits < 0x0001_0000_0000_0000 {
        bits as i64
    } else {
        0
    }
}

/// `decoder.encoding` — WHATWG-canonical label.
#[no_mangle]
pub extern "C" fn js_text_decoder_encoding(handle: f64) -> *mut StringHeader {
    let id = decoder_handle_id(handle);
    let label = DECODER_REGISTRY
        .lock()
        .unwrap()
        .get(&id)
        .map(|s| s.label)
        .unwrap_or("utf-8");
    js_string_from_bytes(label.as_ptr(), label.len() as u32)
}

/// `decoder.fatal` — boolean (NaN-boxed by codegen).
#[no_mangle]
pub extern "C" fn js_text_decoder_fatal(handle: f64) -> f64 {
    let id = decoder_handle_id(handle);
    let fatal = DECODER_REGISTRY
        .lock()
        .unwrap()
        .get(&id)
        .map(|s| s.fatal)
        .unwrap_or(false);
    if fatal {
        f64::from_bits(0x7FFC_0000_0000_0004) // TAG_TRUE
    } else {
        f64::from_bits(0x7FFC_0000_0000_0003) // TAG_FALSE
    }
}

/// `decoder.ignoreBOM` — boolean (NaN-boxed by codegen).
#[no_mangle]
pub extern "C" fn js_text_decoder_ignore_bom(handle: f64) -> f64 {
    let id = decoder_handle_id(handle);
    let ignore = DECODER_REGISTRY
        .lock()
        .unwrap()
        .get(&id)
        .map(|s| s.ignore_bom)
        .unwrap_or(false);
    if ignore {
        f64::from_bits(0x7FFC_0000_0000_0004) // TAG_TRUE
    } else {
        f64::from_bits(0x7FFC_0000_0000_0003) // TAG_FALSE
    }
}

/// `encoder.encode(str)` — UTF-8 encode `value` into a `BufferHeader`.
///
/// Takes a NaN-boxed f64 string value. Returns an i64 pointer to a freshly
/// allocated `BufferHeader` with `len` packed u8 bytes (same shape as
/// `new Uint8Array([...])`). The buffer is registered + marked as Uint8Array
/// so `instanceof Uint8Array` returns true and the standard Uint8Array
/// indexed-access / iteration / decoder paths all work.
///
/// The returned i64 is the raw `BufferHeader*` — the codegen NaN-boxes it
/// with `POINTER_TAG` before handing it to user code.
#[no_mangle]
pub extern "C" fn js_text_encoder_encode_llvm(value: f64) -> i64 {
    let str_ptr = text_encoder_string_ptr(value);
    let (data_ptr, len) = unsafe {
        let l = (*str_ptr).byte_len as usize;
        let d = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        (d, l)
    };

    let buf = buffer_alloc(len as u32);
    unsafe {
        (*buf).length = len as u32;
        if len > 0 {
            std::ptr::copy_nonoverlapping(data_ptr, buffer_data_mut(buf), len);
        }
    }
    mark_as_uint8array(buf as usize);

    buf as i64
}

#[derive(Clone, Copy)]
enum TextEncoderDest {
    Buffer(*mut BufferHeader),
    TypedArray(*mut crate::typedarray::TypedArrayHeader),
}

fn text_value_pointer_addr(value: f64) -> usize {
    let ptr = crate::value::js_nanbox_get_pointer(value);
    if ptr <= 0 {
        0
    } else {
        ptr as usize
    }
}

fn text_string_header_to_string(ptr: *const StringHeader) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

fn text_encoder_describe_received(value: f64) -> String {
    if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        let ptr = unsafe { crate::symbol::js_symbol_to_string(value) } as *const StringHeader;
        return format!("type symbol ({})", text_string_header_to_string(ptr));
    }

    let addr = text_value_pointer_addr(value);
    if addr >= 0x1000 {
        if let Some(kind) = crate::typedarray::lookup_typed_array_kind(addr) {
            return format!("an instance of {}", crate::typedarray::name_for_kind(kind));
        }
        if crate::buffer::is_data_view(addr) {
            return "an instance of DataView".to_string();
        }
        if crate::buffer::is_uint8array_buffer(addr) {
            return "an instance of Uint8Array".to_string();
        }
        if crate::buffer::is_array_buffer(addr) {
            return "an instance of ArrayBuffer".to_string();
        }
        if crate::buffer::is_shared_array_buffer(addr) {
            return "an instance of SharedArrayBuffer".to_string();
        }
        if crate::buffer::is_registered_buffer(addr) {
            return "an instance of Buffer".to_string();
        }
    }

    crate::fs::validate::describe_received(value)
}

fn throw_invalid_encode_into_source(value: f64) -> ! {
    let message = format!(
        "The \"src\" argument must be of type string. Received {}",
        text_encoder_describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn throw_invalid_encode_into_dest(value: f64) -> ! {
    let message = format!(
        "The \"dest\" argument must be an instance of Uint8Array. Received {}",
        text_encoder_describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn text_encoder_encode_into_source(source: f64) -> *const StringHeader {
    let value = crate::value::JSValue::from_bits(source.to_bits());
    if !value.is_any_string() {
        throw_invalid_encode_into_source(source);
    }

    let ptr = crate::value::js_get_string_pointer_unified(source) as *const StringHeader;
    if ptr.is_null() {
        throw_invalid_encode_into_source(source);
    }
    ptr
}

fn text_encoder_encode_into_dest(dest: f64) -> TextEncoderDest {
    let addr = text_value_pointer_addr(dest);
    if addr >= 0x1000 {
        if crate::typedarray::lookup_typed_array_kind(addr) == Some(crate::typedarray::KIND_UINT8) {
            return TextEncoderDest::TypedArray(addr as *mut crate::typedarray::TypedArrayHeader);
        }
        if crate::buffer::is_registered_buffer(addr)
            && !crate::buffer::is_any_array_buffer(addr)
            && !crate::buffer::is_data_view(addr)
        {
            return TextEncoderDest::Buffer(addr as *mut BufferHeader);
        }
    }

    throw_invalid_encode_into_dest(dest)
}

fn text_encoder_result(read: usize, written: usize) -> *mut ObjectHeader {
    let obj = js_object_alloc(0, 2);
    if obj.is_null() {
        return obj;
    }

    let read_key = js_string_from_bytes(b"read".as_ptr(), 4);
    let written_key = js_string_from_bytes(b"written".as_ptr(), 7);
    js_object_set_field_by_name(obj, read_key, read as f64);
    js_object_set_field_by_name(obj, written_key, written as f64);
    obj
}

fn text_encoder_prefix_len(src: &[u8], dest_len: usize) -> (usize, usize) {
    if src.is_empty() || dest_len == 0 {
        return (0, 0);
    }
    if src.is_ascii() {
        let written = src.len().min(dest_len);
        return (written, written);
    }

    match std::str::from_utf8(src) {
        Ok(s) => {
            let mut read = 0usize;
            let mut written = 0usize;
            for ch in s.chars() {
                let byte_len = ch.len_utf8();
                if written + byte_len > dest_len {
                    break;
                }
                written += byte_len;
                read += ch.len_utf16();
            }
            (read, written)
        }
        Err(_) => {
            let written = src.len().min(dest_len);
            let read = crate::string::compute_utf16_len(src.as_ptr(), written as u32) as usize;
            (read, written)
        }
    }
}

/// `encoder.encodeInto(str, dest)` — UTF-8 encode into an existing Uint8Array.
///
/// Returns an object with Node's `{ read, written }` shape. `read` counts UTF-16
/// code units consumed from the source string; `written` counts bytes copied to
/// the destination and never splits a UTF-8 sequence.
#[no_mangle]
pub extern "C" fn js_text_encoder_encode_into_llvm(source: f64, dest: f64) -> i64 {
    let str_ptr = text_encoder_encode_into_source(source);
    let dest = text_encoder_encode_into_dest(dest);

    unsafe {
        let src_len = (*str_ptr).byte_len as usize;
        let src_data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let src = std::slice::from_raw_parts(src_data, src_len);
        let dest_len = match dest {
            TextEncoderDest::Buffer(dest_ptr) => (*dest_ptr).length as usize,
            TextEncoderDest::TypedArray(dest_ptr) => {
                crate::typedarray::typed_array_bytes_mut(dest_ptr)
                    .map(|bytes| bytes.len())
                    .unwrap_or(0)
            }
        };
        let (read, written) = text_encoder_prefix_len(src, dest_len);

        match dest {
            TextEncoderDest::Buffer(dest_ptr) => {
                for (idx, byte) in src.iter().copied().take(written).enumerate() {
                    crate::buffer::js_buffer_set(dest_ptr, idx as i32, byte as i32);
                }
            }
            TextEncoderDest::TypedArray(dest_ptr) => {
                if let Some(bytes) = crate::typedarray::typed_array_bytes_mut(dest_ptr) {
                    bytes[..written].copy_from_slice(&src[..written]);
                }
            }
        }

        text_encoder_result(read, written) as i64
    }
}

/// `decoder.decode(buf)` — UTF-8 decode a NaN-boxed `BufferHeader` value.
///
/// Returns a `*const StringHeader` as i64 — the codegen NaN-boxes with
/// `STRING_TAG`. Both TextEncoder output and `new Uint8Array([...])` share
/// the same packed-u8 BufferHeader layout, so a single read path covers both.
#[no_mangle]
pub extern "C" fn js_text_decoder_decode_llvm(handle: f64, value: f64) -> i64 {
    // Pull the decoder state (encoding / fatal). Unknown handle → utf-8,
    // non-fatal (matches the old stateless default).
    let id = decoder_handle_id(handle);
    let (encoding, fatal, label) = DECODER_REGISTRY
        .lock()
        .unwrap()
        .get(&id)
        .map(|s| (s.encoding, s.fatal, s.label))
        .unwrap_or((DecoderEncoding::Utf8, false, "utf-8"));

    // Node `TextDecoder.prototype.decode(input)` input contract:
    //   - omitted / undefined → decode empty (returns "").
    //   - null / arrays / numbers / strings / any non-buffer-source →
    //     ERR_INVALID_ARG_TYPE.
    //   - ArrayBuffer / SharedArrayBuffer / DataView / TypedArray view →
    //     decode exactly the bytes in the relevant view range.
    let jsval = crate::value::JSValue::from_bits(value.to_bits());
    if jsval.is_undefined() {
        return js_string_from_bytes(std::ptr::null(), 0) as i64;
    }

    let bits = value.to_bits();
    let ptr_usize: usize = {
        const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
        const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
        const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
        if (bits & TAG_MASK) == POINTER_TAG {
            (bits & POINTER_MASK) as usize
        } else if !value.is_nan() && bits != 0 && bits < 0x0001_0000_0000_0000 {
            bits as usize
        } else {
            0
        }
    };

    if ptr_usize < 0x1000 {
        // null, numbers, booleans, small pointers — not a buffer source.
        throw_invalid_decode_input();
    }

    // Route by concrete kind so the byte offset/length is honored and only
    // genuine buffer sources are accepted.
    let bytes: &[u8] = unsafe {
        if crate::typedarray::lookup_typed_array_kind(ptr_usize).is_some() {
            // TypedArray view (incl. Uint16Array, sliced subarray, etc.).
            match crate::typedarray::typed_array_bytes(
                ptr_usize as *const crate::typedarray::TypedArrayHeader,
            ) {
                Some(b) => b,
                None => throw_invalid_decode_input(),
            }
        } else if crate::buffer::is_data_view(ptr_usize)
            || crate::buffer::is_any_array_buffer(ptr_usize)
            || crate::buffer::is_registered_buffer(ptr_usize)
        {
            // DataView, (Shared)ArrayBuffer, or a registered Buffer/Uint8Array
            // — all BufferHeader-backed with the bytes stored inline.
            let buf = ptr_usize as *const BufferHeader;
            let len = (*buf).length as usize;
            let data = (buf as *const u8).add(std::mem::size_of::<BufferHeader>());
            std::slice::from_raw_parts(data, len)
        } else {
            // Plain arrays, plain objects, strings — reject like Node.
            throw_invalid_decode_input();
        }
    };

    decode_bytes(bytes, encoding, fatal, label)
}

fn throw_invalid_decode_input() -> ! {
    crate::fs::validate::throw_type_error_with_code(
        "The \"list\" argument must be an instance of SharedArrayBuffer, \
         ArrayBuffer or ArrayBufferView.",
        "ERR_INVALID_ARG_TYPE",
    )
}

/// Decode `bytes` per `encoding`; returns a `*mut StringHeader` as i64.
/// `fatal` only affects UTF-8 (latin1/utf-16le never error in Node).
fn decode_bytes(bytes: &[u8], encoding: DecoderEncoding, fatal: bool, label: &str) -> i64 {
    match encoding {
        DecoderEncoding::Utf8 => {
            if fatal {
                match std::str::from_utf8(bytes) {
                    Ok(s) => js_string_from_bytes(s.as_ptr(), s.len() as u32) as i64,
                    Err(_) => throw_invalid_encoded_data(label),
                }
            } else {
                // Lossy decode: invalid sequences become U+FFFD, exactly
                // like Node's non-fatal TextDecoder.
                let s = String::from_utf8_lossy(bytes);
                js_string_from_bytes(s.as_ptr(), s.len() as u32) as i64
            }
        }
        DecoderEncoding::SingleByte(table) => {
            // ASCII passes through; 0x80..=0xFF map via the WHATWG index. In
            // fatal mode an unmapped byte (0xFFFD in the table) is a decode
            // error; non-fatal keeps the U+FFFD replacement.
            let mut out = String::with_capacity(bytes.len());
            for &b in bytes {
                let cp = if b < 0x80 {
                    b as u32
                } else {
                    table[(b - 0x80) as usize] as u32
                };
                if fatal && cp == 0xFFFD {
                    throw_invalid_encoded_data(label);
                }
                // Single-byte tables only hold BMP scalars (never surrogates).
                out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
            }
            js_string_from_bytes(out.as_ptr(), out.len() as u32) as i64
        }
        DecoderEncoding::Utf16Le => {
            // Little-endian UTF-16 code units; an odd trailing byte and
            // unpaired surrogates decode to U+FFFD (lossy, matching Node's
            // non-fatal default). We keep inputs in safe ranges in tests.
            let mut units: Vec<u16> = Vec::with_capacity(bytes.len() / 2);
            let mut i = 0;
            while i + 1 < bytes.len() {
                units.push(u16::from_le_bytes([bytes[i], bytes[i + 1]]));
                i += 2;
            }
            if i < bytes.len() {
                units.push(0xFFFD);
            }
            let s = String::from_utf16_lossy(&units);
            js_string_from_bytes(s.as_ptr(), s.len() as u32) as i64
        }
    }
}

fn throw_invalid_encoded_data(encoding: &str) -> ! {
    let message = format!("The encoded data was not valid for encoding {encoding}");
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_ENCODING_INVALID_ENCODED_DATA")
}

/// Keepalive anchors — these `#[no_mangle]` fns are only called from
/// generated `.o`, so the auto-optimize whole-program bitcode rebuild
/// would dead-strip them without `#[used]` retention (see
/// [[project_auto_optimize_keepalive_3320]]).
#[used]
static KEEP_TEXT_DECODER_NEW: extern "C" fn(f64, f64, f64) -> i64 = js_text_decoder_new;
#[used]
static KEEP_TEXT_DECODER_DECODE: extern "C" fn(f64, f64) -> i64 = js_text_decoder_decode_llvm;
#[used]
static KEEP_TEXT_DECODER_ENCODING: extern "C" fn(f64) -> *mut StringHeader =
    js_text_decoder_encoding;
#[used]
static KEEP_TEXT_DECODER_FATAL: extern "C" fn(f64) -> f64 = js_text_decoder_fatal;
#[used]
static KEEP_TEXT_DECODER_IGNORE_BOM: extern "C" fn(f64) -> f64 = js_text_decoder_ignore_bom;

/// TextDecoder / TextEncoder registry-handle property surface for VALUE
/// reads (`K.decode.bind(K)` — the shape a minified SDK's cached decodeText
/// helper takes; pre-fix the read returned undefined and `.bind` threw
/// "Bind must be called on a function"). Methods reify as bound methods —
/// the dynamic-call arm in `native_call_method.rs` executes them on call —
/// and accessors return their values directly.
pub(crate) unsafe fn text_handle_property(
    raw: usize,
    key_bytes: &[u8],
    key_ptr: *const u8,
    key_len: usize,
) -> Option<crate::value::JSValue> {
    let this_f64 = f64::from_bits(crate::value::js_nanbox_pointer(raw as i64).to_bits());
    if is_known_text_decoder_id(raw as i64) {
        match key_bytes {
            b"decode" => {
                let result = crate::object::js_class_method_bind(this_f64, key_ptr, key_len);
                return Some(crate::value::JSValue::from_bits(result.to_bits()));
            }
            b"encoding" => {
                let s = js_text_decoder_encoding(this_f64);
                return Some(crate::value::JSValue::string_ptr(s));
            }
            b"fatal" => {
                return Some(crate::value::JSValue::from_bits(
                    js_text_decoder_fatal(this_f64).to_bits(),
                ));
            }
            b"ignoreBOM" => {
                return Some(crate::value::JSValue::from_bits(
                    js_text_decoder_ignore_bom(this_f64).to_bits(),
                ));
            }
            _ => {}
        }
    }
    if raw as i64 == TEXT_ENCODER_SENTINEL_ID && matches!(key_bytes, b"encode" | b"encodeInto") {
        let result = crate::object::js_class_method_bind(this_f64, key_ptr, key_len);
        return Some(crate::value::JSValue::from_bits(result.to_bits()));
    }
    None
}
