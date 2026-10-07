//! Node-compatible argument validation for the `fs` module (#2013).
//!
//! Node throws synchronously on invalid `fs` arguments with a specific
//! `.code`: a non path-like first argument yields `TypeError
//! [ERR_INVALID_ARG_TYPE]`, and the `fd`-accepting readers/writers
//! (`readFileSync`/`writeFileSync`) treat a numeric first argument as a file
//! descriptor — an invalid one fails with `Error [EBADF]`. Perry's `fs`
//! helpers previously returned a sentinel (empty string, `-1`, a zeroed
//! stats object) and never threw, so `assert.throws`-style tests saw
//! "Missing expected exception" once #1924 stopped masking the no-throw case.
//!
//! These helpers are the reusable validation surface called from the top of
//! the `fs` sync entry points. The error `.code` is recorded in the
//! per-message side table (`node_submodules`) so the `.code` getter recovers
//! it on the caught error — the same mechanism `fs` already uses for POSIX
//! errors like `ENOENT`.

use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;

use crate::closure::ClosureHeader;

/// True if `value` is a valid Node "path-like" — a string (including inline
/// SSO short strings), a `Buffer`, or a `file:` URL object. Mirrors the type
/// acceptance of Node's internal `getValidatedPath`.
pub(crate) fn is_path_like(value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_any_string() {
        return true;
    }
    if crate::buffer::js_buffer_is_buffer(value.to_bits() as i64) == 1 {
        return true;
    }
    if jv.is_pointer() {
        let obj = jv.as_pointer::<crate::object::ObjectHeader>();
        if !obj.is_null() {
            let protocol = crate::url::get_string_content(crate::object::js_object_get_field_f64(
                obj,
                crate::url::parse::URL_PROTOCOL,
            ));
            return protocol == "file:" && unsafe { is_valid_file_url_path_object(obj) };
        }
    }
    false
}

fn encoded_path_separator(pathname: &str) -> bool {
    let bytes = pathname.as_bytes();
    if bytes.len() < 3 {
        return false;
    }
    for i in 0..=(bytes.len() - 3) {
        if bytes[i] != b'%' {
            continue;
        }
        let hex_hi = bytes[i + 1];
        let hex_lo = bytes[i + 2] | 0x20;
        if hex_hi == b'2' && hex_lo == b'f' {
            return true;
        }
        #[cfg(windows)]
        if hex_hi == b'5' && hex_lo == b'c' {
            return true;
        }
    }
    false
}

unsafe fn file_url_host(obj: *const crate::object::ObjectHeader) -> String {
    let hostname = crate::url::get_string_content(crate::object::js_object_get_field_f64(
        obj,
        crate::url::parse::URL_HOSTNAME,
    ));
    if !hostname.is_empty() {
        return hostname;
    }
    crate::url::get_string_content(crate::object::js_object_get_field_f64(
        obj,
        crate::url::parse::URL_HOST,
    ))
}

unsafe fn file_url_pathname(obj: *const crate::object::ObjectHeader) -> String {
    crate::url::get_string_content(crate::object::js_object_get_field_f64(
        obj,
        crate::url::parse::URL_PATHNAME,
    ))
}

unsafe fn is_valid_file_url_path_object(obj: *const crate::object::ObjectHeader) -> bool {
    let host = file_url_host(obj);
    if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
        return false;
    }
    !encoded_path_separator(&file_url_pathname(obj))
}

pub(crate) unsafe fn validate_file_url_path_object(obj: *const crate::object::ObjectHeader) {
    let host = file_url_host(obj);
    if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
        throw_type_error_with_code(
            "File URL host must be \"localhost\" or empty on this platform",
            "ERR_INVALID_FILE_URL_HOST",
        );
    }
    if encoded_path_separator(&file_url_pathname(obj)) {
        throw_type_error_with_code(
            "File URL path must not include encoded / characters",
            "ERR_INVALID_FILE_URL_PATH",
        );
    }
}

fn validate_path_like_value(_arg_name: &str, value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_any_string() {
        return true;
    }
    if crate::buffer::js_buffer_is_buffer(value.to_bits() as i64) == 1 {
        return true;
    }
    if !jv.is_pointer() {
        return false;
    }
    let obj = jv.as_pointer::<crate::object::ObjectHeader>();
    if obj.is_null() {
        return false;
    }
    let protocol = crate::url::get_string_content(crate::object::js_object_get_field_f64(
        obj,
        crate::url::parse::URL_PROTOCOL,
    ));
    if protocol.is_empty() {
        return false;
    }
    if protocol != "file:" {
        throw_type_error_with_code("The URL must be of scheme file", "ERR_INVALID_URL_SCHEME");
    }
    unsafe { validate_file_url_path_object(obj) };
    true
}

/// True if `value` is a JS number (a plain IEEE double *or* an INT32-tagged
/// small integer). `JSValue::is_number` deliberately excludes the INT32 tag,
/// so both must be checked.
pub fn is_numeric(jv: JSValue) -> bool {
    jv.is_number() || jv.is_int32()
}

fn is_nullish(jv: JSValue) -> bool {
    jv.is_undefined() || jv.is_null()
}

fn numeric_to_i32(jv: JSValue) -> i32 {
    if jv.is_int32() {
        jv.as_int32()
    } else {
        jv.as_number() as i32
    }
}

/// Rust compatibility adapter; only JS strings can retain unpaired surrogates.
pub fn describe_received(value: f64) -> String {
    string_header_to_string(describe_received_js(value))
}

fn received_text(text: &str) -> *mut StringHeader {
    js_string_from_bytes(text.as_ptr(), text.len() as u32)
}

