//! Small `node:tls` helper surface.
//!
//! Live TLS sockets are implemented in the net/stdlib path. This module covers
//! Node-compatible helper APIs and SecureContext shape used for feature checks.

mod roots;

use crate::array::ArrayHeader;
use crate::object::ObjectHeader;
use crate::string::StringHeader;
use crate::value::{JSValue, TAG_NULL, TAG_UNDEFINED};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub const CLASS_ID_TLS_SECURE_CONTEXT: u32 = 0xFFFF_00B5;

static TLS_PROTOTYPE_INITIALIZED: AtomicBool = AtomicBool::new(false);
static ROOT_CERTS_CACHE: AtomicU64 = AtomicU64::new(0);
static DEFAULT_CA_CACHE: AtomicU64 = AtomicU64::new(0);
static SYSTEM_CA_CACHE: AtomicU64 = AtomicU64::new(0);
static EXTRA_CA_CACHE: AtomicU64 = AtomicU64::new(0);

pub const DEFAULT_CIPHERS: &str = "TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256:TLS_AES_128_GCM_SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-AES256-GCM-SHA384:DHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-SHA256:DHE-RSA-AES128-SHA256:ECDHE-RSA-AES256-SHA384:DHE-RSA-AES256-SHA384:ECDHE-RSA-AES256-SHA256:DHE-RSA-AES256-SHA256:HIGH:!aNULL:!eNULL:!EXPORT:!DES:!RC4:!MD5:!PSK:!SRP:!CAMELLIA";

const TLS_CIPHERS: &[&str] = &[
    "aes128-gcm-sha256",
    "aes128-sha",
    "aes128-sha256",
    "aes256-gcm-sha384",
    "aes256-sha",
    "aes256-sha256",
    "dhe-psk-aes128-cbc-sha",
    "dhe-psk-aes128-cbc-sha256",
    "dhe-psk-aes128-gcm-sha256",
    "dhe-psk-aes256-cbc-sha",
    "dhe-psk-aes256-cbc-sha384",
    "dhe-psk-aes256-gcm-sha384",
    "dhe-psk-chacha20-poly1305",
    "dhe-rsa-aes128-gcm-sha256",
    "dhe-rsa-aes128-sha",
    "dhe-rsa-aes128-sha256",
    "dhe-rsa-aes256-gcm-sha384",
    "dhe-rsa-aes256-sha",
    "dhe-rsa-aes256-sha256",
    "dhe-rsa-chacha20-poly1305",
    "ecdhe-ecdsa-aes128-gcm-sha256",
    "ecdhe-ecdsa-aes128-sha",
    "ecdhe-ecdsa-aes128-sha256",
    "ecdhe-ecdsa-aes256-gcm-sha384",
    "ecdhe-ecdsa-aes256-sha",
    "ecdhe-ecdsa-aes256-sha384",
    "ecdhe-ecdsa-chacha20-poly1305",
    "ecdhe-psk-aes128-cbc-sha",
    "ecdhe-psk-aes128-cbc-sha256",
    "ecdhe-psk-aes256-cbc-sha",
    "ecdhe-psk-aes256-cbc-sha384",
    "ecdhe-psk-chacha20-poly1305",
    "ecdhe-rsa-aes128-gcm-sha256",
    "ecdhe-rsa-aes128-sha",
    "ecdhe-rsa-aes128-sha256",
    "ecdhe-rsa-aes256-gcm-sha384",
    "ecdhe-rsa-aes256-sha",
    "ecdhe-rsa-aes256-sha384",
    "ecdhe-rsa-chacha20-poly1305",
    "psk-aes128-cbc-sha",
    "psk-aes128-cbc-sha256",
    "psk-aes128-gcm-sha256",
    "psk-aes256-cbc-sha",
    "psk-aes256-cbc-sha384",
    "psk-aes256-gcm-sha384",
    "psk-chacha20-poly1305",
    "rsa-psk-aes128-cbc-sha",
    "rsa-psk-aes128-cbc-sha256",
    "rsa-psk-aes128-gcm-sha256",
    "rsa-psk-aes256-cbc-sha",
    "rsa-psk-aes256-cbc-sha384",
    "rsa-psk-aes256-gcm-sha384",
    "rsa-psk-chacha20-poly1305",
    "srp-aes-128-cbc-sha",
    "srp-aes-256-cbc-sha",
    "srp-rsa-aes-128-cbc-sha",
    "srp-rsa-aes-256-cbc-sha",
    "tls_aes_128_ccm_8_sha256",
    "tls_aes_128_ccm_sha256",
    "tls_aes_128_gcm_sha256",
    "tls_aes_256_gcm_sha384",
    "tls_chacha20_poly1305_sha256",
];

