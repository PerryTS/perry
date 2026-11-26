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
//! `TextEncoder` / `TextDecoder` instances are ORDINARY objects — a
//! `GC_TYPE_OBJECT` with a real ShapeId, a family class id and a per-family
//! prototype — identical in kind to an object TypeScript itself creates. That
//! is the honest-tag invariant of #340/#341: a `POINTER_TAG` value is always a
//! dereferenceable GC cell, and a runtime-made object is not a second kind of
//! thing.
//!
//! They used to be small registry integers wearing `POINTER_TAG`, which made a
//! value's identity its registry id: every `TextEncoder` shared one sentinel,
//! so two of them were `===`, two collapsed into one `Map`/`Set`/`WeakMap` key,
//! and `JSON.stringify` gave `null` where node gives `{}` (#10821).
//!
//! A decoder's entire state is three scalars, so it rides in the object's own
//! `ObjectMeta.native_state` word (packing below) and `DECODER_REGISTRY` — a
//! mutex-guarded map that grew one entry per `new TextDecoder()` and was never
//! pruned — is gone outright, taking a leak and a lock off the decode path.

use std::sync::Mutex;

use crate::buffer::BufferHeader;
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
    Utf16Be,
}

#[derive(Clone, Copy)]
struct DecoderState {
    encoding: DecoderEncoding,
    /// WHATWG-canonical label reported by `decoder.encoding`.
    label: &'static str,
    fatal: bool,
    ignore_bom: bool,
}

// ---------------------------------------------------------------------------
// Honest tags (#340/#341, #10821): instances are ORDINARY objects.
//
// `js_object_alloc(CLASS_ID, 0)` gives a `GC_TYPE_OBJECT` with a real ShapeId
// and ZERO own keys, linked to the per-family prototype that carries
// `decode` / `encode` / `encodeInto` as methods and `encoding` / `fatal` /
// `ignoreBOM` as accessors. Reads take the ordinary prototype path — this
// family has no handle-dispatch arm any more — so node parity follows from the
// object's shape rather than from a dispatch table: `typeof` is `"object"`,
// `Object.keys` is `[]`, `JSON.stringify` is `{}`, `instanceof` works, and two
// instances are two objects (#10821).
//
// Per-instance state lives in `ObjectMeta.native_state`, packed as
//     bit 0       present
//     bit 1       fatal
//     bit 2       ignoreBOM
//     bit 3       a `stream: true` decode has consumed (or ruled out) the BOM
//     bits 8..32  index into `ENCODINGS`
//     bits 32..34 how many bytes an unfinished `stream: true` decode keeps
//     bits 40..64 those bytes (at most 3: the start of a UTF-8 sequence, or
//                 an odd byte and/or a high surrogate of UTF-16)
//
// `ENCODINGS` interns the DISTINCT encodings a program actually names. It is
// bounded by the compile-time label set (~40 entries), never grows per decoder,
// and therefore needs no pruning when a decoder dies. It replaces
// `DECODER_REGISTRY`, which grew one entry per `new TextDecoder()` and was
// never released — so this also removes a leak and a mutex from `decode`.
// ---------------------------------------------------------------------------

/// Class ids in the web-builtin block (`0xFFFF_24xx`); `0x2401..=0x2406` are
/// AbortController/AbortSignal/Event/CustomEvent/DOMException/EventTarget.
pub(crate) const TEXT_ENCODER_CLASS_ID: u32 = crate::native_class_ids::TEXT_ENCODER;
pub(crate) const TEXT_DECODER_CLASS_ID: u32 = crate::native_class_ids::TEXT_DECODER;

const STATE_PRESENT: u64 = 1;
const STATE_FATAL: u64 = 1 << 1;
const STATE_IGNORE_BOM: u64 = 1 << 2;
const STATE_STREAM_STARTED: u64 = 1 << 3;
const STATE_ENCODING_SHIFT: u32 = 8;
const STATE_ENCODING_MASK: u64 = 0xFF_FFFF;
const STATE_PENDING_LEN_SHIFT: u32 = 32;
const STATE_PENDING_SHIFT: u32 = 40;
/// The bits that describe an unfinished stream; a stream's end clears them.
const STATE_STREAM_MASK: u64 =
    STATE_STREAM_STARTED | (0b11 << STATE_PENDING_LEN_SHIFT) | (0xFF_FFFF << STATE_PENDING_SHIFT);