/// Canonical JS renderer. Keep inspected strings and implicitly coerced names
/// in the runtime string domain, including when composing public error messages.
pub(crate) fn describe_received_js(value: f64) -> *mut StringHeader {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return received_text("undefined");
    }
    if jv.is_null() {
        return received_text("null");
    }
    if jv.is_bool() {
        return received_text(&format!("type boolean ({})", jv.as_bool()));
    }
    // Denormal bits can resemble legacy addresses; never probe them as pointers.
    if is_numeric(jv) {
        let n = if jv.is_int32() {
            jv.as_int32() as f64
        } else {
            value
        };
        let number = if n == 0.0 && n.is_sign_negative() {
            "-0".to_string()
        } else {
            crate::string::js_format_f64(n)
        };
        return received_text(&format!("type number ({number})"));
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    if jv.is_any_string() {
        let inspected =
            scope.root_string_ptr(inspect_string_for_received_js(value.get_nanbox_f64()));
        let prefix = scope.root_string_ptr(js_string_from_bytes(b"type string (".as_ptr(), 13));
        let joined = scope.root_string_ptr(prefix.with_const_ptr(|a| {
            inspected.with_const_ptr(|b| crate::string::js_string_concat(a, b))
        }));
        let suffix = scope.root_string_ptr(js_string_from_bytes(b")".as_ptr(), 1));
        return joined
            .with_const_ptr(|a| suffix.with_const_ptr(|b| crate::string::js_string_concat(a, b)));
    }
    if jv.is_bigint() {
        return received_text(&format!(
            "type bigint ({}n)",
            bigint_decimal(value.get_nanbox_f64())
        ));
    }
    if unsafe { crate::symbol::js_is_symbol(value.get_nanbox_f64()) != 0 } {
        let ptr = unsafe { crate::symbol::js_symbol_to_string(value.get_nanbox_f64()) }
            as *const StringHeader;
        return received_text(&format!("type symbol ({})", string_header_to_string(ptr)));
    }
    if !super::stream::extract_closure_ptr(value.get_nanbox_f64()).is_null() {
        let name = scope.root_nanbox_f64(received_property(value.get_nanbox_f64(), "name"));
        return received_name("function ", name.get_nanbox_f64());
    }
    if jv.is_pointer() {
        let constructor =
            scope.root_nanbox_f64(received_property(value.get_nanbox_f64(), "constructor"));
        if crate::value::js_is_truthy(constructor.get_nanbox_f64()) != 0 {
            let constructor =
                scope.root_nanbox_f64(received_property(value.get_nanbox_f64(), "constructor"));
            let key = scope.root_nanbox_f64(received_key("name"));
            // Node uses `"name" in value.constructor`: the second read may
            // be primitive, which must throw an uncoded language TypeError.
            if crate::object::js_in_operator(constructor.get_nanbox_f64(), key.get_nanbox_f64())
                .to_bits()
                == crate::value::TAG_TRUE
            {
                let constructor =
                    scope.root_nanbox_f64(received_property(value.get_nanbox_f64(), "constructor"));
                let name =
                    scope.root_nanbox_f64(received_property(constructor.get_nanbox_f64(), "name"));
                return received_name("an instance of ", name.get_nanbox_f64());
            }
        }
        if let Some(fallback) = received_native_fallback(value.get_nanbox_f64()) {
            return fallback;
        }
        let addr = crate::value::addr_class::object_ref_addr(value.get_nanbox_f64());
        let header = unsafe { crate::value::addr_class::try_read_tracked_gc_header(addr) };
        let null_proto = header.is_some_and(|h| unsafe {
            (*h.as_ptr())._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0
        });
        if null_proto
            || crate::object::prototype_chain::object_static_prototype(addr)
                == Some(crate::value::TAG_NULL)
        {
            let empty = header.is_some_and(|h| unsafe {
                (*h.as_ptr()).obj_type == crate::gc::GC_TYPE_OBJECT
                    && crate::object::object_keys(addr as *const crate::object::ObjectHeader)
                        .count()
                        == 0
            });
            return received_text(if empty {
                "[Object: null prototype] {}"
            } else {
                "[Object: null prototype]"
            });
        }
        return received_text("[Object]");
    }
    received_text("an unsupported value")
}

/// Build a received-value error without round-tripping through UTF-8 Rust text.
pub(crate) fn build_received_type_error(prefix: &str, value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let received = scope.root_string_ptr(describe_received_js(value.get_nanbox_f64()));
    let prefix = scope.root_string_ptr(js_string_from_bytes(prefix.as_ptr(), prefix.len() as u32));
    let message = scope
        .root_string_ptr(prefix.with_const_ptr(|a| {
            received.with_const_ptr(|b| crate::string::js_string_concat(a, b))
        }));
    message.with_mut_ptr(|s| {
        crate::node_submodules::register_error_code_pub(s, "ERR_INVALID_ARG_TYPE")
    });
    let error = message.with_mut_ptr(|s| crate::error::js_typeerror_new(s));
    crate::value::js_nanbox_pointer(error as i64)
}

pub(crate) fn throw_received_type_error(prefix: &str, value: f64) -> ! {
    crate::exception::js_throw(build_received_type_error(prefix, value))
}

fn received_key(name: &str) -> f64 {
    crate::value::js_nanbox_string(js_string_from_bytes(name.as_ptr(), name.len() as u32) as i64)
}

