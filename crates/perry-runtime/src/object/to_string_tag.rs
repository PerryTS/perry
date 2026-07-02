//! `Object.prototype.toString` brand detection and `Symbol.toStringTag`
//! resolution (split out of `object/mod.rs`, behavior-preserving).

use super::*;

use crate::arena::arena_alloc_gc;
use crate::ArrayHeader;
use crate::JSValue;
use std::cell::{Cell, RefCell, UnsafeCell};
use std::collections::HashMap;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicU8, Ordering};
use std::sync::RwLock;

pub(crate) fn web_stream_to_string_tag(value: f64) -> Option<&'static str> {
    if !value.is_finite() || value <= 0.0 || value.fract() != 0.0 {
        return None;
    }
    let kind_probe = stream_handle_kind_probe()?;
    match unsafe { kind_probe(value as usize) } {
        1 => Some("ReadableStream"),
        2 => Some("WritableStream"),
        5 => Some("TransformStream"),
        _ => None,
    }
}

pub(crate) fn fetch_handle_to_string_tag(value: f64) -> Option<&'static str> {
    let bits = value.to_bits();
    if (bits >> 48) != 0x7FFD {
        return None;
    }
    let id = (bits & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_handle_band(id) {
        return None;
    }
    let kind_probe = crate::object::fetch_handle_kind_probe()?;
    match unsafe { kind_probe(id) } {
        1 => Some("Response"),
        2 => Some("Request"),
        3 => Some("Headers"),
        4 => Some("Blob"),
        5 => Some("File"),
        _ => None,
    }
}

