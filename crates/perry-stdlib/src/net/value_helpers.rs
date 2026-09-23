//! NaN-boxed JS value readers shared by the `net`/`tls` FFI surface.
//!
//! Split out of `net/mod.rs` (2000-line file cap). Pure move — the
//! helpers keep their names, signatures and behaviour; only their
//! visibility widened to `pub(super)` so `net` and its sibling
//! submodules can still reach them.

use perry_runtime::buffer::BufferHeader;
use perry_runtime::{JSValue, StringHeader};

pub(super) unsafe fn string_from_header_i64(ptr: i64) -> Option<String> {
    crate::common::string_from_header(ptr as *const StringHeader)
}

/// Issue #770 — true iff `val_f64` carries `POINTER_TAG` (0x7FFD), i.e.
/// it's a real heap-pointer NaN-box (object or closure). Plain `f64`
/// ports like `80.0` never reach this band, and `undefined` / `null`
/// land in `0x7FFC` so they're cleanly rejected — which matters
/// because the dispatch table pads missing user args with
/// `TAG_UNDEFINED`.
pub(super) fn is_nanboxed_pointer(val_f64: f64) -> bool {
    (val_f64.to_bits() >> 48) == 0x7FFD
}

pub(super) unsafe fn unbox_pointer(val_f64: f64) -> *mut u8 {
    let bits = val_f64.to_bits();
    (bits & 0x0000_FFFF_FFFF_FFFF) as *mut u8
}