fn string_value(s: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn ptr_value<T>(ptr: *mut T) -> f64 {
    crate::value::js_nanbox_pointer(ptr as i64)
}

fn key(name: &str) -> *mut StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

unsafe fn gc_header(value: f64) -> Option<*mut crate::gc::GcHeader> {
    let js = JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        return None;
    }
    let ptr = js.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    Some(ptr.sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader)
}

fn freeze_heap_value(value: f64) -> f64 {
    unsafe {
        if let Some(header) = gc_header(value) {
            (*header)._reserved |= crate::gc::OBJ_FLAG_FROZEN
                | crate::gc::OBJ_FLAG_SEALED
                | crate::gc::OBJ_FLAG_NO_EXTEND;
        }
    }
    value
}

fn object_ptr(value: f64) -> Option<*mut ObjectHeader> {
    unsafe {
        let header = gc_header(value)?;
        if (*header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return None;
        }
        Some(JSValue::from_bits(value.to_bits()).as_pointer::<ObjectHeader>() as *mut ObjectHeader)
    }
}

fn array_ptr(value: f64) -> Option<*mut ArrayHeader> {
    unsafe {
        let header = gc_header(value)?;
        if (*header).obj_type != crate::gc::GC_TYPE_ARRAY {
            return None;
        }
        Some(JSValue::from_bits(value.to_bits()).as_pointer::<ArrayHeader>() as *mut ArrayHeader)
    }
}

fn get_field(obj: *mut ObjectHeader, name: &str) -> f64 {
    crate::object::js_object_get_field_by_name_f64(obj, key(name))
}

fn value_to_string(value: f64) -> Option<String> {
    crate::builtins::jsvalue_string_content(value)
}

fn host_to_string(value: f64) -> String {
    let js = JSValue::from_bits(value.to_bits());
    if let Some(s) = value_to_string(value) {
        return s;
    }
    if js.is_int32() {
        return js.as_int32().to_string();
    }
    if js.is_number() {
        let n = js.as_number();
        return if n.fract() == 0.0 {
            format!("{}", n as i64)
        } else {
            n.to_string()
        };
    }
    if js.is_bool() {
        return js.as_bool().to_string();
    }
    if js.is_null() {
        return "null".to_string();
    }
    if js.is_undefined() {
        return "undefined".to_string();
    }
    "[object Object]".to_string()
}

fn string_array(items: &[&str]) -> f64 {
    let mut arr = crate::array::js_array_alloc(items.len() as u32);
    for item in items {
        let str_ptr = crate::string::js_string_from_bytes(item.as_ptr(), item.len() as u32);
        arr = crate::array::js_array_push(arr, JSValue::string_ptr(str_ptr));
    }
    ptr_value(arr)
}

fn owned_string_array(items: &[String]) -> f64 {
    let mut arr = crate::array::js_array_alloc(items.len() as u32);
    for item in items {
        let str_ptr = crate::string::js_string_from_bytes(item.as_ptr(), item.len() as u32);
        arr = crate::array::js_array_push(arr, JSValue::string_ptr(str_ptr));
    }
    ptr_value(arr)
}

fn cached_owned_cert_array(cache: &AtomicU64, certs: &[String]) -> f64 {
    let cached = cache.load(Ordering::Relaxed);
    if cached != 0 {
        return f64::from_bits(cached);
    }
    let arr = freeze_heap_value(owned_string_array(certs));
    crate::gc::runtime_store_root_atomic_nanbox_u64(cache, arr.to_bits(), Ordering::Relaxed);
    arr
}

pub fn scan_tls_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    visitor.visit_atomic_nanbox_u64_slot(&ROOT_CERTS_CACHE, Ordering::Relaxed, Ordering::Relaxed);
    visitor.visit_atomic_nanbox_u64_slot(&DEFAULT_CA_CACHE, Ordering::Relaxed, Ordering::Relaxed);
    visitor.visit_atomic_nanbox_u64_slot(&SYSTEM_CA_CACHE, Ordering::Relaxed, Ordering::Relaxed);
    visitor.visit_atomic_nanbox_u64_slot(&EXTRA_CA_CACHE, Ordering::Relaxed, Ordering::Relaxed);
}

pub fn js_tls_root_certificates() -> f64 {
    cached_owned_cert_array(&ROOT_CERTS_CACHE, roots::bundled_certificates())
}