/// Resolve GetV locally: nullish receivers throw, while primitive receivers
/// consult their intrinsic prototype without replacing the accessor receiver.
fn received_property(value: f64, name: &str) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let jv = JSValue::from_bits(value.get_nanbox_u64());
    if is_nullish(jv) {
        crate::error::js_throw_type_error_property_access(
            jv.is_null() as u32,
            name.as_ptr(),
            name.len(),
        );
    }
    let key = scope.root_nanbox_f64(received_key(name));
    if crate::proxy::js_proxy_is_proxy(value.get_nanbox_f64()) != 0 {
        return crate::proxy::proxy_get_with_receiver(
            value.get_nanbox_f64(),
            key.get_nanbox_f64(),
            value.get_nanbox_f64(),
        );
    }
    let primitive = if jv.is_any_string() {
        Some("String")
    } else if jv.is_bool() {
        Some("Boolean")
    } else if jv.is_bigint() {
        Some("BigInt")
    } else if unsafe { crate::symbol::js_is_symbol(value.get_nanbox_f64()) != 0 } {
        Some("Symbol")
    } else if is_numeric(jv) && crate::object::class_ref_id(value.get_nanbox_f64()).is_none() {
        Some("Number")
    } else {
        None
    };
    if let Some(brand) = primitive {
        let prototype = scope.root_nanbox_f64(crate::object::builtin_prototype_value(brand));
        return received_inherited_property(
            value.get_nanbox_f64(),
            prototype.get_nanbox_u64(),
            key.get_nanbox_f64(),
        );
    }
    // Public generic dispatch can retry an intrinsic after an explicit missing
    // chain, or hardcode a native constructor. Diagnostics must not do either.
    if name == "constructor" {
        if let Some(brand) = received_native_brand(value.get_nanbox_f64()) {
            if crate::object::js_object_has_own(value.get_nanbox_f64(), key.get_nanbox_f64())
                .to_bits()
                != crate::value::TAG_TRUE
            {
                let addr = crate::value::addr_class::object_ref_addr(value.get_nanbox_f64());
                let prototype = scope.root_nanbox_f64(
                    match crate::object::prototype_chain::object_static_prototype(addr) {
                        Some(bits) => f64::from_bits(bits),
                        None if received_has_null_prototype(addr) => {
                            f64::from_bits(crate::value::TAG_NULL)
                        }
                        None => crate::object::builtin_prototype_value(brand),
                    },
                );
                return received_inherited_property(
                    value.get_nanbox_f64(),
                    prototype.get_nanbox_u64(),
                    key.get_nanbox_f64(),
                );
            }
        }
    }
    unsafe {
        crate::object::js_object_get_property_key(value.get_nanbox_f64(), key.get_nanbox_f64())
    }
}

fn received_inherited_property(receiver: f64, prototype: u64, key: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let prototype = scope.root_nanbox_f64(f64::from_bits(prototype));
    let key = scope.root_nanbox_f64(key);
    let previous = crate::object::accessor_receiver_override_begin(receiver.get_nanbox_f64());
    let previous = previous.map(|value| scope.root_nanbox_f64(value));
    // Restore the displaced override on both success and user exceptions.
    let result = crate::exception::catch_js_throw(|| {
        let key_ptr = crate::value::js_get_string_pointer_unified(key.get_nanbox_f64());
        crate::object::prototype_chain::resolve_inherited_field_from_prototype(
            crate::value::addr_class::object_ref_addr(receiver.get_nanbox_f64()),
            prototype.get_nanbox_u64(),
            key_ptr as *const StringHeader,
        )
        .unwrap_or_else(JSValue::undefined)
        .bits()
    });
    crate::object::accessor_receiver_override_end(previous.map(|value| value.get_nanbox_f64()));
    match result {
        Ok(bits) => f64::from_bits(bits),
        Err(error) => crate::exception::js_throw(error),
    }
}

fn received_native_brand(value: f64) -> Option<&'static str> {
    let addr = crate::value::addr_class::object_ref_addr(value);
    if let Some(kind) = crate::typedarray::lookup_typed_array_kind(addr) {
        return Some(crate::typedarray::name_for_kind(kind));
    }
    match crate::buffer::buffer_brand(addr) {
        Some(crate::buffer::BufferBrand::NodeBuffer) => Some("Buffer"),
        Some(crate::buffer::BufferBrand::Uint8Array) => Some("Uint8Array"),
        Some(crate::buffer::BufferBrand::DataView) => Some("DataView"),
        Some(crate::buffer::BufferBrand::ArrayBuffer) => {
            Some(if crate::buffer::is_shared_array_buffer(addr) {
                "SharedArrayBuffer"
            } else {
                "ArrayBuffer"
            })
        }
        _ if crate::date::is_date_cell_addr(addr) => Some("Date"),
        _ => None,
    }
}

