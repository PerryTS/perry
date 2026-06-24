//! NaN-boxed JS value ⇄ Rust conversion helpers.
//!
//! Split out of `lib.rs` to keep that file under the 2000-line size gate.
//! These are the pure value-extraction utilities the FFI entry points use to
//! read host/port/option fields off JS option objects, turn a `socket.write`
//! argument into wire bytes, and build the `Error`-shaped object the
//! `'error'` listener receives. Every symbol stays `pub(crate)` and is
//! re-exported from the crate root (`pub(crate) use jsvalue::*;`), so the
//! existing `crate::<fn>` call sites in `lib.rs` and the sibling modules
//! (`tls`, `classes`, `ip`, `lifecycle`, `option_setters`) are unchanged.

use perry_ffi::{
    alloc_string, build_object_shape, js_object_alloc_with_shape, js_object_set_field,
    nanbox_string_bits, BufferHeader, JsValue, ObjectHeader, StringHeader,
};

pub(crate) unsafe fn string_from_header_i64(ptr: i64) -> Option<String> {
    let p = ptr as usize;
    // Small-handle cutoff: a POINTER-tagged payload below 0x100000 is a
    // registry handle (fetch/zlib/proxy/...), not a heap StringHeader. Reject
    // it before the dereference. See the project guideline on `value < 0x100000`
    // handle detection.
    if p < 0x100000 {
        return None;
    }
    let hdr = ptr as *const StringHeader;
    let len = (*hdr).byte_len as usize;
    let data_ptr = (hdr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

// Runtime entrypoints provided by perry-runtime (declared as extern so
// perry-ext-net doesn't need to depend on the perry-runtime rlib).
extern "C" {
    fn js_string_from_bytes(data: *const u8, len: u32) -> *mut StringHeader;
    fn js_object_get_field_by_name_f64(obj: *const ObjectHeader, key: *const StringHeader) -> f64;
    /// Issue #1131 — returns 1 if `ptr` is a registered Buffer /
    /// Uint8Array in the runtime's `BUFFER_REGISTRY`. This is the only
    /// safe way to tell a `BufferHeader` apart from a `StringHeader`
    /// after both have been NaN-boxed and stripped to a raw pointer
    /// (a `Buffer` carries `POINTER_TAG`, a JS string `STRING_TAG`, but
    /// the dispatch shims pass us the full NaN-box bits and we still
    /// have to distinguish a pointer-tagged Buffer from a
    /// pointer-tagged non-buffer object). Defined in
    /// `crates/perry-runtime/src/buffer.rs::js_buffer_is_buffer`.
    fn js_buffer_is_buffer(ptr: i64) -> i32;
}

/// Issue #1131 — read a NaN-boxed JS value as the raw bytes to put on
/// the wire for `socket.write(chunk)`. Outbound mirror of
/// `perry-ext-http-server`'s `jsvalue_to_body_bytes` (#1124): a JS
/// string and a `Buffer` have *different* memory layouts
/// (`StringHeader` is 20 bytes, `{ utf16_len, byte_len, capacity,
/// refcount, flags }`; `BufferHeader` is 8 bytes, `{ length, capacity
/// }`, data immediately after). The pre-#1131 code unconditionally
/// reinterpreted the chunk pointer as a `*const BufferHeader`, so
/// `socket.write("ping")` read the string's `utf16_len` as the buffer
/// length and pulled "data" from `ptr + 8` — the middle of the
/// `StringHeader` struct — emitting garbage instead of the UTF-8
/// bytes.
///
/// Probe the runtime's `BUFFER_REGISTRY` first (`js_buffer_is_buffer`)
/// to pick the `BufferHeader` layout for real Buffers / Uint8Arrays;
/// otherwise read through the `StringHeader` layout for JS strings;
/// otherwise stringify numbers / bools the same way `res.write(n)`
/// does (Node throws `ERR_INVALID_ARG_TYPE` here, but Perry's existing
/// body-write paths are lenient and stringify — keep parity with
/// that). `null` / `undefined` produce `None` (no bytes written).
pub(crate) unsafe fn jsvalue_to_socket_bytes(value: f64) -> Option<Vec<u8>> {
    let v = JsValue::from_bits(value.to_bits());
    if v.is_undefined() || v.is_null() {
        return None;
    }
    // JS string — STRING_TAG, `StringHeader` layout.
    if v.is_string() {
        let ptr = unbox_pointer(value) as *const StringHeader;
        if ptr.is_null() {
            return None;
        }
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        return Some(std::slice::from_raw_parts(data, len).to_vec());
    }
    // Heap pointer — could be a Buffer / Uint8Array (BufferHeader
    // layout) or some other object. Only the buffer registry can positively
    // identify it; anything else must NOT be reinterpreted as bytes.
    if v.is_pointer() {
        let raw = (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
        // Small-handle cutoff: a sub-0x100000 payload is a registry handle, not
        // a heap pointer — never dereference it. (`value < 0x100000` guideline.)
        if (raw as u64) < 0x100000 {
            return None;
        }
        if js_buffer_is_buffer(raw) != 0 {
            let buf = raw as *const BufferHeader;
            if !buf.is_null() {
                let len = (*buf).length as usize;
                let data = (buf as *const u8).add(std::mem::size_of::<BufferHeader>());
                return Some(std::slice::from_raw_parts(data, len).to_vec());
            }
        }
        // Non-buffer pointer: do NOT reinterpret it as a `StringHeader`. A real
        // JS string carries STRING_TAG and is handled by the `v.is_string()`
        // branch above (the runtime never tags a string with POINTER_TAG —
        // `js_nanbox_is_string` keys solely off STRING_TAG). A POINTER_TAG value
        // here is a heap object/closure using the `ObjectHeader` layout, so
        // reading it through `StringHeader` would put object metadata and
        // adjacent heap bytes on the wire (e.g. `socket.write({})`). Reject it.
        return None;
    }
    // Number / bool — stringify (parity with the lenient
    // `res.write(value)` body path; Node would throw here).
    if v.is_number() {
        return Some(v.to_number().to_string().into_bytes());
    }
    if v.is_bool() {
        return Some(
            if v.to_bool() { "true" } else { "false" }
                .to_string()
                .into_bytes(),
        );
    }
    None
}

/// True iff `val_f64` carries `POINTER_TAG` (0x7FFD) — a real pointer
/// to a heap object or closure. Used to discriminate the
/// positional `net.connect(port, host)` overload (arg1 is a plain
/// number) from the options-object `net.connect({host, port}, cb?)`
/// overload (arg1 is a NaN-boxed object pointer), and to detect a
/// real `connectListener` closure in the trailing arg slot.
///
/// Narrower than "any NaN-tagged value": the dispatch table pads
/// missing user args with `TAG_UNDEFINED` (`0x7FFC` band), so this
/// check has to reject `undefined` cleanly to keep "user passed only
/// 2 args" from misfiring as "user passed a callback". Issue #770.
pub(crate) fn is_nanboxed_pointer(val_f64: f64) -> bool {
    (val_f64.to_bits() >> 48) == 0x7FFD
}

/// Unbox a NaN-boxed value to the raw 48-bit pointer payload, regardless
/// of which `0x7FFx` tag it carries.
pub(crate) unsafe fn unbox_pointer(val_f64: f64) -> *mut u8 {
    let bits = val_f64.to_bits();
    (bits & 0x0000_FFFF_FFFF_FFFF) as *mut u8
}

/// Extract a string field from a NaN-boxed JS object. Accepts string
/// values and numeric values (numbers stringified) — Node accepts both
/// shapes for `port` etc.
pub(crate) unsafe fn get_object_string_field(obj_f64: f64, field_name: &str) -> Option<String> {
    if !is_nanboxed_pointer(obj_f64) {
        return None;
    }
    let obj_ptr = unbox_pointer(obj_f64) as *const ObjectHeader;
    // Small-handle cutoff before dereferencing: `is_nanboxed_pointer` only
    // checks the POINTER_TAG, so a sub-0x100000 payload (a registry handle, not
    // a heap object) would otherwise be read as an ObjectHeader. `< 0x100000`
    // also subsumes the null check. See the `value < 0x100000` handle guideline.
    if (obj_ptr as usize) < 0x100000 {
        return None;
    }
    let key = js_string_from_bytes(field_name.as_ptr(), field_name.len() as u32);
    let val_f64 = js_object_get_field_by_name_f64(obj_ptr, key);
    let val = JsValue::from_bits(val_f64.to_bits());
    if val.is_undefined() || val.is_null() {
        return None;
    }
    if val.is_string() {
        return string_from_header_i64(val.as_string_ptr() as i64);
    }
    if val.is_number() {
        return Some(format!("{}", val.to_number() as i64));
    }
    None
}

pub(crate) unsafe fn get_object_number_field(obj_f64: f64, field_name: &str) -> Option<f64> {
    if !is_nanboxed_pointer(obj_f64) {
        return None;
    }
    let obj_ptr = unbox_pointer(obj_f64) as *const ObjectHeader;
    // Small-handle cutoff before dereferencing: `is_nanboxed_pointer` only
    // checks the POINTER_TAG, so a sub-0x100000 payload (a registry handle, not
    // a heap object) would otherwise be read as an ObjectHeader. `< 0x100000`
    // also subsumes the null check. See the `value < 0x100000` handle guideline.
    if (obj_ptr as usize) < 0x100000 {
        return None;
    }
    let key = js_string_from_bytes(field_name.as_ptr(), field_name.len() as u32);
    let val_f64 = js_object_get_field_by_name_f64(obj_ptr, key);
    let val = JsValue::from_bits(val_f64.to_bits());
    if val.is_undefined() || val.is_null() {
        return None;
    }
    if val.is_number() {
        return Some(val.to_number());
    }
    // Some npm code passes `port` as a string — accept that too.
    if val.is_string() {
        if let Some(s) = string_from_header_i64(val.as_string_ptr() as i64) {
            if let Ok(n) = s.parse::<f64>() {
                return Some(n);
            }
        }
    }
    None
}

/// Read a boolean option off a NaN-boxed JS object. Accepts real
/// booleans plus numbers (`rejectUnauthorized: 0` shows up in npm
/// code). `None` when the field is absent/undefined/null. #4971.
pub(crate) unsafe fn get_object_bool_field(obj_f64: f64, field_name: &str) -> Option<bool> {
    if !is_nanboxed_pointer(obj_f64) {
        return None;
    }
    let obj_ptr = unbox_pointer(obj_f64) as *const ObjectHeader;
    // Small-handle cutoff before dereferencing: `is_nanboxed_pointer` only
    // checks the POINTER_TAG, so a sub-0x100000 payload (a registry handle, not
    // a heap object) would otherwise be read as an ObjectHeader. `< 0x100000`
    // also subsumes the null check. See the `value < 0x100000` handle guideline.
    if (obj_ptr as usize) < 0x100000 {
        return None;
    }
    let key = js_string_from_bytes(field_name.as_ptr(), field_name.len() as u32);
    let val = JsValue::from_bits(js_object_get_field_by_name_f64(obj_ptr, key).to_bits());
    if val.is_undefined() || val.is_null() {
        return None;
    }
    if val.is_bool() {
        return Some(val.to_bool());
    }
    if val.is_number() {
        return Some(val.to_number() != 0.0);
    }
    None
}

/// Build an `Error`-shaped object `{ message: msg }` so user code can
/// read `err.message` from the `'error'` listener — Node emits Error
/// instances, not raw strings. Returns a NaN-boxed `f64` pointing at
/// the object. Issue #770.
pub(crate) unsafe fn build_error_object(msg: &str) -> f64 {
    let keys: [&str; 1] = ["message"];
    let (packed, shape_id) = build_object_shape(&keys);
    let obj: *mut ObjectHeader =
        js_object_alloc_with_shape(shape_id, 1, packed.as_ptr(), packed.len() as u32);
    if obj.is_null() {
        // Fall back to the bare string so the listener still receives
        // *something* if the object alloc failed.
        let s = alloc_string(msg);
        return f64::from_bits(nanbox_string_bits(s.as_raw()));
    }
    let s = alloc_string(msg);
    let v = JsValue::from_string_ptr(s.as_raw());
    js_object_set_field(obj, 0, v);
    let obj_v = JsValue::from_object_ptr(obj as *mut u8);
    f64::from_bits(obj_v.bits())
}