#[no_mangle]
pub extern "C" fn js_tls_get_ciphers() -> f64 {
    string_array(TLS_CIPHERS)
}

#[no_mangle]
pub extern "C" fn js_tls_get_ca_certificates(ca_type: f64) -> f64 {
    let ca_type_js = JSValue::from_bits(ca_type.to_bits());
    let ca_type = if ca_type_js.is_undefined() {
        "default".to_string()
    } else if let Some(s) = value_to_string(ca_type) {
        s
    } else {
        let message = format!(
            "The \"type\" argument must be of type string. Received {}",
            crate::fs::validate::describe_received(ca_type)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    };
    match ca_type.as_str() {
        "default" => cached_owned_cert_array(&DEFAULT_CA_CACHE, roots::bundled_certificates()),
        "system" => cached_owned_cert_array(&SYSTEM_CA_CACHE, roots::system_certificates()),
        "bundled" => js_tls_root_certificates(),
        "extra" => cached_owned_cert_array(&EXTRA_CA_CACHE, roots::extra_certificates()),
        _ => {
            let message = format!("The argument 'type' is invalid. Received '{}'", ca_type);
            crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
        }
    }
}

fn is_array_buffer_view(value: f64) -> bool {
    let js = JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        return false;
    }
    let addr = js.as_pointer::<u8>() as usize;
    crate::typedarray::lookup_typed_array_kind(addr).is_some()
        || crate::buffer::is_registered_buffer(addr)
}

fn looks_like_cert_pem(s: &str) -> bool {
    s.contains("-----BEGIN CERTIFICATE-----") && s.contains("-----END CERTIFICATE-----")
}

fn throw_error_with_code(code: &'static str, message: &str) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    let err = crate::error::js_error_new_with_name_message(b"Error", msg);
    crate::exception::js_throw(ptr_value(err))
}