fn received_has_null_prototype(addr: usize) -> bool {
    unsafe { crate::value::addr_class::try_read_tracked_gc_header(addr) }.is_some_and(
        |header| unsafe { (*header.as_ptr())._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0 },
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReceivedNativePrototype {
    Intrinsic,
    Complex,
    Ordinary,
}

/// Inspect link authority, not properties: no constructor Get, descriptor
/// accessor execution or Proxy trap belongs in negative-depth fallback.
fn received_native_prototype_shape(prototype: f64, intrinsic: f64) -> ReceivedNativePrototype {
    let scope = crate::gc::RuntimeHandleScope::new();
    let current = scope.root_nanbox_f64(prototype);
    let intrinsic = scope.root_nanbox_f64(intrinsic);
    let ordinary = scope.root_nanbox_f64(crate::object::builtin_prototype_value("Object"));
    // Match the generic runtime chain bound. Corrupt/cyclic/opaque chains
    // conservatively retain ordinary rendering, never a new user-visible error.
    for _ in 0..32 {
        if current.get_nanbox_u64() == intrinsic.get_nanbox_u64() {
            return ReceivedNativePrototype::Intrinsic;
        }
        if current.get_nanbox_u64() == ordinary.get_nanbox_u64()
            || !JSValue::from_bits(current.get_nanbox_u64()).is_pointer()
            || crate::proxy::js_proxy_is_proxy(current.get_nanbox_f64()) != 0
        {
            return ReceivedNativePrototype::Ordinary;
        }
        let addr = crate::value::addr_class::object_ref_addr(current.get_nanbox_f64());
        match crate::object::prototype_chain::object_static_prototype(addr) {
            Some(crate::value::TAG_NULL) => return ReceivedNativePrototype::Complex,
            Some(bits) => current.set_nanbox_u64(bits),
            // A later recorded edge overrides the sticky born-null header bit.
            None if received_has_null_prototype(addr) => return ReceivedNativePrototype::Complex,
            None => {
                // A missing instance edge can still have a class-default link.
                // Inspect the published class prototypes without any user Get.
                let class_id = unsafe {
                    crate::value::addr_class::try_read_tracked_gc_header(addr)
                        .filter(|header| (*header.as_ptr()).obj_type == crate::gc::GC_TYPE_OBJECT)
                        .map_or(0, |_| (*(addr as *const crate::ObjectHeader)).class_id)
                };
                if class_id == 0 {
                    return ReceivedNativePrototype::Ordinary;
                }
                let declared = crate::object::class_decl_prototype_object(class_id);
                let next = if declared.is_null() {
                    crate::object::class_prototype_object(class_id)
                } else {
                    declared
                };
                if next.is_null() {
                    return ReceivedNativePrototype::Ordinary;
                }
                current.set_nanbox_f64(crate::value::js_nanbox_pointer(next as i64));
            }
        }
    }
    ReceivedNativePrototype::Ordinary
}

/// The received fallback is inspection at depth -1, not public util.inspect.
/// Resolve native identity and prototype shape without another constructor read.
fn received_native_fallback(value: f64) -> Option<*mut StringHeader> {
    let brand = received_native_brand(value)?;
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let addr = crate::value::addr_class::object_ref_addr(value.get_nanbox_f64());
    let recorded = crate::object::prototype_chain::object_static_prototype(addr);
    let prototype = recorded.map(|bits| scope.root_nanbox_f64(f64::from_bits(bits)));
    let null_proto = recorded == Some(crate::value::TAG_NULL)
        || (recorded.is_none() && received_has_null_prototype(addr));
    let intrinsic = scope.root_nanbox_f64(crate::object::builtin_prototype_value(brand));
    let shape = if null_proto {
        ReceivedNativePrototype::Intrinsic
    } else if let Some(prototype) = prototype.as_ref() {
        received_native_prototype_shape(prototype.get_nanbox_f64(), intrinsic.get_nanbox_f64())
    } else {
        ReceivedNativePrototype::Intrinsic
    };
    let custom = shape == ReceivedNativePrototype::Complex;
    let typed = crate::typedarray::lookup_typed_array_kind(
        crate::value::addr_class::object_ref_addr(value.get_nanbox_f64()),
    )
    .is_some()
        || matches!(brand, "Uint8Array" | "Buffer");
    if shape == ReceivedNativePrototype::Ordinary {
        return Some(received_text(if typed { "[Object]" } else { "{}" }));
    }
    let suffix = if null_proto {
        ": null prototype"
    } else if custom {
        " <Complex prototype>"
    } else {
        ""
    };
    let label = if brand == "Buffer" && (null_proto || custom) {
        "Uint8Array"
    } else {
        brand
    };
    if brand == "Date" {
        let timestamp = crate::date::date_cell_timestamp(value.get_nanbox_f64());
        let payload = if timestamp.is_nan() {
            "Invalid Date".to_string()
        } else {
            string_header_to_string(crate::date::js_date_to_iso_string(value.get_nanbox_f64()))
        };
        return Some(received_text(&if null_proto {
            format!("[Date{suffix}] {payload}")
        } else if custom {
            format!("Date{suffix} {payload}")
        } else {
            payload
        }));
    }
    if brand == "Buffer" && !null_proto && !custom {
        // Node's native Buffer inspection performs one additional constructor
        // Get, tolerating even a throwing getter before rendering its payload.
        // This is the payload arm only, not another prototype-classification Get.
        let _ = crate::exception::catch_js_throw(|| {
            received_property(value.get_nanbox_f64(), "constructor")
        });
        return Some(received_text(&crate::builtins::format_jsvalue(
            value.get_nanbox_f64(),
            0,
        )));
    }
    if typed && !null_proto && brand != "Buffer" {
        let addr = crate::value::addr_class::object_ref_addr(value.get_nanbox_f64());
        let length = if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
            unsafe {
                crate::typedarray::element_length(
                    addr as *const crate::typedarray::TypedArrayHeader,
                )
            }
        } else {
            crate::buffer::js_buffer_length(addr as *const crate::buffer::BufferHeader) as u32
        };
        if custom && length == 0 {
            return Some(received_text(&format!("{label}{suffix} {{}}")));
        }
        // Inspect constructor descriptors, never Get: fallback must not invoke
        // the user getter a fourth time merely to determine a native label.
        let key = scope.root_nanbox_f64(received_key("constructor"));
        let descriptor =
            scope.root_nanbox_f64(crate::object::js_object_get_own_property_descriptor(
                intrinsic.get_nanbox_f64(),
                key.get_nanbox_f64(),
            ));
        let constructor = scope.root_nanbox_f64(
            if descriptor.get_nanbox_u64() == crate::value::TAG_UNDEFINED {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            } else {
                received_property(descriptor.get_nanbox_f64(), "value")
            },
        );
        let builtin = scope.root_nanbox_f64(crate::object::js_get_global_this_builtin_value(
            brand.as_ptr(),
            brand.len(),
        ));
        let changed = constructor.get_nanbox_u64() != builtin.get_nanbox_u64();
        if !custom && changed {
            return Some(received_text(&if length == 0 {
                format!("TypedArray(0) [{label}] []")
            } else {
                format!("[TypedArray [{label}]]")
            }));
        }
        if !custom && length == 0 {
            return Some(received_text(&format!("{label}(0) []")));
        }
    }
    Some(received_text(&format!("[{label}{suffix}]")))
}

fn received_name(prefix: &str, value: f64) -> *mut StringHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    // Node interpolates names with implicit ToString, not explicit String().
    // Symbol names therefore throw before an ERR_INVALID_ARG_TYPE is built.
    let name = scope.root_string_ptr(crate::value::to_string::js_jsvalue_to_string_impl(
        value.get_nanbox_f64(),
        true,
    ));
    let prefix = scope.root_string_ptr(received_text(prefix));
    prefix.with_const_ptr(|a| name.with_const_ptr(|b| crate::string::js_string_concat(a, b)))
}