unsafe fn string_value_to_owned(value: f64) -> Option<String> {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if !jv.is_any_string() {
        return None;
    }
    let s = crate::builtins::js_string_coerce(value);
    if s.is_null() {
        return None;
    }
    let len = (*s).byte_len as usize;
    let data = (s as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
    std::str::from_utf8(std::slice::from_raw_parts(data, len))
        .ok()
        .map(ToOwned::to_owned)
}

unsafe fn object_to_string_tag_property(value: f64) -> Option<String> {
    let bits = value.to_bits();
    if (bits & 0xFFFF_0000_0000_0000) != 0x7FFD_0000_0000_0000 {
        return None;
    }
    let raw_addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    if raw_addr < 0x1000 {
        return None;
    }
    let sym = crate::symbol::well_known_symbol("toStringTag");
    if sym.is_null() {
        return None;
    }
    let sym_f64 = f64::from_bits(0x7FFD_0000_0000_0000 | (sym as u64 & 0x0000_FFFF_FFFF_FFFF));
    // Spec §20.1.3.6 step 6: Get(O, @@toStringTag) — own property first, then
    // the explicit prototype chain (set via Object.setPrototypeOf or the
    // runtime's object_set_static_prototype).
    let tag_value = crate::symbol::own_symbol_property(value, sym_f64)
        .or_else(|| crate::symbol::inherited_symbol_property(value, sym_f64))?;
    string_value_to_owned(tag_value)
}

/// The `%TypedArray%.prototype [ @@toStringTag ]` value for `value` if it is a
/// TypedArray (the constructor name, e.g. `"Int8Array"` / `"Uint8Array"`),
/// else `None`. Covers both the raw-pointer typed-array representation and
/// Perry's buffer-backed `Uint8Array`/`Uint8ClampedArray` (Node's `Buffer` is
/// a `Uint8Array`, so it too reports `"Uint8Array"`). `ArrayBuffer` /
/// `SharedArrayBuffer` / `DataView` / `CryptoKey` are NOT typed arrays and
/// return `None` (their `@@toStringTag` getter yields `undefined`). Shared by
/// `js_object_to_string`'s typed-array brand arm and the public
/// `%TypedArray%.prototype[@@toStringTag]` accessor getter.
pub(crate) fn typed_array_to_string_tag_name(value: f64) -> Option<&'static str> {
    use crate::value::JSValue;
    let bits = value.to_bits();
    let jsv = JSValue::from_bits(bits);
    let raw_addr = if jsv.is_pointer() {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if bits > 0x1000 && (bits >> 48) == 0 {
        bits as usize
    } else {
        return None;
    };
    if raw_addr < 0x1000 {
        return None;
    }
    if let Some(kind) = crate::typedarray::lookup_typed_array_kind(raw_addr) {
        return Some(crate::typedarray::name_for_kind(kind));
    }
    // Buffer-backed `Uint8Array` (and Node `Buffer`) — registered as a buffer
    // but still a TypedArray. Exclude the non-TypedArray buffer flavours.
    if crate::buffer::is_registered_buffer(raw_addr)
        && crate::buffer::crypto_key_meta(raw_addr).is_none()
        && !crate::buffer::is_array_buffer(raw_addr)
        && !crate::buffer::is_shared_array_buffer(raw_addr)
        && !crate::buffer::is_data_view(raw_addr)
    {
        return Some("Uint8Array");
    }
    None
}

/// `Object.prototype.toString.call(x)` — returns `[object <tag>]` where
/// `<tag>` is read from the value's class-level `Symbol.toStringTag` getter
/// if registered, otherwise `Object` (matching Node for plain objects).
#[no_mangle]
pub unsafe extern "C" fn js_object_to_string(value: f64) -> f64 {
    use crate::value::JSValue;
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
    const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
    let bits = value.to_bits();
    let jsv = JSValue::from_bits(bits);
    // Spec-defined primitive tags (ramda's `_isString.js` / `_isObject.js`
    // / `_isRegExp.js` / `_isArguments.js` IIFEs distinguish on these
    // exact strings; returning `[object Object]` everywhere folded all
    // five branches into the catch-all).
    if jsv.is_undefined() {
        let bytes = b"[object Undefined]";
        let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    }
    if jsv.is_null() {
        let bytes = b"[object Null]";
        let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    }
    if jsv.is_bool() {
        let bytes = b"[object Boolean]";
        let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    }
    if jsv.is_any_string() {
        let bytes = b"[object String]";
        let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    }
    if jsv.is_bigint() {
        // BigInt is BIGINT_TAG-tagged (not POINTER_TAG), so it bypasses the
        // pointer brand block below; Node tags it `[object BigInt]`.
        let bytes = b"[object BigInt]";
        let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    }
    let raw_addr = if jsv.is_pointer() {
        (bits & POINTER_MASK) as usize
    } else if bits > 0x1000 && (bits >> 48) == 0 {
        bits as usize
    } else {
        0
    };
    // Proxy receiver (§20.1.3.6). A revocable Proxy is a POINTER_TAG value
    // whose payload is a small id in the proxy band, NOT a heap pointer, so it
    // must be handled before the brand blocks below dereference `raw_addr`.
    // Spec order: (1) IsArray(O) — recurses through the [[ProxyTarget]] chain
    // and throws a TypeError if any link is revoked (test262 proxy-revoked);
    // (2) builtinTag from arrayness/callability; (3) Get(O, @@toStringTag)
    // through the get trap, falling back to builtinTag when it is not a String
    // (a proxy revoked DURING that Get returns undefined, so it does not throw —
    // test262 proxy-revoked-during-get-call).
    if jsv.is_pointer()
        && crate::value::addr_class::is_proxy_id_band(raw_addr)
        && crate::proxy::js_proxy_is_proxy(value) != 0
    {
        const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
        let is_array = crate::array::js_array_is_array(value).to_bits() == TAG_TRUE;
        let builtin_tag = if is_array {
            "Array"
        } else if crate::proxy::proxy_wraps_callable(value) {
            "Function"
        } else {
            "Object"
        };
        let sym = crate::symbol::well_known_symbol("toStringTag");
        let tag = if sym.is_null() {
            None
        } else {
            let sym_f64 = f64::from_bits(POINTER_TAG | (sym as u64 & POINTER_MASK));
            string_value_to_owned(crate::proxy::js_proxy_get(value, sym_f64))
        };
        let formatted = match tag {
            Some(tag) => format!("[object {}]", tag),
            None => format!("[object {}]", builtin_tag),
        };
        let str_ptr =
            crate::string::js_string_from_bytes(formatted.as_ptr(), formatted.len() as u32);
        return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    }
    if raw_addr >= 0x1000 && crate::date::is_date_cell_addr(raw_addr) {
        let str_ptr = crate::string::js_string_from_bytes(b"[object Date]".as_ptr(), 13);
        return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    }
    if raw_addr >= 0x1000 && crate::buffer::is_registered_buffer(raw_addr) {
        let tag = if crate::buffer::crypto_key_meta(raw_addr).is_some() {
            "CryptoKey"
        } else if crate::buffer::is_array_buffer(raw_addr) {
            "ArrayBuffer"
        } else if crate::buffer::is_shared_array_buffer(raw_addr) {
            "SharedArrayBuffer"
        } else if crate::buffer::is_data_view(raw_addr) {
            "DataView"
        } else {
            "Uint8Array"
        };
        let formatted = format!("[object {}]", tag);
        let bytes = formatted.as_bytes();
        let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    }
    // Map / Set / WeakMap / WeakSet / Promise brands. Node tags these
    // `[object Map]` / `[object Set]` / `[object WeakMap]` / `[object WeakSet]`
    // / `[object Promise]`; without per-type detection they fall through to the
    // generic `[object Object]`. Map/Set are raw-alloc'd (no GcHeader) so detect
    // via their registries before the GC-header object discrimination below.
    if raw_addr >= 0x1000 {
        let tag: Option<&str> = if crate::map::is_registered_map(raw_addr) {
            Some("Map")
        } else if crate::set::is_registered_set(raw_addr) {
            Some("Set")
        } else if crate::regex::is_regex_pointer(raw_addr as *const u8) {
            // `Object.prototype.toString.call(/a/)` is `[object RegExp]` (the
            // brand) — distinct from `/a/.toString()` which is `/a/` (the value).
            Some("RegExp")
        } else if crate::symbol::is_registered_symbol(raw_addr) {
            Some("Symbol")
        } else if let Some(kind) = crate::typedarray::lookup_typed_array_kind(raw_addr) {
            // Typed arrays are raw-i64 pointers with no brand arm; without this
            // they fall through to the `is_number()` fallback below (a small
            // raw-pointer bit pattern reads as a finite f64) → `[object Number]`.
            Some(crate::typedarray::name_for_kind(kind))
        } else {
            None
        };
        if let Some(tag) = tag {
            let formatted = format!("[object {}]", tag);
            let str_ptr =
                crate::string::js_string_from_bytes(formatted.as_ptr(), formatted.len() as u32);
            return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
        }
    }
    if let Some(cid) = crate::weakref::weak_class_id_from_receiver(value) {
        let tag = if cid == crate::weakref::CLASS_ID_WEAKSET {
            "WeakSet"
        } else {
            "WeakMap"
        };
        let formatted = format!("[object {}]", tag);
        let str_ptr =
            crate::string::js_string_from_bytes(formatted.as_ptr(), formatted.len() as u32);
        return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    }
    if crate::promise::js_value_is_promise(value) != 0 {
        let str_ptr = crate::string::js_string_from_bytes(b"[object Promise]".as_ptr(), 16);
        return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    }
    if let Some(tag) = web_stream_to_string_tag(value) {
        let formatted = format!("[object {}]", tag);
        let bytes = formatted.as_bytes();
        let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    }
    if let Some(tag) = fetch_handle_to_string_tag(value) {
        let formatted = format!("[object {}]", tag);
        let bytes = formatted.as_bytes();
        let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    }
    if let Some(obj) = crate::url::object_from_f64(value) {
        if crate::url::url_class::is_url_object_shape(obj) {
            let str_ptr = crate::string::js_string_from_bytes(b"[object URL]".as_ptr(), 12);
            return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
        }
    }
    if let Some(tag) = crate::builtins::boxed_primitive_to_string_tag(value) {
        let formatted = format!("[object {}]", tag);
        let bytes = formatted.as_bytes();
        let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    }
    if let Some(tag) = object_to_string_tag_property(value) {
        let formatted = format!("[object {}]", tag);
        let bytes = formatted.as_bytes();
        let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    }
    if (raw_addr >= 0x10000 && crate::closure::is_closure_ptr(raw_addr))
        || crate::object::is_class_object_ptr(raw_addr as *const u8)
        || is_function_prototype_object_value(value)
    {
        // %Function.prototype% is itself a (callable) Function object, so
        // `Object.prototype.toString.call(Function.prototype)` is
        // "[object Function]" even though Perry stores it as a plain object.
        let bytes = b"[object Function]";
        let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    }
    if jsv.is_int32() {
        let class_id = (bits & 0xFFFF_FFFF) as u32;
        if crate::object::is_class_id_registered(class_id) {
            let bytes = b"[object Function]";
            let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
        }
    }
    if jsv.is_int32() || jsv.is_number() {
        let bytes = b"[object Number]";
        let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    }
    // A Date is a NaN-boxed pointer to a `DateCell` (#2089). Node tags it
    // `[object Date]`; without this it falls through to `[object Object]`.
    if crate::date::is_date_value(value) {
        let bytes = b"[object Date]";
        let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    }
    // Heap-allocated pointers: discriminate Array / Error from generic
    // Object via the GC header type byte.
    //
    // A handle-band value (`< 0x100000`: Web Fetch `Headers`/`Request`/
    // `Response`/`Blob` ids, net/http small handles, …) is a registry id, NOT a
    // heap pointer. It reaches here when the SDK coerces such a handle to a
    // string — e.g. an implicit `ToString(headers)` while assembling a request —
    // and the bare id lands in `raw_addr`. The `>= GC_HEADER_SIZE + 0x1000`
    // floor below only rejects sub-`0x1008` addresses, so a fetch handle
    // (`0x40000`+) sails through and the `(*gc_header).obj_type` back-read
    // dereferences `id - 8` (the unmapped `0x3FFFB` in the `claude -p` SIGSEGV).
    // Treat the whole handle band as a non-heap value so it falls through to the
    // generic `[object Object]` tag instead of being dereferenced (same
    // #5559/#5560 family as `string_from_header` / `gc_obj_type`).
    let raw_ptr = raw_addr as *const u8;
    if !raw_ptr.is_null()
        && (raw_ptr as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000
        && !crate::value::addr_class::is_handle_band(raw_addr)
    {
        if let Some(tag) = arguments_object_to_string_tag(value) {
            return tag;
        }
        let gc_header = raw_ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc_header).obj_type;
        if gc_type == crate::gc::GC_TYPE_ARRAY || gc_type == crate::gc::GC_TYPE_LAZY_ARRAY {
            // #3553: a function's `arguments` object is represented as an array
            // carrying the GC_ARRAY_ARGUMENTS_OBJECT flag. Node tags it
            // `[object Arguments]`, not `[object Array]`.
            let bytes: &[u8] = if crate::array::array_has_arguments_object_flag(
                raw_addr as *const crate::array::ArrayHeader,
            ) {
                b"[object Arguments]"
            } else {
                b"[object Array]"
            };
            let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
        }
        if gc_type == crate::gc::GC_TYPE_ERROR {
            let bytes = b"[object Error]";
            let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
        }
    }
    let mut tag_str: Option<String> = None;
    if (bits & 0xFFFF_0000_0000_0000) == POINTER_TAG {
        let obj_ptr = (bits & POINTER_MASK) as *const ObjectHeader;
        // Skip handle-band ids (Web Fetch / net / http registry handles) — they
        // are POINTER_TAG-boxed but are NOT `ObjectHeader` pointers, so reading
        // `(*obj_ptr).class_id` would dereference the bare id (the same fetch
        // handle that faults at the GcHeader back-read above).
        if !obj_ptr.is_null()
            && (obj_ptr as usize) >= 0x1000
            && !crate::value::addr_class::is_handle_band(obj_ptr as usize)
        {
            let class_id = (*obj_ptr).class_id;
            if class_id == crate::object::CLASS_ID_COMPRESSION_STREAM {
                tag_str = Some("CompressionStream".to_string());
            } else if class_id == crate::object::CLASS_ID_DECOMPRESSION_STREAM {
                tag_str = Some("DecompressionStream".to_string());
            } else if class_id == crate::regex::REGEXP_STRING_ITERATOR_CLASS_ID {
                tag_str = Some("RegExp String Iterator".to_string());
            }
            if let Some(func_ptr) = lookup_to_string_tag_hook(class_id) {
                let getter: extern "C" fn(f64) -> f64 = std::mem::transmute(func_ptr as *const u8);
                let result_f64 = getter(value);
                let rbits = result_f64.to_bits();
                if (rbits & 0xFFFF_0000_0000_0000) == STRING_TAG {
                    let str_ptr = (rbits & POINTER_MASK) as *const crate::string::StringHeader;
                    if !str_ptr.is_null() {
                        let len = (*str_ptr).byte_len as usize;
                        let data = (str_ptr as *const u8)
                            .add(std::mem::size_of::<crate::string::StringHeader>());
                        let bytes = std::slice::from_raw_parts(data, len);
                        if let Ok(s) = std::str::from_utf8(bytes) {
                            tag_str = Some(s.to_string());
                        }
                    }
                }
            }
            // #1479: native-module namespaces don't go through the
            // class toStringTag hook (they share one synthetic
            // class_id), so look them up by module name. Node tags
            // `performance` as "Performance" — wire that up here so
            // `Object.prototype.toString.call(performance)` matches.
            if tag_str.is_none() && class_id == crate::object::native_module::NATIVE_MODULE_CLASS_ID
            {
                if let Some(module_name) =
                    crate::object::native_module::read_native_module_name(obj_ptr)
                {
                    if let Some(tag) = native_module_to_string_tag(&module_name) {
                        tag_str = Some(tag.to_string());
                    }
                }
            }
        }
    }
    let formatted = match tag_str {
        Some(tag) => format!("[object {}]", tag),
        None => "[object Object]".to_string(),
    };
    let bytes = formatted.as_bytes();
    let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK))
}

/// #1479: Map a native-module name (as stored in the namespace
/// ObjectHeader's field 0) to its `Symbol.toStringTag` value. Only
/// modules whose namespace is exposed as a singleton with a defined
/// Node tag belong here — others fall back to "Object" via the
/// caller's `None` arm.
fn native_module_to_string_tag(module: &str) -> Option<&'static str> {
    match module {
        // `Object.prototype.toString.call(performance)` is
        // "[object Performance]" in Node.
        "perf_hooks" => Some("Performance"),
        "crypto.webcrypto" => Some("Crypto"),
        "crypto.subtle" => Some("SubtleCrypto"),
        _ => None,
    }
}