/// Issue #1131 — read a NaN-boxed JS value as the raw bytes for
/// `socket.write(chunk)`. Mirror of perry-ext-net's
/// `jsvalue_to_socket_bytes` (the live path for `node:net` imports is
/// the perry-ext-net copy after the well-known flip; this bundled-net
/// copy stays in sync so the HANDLE_METHOD_DISPATCH fallback through
/// `dispatch_net_socket` is correct too). A JS string is a 20-byte
/// `StringHeader`; a Buffer is an 8-byte `BufferHeader` — reading one
/// through the other's layout (the pre-#1131 unconditional
/// `*BufferHeader` cast) emits garbage. Probe `BUFFER_REGISTRY` first.
pub(super) unsafe fn jsvalue_to_socket_bytes(value: f64) -> Option<Vec<u8>> {
    let v = JSValue::from_bits(value.to_bits());
    if v.is_undefined() || v.is_null() {
        return None;
    }
    if v.is_string() {
        let ptr = unbox_pointer(value) as *const StringHeader;
        if ptr.is_null() {
            return None;
        }
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        return Some(std::slice::from_raw_parts(data, len).to_vec());
    }
    if v.is_pointer() {
        let raw = (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
        if perry_runtime::buffer::js_buffer_is_buffer(raw) != 0 {
            let buf = raw as *const BufferHeader;
            if !buf.is_null() {
                let len = (*buf).length as usize;
                let data = perry_runtime::buffer::buffer_data(
                    buf as *const perry_runtime::buffer::BufferHeader,
                );
                return Some(std::slice::from_raw_parts(data, len).to_vec());
            }
        }
        let sptr = raw as *const StringHeader;
        if !sptr.is_null() {
            let len = (*sptr).byte_len as usize;
            if len <= (1 << 30) {
                let data = (sptr as *const u8).add(std::mem::size_of::<StringHeader>());
                return Some(std::slice::from_raw_parts(data, len).to_vec());
            }
        }
        return None;
    }
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

pub(super) unsafe fn get_object_string_field(obj_f64: f64, field_name: &str) -> Option<String> {
    if !is_nanboxed_pointer(obj_f64) {
        return None;
    }
    let obj_ptr = unbox_pointer(obj_f64) as *const perry_runtime::ObjectHeader;
    if obj_ptr.is_null() {
        return None;
    }
    let key = perry_runtime::js_string_from_bytes(field_name.as_ptr(), field_name.len() as u32);
    let val = perry_runtime::js_object_get_field_by_name(obj_ptr, key);
    if val.is_undefined() || val.is_null() {
        return None;
    }
    if val.is_string() {
        return string_from_header_i64(val.as_string_ptr() as i64);
    }
    if val.is_number() {
        return Some(format!("{}", val.as_number() as i64));
    }
    None
}

pub(super) unsafe fn get_object_value_field(obj_f64: f64, field_name: &str) -> Option<f64> {
    if !is_nanboxed_pointer(obj_f64) {
        return None;
    }
    let obj_ptr = unbox_pointer(obj_f64) as *const perry_runtime::ObjectHeader;
    if !perry_runtime::value::addr_class::is_above_handle_band(obj_ptr as usize) {
        return None;
    }
    let key = perry_runtime::js_string_from_bytes(field_name.as_ptr(), field_name.len() as u32);
    Some(f64::from_bits(
        perry_runtime::js_object_get_field_by_name(obj_ptr, key).bits(),
    ))
}

pub(super) unsafe fn get_object_number_field(obj_f64: f64, field_name: &str) -> Option<f64> {
    if !is_nanboxed_pointer(obj_f64) {
        return None;
    }
    let obj_ptr = unbox_pointer(obj_f64) as *const perry_runtime::ObjectHeader;
    if obj_ptr.is_null() {
        return None;
    }
    let key = perry_runtime::js_string_from_bytes(field_name.as_ptr(), field_name.len() as u32);
    let val = perry_runtime::js_object_get_field_by_name(obj_ptr, key);
    if val.is_undefined() || val.is_null() {
        return None;
    }
    if val.is_number() {
        return Some(val.as_number());
    }
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
pub(super) unsafe fn get_object_bool_field(obj_f64: f64, field_name: &str) -> Option<bool> {
    if !is_nanboxed_pointer(obj_f64) {
        return None;
    }
    let obj_ptr = unbox_pointer(obj_f64) as *const perry_runtime::ObjectHeader;
    if obj_ptr.is_null() {
        return None;
    }
    let key = perry_runtime::js_string_from_bytes(field_name.as_ptr(), field_name.len() as u32);
    let val = perry_runtime::js_object_get_field_by_name(obj_ptr, key);
    if val.is_undefined() || val.is_null() {
        return None;
    }
    if val.is_bool() {
        return Some(val.to_bool());
    }
    if val.is_number() {
        return Some(val.as_number() != 0.0);
    }
    None
}

/// Issue #770 — build an `Error`-shaped object `{ message: msg }` so
/// `socket.on('error', err => err.message)` works. Returns a NaN-boxed
/// f64 pointing at the object, falling back to a bare string on alloc
/// failure. Packed-keys format (NUL-delimited names + hash shape id)
/// mirrors `crates/perry-stdlib/src/sqlite.rs::build_packed_keys`.
pub(super) unsafe fn build_error_object(msg: &str) -> f64 {
    use perry_runtime::JSValue;
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let keys = ["message", "code", "name"];
    let mut packed = Vec::new();
    for key in keys {
        packed.extend_from_slice(key.as_bytes());
        packed.push(0);
    }
    let mut shape_id: u32 = 0x4E45_0000; // "NE" — net error
    for &b in &packed {
        shape_id = shape_id.wrapping_mul(31).wrapping_add(b as u32);
    }
    shape_id = shape_id.wrapping_add(3);
    let s_msg = scope.root_string_ptr(perry_runtime::js_string_from_bytes(
        msg.as_ptr(),
        msg.len() as u32,
    ));
    let obj_ptr = perry_runtime::js_object_alloc_with_shape(
        shape_id,
        3,
        packed.as_ptr(),
        packed.len() as u32,
    );
    if obj_ptr.is_null() {
        return s_msg.with_const_ptr(|s_msg: *const perry_runtime::StringHeader| {
            f64::from_bits(0x7FFF_0000_0000_0000u64 | (s_msg as u64 & 0x0000_FFFF_FFFF_FFFF))
        });
    }
    let obj = scope.root_raw_mut_ptr(obj_ptr);
    obj.with_mut_ptr(|obj| {
        s_msg.with_mut_ptr(|s_msg| {
            perry_runtime::js_object_set_field(obj, 0, JSValue::string_ptr(s_msg))
        })
    });
    let code = if msg.starts_with("ERR_") {
        Some(msg)
    } else if msg.contains("UnknownIssuer")
        || msg.contains("unknown issuer")
        || msg.contains("invalid peer certificate")
    {
        Some("DEPTH_ZERO_SELF_SIGNED_CERT")
    } else if msg.to_ascii_lowercase().contains("connection refused") {
        Some("ECONNREFUSED")
    } else {
        None
    };
    if let Some(code) = code {
        let code = scope.root_string_ptr(perry_runtime::js_string_from_bytes(
            code.as_ptr(),
            code.len() as u32,
        ));
        obj.with_mut_ptr(|obj| {
            code.with_mut_ptr(|code| {
                perry_runtime::js_object_set_field(obj, 1, JSValue::string_ptr(code))
            })
        });
    }
    let name = scope.root_string_ptr(perry_runtime::js_string_from_bytes(b"Error".as_ptr(), 5));
    obj.with_mut_ptr(|obj| {
        name.with_mut_ptr(|name| {
            perry_runtime::js_object_set_field(obj, 2, JSValue::string_ptr(name))
        })
    });
    obj.with_mut_ptr(|obj: *mut perry_runtime::ObjectHeader| {
        f64::from_bits((obj as u64 & 0x0000_FFFF_FFFF_FFFF) | 0x7FFD_0000_0000_0000)
    })
}