fn string_header_to_string(ptr: *const StringHeader) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        if let Ok(text) = std::str::from_utf8(bytes) {
            text.to_owned()
        } else {
            let units: Vec<u16> = (0..(*ptr).utf16_len)
                .map(|i| crate::string::js_string_char_code_at(ptr, i as i32) as u16)
                .collect();
            String::from_utf16_lossy(&units)
        }
    }
}

/// Read a JS string value (heap `StringHeader` or inline SSO) into a Rust
/// `String`. This compatibility boundary cannot retain unpaired surrogates.
fn read_js_string(value: f64) -> String {
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    string_header_to_string(ptr)
}

/// Public accessor for [`read_js_string`] so the shared `validators` module
/// can render a string value's contents in an `ERR_INVALID_ARG_VALUE` /
/// `validateOneOf` message.
pub fn read_js_string_pub(value: f64) -> String {
    read_js_string(value)
}

/// Truncate the original UTF-16 value before choosing Node's quoting form.
fn inspect_string_for_received_js(value: f64) -> *mut StringHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let ptr =
        crate::value::js_get_string_pointer_unified(value.get_nanbox_f64()) as *const StringHeader;
    let string = scope.root_string_ptr(ptr);
    let truncated = string.with_const_ptr(|s| crate::string::js_string_length(s)) > 28;
    let rendered = if truncated {
        let prefix = string.with_const_ptr(|s| crate::string::js_string_slice(s, 0, 25));
        let prefix = scope.root_string_ptr(prefix);
        let suffix = scope.root_string_ptr(js_string_from_bytes(b"...".as_ptr(), 3));
        prefix.with_const_ptr(|a| suffix.with_const_ptr(|b| crate::string::js_string_concat(a, b)))
    } else {
        string.with_mut_ptr(|s| s)
    };
    let rendered = scope.root_string_ptr(rendered);
    let has_quote = rendered.with_const_ptr(|s| {
        (0..crate::string::js_string_length(s))
            .any(|i| crate::string::js_string_char_code_at(s, i as i32) == b'\'' as f64)
    });
    if has_quote {
        unsafe { rendered.with_const_ptr(|s| crate::json::js_json_stringify_string(s)) }
    } else {
        let quote = scope.root_string_ptr(js_string_from_bytes(b"'".as_ptr(), 1));
        let joined = scope.root_string_ptr(quote.with_const_ptr(|a| {
            rendered.with_const_ptr(|b| crate::string::js_string_concat(a, b))
        }));
        joined.with_const_ptr(|a| quote.with_const_ptr(|b| crate::string::js_string_concat(a, b)))
    }
}