per_test_global! {
    /// Distinct `(encoding, canonical label)` pairs named so far. Append-only,
    /// bounded by the compile-time label set — NOT a per-instance registry.
    ///
    /// Per-thread in a test build: `the_encoding_intern_does_not_grow_per_decoder`
    /// asserts on this table's length, and a sibling test naming a fresh label
    /// on another libtest thread would otherwise shift it by one.
    static ENCODINGS: std::sync::LazyLock<Mutex<Vec<(DecoderEncoding, &'static str)>>> =
        std::sync::LazyLock::new(|| Mutex::new(vec![(DecoderEncoding::Utf8, "utf-8")]))
}

fn intern_encoding(encoding: DecoderEncoding, label: &'static str) -> u64 {
    let mut table = ENCODINGS.lock().unwrap();
    // Compared by VALUE: two occurrences of the same canonical label are not
    // guaranteed to be one address (thin LTO may duplicate a literal), and an
    // address compare would then mint a second row for one encoding.
    if let Some(index) = table
        .iter()
        .position(|(e, l)| *e == encoding && *l == label)
    {
        return index as u64;
    }
    table.push((encoding, label));
    (table.len() - 1) as u64
}

fn encoding_at(index: u64) -> (DecoderEncoding, &'static str) {
    ENCODINGS
        .lock()
        .unwrap()
        .get(index as usize)
        .copied()
        .unwrap_or((DecoderEncoding::Utf8, "utf-8"))
}

fn pack_decoder_state(state: &DecoderState) -> u64 {
    let index = intern_encoding(state.encoding, state.label);
    let mut word = STATE_PRESENT | (index << STATE_ENCODING_SHIFT);
    if state.fatal {
        word |= STATE_FATAL;
    }
    if state.ignore_bom {
        word |= STATE_IGNORE_BOM;
    }
    word
}

fn unpack_decoder_state(word: u64) -> Option<DecoderState> {
    if word & STATE_PRESENT == 0 {
        return None;
    }
    let (encoding, label) = encoding_at((word >> STATE_ENCODING_SHIFT) & STATE_ENCODING_MASK);
    Some(DecoderState {
        encoding,
        label,
        fatal: word & STATE_FATAL != 0,
        ignore_bom: word & STATE_IGNORE_BOM != 0,
    })
}

/// The `native_state` word of a text instance, or `None` for any other value.
/// Gated on the class id in the object header, so a foreign receiver
/// (`TextDecoder.prototype.decode.call({})`) is rejected rather than
/// misinterpreted — the same brand check node performs.
fn text_native_state(value: f64, class_id: u32) -> Option<u64> {
    let bits = value.to_bits();
    if (bits & crate::value::TAG_MASK) != crate::value::POINTER_TAG {
        return None;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    let header = unsafe { crate::value::addr_class::try_read_gc_header(addr)? };
    if crate::gc_type_is_not_object!(header.obj_type) {
        return None;
    }
    let obj = addr as *mut ObjectHeader;
    unsafe {
        if (*obj).class_id != class_id {
            return None;
        }
        let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
        if meta.is_null() {
            return Some(0);
        }
        Some((*meta).native_state)
    }
}

/// Decoder state for a receiver, or the utf-8 non-fatal default for a receiver
/// that carries none (matching the previous "unknown handle" behaviour).
fn decoder_state_of(value: f64) -> DecoderState {
    text_native_state(value, TEXT_DECODER_CLASS_ID)
        .and_then(unpack_decoder_state)
        .unwrap_or(DecoderState {
            encoding: DecoderEncoding::Utf8,
            label: "utf-8",
            fatal: false,
            ignore_bom: false,
        })
}

/// Allocate a text instance: an ordinary object with no own keys, linked to
/// its family prototype so methods and accessors resolve by ordinary lookup.
///
/// The prototype is where this family's whole surface lives, so it is not
/// optional: without it `d.decode` and `d.encoding` are `undefined`. It is
/// built by `populate_builtin_prototype_methods` under `global-text`, which the
/// compiler's feature analysis must therefore enable for any program that
/// constructs one — see the `TextEncoder`/`TextDecoder` arm of
/// `collect_modules/feature_detect.rs`, which matches the folded HIR nodes and
/// not just a quoted type name for this reason.
fn alloc_text_object(class_id: u32, builtin_name: &str, state: u64) -> i64 {
    let obj = js_object_alloc(class_id, 0);
    if obj.is_null() {
        return 0;
    }
    // Everything below can allocate -- materializing the prototype builds
    // `globalThis` lazily and the meta record is its own allocation -- and
    // `GC_TYPE_OBJECT` is movable, so the instance is re-read through its
    // handle after each one rather than carried as a raw pointer.
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let proto_value = crate::object::builtin_prototype_value(builtin_name);
    debug_assert!(
        crate::value::JSValue::from_bits(proto_value.to_bits()).is_pointer(),
        "{builtin_name}.prototype is missing: the instance would have no methods"
    );
    if crate::value::JSValue::from_bits(proto_value.to_bits()).is_pointer() {
        obj_handle.with_mut_ptr::<ObjectHeader, _>(|obj| {
            crate::object::prototype_chain::object_link_class_default_prototype(
                obj as usize,
                proto_value.to_bits(),
            );
        });
    }
    if state != 0 {
        obj_handle.with_mut_ptr::<ObjectHeader, _>(|obj| unsafe {
            let meta = crate::object::object_meta_ensure(
                crate::meta_obj_runtime_born!(obj),
                crate::object::MetaReason::RuntimeHandle,
            );
            // A decoder whose state word never landed would read back as the
            // lenient utf-8 default -- wrong label, wrong `fatal` -- rather than
            // fail, so the only way this stays honest is that the meta exists.
            debug_assert!(!meta.is_null(), "a text instance must carry its meta");
            if !meta.is_null() {
                (*meta).native_state = state;
            }
        });
    }
    obj_handle.with_mut_ptr::<ObjectHeader, _>(|obj| obj as i64)
}

/// Map a user-supplied encoding label to (enum, canonical-name).
/// Returns `None` for unsupported labels (caller throws `RangeError`).
/// Decode immutable bytes with a TextDecoder label, for APIs that expose the
/// same encoding semantics without creating a user-visible decoder object.
/// Returns the allocated StringHeader pointer, or `None` for an unsupported
/// label so the caller can raise its API-specific error.
/// An over-long result is reported, not thrown: the caller throws once its
/// own Rust values are dropped.
pub(crate) fn decode_bytes_with_label(
    bytes: &[u8],
    raw_label: &str,
) -> Option<Result<i64, crate::buffer::StringTooLong>> {
    let (encoding, _) = resolve_decoder_label(raw_label)?;
    Some(
        match decode_bytes(strip_bom(bytes, encoding), encoding, false) {
            Ok(decoded) => Ok(decoded),
            Err(DecodeFailure::TooLong) => Err(crate::buffer::StringTooLong),
            Err(DecodeFailure::Malformed) => unreachable!("a non-fatal decode is never malformed"),
        },
    )
}

/// Consume one leading BOM, as TextDecoder's default `ignoreBOM: false` does.
/// Only Unicode encodings have BOM signatures; single-byte labels preserve
/// these bytes as ordinary content.
fn strip_bom(bytes: &[u8], encoding: DecoderEncoding) -> &[u8] {
    bytes.strip_prefix(bom_of(encoding)).unwrap_or(bytes)
}

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
        "utf-16be" | "unicodefffe" => Some((DecoderEncoding::Utf16Be, "utf-16be")),
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
    crate::exception::throw_core(crate::error::CoreError::type_error(msg))
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

/// `new TextEncoder()` — an ordinary object with no own keys, linked to
/// `TextEncoder.prototype`. It carries no state (it always encodes UTF-8), but
/// it is still one object per construction: every encoder used to share one
/// sentinel id, which made `new TextEncoder() === new TextEncoder()` true and
/// collapsed two encoders into one `Map` key (#10821).
#[no_mangle]
pub extern "C" fn js_text_encoder_new() -> i64 {
    if crate::hot_diag::receiver_repr_on() {
        crate::hot_diag::receiver_repr_note_constructed(crate::hot_diag::ReceiverReprFamily::Text);
    }
    alloc_text_object(TEXT_ENCODER_CLASS_ID, "TextEncoder", 0)
}

/// `new TextDecoder(label?, { fatal?, ignoreBOM? })` — validates the label and
/// returns an ordinary object linked to `TextDecoder.prototype`, carrying its
/// decode state in the object's own meta word. An unsupported label throws a
/// `RangeError` (`ERR_ENCODING_NOT_SUPPORTED`).
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
    if crate::hot_diag::receiver_repr_on() {
        crate::hot_diag::receiver_repr_note_constructed(crate::hot_diag::ReceiverReprFamily::Text);
    }
    // The whole per-instance state is three scalars, so it rides in the
    // object's own meta word — there is no registry entry to allocate here and
    // nothing to release when the decoder dies.
    alloc_text_object(
        TEXT_DECODER_CLASS_ID,
        "TextDecoder",
        pack_decoder_state(&state),
    )
}

/// `decoder.encoding` — WHATWG-canonical label.
#[no_mangle]
pub extern "C" fn js_text_decoder_encoding(handle: f64) -> *mut StringHeader {
    let label = decoder_state_of(handle).label;
    js_string_from_bytes(label.as_ptr(), label.len() as u32)
}

/// `decoder.fatal` — boolean (NaN-boxed by codegen).
#[no_mangle]
pub extern "C" fn js_text_decoder_fatal(handle: f64) -> f64 {
    let fatal = decoder_state_of(handle).fatal;
    if fatal {
        f64::from_bits(0x7FFC_0000_0000_0004) // TAG_TRUE
    } else {
        f64::from_bits(0x7FFC_0000_0000_0003) // TAG_FALSE
    }
}

/// `decoder.ignoreBOM` — boolean (NaN-boxed by codegen).
#[no_mangle]
pub extern "C" fn js_text_decoder_ignore_bom(handle: f64) -> f64 {
    let ignore = decoder_state_of(handle).ignore_bom;
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
    // #7341: `data_ptr` points into the StringHeader's payload and is read by
    // the copy BELOW `buffer_alloc`. An evacuating minor inside that
    // allocation relocates the string and the copy reads retired from-space —
    // the same stale-`memmove` fault as `js_buffer_from_string`.
    let _no_move = crate::gc::GcSuppressScope::new();
    let (data_ptr, len) = unsafe {
        let l = (*str_ptr).byte_len as usize;
        let d = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        (d, l)
    };

    crate::value::JSValue::from_bits(
        crate::buffer::bytes::from_slice(crate::buffer::bytes::Brand::Uint8Array, unsafe {
            std::slice::from_raw_parts(data_ptr, len)
        })
        .to_bits(),
    )
    .as_pointer::<BufferHeader>() as i64
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
        if let Some(kind) = crate::typedarray_hook_slots::lookup_typed_array_kind(addr) {
            return format!(
                "an instance of {}",
                crate::typedarray_hook_slots::name_for_kind(kind)
            );
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

fn text_encoder_encode_into_dest(dest: f64) -> f64 {
    let addr = text_value_pointer_addr(dest);
    if addr >= 0x1000 {
        if crate::typedarray_hook_slots::lookup_typed_array_kind(addr)
            == Some(crate::typedarray_hook_slots::KIND_UINT8)
        {
            return dest;
        }
        if crate::buffer::is_registered_buffer(addr)
            && !crate::buffer::is_any_array_buffer(addr)
            && !crate::buffer::is_data_view(addr)
        {
            return dest;
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

    let (read, written) = crate::buffer::bytes::no_gc(|scope| unsafe {
        let src_len = (*str_ptr).byte_len as usize;
        let src_data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let src = std::slice::from_raw_parts(src_data, src_len);
        let Ok(bytes) = crate::buffer::bytes::bytes_mut(dest, scope) else {
            return (0, 0);
        };
        let (read, written) = text_encoder_prefix_len(src, bytes.len());
        bytes[..written].copy_from_slice(&src[..written]);
        (read, written)
    });
    text_encoder_result(read, written) as i64
}

/// The bytes an unfinished `stream: true` decode keeps for the next call.
fn stream_pending(word: u64) -> ([u8; 3], usize) {
    let len = ((word >> STATE_PENDING_LEN_SHIFT) & 0b11) as usize;
    let bytes = (word >> STATE_PENDING_SHIFT).to_le_bytes();
    ([bytes[0], bytes[1], bytes[2]], len)
}

fn with_stream_state(word: u64, started: bool, pending: &[u8]) -> u64 {
    debug_assert!(pending.len() <= 3);
    let mut word = word & !STATE_STREAM_MASK;
    if started {
        word |= STATE_STREAM_STARTED;
    }
    let mut packed = [0u8; 8];
    packed[..pending.len()].copy_from_slice(pending);
    word | ((pending.len() as u64) << STATE_PENDING_LEN_SHIFT)
        | (u64::from_le_bytes(packed) << STATE_PENDING_SHIFT)
}

/// Store a decoder's state word back on the receiver `decoder_state_of` read.
fn store_decoder_word(value: f64, word: u64) {
    if text_native_state(value, TEXT_DECODER_CLASS_ID).is_none() {
        return;
    }
    let obj = (value.to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader;
    unsafe {
        let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
        if !meta.is_null() {
            (*meta).native_state = word;
        }
    }
}

fn bom_of(encoding: DecoderEncoding) -> &'static [u8] {
    match encoding {
        DecoderEncoding::Utf8 => &[0xEF, 0xBB, 0xBF],
        DecoderEncoding::Utf16Le => &[0xFF, 0xFE],
        DecoderEncoding::Utf16Be => &[0xFE, 0xFF],
        DecoderEncoding::SingleByte(_) => &[],
    }
}

/// How many trailing bytes of `bytes` start a sequence the next chunk can
/// still complete: a UTF-8 sequence whose bytes so far are a valid prefix, or
/// for UTF-16 an odd byte and/or a high surrogate waiting for its low half.
/// An invalid prefix is not kept: it decodes (to U+FFFD) in this call.
fn incomplete_tail_len(bytes: &[u8], encoding: DecoderEncoding) -> usize {
    match encoding {
        DecoderEncoding::Utf8 => {
            for back in 1..=bytes.len().min(3) {
                let lead = bytes[bytes.len() - back];
                if (0x80..=0xBF).contains(&lead) {
                    continue;
                }
                let need = match lead {
                    0xC2..=0xDF => 2,
                    0xE0..=0xEF => 3,
                    0xF0..=0xF4 => 4,
                    _ => return 0,
                };
                if back >= need {
                    return 0;
                }
                // The second byte has a narrower range after some leads.
                let second_ok = |b: u8| match lead {
                    0xE0 => (0xA0..=0xBF).contains(&b),
                    0xED => (0x80..=0x9F).contains(&b),
                    0xF0 => (0x90..=0xBF).contains(&b),
                    0xF4 => (0x80..=0x8F).contains(&b),
                    _ => (0x80..=0xBF).contains(&b),
                };
                let tail = &bytes[bytes.len() - back..];
                return if tail.len() < 2 || second_ok(tail[1]) {
                    back
                } else {
                    0
                };
            }
            0
        }
        DecoderEncoding::Utf16Le | DecoderEncoding::Utf16Be => {
            let odd = bytes.len() % 2;
            let whole = bytes.len() - odd;
            if whole >= 2 {
                let pair = [bytes[whole - 2], bytes[whole - 1]];
                let unit = match encoding {
                    DecoderEncoding::Utf16Be => u16::from_be_bytes(pair),
                    _ => u16::from_le_bytes(pair),
                };
                if (0xD800..=0xDBFF).contains(&unit) {
                    return 2 + odd;
                }
            }
            odd
        }
        DecoderEncoding::SingleByte(_) => 0,
    }
}

/// One `decode` call over `input` with the decoder's state `word`: the decoded
/// string (an error when `fatal` rejects the input or the text is too long) and the word to store. A
/// `stream: true` call keeps an unfinished sequence for the next one and
/// consumes the BOM once per stream; any other call ends the stream, so what
/// is still pending decodes now and the next call starts a new stream. A
/// rejected decode also ends the stream.
fn decode_with_state(
    state: &DecoderState,
    word: u64,
    input: &[u8],
    stream: bool,
) -> (Result<i64, DecodeFailure>, u64) {
    let (pending, pending_len) = stream_pending(word);
    let joined;
    let mut bytes = if pending_len == 0 {
        input
    } else {
        joined = [&pending[..pending_len], input].concat();
        &joined[..]
    };
    let mut started = word & STATE_STREAM_STARTED != 0;
    if !started {
        let bom = bom_of(state.encoding);
        if !state.ignore_bom && !bom.is_empty() {
            if stream && bytes.len() < bom.len() && bom.starts_with(bytes) {
                // Too few bytes yet to tell a BOM from content.
                let empty = js_string_from_bytes(std::ptr::null(), 0) as i64;
                return (Ok(empty), with_stream_state(word, false, bytes));
            }
            bytes = bytes.strip_prefix(bom).unwrap_or(bytes);
        }
        started = pending_len > 0 || !input.is_empty();
    }
    let keep = if stream {
        incomplete_tail_len(bytes, state.encoding)
    } else {
        0
    };
    let (body, tail) = bytes.split_at(bytes.len() - keep);
    let decoded = decode_bytes(body, state.encoding, state.fatal);
    let next = if stream && decoded.is_ok() {
        with_stream_state(word, started, tail)
    } else {
        with_stream_state(word, false, &[])
    };
    (decoded, next)
}

/// `options.stream` of a `decode(input, options)` call.
fn decode_stream_option(options: f64) -> bool {
    let jsval = crate::value::JSValue::from_bits(options.to_bits());
    if !jsval.is_pointer() {
        return false;
    }
    let obj = jsval.as_pointer::<ObjectHeader>();
    if obj.is_null() {
        return false;
    }
    let key = js_string_from_bytes(b"stream".as_ptr(), 6);
    let value = crate::object::js_object_get_field_by_name(obj, key);
    crate::value::js_is_truthy(f64::from_bits(value.bits())) != 0
}

/// `decoder.decode(buf)` — UTF-8 decode a NaN-boxed `BufferHeader` value.
///
/// Returns a `*const StringHeader` as i64 — the codegen NaN-boxes with
/// `STRING_TAG`. Both TextEncoder output and `new Uint8Array([...])` share
/// the same packed-u8 BufferHeader layout, so a single read path covers both.
#[no_mangle]
pub extern "C" fn js_text_decoder_decode_llvm(handle: f64, value: f64) -> i64 {
    text_decoder_decode(handle, value, false)
}

/// `decoder.decode(input, { stream })`: the shared body of both entry points.
fn text_decoder_decode(handle: f64, value: f64, stream: bool) -> i64 {
    // Pull the decoder state (encoding / fatal). Unknown handle → utf-8,
    // non-fatal (matches the old stateless default).
    let state = decoder_state_of(handle);
    let word = text_native_state(handle, TEXT_DECODER_CLASS_ID).unwrap_or(0);

    // Node `TextDecoder.prototype.decode(input)` input contract:
    //   - omitted / undefined → no bytes (still ends a stream).
    //   - null / arrays / numbers / strings / any non-buffer-source →
    //     ERR_INVALID_ARG_TYPE.
    //   - ArrayBuffer / SharedArrayBuffer / DataView / TypedArray view →
    //     decode exactly the bytes in the relevant view range.
    let jsval = crate::value::JSValue::from_bits(value.to_bits());
    let (decoded, next) = if jsval.is_undefined() {
        decode_with_state(&state, word, &[], stream)
    } else {
        let bits = value.to_bits();
        let ptr_usize: usize = {
            const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
            const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
            const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
            if (bits & TAG_MASK) == POINTER_TAG {
                (bits & POINTER_MASK) as usize
            } else {
                0
            }
        };

        if ptr_usize < 0x1000 {
            // null, numbers, booleans, small pointers — not a buffer source.
            throw_invalid_decode_input();
        }

        let value = crate::value::js_nanbox_pointer(ptr_usize as i64);
        match crate::buffer::bytes::pin(value) {
            Ok(pin) => decode_with_state(
                &state,
                word,
                unsafe { std::slice::from_raw_parts(pin.as_ptr(), pin.len()) },
                stream,
            ),
            Err(
                crate::buffer::bytes::NotBytes::UnstableForeign
                | crate::buffer::bytes::NotBytes::Frozen,
            ) => {
                // Foreign engines have no retain protocol. Copy in a borrow scope
                // before decoding can allocate the returned JavaScript string.
                let input = crate::buffer::bytes::no_gc(|scope| {
                    crate::buffer::bytes::bytes(value, scope).map(<[u8]>::to_vec)
                })
                .unwrap_or_else(|_| throw_invalid_decode_input());
                decode_with_state(&state, word, &input, stream)
            }
            Err(_) => throw_invalid_decode_input(),
        }
    };
    if next != word {
        store_decoder_word(handle, next);
    }
    match decoded {
        Ok(decoded) => decoded,
        Err(DecodeFailure::Malformed) => throw_invalid_encoded_data(state.label),
        Err(DecodeFailure::TooLong) => crate::string::throw_string_too_long(),
    }
}

fn throw_invalid_decode_input() -> ! {
    crate::fs::validate::throw_type_error_with_code(
        "The \"list\" argument must be an instance of SharedArrayBuffer, \
         ArrayBuffer or ArrayBufferView.",
        "ERR_INVALID_ARG_TYPE",
    )
}

/// Why [`decode_bytes`] produced no string. The caller throws: a JS throw
/// skips Rust drops, so none may be pending at the throw.
enum DecodeFailure {
    /// `fatal` is set and the input is malformed.
    Malformed,
    /// The text is longer than `MAX_STRING_LENGTH` units (node:
    /// `ERR_STRING_TOO_LONG`).
    TooLong,
}

/// Text past the string limit is refused before it becomes a string.
fn decoded_string(s: &str) -> Result<i64, DecodeFailure> {
    if crate::string::decoded_text_over_limit(s) {
        return Err(DecodeFailure::TooLong);
    }
    Ok(js_string_from_bytes(s.as_ptr(), s.len() as u32) as i64)
}

/// Decode `bytes` per `encoding`; returns a `*mut StringHeader` as i64.
fn decode_bytes(
    bytes: &[u8],
    encoding: DecoderEncoding,
    fatal: bool,
) -> Result<i64, DecodeFailure> {
    let decoded = match encoding {
        DecoderEncoding::Utf8 => {
            if fatal {
                let s = std::str::from_utf8(bytes).map_err(|_| DecodeFailure::Malformed)?;
                decoded_string(s)?
            } else {
                // Lossy decode: invalid sequences become U+FFFD, exactly
                // like Node's non-fatal TextDecoder.
                let s = String::from_utf8_lossy(bytes);
                decoded_string(&s)?
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
                    return Err(DecodeFailure::Malformed);
                }
                // Single-byte tables only hold BMP scalars (never surrogates).
                out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
            }
            decoded_string(&out)?
        }
        DecoderEncoding::Utf16Le | DecoderEncoding::Utf16Be => {
            let to_unit = match encoding {
                DecoderEncoding::Utf16Be => u16::from_be_bytes,
                _ => u16::from_le_bytes,
            };
            let pairs = bytes.chunks_exact(2);
            let odd_byte = !pairs.remainder().is_empty();
            let mut units: Vec<u16> = pairs.map(|p| to_unit([p[0], p[1]])).collect();
            let s = match String::from_utf16(&units) {
                Ok(s) if !odd_byte => s,
                _ if fatal => return Err(DecodeFailure::Malformed),
                _ => {
                    // A trailing high surrogate and the odd byte after it are
                    // one end-of-input error, so they share one U+FFFD.
                    if odd_byte && !matches!(units.last(), Some(0xD800..=0xDBFF)) {
                        units.push(0xFFFD);
                    }
                    String::from_utf16_lossy(&units)
                }
            };
            decoded_string(&s)?
        }
    };
    Ok(decoded)
}

fn throw_invalid_encoded_data(encoding: &str) -> ! {
    let message = format!("The encoded data was not valid for encoding {encoding}");
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_ENCODING_INVALID_ENCODED_DATA")
}

/// Keepalive anchors — these `#[no_mangle]` fns are only called from
/// generated `.o`, so the auto-optimize whole-program bitcode rebuild
/// would dead-strip them without `#[used]` retention (see
/// [[project_auto_optimize_keepalive_3320]]).
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_TEXT_DECODER_NEW: extern "C" fn(f64, f64, f64) -> i64 = js_text_decoder_new;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_TEXT_DECODER_DECODE: extern "C" fn(f64, f64) -> i64 = js_text_decoder_decode_llvm;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_TEXT_DECODER_ENCODING: extern "C" fn(f64) -> *mut StringHeader =
    js_text_decoder_encoding;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_TEXT_DECODER_FATAL: extern "C" fn(f64) -> f64 = js_text_decoder_fatal;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_TEXT_DECODER_IGNORE_BOM: extern "C" fn(f64) -> f64 = js_text_decoder_ignore_bom;

/// TextDecoder / TextEncoder registry-handle property surface for VALUE
/// reads (`K.decode.bind(K)` — the shape a minified SDK's cached decodeText
/// helper takes; pre-fix the read returned undefined and `.bind` threw
/// "Bind must be called on a function"). Methods reify as bound methods —
/// the dynamic-call arm in `native_call_method.rs` executes them on call —
/// and accessors return their values directly.
// ---------------------------------------------------------------------------
// Prototype thunks.
//
// A native method or accessor installed on a shared prototype is a
// `ClosureHeader` body; it reads its receiver from its `this` parameter.
// `temporal_proto_getter_thunk` is the template.
// Each thunk brand-checks its receiver against the family class id, so
// `TextDecoder.prototype.decode.call({})` throws a TypeError exactly as node
// does, instead of silently decoding as utf-8.
// ---------------------------------------------------------------------------

fn require_text_brand(value: f64, class_id: u32, message: &[u8]) {
    if text_native_state(value, class_id).is_none() {
        throw_type_error(message);
    }
}

pub(crate) extern "C" fn text_decoder_decode_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    input: f64,
    options: f64,
) -> f64 {
    let this = this.as_f64();
    require_text_brand(
        this,
        TEXT_DECODER_CLASS_ID,
        b"TextDecoder.prototype.decode called on an incompatible receiver",
    );
    let s = text_decoder_decode(this, input, decode_stream_option(options));
    f64::from_bits(crate::value::JSValue::string_ptr(s as *mut StringHeader).bits())
}

pub(crate) extern "C" fn text_decoder_encoding_getter(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let this = this.as_f64();
    require_text_brand(
        this,
        TEXT_DECODER_CLASS_ID,
        b"TextDecoder.prototype.encoding called on an incompatible receiver",
    );
    let s = js_text_decoder_encoding(this);
    f64::from_bits(crate::value::JSValue::string_ptr(s).bits())
}

pub(crate) extern "C" fn text_decoder_fatal_getter(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let this = this.as_f64();
    require_text_brand(
        this,
        TEXT_DECODER_CLASS_ID,
        b"TextDecoder.prototype.fatal called on an incompatible receiver",
    );
    js_text_decoder_fatal(this)
}

pub(crate) extern "C" fn text_decoder_ignore_bom_getter(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let this = this.as_f64();
    require_text_brand(
        this,
        TEXT_DECODER_CLASS_ID,
        b"TextDecoder.prototype.ignoreBOM called on an incompatible receiver",
    );
    js_text_decoder_ignore_bom(this)
}

pub(crate) extern "C" fn text_encoder_encoding_getter(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let this = this.as_f64();
    require_text_brand(
        this,
        TEXT_ENCODER_CLASS_ID,
        b"TextEncoder.prototype.encoding called on an incompatible receiver",
    );
    let s = js_string_from_bytes(b"utf-8".as_ptr(), 5);
    f64::from_bits(crate::value::JSValue::string_ptr(s).bits())
}

pub(crate) extern "C" fn text_encoder_encode_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    input: f64,
) -> f64 {
    let this = this.as_f64();
    require_text_brand(
        this,
        TEXT_ENCODER_CLASS_ID,
        b"TextEncoder.prototype.encode called on an incompatible receiver",
    );
    crate::value::js_nanbox_pointer(js_text_encoder_encode_llvm(input))
}

pub(crate) extern "C" fn text_encoder_encode_into_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    source: f64,
    dest: f64,
) -> f64 {
    let this = this.as_f64();
    require_text_brand(
        this,
        TEXT_ENCODER_CLASS_ID,
        b"TextEncoder.prototype.encodeInto called on an incompatible receiver",
    );
    crate::value::js_nanbox_pointer(js_text_encoder_encode_into_llvm(source, dest))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #10821 / #340 / #341 -- instances are ORDINARY objects.
    ///
    /// The representation invariant: a text receiver is a `GC_TYPE_OBJECT`
    /// with the family class id, above the small-handle band, carrying ZERO
    /// own keys. That last part is what keeps `Object.keys` / `JSON.stringify`
    /// matching node without a per-kind arm anywhere.
    #[test]
    fn text_instances_are_ordinary_objects_with_no_own_keys() {
        let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
        let cases = [
            (js_text_encoder_new(), TEXT_ENCODER_CLASS_ID),
            (
                js_text_decoder_new(undef, undef, undef),
                TEXT_DECODER_CLASS_ID,
            ),
        ];
        for (raw, class_id) in cases {
            let addr = raw as usize;
            assert!(
                !crate::value::addr_class::is_handle_band(addr),
                "an instance must be a real heap object, got {addr:#x}"
            );
            let header = unsafe { crate::value::addr_class::try_read_gc_header(addr) }
                .expect("instances carry a GcHeader");
            assert_eq!(header.obj_type, crate::gc::GC_TYPE_OBJECT);
            let obj = addr as *mut ObjectHeader;
            assert_eq!(unsafe { (*obj).class_id }, class_id);
            let keys_view = unsafe { crate::object::object_keys(obj) };
            let keys = keys_view.arr();
            let key_count = if keys.is_null() { 0 } else { keys_view.count() };
            assert_eq!(key_count, 0, "an instance must have no own keys");
        }
    }

    /// The COMPILED read path, which is not the one the other tests take.
    /// An emitted `td.decode` value read is a per-site inline cache whose miss
    /// edge calls `js_object_get_field_slow` with the receiver's 48-bit
    /// payload — not `js_object_get_field_by_name`. The method has to resolve
    /// on the family prototype through THAT entry too, and nothing else in this
    /// file would notice if it did not: measured, a program whose reads took
    /// this edge answered `undefined` while every by-name test passed.
    #[test]
    fn a_method_value_read_resolves_through_the_ic_miss_entry() {
        let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
        for (raw, name) in [
            (js_text_decoder_new(undef, undef, undef), "decode"),
            (js_text_encoder_new(), "encode"),
            (js_text_encoder_new(), "encodeInto"),
        ] {
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            let value = crate::object::js_object_get_field_slow(
                (raw as u64 & crate::value::POINTER_MASK) as i64,
                key,
            );
            let jsv = crate::value::JSValue::from_bits(value.to_bits());
            assert!(
                jsv.is_pointer(),
                "{name}: the IC miss edge must answer the prototype method, got {:#018x}",
                value.to_bits()
            );
            let ptr = (value.to_bits() & crate::value::POINTER_MASK) as usize;
            assert!(
                crate::closure::is_closure_ptr(ptr),
                "{name}: the answer must be callable"
            );
        }
    }

    /// Two constructions are two objects. Every `TextEncoder` used to be the
    /// same sentinel id, which made `e1 === e2` true and collapsed two
    /// encoders into one `Map` key (#10821).
    #[test]
    fn text_instances_are_distinct_objects() {
        let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
        assert_ne!(js_text_encoder_new(), js_text_encoder_new());
        assert_ne!(
            js_text_decoder_new(undef, undef, undef),
            js_text_decoder_new(undef, undef, undef)
        );
    }

    /// The whole per-instance state round-trips through the meta word, so
    /// there is no registry entry behind a decoder and nothing to release.
    #[test]
    fn decoder_state_round_trips_through_the_meta_word() {
        let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
        let truthy = f64::from_bits(crate::value::TAG_TRUE);
        let label = crate::string::js_string_from_bytes(b"latin1".as_ptr(), 6);
        let label_value = f64::from_bits(crate::value::JSValue::string_ptr(label).bits());
        let raw = js_text_decoder_new(label_value, undef, truthy);
        let boxed = crate::value::js_nanbox_pointer(raw);

        let state = decoder_state_of(boxed);
        assert_eq!(state.label, "windows-1252");
        assert!(!state.fatal);
        assert!(state.ignore_bom);

        // …and through the public natives the prototype accessors call.
        let encoding = js_text_decoder_encoding(boxed);
        let got = unsafe {
            let len = (*encoding).byte_len as usize;
            let data = (encoding as *const u8).add(std::mem::size_of::<StringHeader>());
            std::str::from_utf8(std::slice::from_raw_parts(data, len))
                .expect("ASCII")
                .to_string()
        };
        assert_eq!(got, "windows-1252");
        assert_eq!(
            js_text_decoder_ignore_bom(boxed).to_bits(),
            crate::value::TAG_TRUE
        );
        assert_eq!(
            js_text_decoder_fatal(boxed).to_bits(),
            crate::value::TAG_FALSE
        );
    }

    /// The encoding intern is bounded by the compile-time label set: naming the
    /// same encoding again must NOT grow it. This is the property that lets
    /// `DECODER_REGISTRY` go away -- that table grew one entry per decoder and
    /// was never pruned, so a long-running program leaked one entry per
    /// `new TextDecoder()`.
    #[test]
    fn the_encoding_intern_does_not_grow_per_decoder() {
        let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
        let decoder = |label: &str| {
            let s = crate::string::js_string_from_bytes(label.as_ptr(), label.len() as u32);
            let value = f64::from_bits(crate::value::JSValue::string_ptr(s).bits());
            js_text_decoder_new(value, undef, undef)
        };
        // Warm every label this test names, so the measurement below is only
        // about REPEATS.
        let _ = js_text_decoder_new(undef, undef, undef);
        for label in ["latin1", "utf-16le", "ibm866"] {
            let _ = decoder(label);
        }
        let before = ENCODINGS.lock().unwrap().len();
        for _ in 0..64 {
            let _ = js_text_decoder_new(undef, undef, undef);
            for label in ["latin1", "utf-16le", "ibm866", "windows-1252"] {
                let _ = decoder(label);
            }
        }
        assert_eq!(
            ENCODINGS.lock().unwrap().len(),
            before,
            "interning is per ENCODING, not per decoder"
        );
        // …and the counter-direction: a label the program has not named yet
        // MUST add exactly one row, or the assertion above would pass on an
        // implementation that simply never grows.
        assert!(
            resolve_decoder_label("koi8-r").is_some(),
            "koi8-r has to be a supported label for the rest of this to mean anything"
        );
        let fresh = decoder("koi8-r");
        assert_ne!(fresh, 0);
        assert_eq!(
            ENCODINGS.lock().unwrap().len(),
            before + 1,
            "a newly named encoding interns exactly once"
        );
        let _ = decoder("koi8-r");
        assert_eq!(
            ENCODINGS.lock().unwrap().len(),
            before + 1,
            "…and naming it again does not"
        );
    }

    #[test]
    fn utf_16be_labels_decode_big_endian_and_consume_their_bom() {
        for label in ["utf-16be", "unicodefffe"] {
            let decoded = decode_bytes_with_label(&[0xFE, 0xFF, 0x00, 0x41, 0x20, 0xAC], label)
                .unwrap()
                .unwrap_or_else(|_| unreachable!("six bytes are never too long"));
            assert_eq!(
                text_string_header_to_string(decoded as *const StringHeader),
                "A€",
                "{label}"
            );
        }
        assert!(resolve_decoder_label("utf16be").is_none());
    }

    #[test]
    fn a_fatal_decode_error_releases_the_input_pin() {
        let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
        let truthy = f64::from_bits(crate::value::TAG_TRUE);
        let label = crate::string::js_string_from_bytes(b"utf-16be".as_ptr(), 8);
        let label = f64::from_bits(crate::value::JSValue::string_ptr(label).bits());
        let decoder = crate::value::js_nanbox_pointer(js_text_decoder_new(label, truthy, undef));
        // One byte is half a code unit.
        let ab = crate::buffer::js_array_buffer_new(1) as usize;
        let input = crate::value::js_nanbox_pointer(ab as i64);
        let pin = crate::buffer::bytes::pin(input).unwrap();
        assert!(crate::buffer::bytes::has_pins(ab));
        drop(pin);

        let outcome = crate::exception::catch_thrown(|| {
            js_text_decoder_decode_llvm(decoder, input);
            undef
        });
        assert!(outcome.is_err());
        assert!(!crate::buffer::bytes::has_pins(ab));
    }

    /// A foreign receiver is refused rather than silently decoded as utf-8 --
    /// the brand check the prototype thunks make, matching node's
    /// `TextDecoder.prototype.decode.call({})`.
    #[test]
    fn text_native_state_rejects_a_foreign_receiver() {
        let plain = crate::object::js_object_alloc(0, 0);
        let boxed = crate::value::js_nanbox_pointer(plain as i64);
        assert!(text_native_state(boxed, TEXT_DECODER_CLASS_ID).is_none());
        assert!(text_native_state(boxed, TEXT_ENCODER_CLASS_ID).is_none());
        // …and an encoder is not a decoder.
        let enc = crate::value::js_nanbox_pointer(js_text_encoder_new());
        assert!(text_native_state(enc, TEXT_DECODER_CLASS_ID).is_none());
        assert!(text_native_state(enc, TEXT_ENCODER_CLASS_ID).is_some());
    }

    /// `TextDecoder.decode(dataView)` must read the backing store, not the
    /// DataView's construction-time snapshot. A `Uint32Array` over the same
    /// `ArrayBuffer` writes straight into the backing, so the snapshot decoded
    /// as the pre-write bytes (four NULs for a freshly allocated buffer) while
    /// decoding the `ArrayBuffer` itself produced the right text — the same
    /// stale-view class as the numeric accessors, and the reason every
    /// native-span consumer resolves through `buffer::resolve_span_data_ptr`
    /// (#6515).
    #[test]
    fn text_decoder_reads_backing_store_of_a_data_view() {
        let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
        let ab = crate::buffer::js_array_buffer_new(4);
        let ab_value = f64::from_bits(crate::value::JSValue::pointer(ab as *const u8).bits());
        let words = crate::typedarray_hook_slots::js_typed_array_view(
            crate::typedarray_hook_slots::KIND_UINT32 as i32,
            ab_value,
            undef,
            undef,
        );
        assert!(!words.is_null());
        let dv = crate::buffer::js_data_view_new(ab_value, undef, undef);

        // "ABCD" on a little-endian host, "DCBA" on a big-endian one — the
        // assertion compares against the backing rather than a fixed literal.
        crate::typedarray_hook_slots::js_typed_array_set(words, 0, 0x4443_4241u32 as f64);
        let expected = crate::buffer::bytes::no_gc(|scope| {
            std::str::from_utf8(
                &crate::buffer::bytes::bytes(crate::value::js_nanbox_pointer(ab as i64), scope)
                    .unwrap()[..4],
            )
            .expect("ASCII bytes")
            .to_string()
        });
        assert_ne!(
            expected, "\0\0\0\0",
            "the typed-array store must have reached the ArrayBuffer"
        );

        // An unregistered handle decodes as non-fatal utf-8 — the default this
        // test wants, and no decoder registry setup.
        let decoded = js_text_decoder_decode_llvm(0.0, dv);
        let s = decoded as *const StringHeader;
        assert!(!s.is_null());
        let got = unsafe {
            let len = (*s).byte_len as usize;
            let data = (s as *const u8).add(std::mem::size_of::<StringHeader>());
            std::str::from_utf8(std::slice::from_raw_parts(data, len))
                .expect("ASCII bytes")
                .to_string()
        };
        assert_eq!(got, expected, "decode(DataView) must see the backing bytes");
    }

    /// `decode` consumes one leading BOM for every Unicode encoding unless the
    /// decoder was built with `ignoreBOM` (#15462); a single-byte encoding
    /// keeps the bytes.
    #[test]
    fn decode_consumes_a_leading_bom_unless_ignore_bom() {
        let _serialized = crate::array::test_serialize();
        let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
        let truthy = f64::from_bits(crate::value::TAG_TRUE);
        let decode = |label: &str, ignore_bom: f64, bytes: &[u8]| {
            let label_ptr = crate::string::js_string_from_bytes(label.as_ptr(), label.len() as u32);
            let label = f64::from_bits(crate::value::JSValue::string_ptr(label_ptr).bits());
            let decoder =
                crate::value::js_nanbox_pointer(js_text_decoder_new(label, undef, ignore_bom));
            let values: Vec<f64> = bytes.iter().map(|&b| b as f64).collect();
            let arr = crate::array::js_array_from_f64(values.as_ptr(), values.len() as u32);
            let buf = crate::buffer::js_uint8array_new(crate::value::js_nanbox_pointer(arr as i64));
            let input = f64::from_bits(crate::value::JSValue::pointer(buf as *const u8).bits());
            let s = js_text_decoder_decode_llvm(decoder, input) as *const StringHeader;
            crate::string::string_as_str(s).to_string()
        };
        for (label, bom, body) in [
            ("utf-8", &[0xEF, 0xBB, 0xBF][..], &[0x48][..]),
            ("utf-16le", &[0xFF, 0xFE], &[0x48, 0x00]),
            ("utf-16be", &[0xFE, 0xFF], &[0x00, 0x48]),
        ] {
            let bytes = [bom, body].concat();
            assert_eq!(decode(label, undef, &bytes), "H", "{label}");
            assert_eq!(
                decode(label, truthy, &bytes),
                "\u{FEFF}H",
                "{label} ignoreBOM"
            );
        }
        assert_eq!(
            decode("windows-1252", undef, &[0xEF, 0xBB, 0xBF]),
            "\u{EF}\u{BB}\u{BF}"
        );
    }

    /// What a `stream: true` decode keeps for the next call: a valid prefix of
    /// a UTF-8 sequence, or a UTF-16 odd byte and/or unpaired high surrogate.
    #[test]
    fn incomplete_tail_len_keeps_only_a_completable_prefix() {
        let utf8 = DecoderEncoding::Utf8;
        assert_eq!(incomplete_tail_len(&[0x41, 0xE2], utf8), 1);
        assert_eq!(incomplete_tail_len(&[0xE2, 0x82], utf8), 2);
        assert_eq!(incomplete_tail_len(&[0xE2, 0x82, 0xAC], utf8), 0);
        assert_eq!(incomplete_tail_len(&[0xF0, 0x9F, 0x98], utf8), 3);
        // E0 80 can never complete: it decodes now.
        assert_eq!(incomplete_tail_len(&[0xE0, 0x80], utf8), 0);
        assert_eq!(incomplete_tail_len(&[0xFF], utf8), 0);
        let le = DecoderEncoding::Utf16Le;
        assert_eq!(incomplete_tail_len(&[0x41, 0x00, 0x3D], le), 1);
        assert_eq!(incomplete_tail_len(&[0x3D, 0xD8], le), 2);
        assert_eq!(incomplete_tail_len(&[0x3D, 0xD8, 0x00], le), 3);
        assert_eq!(incomplete_tail_len(&[0x3D, 0xD8, 0x00, 0xDE], le), 0);
        assert_eq!(
            incomplete_tail_len(&[0xD8, 0x3D], DecoderEncoding::Utf16Be),
            2
        );
    }

    /// The pending bytes and the BOM flag ride in the state word next to the
    /// encoding, and a stream's end clears exactly those bits (#15476).
    #[test]
    fn stream_state_round_trips_through_the_word() {
        let base = STATE_PRESENT | STATE_FATAL | (5 << STATE_ENCODING_SHIFT);
        let word = with_stream_state(base, true, &[0xE2, 0x82]);
        assert_eq!(stream_pending(word), ([0xE2, 0x82, 0], 2));
        assert_ne!(word & STATE_STREAM_STARTED, 0);
        assert_eq!((word >> STATE_ENCODING_SHIFT) & STATE_ENCODING_MASK, 5);
        assert_eq!(with_stream_state(word, false, &[]), base);
    }
}