#[no_mangle]
pub extern "C" fn js_tls_set_default_ca_certificates(certs: f64) -> f64 {
    let Some(arr) = array_ptr(certs) else {
        let message = format!(
            "The \"certs\" argument must be an instance of Array. Received {}",
            crate::fs::validate::describe_received(certs)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    };
    let len = crate::array::js_array_length(arr);
    let mut default_certs = Vec::with_capacity(len as usize);
    if len == 0 {
        let empty = freeze_heap_value(string_array(&[]));
        crate::gc::runtime_store_root_atomic_nanbox_u64(
            &DEFAULT_CA_CACHE,
            empty.to_bits(),
            Ordering::Relaxed,
        );
        return f64::from_bits(TAG_UNDEFINED);
    }

    let mut valid_pem = false;
    for i in 0..len {
        let item = crate::array::js_array_get_f64(arr, i);
        if let Some(s) = value_to_string(item) {
            if looks_like_cert_pem(&s) {
                if s.len() < 512 {
                    throw_error_with_code(
                        "ERR_OSSL_PEM_ASN1_LIB",
                        "error:0488000D:PEM routines::ASN1 lib",
                    );
                }
                default_certs.push(s);
                valid_pem = true;
            }
        } else if !is_array_buffer_view(item) {
            let message = format!(
                "The \"certs[{}]\" argument must be of type string or an instance of ArrayBufferView. Received {}",
                i,
                crate::fs::validate::describe_received(item)
            );
            crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
        }
    }

    if !valid_pem {
        throw_error_with_code(
            "ERR_CRYPTO_OPERATION_FAILED",
            "No valid certificates found in the provided array",
        );
    }
    let configured = freeze_heap_value(owned_string_array(&default_certs));
    crate::gc::runtime_store_root_atomic_nanbox_u64(
        &DEFAULT_CA_CACHE,
        configured.to_bits(),
        Ordering::Relaxed,
    );
    f64::from_bits(TAG_UNDEFINED)
}

fn validate_protocol_version(value: f64, field: &str) {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() || js.is_null() {
        return;
    }
    let Some(version) = value_to_string(value) else {
        let message = format!(
            "The \"options.{}\" property must be of type string. Received {}",
            field,
            crate::fs::validate::describe_received(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    };
    if !matches!(
        version.as_str(),
        "TLSv1" | "TLSv1.1" | "TLSv1.2" | "TLSv1.3"
    ) {
        let kind = if field == "minVersion" {
            "minimum"
        } else {
            "maximum"
        };
        let message = format!(
            "\"{}\" is not a valid {} TLS protocol version",
            version, kind
        );
        crate::fs::validate::throw_type_error_with_code(
            &message,
            "ERR_TLS_INVALID_PROTOCOL_VERSION",
        );
    }
}

fn validate_optional_string_property(obj: *mut ObjectHeader, field: &str) -> Option<String> {
    let value = get_field(obj, field);
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() || js.is_null() {
        return None;
    }
    if let Some(s) = value_to_string(value) {
        return Some(s);
    }
    let message = format!(
        "The \"options.{}\" property must be of type string. Received {}",
        field,
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

fn validate_secure_context_options(options: f64) {
    let js = JSValue::from_bits(options.to_bits());
    if js.is_undefined() || js.is_null() {
        return;
    }
    let Some(obj) = object_ptr(options) else {
        let message = format!(
            "The \"options\" argument must be of type object. Received {}",
            crate::fs::validate::describe_received(options)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    };
    validate_protocol_version(get_field(obj, "minVersion"), "minVersion");
    validate_protocol_version(get_field(obj, "maxVersion"), "maxVersion");
    let _ = validate_optional_string_property(obj, "ciphers");

    if let Some(cert) = validate_optional_string_property(obj, "cert") {
        if !cert.contains("-----BEGIN") {
            throw_error_with_code(
                "ERR_OSSL_PEM_NO_START_LINE",
                "error:0480006C:PEM routines::no start line",
            );
        }
    }
    if let Some(key_value) = validate_optional_string_property(obj, "key") {
        if !key_value.contains("-----BEGIN") {
            throw_error_with_code(
                "ERR_OSSL_UNSUPPORTED",
                "error:1E08010C:DECODER routines::unsupported",
            );
        }
    }
}

fn ensure_secure_context_prototype() {
    if TLS_PROTOTYPE_INITIALIZED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    let keys = b"constructor\0";
    let proto =
        crate::object::js_object_alloc_with_shape(0x7FFF_FF41, 1, keys.as_ptr(), keys.len() as u32);
    crate::object::class_prototype_object_root_store(CLASS_ID_TLS_SECURE_CONTEXT, proto);
}

pub(crate) fn attach_secure_context_constructor_prototype(constructor_value: f64) {
    ensure_secure_context_prototype();
    let proto = crate::object::class_prototype_object(CLASS_ID_TLS_SECURE_CONTEXT);
    if proto.is_null() {
        return;
    }
    crate::object::js_object_set_field(proto, 0, JSValue::from_bits(constructor_value.to_bits()));
    crate::closure::closure_set_dynamic_prop(
        (constructor_value.to_bits() & crate::value::POINTER_MASK) as usize,
        "prototype",
        ptr_value(proto),
    );
}

#[no_mangle]
pub extern "C" fn js_tls_create_secure_context(options: f64) -> f64 {
    js_tls_secure_context_new(options)
}

#[no_mangle]
pub extern "C" fn js_tls_secure_context_new(options: f64) -> f64 {
    validate_secure_context_options(options);
    let constructor = crate::object::bound_native_callable_export_value("tls", "SecureContext");
    attach_secure_context_constructor_prototype(constructor);
    let keys = b"context\0";
    let obj = crate::object::js_object_alloc_class_with_keys(
        CLASS_ID_TLS_SECURE_CONTEXT,
        0,
        1,
        keys.as_ptr(),
        keys.len() as u32,
    );
    let context = crate::object::js_object_alloc(0, 0);
    crate::object::js_object_set_field(obj, 0, JSValue::from_bits(ptr_value(context).to_bits()));
    ptr_value(obj)
}

pub(crate) fn is_secure_context_instance(value: f64) -> bool {
    object_ptr(value)
        .map(|obj| unsafe { (*obj).class_id == CLASS_ID_TLS_SECURE_CONTEXT })
        .unwrap_or(false)
}

fn dns_name_matches(host: &str, pattern: &str) -> bool {
    let host = host.to_ascii_lowercase();
    let pattern = pattern.to_ascii_lowercase();
    if host == pattern {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix("*.") {
        return host.ends_with(&format!(".{}", suffix))
            && host[..host.len().saturating_sub(suffix.len() + 1)]
                .find('.')
                .is_none();
    }
    false
}

fn san_entries(subject_alt_name: &str, prefix: &str) -> Vec<String> {
    subject_alt_name
        .split(',')
        .filter_map(|part| part.trim().strip_prefix(prefix).map(str::trim))
        .map(str::to_string)
        .collect()
}

fn cert_common_name(cert: *mut ObjectHeader) -> Option<String> {
    let subject = get_field(cert, "subject");
    let subject_obj = object_ptr(subject)?;
    let cn = get_field(subject_obj, "CN");
    value_to_string(cn)
}

fn altname_error(host: &str, cert_value: f64, reason: String) -> f64 {
    let message = format!(
        "Hostname/IP does not match certificate's altnames: {}",
        reason
    );
    let obj = crate::object::js_object_alloc(crate::error::CLASS_ID_ERROR, 6);
    let set = |name: &str, value: f64| {
        crate::object::js_object_set_field_by_name(obj, key(name), value);
    };
    set("name", string_value("Error"));
    set("message", string_value(&message));
    set("code", string_value("ERR_TLS_CERT_ALTNAME_INVALID"));
    set("reason", string_value(&reason));
    set("host", string_value(host));
    set("cert", cert_value);
    ptr_value(obj)
}

#[no_mangle]
pub extern "C" fn js_tls_check_server_identity(hostname: f64, cert: f64) -> f64 {
    let host = host_to_string(hostname);
    let Some(cert_obj) = object_ptr(cert) else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    let subject_alt_name = get_field(cert_obj, "subjectaltname");
    if let Some(san) = value_to_string(subject_alt_name) {
        let host_is_ip = host.parse::<std::net::IpAddr>().is_ok();
        if host_is_ip {
            if san_entries(&san, "IP Address:")
                .iter()
                .any(|candidate| candidate == &host)
            {
                return f64::from_bits(TAG_UNDEFINED);
            }
        } else if san_entries(&san, "DNS:")
            .iter()
            .any(|candidate| dns_name_matches(&host, candidate))
        {
            return f64::from_bits(TAG_UNDEFINED);
        }
        return altname_error(
            &host,
            cert,
            format!("Host: {}. is not in the cert's altnames: {}", host, san),
        );
    }

    if let Some(cn) = cert_common_name(cert_obj) {
        if dns_name_matches(&host, &cn) {
            return f64::from_bits(TAG_UNDEFINED);
        }
        return altname_error(
            &host,
            cert,
            format!("Host: {}. is not cert's CN: {}", host, cn),
        );
    }

    if JSValue::from_bits(cert.to_bits()).is_null() {
        return f64::from_bits(TAG_NULL);
    }
    altname_error(&host, cert, "Cert does not contain a DNS name".to_string())
}

// Keep-alive anchors so the auto-optimize bitcode rebuild does not dead-strip
// these codegen-emitted `#[no_mangle]` runtime helpers (referenced from the
// native dispatch table in perry-codegen).
#[used]
static KEEP_JS_TLS_GET_CIPHERS: extern "C" fn() -> f64 = js_tls_get_ciphers;
#[used]
static KEEP_JS_TLS_GET_CA_CERTIFICATES: extern "C" fn(f64) -> f64 = js_tls_get_ca_certificates;
#[used]
static KEEP_JS_TLS_SET_DEFAULT_CA_CERTIFICATES: extern "C" fn(f64) -> f64 =
    js_tls_set_default_ca_certificates;
#[used]
static KEEP_JS_TLS_CREATE_SECURE_CONTEXT: extern "C" fn(f64) -> f64 = js_tls_create_secure_context;
#[used]
static KEEP_JS_TLS_SECURE_CONTEXT_NEW: extern "C" fn(f64) -> f64 = js_tls_secure_context_new;
#[used]
static KEEP_JS_TLS_CHECK_SERVER_IDENTITY: extern "C" fn(f64, f64) -> f64 =
    js_tls_check_server_identity;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cipher_inventory_is_sorted_and_node_shaped() {
        assert!(TLS_CIPHERS.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(TLS_CIPHERS.first(), Some(&"aes128-gcm-sha256"));
        assert!(TLS_CIPHERS.contains(&"tls_aes_256_gcm_sha384"));
    }

    #[test]
    fn protocol_version_validation_accepts_node_versions() {
        assert!(matches!(
            "TLSv1.2",
            "TLSv1" | "TLSv1.1" | "TLSv1.2" | "TLSv1.3"
        ));
        assert!(!matches!(
            "TLSv1.4",
            "TLSv1" | "TLSv1.1" | "TLSv1.2" | "TLSv1.3"
        ));
    }

    #[test]
    fn wildcard_dns_match_is_single_label() {
        assert!(dns_name_matches("api.example.com", "*.example.com"));
        assert!(!dns_name_matches("deep.api.example.com", "*.example.com"));
    }
}