/// Decimal rendering of a BigInt value for the `Received type bigint (…n)`
/// clause.
fn bigint_decimal(value: f64) -> String {
    let ptr = (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::bigint::BigIntHeader;
    if ptr.is_null() {
        return "0".to_string();
    }
    let s = crate::bigint::js_bigint_to_string(ptr);
    if s.is_null() {
        return "0".to_string();
    }
    unsafe {
        let len = (*s).byte_len as usize;
        let data = (s as *const u8).add(std::mem::size_of::<StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

/// Throw `TypeError [ERR_INVALID_ARG_TYPE]` for a bad path argument, matching
/// Node's message shape. Diverges via `js_throw`.
pub(crate) fn throw_invalid_path_arg(arg_name: &str, value: f64) -> ! {
    throw_received_type_error(
        &format!(
            "The \"{}\" argument must be of type string or an instance of Buffer or URL. Received ",
            arg_name
        ),
        value,
    );
}

/// Throw `Error [EBADF]` for a numeric fd that is not an open descriptor.
fn throw_ebadf(syscall: &'static str) -> ! {
    crate::exception::js_throw(build_ebadf_error_value(syscall))
}

pub(crate) fn build_ebadf_error_value(syscall: &'static str) -> f64 {
    let message = format!("EBADF: bad file descriptor, {}", syscall);
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, "EBADF");
    crate::node_submodules::register_error_syscall(msg, syscall);
    #[cfg(unix)]
    crate::node_submodules::register_error_errno(msg, -libc::EBADF);
    #[cfg(not(unix))]
    crate::node_submodules::register_error_errno(msg, -9);
    let err = crate::error::js_error_new_with_message(msg);
    crate::value::js_nanbox_pointer(err as i64)
}

pub(crate) fn build_type_error_with_code_value(message: &str, code: &'static str) -> f64 {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    let err = crate::error::js_typeerror_new(msg);
    crate::value::js_nanbox_pointer(err as i64)
}

pub fn throw_type_error_with_code(message: &str, code: &'static str) -> ! {
    crate::exception::js_throw(build_type_error_with_code_value(message, code))
}

/// Throw Node's null-byte path error. Node uses `ERR_INVALID_ARG_VALUE`
/// rather than an fs errno when a decoded PathLike contains `\0`.
pub(crate) fn throw_invalid_path_arg_value(arg_name: &str, received: &str) -> ! {
    let display = received.replace('\0', "\\x00");
    let message = format!(
        "The argument '{arg_name}' must be a string, Uint8Array, or URL without null bytes. Received '{display}'"
    );
    throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE")
}

/// Validate a `node:events` listener argument (#3072).
///
/// `listener_bits` is the *raw NaN-box bit pattern* of the JS value passed
/// for an EventEmitter listener (codegen routes these methods through
/// `NA_JSV`, so the callee receives the full value rather than a
/// pre-stripped pointer). Returns the closure pointer as an `i64` when the
/// value is callable; otherwise throws `TypeError [ERR_INVALID_ARG_TYPE]`
/// with Node's `The "listener" argument must be of type function. Received …`
/// message — matching `EventEmitter#on/once/addListener/prependListener/
/// prependOnceListener/removeListener/off`.
///
/// Shared by every listener-taking surface (`EventEmitter.prototype`'s
/// methods, `process.on`, `fs.watch`) so the validation, error class, code
/// and message stay byte-identical across them.
///
/// # Safety
///
/// `name_ptr`/`name_len` must describe a valid UTF-8 byte range (typically a
/// `&'static str`). Callers pass `"listener"`.
#[no_mangle]
pub unsafe extern "C" fn js_validate_event_listener(
    listener_bits: i64,
    name_ptr: *const u8,
    name_len: u32,
) -> i64 {
    let value = f64::from_bits(listener_bits as u64);
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<u8>() as usize;
        if crate::closure::is_closure_ptr(ptr) {
            return ptr as i64;
        }
    }
    let name = if name_ptr.is_null() || name_len == 0 {
        "listener".to_string()
    } else {
        let bytes = std::slice::from_raw_parts(name_ptr, name_len as usize);
        String::from_utf8_lossy(bytes).into_owned()
    };
    throw_received_type_error(
        &format!("The \"{name}\" argument must be of type function. Received "),
        value,
    );
}

/// `#[used]` keepalive so the auto-optimize whole-program-LLVM rebuild does
/// not dead-strip this codegen-invoked `#[no_mangle]` entry point (see
/// project_auto_optimize_keepalive_3320). Called only from generated `.o`
/// via the stdlib/ext events validators, so without an anchor the bitcode
/// internalizer drops it and the default `perry file.ts -o out` link fails.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_VALIDATE_EVENT_LISTENER: unsafe extern "C" fn(i64, *const u8, u32) -> i64 =
    js_validate_event_listener;

/// Validate the first argument of a path-only `fs` sync function (one that
/// does NOT accept a file descriptor — `accessSync`, `statSync`, `mkdirSync`,
/// `readdirSync`, `unlinkSync`, …). Throws `ERR_INVALID_ARG_TYPE` on any
/// non path-like value (including numbers). No-op when the value is valid.
pub(crate) fn validate_path(arg_name: &str, value: f64) {
    if !validate_path_like_value(arg_name, value) {
        throw_invalid_path_arg(arg_name, value);
    }
}

/// Validate the first argument of an fd-accepting reader/writer
/// (`readFileSync`, `writeFileSync`). A path-like value is accepted as-is; a
/// numeric value is treated as a file descriptor and, if it is not open,
/// throws `EBADF` (matching `fs.readFileSync(123)`); anything else throws
/// `ERR_INVALID_ARG_TYPE`. `syscall` names the operation for the EBADF error.
pub(crate) fn validate_path_or_fd(arg_name: &str, value: f64, syscall: &'static str) {
    if validate_path_like_value(arg_name, value) {
        return;
    }
    if let Some(fd) = crate::fs::filehandle_object_fd(value) {
        if !crate::fs::fd_is_registered(fd) {
            throw_ebadf(syscall);
        }
        return;
    }
    let jv = JSValue::from_bits(value.to_bits());
    if is_numeric(jv) {
        // A numeric first argument is a file descriptor. Perry's readers and
        // writers already serve a *registered* fd (`numeric_fd_value` +
        // `FD_REGISTRY`); the validation contract here is only to reject an
        // unknown/closed fd with `EBADF` (e.g. `fs.readFileSync(123)`).
        if !crate::fs::fd_is_registered(numeric_to_i32(jv)) {
            throw_ebadf(syscall);
        }
        return;
    }
    throw_invalid_path_arg(arg_name, value);
}

/// Validate that `value` is a JS number suitable for a file descriptor —
/// finite integer in `[0, 2^31-1]`. Matches Node's `validateInt32(fd, 'fd', 0)`.
///
/// Non-numbers raise `TypeError [ERR_INVALID_ARG_TYPE]`; `NaN`, `Infinity`,
/// non-integers, and out-of-range integers raise `RangeError
/// [ERR_OUT_OF_RANGE]`. The filehandle path (`filehandle_fd(closure) as f64`)
/// always passes a real `i32`-ranged value, so the validators are no-ops there.
pub(crate) fn validate_fd(value: f64) {
    validate_int32(value, "fd", 0, i32::MAX as i64);
}

/// Issue #2013 — validate an `fd` argument AND verify it's an open
/// descriptor in Perry's `FD_REGISTRY`. Mirrors Node's "validate fd
/// type, then bounce on EBADF" pattern for the fd-only sync surface
/// (`fs.closeSync`, `fs.readSync`, `fs.readvSync`, `fs.fsyncSync`,
/// `fs.fdatasyncSync`, `fs.fchmodSync`, `fs.fchownSync`, …). Path-or-fd
/// readers/writers route through `validate_path_or_fd` instead, which
/// has its own EBADF branch keyed on the same registry probe.
pub(crate) fn validate_fd_open(value: f64, syscall: &'static str) {
    validate_fd(value);
    let jv = JSValue::from_bits(value.to_bits());
    let fd = numeric_to_i32(jv);
    if !crate::fs::fd_is_registered(fd) {
        throw_ebadf_pub(syscall);
    }
}

/// Public alias for `throw_ebadf` so the fs entry points can throw a
/// matching `EBADF` from outside this module (#2013).
pub(crate) fn throw_ebadf_pub(syscall: &'static str) -> ! {
    throw_ebadf(syscall)
}

/// Issue #3332 — callback-style fd helpers (`fs.close`, `fs.fsync`,
/// `fs.fdatasync`, `fs.fchmod`) must DELIVER the `EBADF` error to the
/// callback rather than throw it. The fd *type* validation still throws
/// synchronously (matching Node's `validateInt32` on a non-numeric fd);
/// only the "valid type but unknown descriptor" case becomes a deferred
/// callback error. Returns `Some(err_value)` when the fd is not open,
/// `None` when it is registered.
pub(crate) fn fd_open_callback_error(value: f64, syscall: &'static str) -> Option<f64> {
    validate_fd(value);
    let jv = JSValue::from_bits(value.to_bits());
    let fd = numeric_to_i32(jv);
    if crate::fs::fd_is_registered(fd) {
        None
    } else {
        Some(build_ebadf_error_value(syscall))
    }
}

/// Validate callback-style fs APIs that require a callback. Node's current fs
/// validators use the argument name `"cb"` for most legacy callback entry
/// points; `fs.opendir` is the notable `"callback"` exception, so callers pass
/// the exact name they need.
pub(crate) fn validate_required_callback(arg_name: &str, value: f64) -> *const ClosureHeader {
    let ptr = super::stream::extract_closure_ptr(value);
    if ptr.is_null() {
        throw_received_type_error(
            &format!(
                "The \"{}\" argument must be of type function. Received ",
                arg_name
            ),
            value,
        );
    }
    ptr
}

fn gc_type_for_value(value: f64) -> Option<u8> {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let gc_header = unsafe { &*(ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader) };
    if gc_header.obj_type <= crate::gc::GC_TYPE_MAX {
        Some(gc_header.obj_type)
    } else {
        None
    }
}

fn is_options_object_like(value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_pointer() {
        return true;
    }
    crate::buffer::js_buffer_is_buffer(value.to_bits() as i64) == 1
        || !super::stream::extract_closure_ptr(value).is_null()
}

fn is_plain_options_object(value: f64) -> bool {
    if crate::buffer::js_buffer_is_buffer(value.to_bits() as i64) == 1 {
        return false;
    }
    if !super::stream::extract_closure_ptr(value).is_null() {
        return false;
    }
    gc_type_for_value(value) == Some(crate::gc::GC_TYPE_OBJECT)
}

/// Validate fs options parameters that accept either an encoding string or an
/// object (`readFile`, `writeFile`, `readdir`, `readlink`, `realpath`,
/// `mkdtemp`, ...). `null` and `undefined` are accepted.
pub(crate) fn validate_string_or_object_options(arg_name: &str, value: f64) {
    let jv = JSValue::from_bits(value.to_bits());
    if is_nullish(jv) || jv.is_any_string() || is_options_object_like(value) {
        return;
    }
    throw_received_type_error(
        &format!(
            "The \"{}\" argument must be one of type string or object. Received ",
            arg_name
        ),
        value,
    );
}

/// Validate fs options parameters that accept only an options object. Node
/// accepts an omitted value but rejects `null`, arrays, functions, and
/// primitives with `ERR_INVALID_ARG_TYPE`.
pub(crate) fn object_options_type_error_value(arg_name: &str, value: f64) -> Option<f64> {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() || is_plain_options_object(value) {
        return None;
    }
    Some(build_received_type_error(
        &format!("The \"{arg_name}\" argument must be of type object. Received "),
        value,
    ))
}

pub(crate) fn validate_object_options(arg_name: &str, value: f64) {
    if let Some(err) = object_options_type_error_value(arg_name, value) {
        crate::exception::js_throw(err);
    }
}

/// Validate the options object passed to `fs.mkdir*` (#3662). Node accepts an
/// omitted value or a numeric/string `mode` shorthand without inspecting it;
/// when an options *object* is supplied it requires `recursive` to be a boolean
/// and `mode` to be a number or string, throwing
/// `TypeError [ERR_INVALID_ARG_TYPE]` otherwise. `recursive` is checked first,
/// matching Node's `validateBoolean(recursive, 'options.recursive')` ordering.
pub(crate) fn validate_mkdir_options(options_value: f64) {
    if !is_plain_options_object(options_value) {
        return;
    }
    let obj =
        JSValue::from_bits(options_value.to_bits()).as_pointer::<crate::object::ObjectHeader>();

    let recursive_key = js_string_from_bytes(b"recursive".as_ptr(), 9);
    let recursive = crate::object::js_object_get_field_by_name_f64(obj, recursive_key);
    let recursive_jv = JSValue::from_bits(recursive.to_bits());
    if !is_nullish(recursive_jv) && !recursive_jv.is_bool() {
        throw_received_type_error(
            "The \"options.recursive\" property must be of type boolean. Received ",
            recursive,
        );
    }

    let mode_key = js_string_from_bytes(b"mode".as_ptr(), 4);
    let mode = crate::object::js_object_get_field_by_name_f64(obj, mode_key);
    let mode_jv = JSValue::from_bits(mode.to_bits());
    if !is_nullish(mode_jv) && !is_numeric(mode_jv) && !mode_jv.is_any_string() {
        throw_received_type_error(
            "The \"options.mode\" property must be of type number. Received ",
            mode,
        );
    }
}

/// Validate the `mode` bitmask used by `fs.access*` and `fs.copyFile*`.
/// Node treats `null`/`undefined` as the default, rejects non-numbers with a
/// short `ERR_INVALID_ARG_TYPE` message, and rejects values whose integer part
/// is outside the supported 0..=7 bitmask.
pub(crate) fn validate_fs_mode(value: f64) {
    let jv = JSValue::from_bits(value.to_bits());
    if is_nullish(jv) {
        return;
    }
    if !is_numeric(jv) {
        throw_type_error_with_code(
            "mode must be int32 or null/undefined",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    let n = if jv.is_int32() {
        jv.as_int32() as f64
    } else {
        jv.as_number()
    };
    if !n.is_finite() {
        throw_range_error_with_code("mode is out of range");
    }
    let mode = n as i64;
    if !(0..=7).contains(&mode) {
        throw_range_error_with_code("mode is out of range: >= 0 && <= 7");
    }
}

/// Validate that `value` is a finite integer in `[min, max]`. On type or
/// range failure throws Node's `ERR_INVALID_ARG_TYPE` / `ERR_OUT_OF_RANGE`
/// with the same `Received` clause shape Node uses.
pub(crate) fn validate_int32(value: f64, arg_name: &str, min: i64, max: i64) {
    let jv = JSValue::from_bits(value.to_bits());
    if !is_numeric(jv) {
        throw_received_type_error(
            &format!(
                "The \"{}\" argument must be of type number. Received ",
                arg_name
            ),
            value,
        );
    }
    let n = if jv.is_int32() {
        jv.as_int32() as f64
    } else {
        jv.as_number()
    };
    if !n.is_finite() || n.fract() != 0.0 {
        let received = if n.is_nan() {
            "NaN".to_string()
        } else if n.is_infinite() {
            if n.is_sign_negative() {
                "-Infinity".to_string()
            } else {
                "Infinity".to_string()
            }
        } else {
            format_received_number(n)
        };
        let message = format!(
            "The value of \"{}\" is out of range. It must be an integer. Received {}",
            arg_name, received
        );
        throw_range_error_with_code(&message);
    }
    let i = n as i64;
    if i < min || i > max {
        let message = format!(
            "The value of \"{}\" is out of range. It must be >= {} && <= {}. Received {}",
            arg_name,
            min,
            max,
            format_received_number(n)
        );
        throw_range_error_with_code(&message);
    }
}

/// Render an `f64` the way Node prints it in a `Received …` clause for
/// value errors (`ERR_OUT_OF_RANGE` / `ERR_INVALID_ARG_VALUE`): integers
/// without a fractional part, and `NaN`/`Infinity`/`-Infinity` spelled out
/// (Rust's `Display` would print `inf`/`NaN`). Shared by the `net`/`fs`/
/// `buffer` validators and the `process` exit/cpuUsage validators
/// (#3039-#3049).
pub fn format_received_number(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        }
        .to_string();
    }
    if n.fract() == 0.0 && n.abs() < 1e21 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

/// Validate that `value` is a function (closure). On failure throws
/// `TypeError [ERR_INVALID_ARG_TYPE]`. Mirrors Node's `validateFunction`
/// helper — used to catch `fs.exists(path)` / `fs.copyFile(src, dest, 0, 0)`
/// where the trailing callback is missing or the wrong type.
pub(crate) fn validate_function(arg_name: &str, value: f64) {
    if super::stream::extract_closure_ptr(value).is_null() {
        throw_received_type_error(
            &format!(
                "The \"{}\" argument must be of type function. Received ",
                arg_name
            ),
            value,
        );
    }
}

pub fn throw_range_error_with_code(message: &str) -> ! {
    throw_range_error_named(message, "ERR_OUT_OF_RANGE")
}

/// Throw a `RangeError` carrying an explicit Node error `code`. Most callers
/// want [`throw_range_error_with_code`] (which fixes `code` to
/// `ERR_OUT_OF_RANGE`); this variant lets the `net` port validators raise
/// `ERR_SOCKET_BAD_PORT` with the same machinery (#2013).
pub fn throw_range_error_named(message: &str, code: &'static str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Throw a plain `Error` (name `"Error"`) carrying an explicit Node error
/// `code`. Used by `node:crypto` Sign/Verify finalized-state guards (#2963),
/// which raise `Error [ERR_CRYPTO_INVALID_STATE]: Not initialised` once a
/// handle has been consumed by `.sign()`/`.verify()`.
pub fn throw_error_with_code(message: &str, code: &'static str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    let err = crate::error::js_error_new_with_message(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[cfg(test)]
#[path = "received_diagnostic_tests.rs"]
mod received_diagnostic_tests;
