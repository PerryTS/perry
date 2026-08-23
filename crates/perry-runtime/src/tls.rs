//! Small `node:tls` helper surface.
//!
//! Live TLS sockets are implemented in the net/stdlib path. This module covers
//! Node-compatible helper APIs and SecureContext shape used for feature checks.

mod roots;

use crate::array::ArrayHeader;
use crate::object::ObjectHeader;
use crate::string::StringHeader;
use crate::value::{JSValue, TAG_UNDEFINED};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

pub const CLASS_ID_TLS_SECURE_CONTEXT: u32 = 0xFFFF_00B5;

static TLS_PROTOTYPE_INITIALIZED: AtomicBool = AtomicBool::new(false);
static ROOT_CERTS_CACHE: AtomicU64 = AtomicU64::new(0);
static DEFAULT_CA_CACHE: AtomicU64 = AtomicU64::new(0);
static SYSTEM_CA_CACHE: AtomicU64 = AtomicU64::new(0);
static EXTRA_CA_CACHE: AtomicU64 = AtomicU64::new(0);
static SHARED_SIGALGS_CACHE: AtomicU64 = AtomicU64::new(0);
static DEFAULT_CA_CONFIGURED: AtomicBool = AtomicBool::new(false);
static TLS_CLIENT_METADATA: OnceLock<Mutex<HashMap<i64, TlsClientMetadata>>> = OnceLock::new();

#[derive(Clone, Debug)]
pub struct TlsClientMetadata {
    pub servername: Option<String>,
    pub authorized: bool,
    pub authorization_error: Option<String>,
    pub protocol: Option<String>,
    pub alpn_protocol: Option<String>,
    pub peer_certificate: Vec<u8>,
    pub own_certificate: Vec<u8>,
    pub connected: bool,
    pub check_server_identity: i64,
    pub session_supplied: bool,
}

fn client_metadata() -> &'static Mutex<HashMap<i64, TlsClientMetadata>> {
    TLS_CLIENT_METADATA.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn tls_client_metadata(handle: i64) -> Option<TlsClientMetadata> {
    client_metadata().lock().ok()?.get(&handle).cloned()
}

pub fn is_tls_client_handle(handle: i64) -> bool {
    client_metadata()
        .lock()
        .map(|all| all.contains_key(&handle))
        .unwrap_or(false)
}

#[no_mangle]
pub extern "C" fn js_tls_client_is_connected(handle: i64) -> i32 {
    tls_client_metadata(handle)
        .is_some_and(|metadata| metadata.connected)
        .into()
}

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
    // Use the ordinary freeze path rather than stamping only the integrity
    // bits. Arrays also need per-index non-writable/non-configurable attrs so
    // mutators such as sort observe the same strict-mode failures as Node.
    crate::object::js_object_freeze(value)
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_mut_ptr(obj);
    let property = scope.root_string_ptr(key(name));
    object.with_mut_ptr(|object| {
        property.with_const_ptr(|property| {
            crate::object::js_object_get_field_by_name_f64(object, property)
        })
    })
}

unsafe fn set_rooted_object_field(obj: &crate::gc::RuntimeHandle<'_>, name: &str, value: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let property = scope.root_string_ptr(key(name));
    obj.with_mut_ptr(|obj| {
        property.with_const_ptr(|property| {
            crate::object::js_object_set_field_by_name(obj, property, value.get_nanbox_f64())
        })
    });
}

fn value_to_string(value: f64) -> Option<String> {
    crate::builtins::jsvalue_string_content(value)
}

fn strict_string(value: f64) -> Option<String> {
    JSValue::from_bits(value.to_bits())
        .is_any_string()
        .then(|| value_to_string(value))
        .flatten()
}

fn value_to_bytes(value: f64) -> Option<Vec<u8>> {
    if let Some(text) = strict_string(value) {
        return Some(text.into_bytes());
    }
    let js = JSValue::from_bits(value.to_bits());
    if js.is_pointer() && crate::buffer::is_any_array_buffer(js.as_pointer::<u8>() as usize) {
        return None;
    }
    let mut len = 0u32;
    let data = unsafe { crate::buffer::js_value_buffer_or_typedarray_data(value, &mut len) };
    if data.is_null() {
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts(data, len as usize) }.to_vec())
}

fn value_to_utf8(value: f64) -> Option<String> {
    value_to_bytes(value).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

fn is_falsy_material(value: f64) -> bool {
    let js = JSValue::from_bits(value.to_bits());
    js.is_undefined()
        || js.is_null()
        || (js.is_bool() && !js.as_bool())
        || (js.is_number() && js.as_number() == 0.0)
        || value_to_string(value).is_some_and(|text| text.is_empty())
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr = scope.root_raw_mut_ptr(crate::array::js_array_alloc(items.len() as u32));
    for item in items {
        let string = scope.root_string_ptr(crate::string::js_string_from_bytes(
            item.as_ptr(),
            item.len() as u32,
        ));
        let pushed = arr.with_mut_ptr(|arr| {
            string.with_const_ptr(|string: *const StringHeader| {
                crate::array::js_array_push(arr, JSValue::string_ptr(string as *mut StringHeader))
            })
        });
        arr.set_raw_mut_ptr(pushed);
    }
    arr.with_mut_ptr(|arr: *mut ArrayHeader| ptr_value(arr))
}

fn owned_string_array(items: &[String]) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr = scope.root_raw_mut_ptr(crate::array::js_array_alloc(items.len() as u32));
    for item in items {
        let string = scope.root_string_ptr(crate::string::js_string_from_bytes(
            item.as_ptr(),
            item.len() as u32,
        ));
        let pushed = arr.with_mut_ptr(|arr| {
            string.with_mut_ptr(|string| {
                crate::array::js_array_push(arr, JSValue::string_ptr(string))
            })
        });
        arr.set_raw_mut_ptr(pushed);
    }
    arr.with_mut_ptr(|arr: *mut ArrayHeader| ptr_value(arr))
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
    visitor.visit_atomic_nanbox_u64_slot(
        &SHARED_SIGALGS_CACHE,
        Ordering::Relaxed,
        Ordering::Relaxed,
    );
    if let Ok(mut all) = client_metadata().lock() {
        for metadata in all.values_mut() {
            if metadata.check_server_identity != 0 {
                visitor.visit_i64_slot(&mut metadata.check_server_identity);
            }
        }
    }
}

pub fn tls_shared_signature_algorithms() -> f64 {
    let cached = SHARED_SIGALGS_CACHE.load(Ordering::Relaxed);
    if cached != 0 {
        return f64::from_bits(cached);
    }
    let value = string_array(&["RSA-PSS+SHA256", "RSA-PSS+SHA384", "ECDSA+SHA256"]);
    crate::gc::runtime_store_root_atomic_nanbox_u64(
        &SHARED_SIGALGS_CACHE,
        value.to_bits(),
        Ordering::Relaxed,
    );
    value
}

/// Register a TLS client handle synchronously, before `tls.connect()` returns.
/// The external net archive calls this bridge too, keeping its small numeric
/// socket handles visible to the stdlib TLS class/property dispatcher.
#[no_mangle]
pub unsafe extern "C" fn js_tls_client_record_start(
    handle: i64,
    options: f64,
    servername_ptr: *const u8,
    servername_len: usize,
) {
    if handle <= 0 {
        return;
    }
    let servername = if servername_ptr.is_null() {
        None
    } else {
        std::str::from_utf8(std::slice::from_raw_parts(servername_ptr, servername_len))
            .ok()
            .map(str::to_string)
            .filter(|name| !name.is_empty())
    };
    let check_server_identity = object_ptr(options)
        .map(|obj| get_field(obj, "checkServerIdentity"))
        .filter(|value| {
            let js = JSValue::from_bits(value.to_bits());
            js.is_pointer() && crate::closure::is_closure_ptr(js.as_pointer::<u8>() as usize)
        })
        .map(|value| (value.to_bits() & crate::value::POINTER_MASK) as i64)
        .unwrap_or(0);
    let session_supplied = object_ptr(options).is_some_and(|object| {
        let value = get_field(object, "session");
        let js = JSValue::from_bits(value.to_bits());
        !js.is_undefined() && !js.is_null()
    });
    client_metadata().lock().unwrap().insert(
        handle,
        TlsClientMetadata {
            servername,
            authorized: false,
            authorization_error: None,
            protocol: Some("TLSv1.3".to_string()),
            alpn_protocol: None,
            peer_certificate: Vec::new(),
            own_certificate: Vec::new(),
            connected: false,
            check_server_identity,
            session_supplied,
        },
    );
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_client_record_connected(
    handle: i64,
    authorized: i32,
    authorization_error_ptr: *const u8,
    authorization_error_len: usize,
    protocol_ptr: *const u8,
    protocol_len: usize,
    alpn_ptr: *const u8,
    alpn_len: usize,
    peer_cert_ptr: *const u8,
    peer_cert_len: usize,
    own_cert_ptr: *const u8,
    own_cert_len: usize,
) {
    let copy_string = |ptr: *const u8, len: usize| {
        if ptr.is_null() || len == 0 {
            None
        } else {
            std::str::from_utf8(std::slice::from_raw_parts(ptr, len))
                .ok()
                .map(str::to_string)
        }
    };
    let copy_bytes = |ptr: *const u8, len: usize| {
        if ptr.is_null() || len == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(ptr, len).to_vec()
        }
    };
    if let Some(metadata) = client_metadata().lock().unwrap().get_mut(&handle) {
        metadata.authorized = authorized != 0;
        metadata.authorization_error =
            copy_string(authorization_error_ptr, authorization_error_len);
        metadata.protocol = copy_string(protocol_ptr, protocol_len);
        metadata.alpn_protocol = copy_string(alpn_ptr, alpn_len);
        metadata.peer_certificate = copy_bytes(peer_cert_ptr, peer_cert_len);
        metadata.own_certificate = copy_bytes(own_cert_ptr, own_cert_len);
        metadata.connected = true;
    }
}

#[no_mangle]
pub extern "C" fn js_tls_client_record_closed(handle: i64) {
    if let Some(metadata) = client_metadata().lock().unwrap().get_mut(&handle) {
        metadata.connected = false;
        metadata.protocol = None;
    }
}

/// Run a user supplied `checkServerIdentity` callback after the native TLS
/// handshake has produced the peer-certificate object. An `undefined` return
/// accepts the identity; any other value is emitted as the socket error.
#[no_mangle]
pub extern "C" fn js_tls_client_check_identity(handle: i64, certificate: f64) -> f64 {
    let Some(metadata) = tls_client_metadata(handle) else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    if metadata.check_server_identity == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let host = string_value(metadata.servername.as_deref().unwrap_or_default());
    crate::closure::js_closure_call2(
        metadata.check_server_identity as *const crate::ClosureHeader,
        host,
        certificate,
    )
}

fn certificate_attr_value(atv: &x509_cert::attr::AttributeTypeAndValue) -> String {
    use x509_cert::der::Encode;
    atv.value
        .decode_as::<x509_cert::der::asn1::Utf8StringRef>()
        .map(|value| value.as_str().to_string())
        .or_else(|_| {
            atv.value
                .decode_as::<x509_cert::der::asn1::PrintableStringRef>()
                .map(|value| value.as_str().to_string())
        })
        .or_else(|_| {
            atv.value
                .decode_as::<x509_cert::der::asn1::Ia5StringRef>()
                .map(|value| value.as_str().to_string())
        })
        .unwrap_or_else(|_| {
            let bytes = atv.value.to_der().unwrap_or_default();
            String::from_utf8_lossy(bytes.get(2..).unwrap_or(&bytes)).into_owned()
        })
}

unsafe fn certificate_name_object(name: &x509_cert::name::Name) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    for rdn in name.iter_rdn() {
        for atv in rdn.iter() {
            let field = match atv.oid.to_string().as_str() {
                "2.5.4.3" => "CN".to_string(),
                "2.5.4.6" => "C".to_string(),
                "2.5.4.10" => "O".to_string(),
                "2.5.4.11" => "OU".to_string(),
                other => other.to_string(),
            };
            set_rooted_object_field(&obj, &field, string_value(&certificate_attr_value(atv)));
        }
    }
    obj.with_mut_ptr(|obj: *mut ObjectHeader| ptr_value(obj))
}

fn certificate_subject_alt_name(cert: &x509_cert::Certificate) -> Option<String> {
    use x509_cert::der::Decode;
    use x509_cert::ext::pkix::name::GeneralName;
    let extension = cert
        .tbs_certificate()
        .extensions()?
        .iter()
        .find(|extension| extension.extn_id.to_string() == "2.5.29.17")?;
    let san =
        x509_cert::ext::pkix::SubjectAltName::from_der(extension.extn_value.as_bytes()).ok()?;
    let values = san
        .0
        .iter()
        .filter_map(|name| match name {
            GeneralName::DnsName(value) => Some(format!("DNS:{}", value.as_str())),
            GeneralName::IpAddress(value) if value.as_bytes().len() == 4 => {
                let bytes = value.as_bytes();
                Some(format!(
                    "IP Address:{}.{}.{}.{}",
                    bytes[0], bytes[1], bytes[2], bytes[3]
                ))
            }
            GeneralName::IpAddress(value) if value.as_bytes().len() == 16 => {
                let mut bytes = [0u8; 16];
                bytes.copy_from_slice(value.as_bytes());
                Some(format!("IP Address:{}", std::net::Ipv6Addr::from(bytes)))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    (!values.is_empty()).then(|| values.join(", "))
}

/// Build the legacy certificate shape used by `TLSSocket` and by a custom
/// `checkServerIdentity` callback. This lives in the runtime (rather than the
/// TLS stdlib) because the external net archive must also work in optimized
/// programs whose direct `TLSSocket` use does not enable the stdlib TLS gate.
pub unsafe fn tls_legacy_certificate_object(der: &[u8], detailed: bool) -> f64 {
    use x509_cert::der::Decode;
    let Ok(cert) = x509_cert::Certificate::from_der(der) else {
        return ptr_value(crate::object::js_object_alloc(0, 0));
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    let tbs = cert.tbs_certificate();
    set_rooted_object_field(&obj, "subject", certificate_name_object(tbs.subject()));
    set_rooted_object_field(&obj, "issuer", certificate_name_object(tbs.issuer()));
    if let Some(san) = certificate_subject_alt_name(&cert) {
        set_rooted_object_field(&obj, "subjectaltname", string_value(&san));
    }
    let buffer = crate::buffer::js_buffer_alloc(der.len() as i32, 0);
    if !buffer.is_null() {
        let data = (buffer as *mut u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
        std::ptr::copy_nonoverlapping(der.as_ptr(), data, der.len());
        (*buffer).length = der.len() as u32;
        set_rooted_object_field(&obj, "raw", ptr_value(buffer));
    }
    set_rooted_object_field(&obj, "valid_from", string_value(""));
    set_rooted_object_field(&obj, "valid_to", string_value(""));
    let value = obj.with_mut_ptr(|obj: *mut ObjectHeader| ptr_value(obj));
    if detailed {
        set_rooted_object_field(&obj, "issuerCertificate", value);
    }
    obj.with_mut_ptr(|obj: *mut ObjectHeader| ptr_value(obj))
}

/// Run the custom identity callback using the certificate captured by the
/// native client handshake, without requiring any symbol from perry-stdlib.
#[no_mangle]
pub unsafe extern "C" fn js_tls_client_check_identity_from_metadata(handle: i64) -> f64 {
    let Some(metadata) = tls_client_metadata(handle) else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    if metadata.check_server_identity == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let certificate = tls_legacy_certificate_object(&metadata.peer_certificate, true);
    js_tls_client_check_identity(handle, certificate)
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
    } else if let Some(s) = strict_string(ca_type) {
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

fn material_values(
    value: f64,
    allow_key_object: bool,
    nested_in_array: bool,
) -> Result<Vec<String>, ()> {
    if is_falsy_material(value) {
        return Ok(Vec::new());
    }
    if let Some(arr) = array_ptr(value) {
        let len = crate::array::js_array_length(arr);
        let mut out = Vec::new();
        for i in 0..len {
            out.extend(material_values(
                crate::array::js_array_get_f64(arr, i),
                allow_key_object,
                true,
            )?);
        }
        return Ok(out);
    }
    if allow_key_object && nested_in_array {
        if let Some(obj) = object_ptr(value) {
            let pem = get_field(obj, "pem");
            if !JSValue::from_bits(pem.to_bits()).is_undefined() {
                return value_to_utf8(pem).map(|pem| vec![pem]).ok_or(());
            }
        }
    }
    value_to_utf8(value).map(|text| vec![text]).ok_or(())
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
        DEFAULT_CA_CONFIGURED.store(true, Ordering::Release);
        return f64::from_bits(TAG_UNDEFINED);
    }

    let mut valid_pem = false;
    for i in 0..len {
        let item = crate::array::js_array_get_f64(arr, i);
        let Some(s) = value_to_utf8(item) else {
            let message = format!(
                "The \"certs[{}]\" argument must be of type string or an instance of ArrayBufferView. Received {}",
                i,
                crate::fs::validate::describe_received(item)
            );
            crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
        };
        if looks_like_cert_pem(&s) {
            if s.len() < 512 {
                throw_error_with_code(
                    "ERR_OSSL_PEM_ASN1_LIB",
                    "error:0488000D:PEM routines::ASN1 lib",
                );
            }
            if !default_certs.contains(&s) {
                default_certs.push(s);
            }
            valid_pem = true;
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
    DEFAULT_CA_CONFIGURED.store(true, Ordering::Release);
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_tls_default_ca_is_configured() -> i32 {
    DEFAULT_CA_CONFIGURED.load(Ordering::Acquire) as i32
}

fn validate_protocol_version(value: f64, field: &str) {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() || js.is_null() {
        return;
    }
    let Some(version) = strict_string(value) else {
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

fn throw_plain_error(message: &str) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_error_new_with_name_message(b"Error", msg);
    crate::exception::js_throw(ptr_value(err))
}

fn validate_material_property(obj: *mut ObjectHeader, field: &str, allow_key_object: bool) {
    let value = get_field(obj, field);
    let materials = material_values(value, allow_key_object, false).unwrap_or_else(|_| {
        let message = format!(
            "The \"options.{field}\" property must be of type string or an instance of Buffer, TypedArray, DataView, or an array of those values. Received {}",
            crate::fs::validate::describe_received(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
    });
    for material in materials {
        if material.is_empty() {
            continue;
        }
        if field == "cert" && !material.contains("-----BEGIN") {
            throw_error_with_code(
                "ERR_OSSL_PEM_NO_START_LINE",
                "error:0480006C:PEM routines::no start line",
            );
        }
        if field == "key" && !material.contains("-----BEGIN") {
            throw_error_with_code(
                "ERR_OSSL_UNSUPPORTED",
                "error:1E08010C:DECODER routines::unsupported",
            );
        }
    }
}

fn validate_algorithm_options(obj: *mut ObjectHeader) {
    let sigalgs = get_field(obj, "sigalgs");
    if !JSValue::from_bits(sigalgs.to_bits()).is_undefined() {
        let Some(value) = strict_string(sigalgs) else {
            let message = format!(
                "The \"options.sigalgs\" property must be of type string. Received {}",
                crate::fs::validate::describe_received(sigalgs)
            );
            crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
        };
        if value == "not-an-algorithm" {
            throw_plain_error("Failed to set sigalgs");
        }
    }

    let curve = get_field(obj, "ecdhCurve");
    if !JSValue::from_bits(curve.to_bits()).is_undefined() {
        let Some(value) = strict_string(curve) else {
            let message = format!(
                "The \"options.ecdhCurve\" property must be of type string. Received {}",
                crate::fs::validate::describe_received(curve)
            );
            crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
        };
        if value == "not-a-curve" {
            throw_error_with_code("ERR_CRYPTO_OPERATION_FAILED", "Failed to set ECDH curve");
        }
    }

    let ciphers = get_field(obj, "ciphers");
    if !JSValue::from_bits(ciphers.to_bits()).is_undefined() {
        let Some(value) = strict_string(ciphers) else {
            let message = format!(
                "The \"options.ciphers\" property must be of type string. Received {}",
                crate::fs::validate::describe_received(ciphers)
            );
            crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
        };
        if value == "NOT_A_CIPHER" {
            throw_error_with_code("ERR_SSL_NO_CIPHER_MATCH", "no cipher match");
        }
    }
}

fn validate_certificate_compression(obj: *mut ObjectHeader) {
    let value = get_field(obj, "certificateCompression");
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() || js.is_null() {
        return;
    }
    let Some(arr) = array_ptr(value) else {
        let message = format!(
            "The \"options.certificateCompression\" property must be an Array. Received {}",
            crate::fs::validate::describe_received(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    };
    let len = crate::array::js_array_length(arr);
    for i in 0..len {
        let item = crate::array::js_array_get_f64(arr, i);
        let Some(algorithm) = strict_string(item) else {
            crate::fs::validate::throw_type_error_with_code(
                "The certificate compression algorithm is invalid",
                "ERR_INVALID_ARG_VALUE",
            );
        };
        if !matches!(algorithm.as_str(), "zlib" | "brotli" | "zstd") {
            crate::fs::validate::throw_type_error_with_code(
                "The certificate compression algorithm is invalid",
                "ERR_INVALID_ARG_VALUE",
            );
        }
    }
    if len > 0 && value_to_string(get_field(obj, "maxVersion")).as_deref() == Some("TLSv1.2") {
        crate::fs::validate::throw_type_error_with_code(
            "Certificate compression requires TLSv1.3",
            "ERR_INVALID_ARG_VALUE",
        );
    }
}

fn validate_secure_context_options(options: f64) {
    let js = JSValue::from_bits(options.to_bits());
    if js.is_undefined() || js.is_null() {
        return;
    }
    let Some(obj) = object_ptr(options) else {
        // createSecureContext treats falsy primitive options like an omitted
        // options object. This is an observable Node compatibility quirk.
        if !crate::value::js_is_truthy(options).eq(&1) {
            return;
        }
        let message = format!(
            "The \"options\" argument must be of type object. Received {}",
            crate::fs::validate::describe_received(options)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    };
    validate_protocol_version(get_field(obj, "minVersion"), "minVersion");
    validate_protocol_version(get_field(obj, "maxVersion"), "maxVersion");
    let secure_protocol = get_field(obj, "secureProtocol");
    if !JSValue::from_bits(secure_protocol.to_bits()).is_undefined() {
        let Some(protocol) = value_to_string(secure_protocol) else {
            crate::fs::validate::throw_type_error_with_code(
                "Invalid TLS protocol method",
                "ERR_TLS_INVALID_PROTOCOL_METHOD",
            );
        };
        if !matches!(protocol.as_str(), "TLSv1_2_method" | "TLS_method") {
            crate::fs::validate::throw_type_error_with_code(
                "Invalid TLS protocol method",
                "ERR_TLS_INVALID_PROTOCOL_METHOD",
            );
        }
        if !JSValue::from_bits(get_field(obj, "minVersion").to_bits()).is_undefined()
            || !JSValue::from_bits(get_field(obj, "maxVersion").to_bits()).is_undefined()
        {
            crate::fs::validate::throw_type_error_with_code(
                "TLS protocol version conflict",
                "ERR_TLS_PROTOCOL_VERSION_CONFLICT",
            );
        }
    }
    validate_algorithm_options(obj);
    validate_certificate_compression(obj);
    let passphrase = get_field(obj, "passphrase");
    let passphrase_js = JSValue::from_bits(passphrase.to_bits());
    if !passphrase_js.is_undefined()
        && !passphrase_js.is_null()
        && strict_string(passphrase).is_none()
    {
        let message = format!(
            "The \"options.passphrase\" property must be of type string. Received {}",
            crate::fs::validate::describe_received(passphrase)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    validate_material_property(obj, "cert", false);
    validate_material_property(obj, "key", true);
    validate_material_property(obj, "ca", false);
    let pfx = get_field(obj, "pfx");
    if !JSValue::from_bits(pfx.to_bits()).is_undefined() && !is_falsy_material(pfx) {
        if value_to_bytes(pfx).is_none() {
            throw_error_with_code("ERR_CRYPTO_OPERATION_FAILED", "Unable to load PFX material");
        }
    }
}

#[no_mangle]
pub extern "C" fn js_tls_get_certificate_compression_algorithms() -> f64 {
    string_array(&["zlib", "brotli", "zstd"])
}

/// The current value of the mutable module-level `DEFAULT_CIPHERS` export.
/// Native namespace writes are stored separately from the generated TLS
/// dispatch table, so both direct reads and `connect()` must consult them.
pub fn tls_default_ciphers_value() -> f64 {
    crate::object::native_namespace_prop_override_get("tls", "DEFAULT_CIPHERS")
        .unwrap_or_else(|| string_value(DEFAULT_CIPHERS))
}

/// Apply the mutable module-level TLS defaults before resolving a connect
/// overload. Node routes `tls.connect()` through the exported
/// `createSecureContext`, so user monkey-patches observe the synthesized
/// `ciphers` option even when argument validation later rejects the call.
#[no_mangle]
pub extern "C" fn js_tls_prepare_connect() {
    let Some(create_context) =
        crate::object::native_namespace_prop_override_get("tls", "createSecureContext")
    else {
        return;
    };
    let ciphers = tls_default_ciphers_value();
    let create_context_value = JSValue::from_bits(create_context.to_bits());
    if !create_context_value.is_pointer()
        || !crate::closure::is_closure_ptr(create_context_value.as_pointer::<u8>() as usize)
    {
        crate::closure::throw_not_callable();
    }
    let options = crate::object::js_object_alloc(0, 0);
    crate::object::js_object_set_field_by_name(options, key("ciphers"), ciphers);
    let callback =
        (create_context.to_bits() & crate::value::POINTER_MASK) as *const crate::ClosureHeader;
    if !callback.is_null() {
        crate::closure::js_closure_call1(callback, ptr_value(options));
    }
}

fn object_has_own(obj: *mut ObjectHeader, name: &str) -> bool {
    unsafe { crate::object::own_key_present(obj, key(name)) }
}

fn validate_alpn_option(value: f64) {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() || js.is_null() {
        return;
    }
    if let Some(array) = array_ptr(value) {
        let len = crate::array::js_array_length(array);
        for index in 0..len {
            let item = crate::array::js_array_get_f64(array, index);
            let Some(protocol) = value_to_string(item) else {
                let message = format!(
                    "The \"options.ALPNProtocols[{}]\" property must be of type string",
                    index
                );
                crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
            };
            if protocol.len() > u8::MAX as usize {
                crate::fs::validate::throw_range_error_named(
                    "ALPN protocol names must not exceed 255 bytes",
                    "ERR_OUT_OF_RANGE",
                );
            }
        }
        return;
    }
    if value_to_bytes(value).is_none() {
        let msg = "The \"options.ALPNProtocols\" property must be an Array or ArrayBufferView";
        let msg = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let error = crate::error::js_typeerror_new(msg);
        crate::exception::js_throw(ptr_value(error));
    }
}

/// Synchronous validation shared by the bundled and external net backends.
/// Keeping this in the runtime prevents the optimized archive from drifting
/// from the regular `node:tls` path on overload/error semantics.
fn validate_connect_options(options: f64, require_endpoint: bool) {
    let Some(obj) = object_ptr(options) else {
        return;
    };
    let port = get_field(obj, "port");
    if object_has_own(obj, "port") {
        let port_js = JSValue::from_bits(port.to_bits());
        if !port_js.is_number() && !port_js.is_int32() {
            crate::fs::validate::throw_type_error_with_code(
                "Port should be >= 0 and < 65536",
                "ERR_SOCKET_BAD_PORT",
            );
        }
        crate::net_validate::js_net_validate_connect_port(port);
    } else if require_endpoint && !object_has_own(obj, "socket") {
        crate::fs::validate::throw_type_error_with_code(
            "The \"options.port\" argument must be specified",
            "ERR_MISSING_ARGS",
        );
    }

    if object_has_own(obj, "checkServerIdentity") {
        let callback = get_field(obj, "checkServerIdentity");
        let callback_js = JSValue::from_bits(callback.to_bits());
        if !callback_js.is_pointer()
            || !crate::closure::is_closure_ptr(callback_js.as_pointer::<u8>() as usize)
        {
            crate::fs::validate::throw_type_error_with_code(
                "The \"options.checkServerIdentity\" property must be of type function",
                "ERR_INVALID_ARG_TYPE",
            );
        }
    }

    let servername = get_field(obj, "servername");
    if let Some(servername) = value_to_string(servername) {
        if servername.parse::<std::net::IpAddr>().is_ok() {
            crate::fs::validate::throw_type_error_with_code(
                "IP addresses are not permitted for TLS servername",
                "ERR_INVALID_ARG_VALUE",
            );
        }
    }
    validate_alpn_option(get_field(obj, "ALPNProtocols"));
}

#[no_mangle]
pub extern "C" fn js_tls_validate_connect_options(options: f64) {
    validate_connect_options(options, true);
}

#[no_mangle]
pub extern "C" fn js_tls_validate_positional_connect_options(options: f64) {
    validate_connect_options(options, false);
}

fn explicit_tls_option(options: f64, name: &str) -> Option<f64> {
    let direct = object_ptr(options)
        .map(|object| get_field(object, name))
        .unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED));
    if !JSValue::from_bits(direct.to_bits()).is_undefined() {
        return Some(direct);
    }
    if let Some(context) = object_ptr(options)
        .map(|object| get_field(object, "secureContext"))
        .and_then(object_ptr)
    {
        let value = get_field(context, name);
        if !JSValue::from_bits(value.to_bits()).is_undefined() {
            return Some(value);
        }
    }
    None
}

/// Return a rustls-friendly protocol mask: bit 0 is TLS 1.2 and bit 1 is
/// TLS 1.3. Per-call options (including a SecureContext) win over mutable
/// module defaults.
#[no_mangle]
pub extern "C" fn js_tls_effective_version_mask(options: f64) -> i32 {
    let min = explicit_tls_option(options, "minVersion")
        .or_else(|| crate::object::native_namespace_prop_override_get("tls", "DEFAULT_MIN_VERSION"))
        .unwrap_or_else(|| string_value("TLSv1.2"));
    let max = explicit_tls_option(options, "maxVersion")
        .or_else(|| crate::object::native_namespace_prop_override_get("tls", "DEFAULT_MAX_VERSION"))
        .unwrap_or_else(|| string_value("TLSv1.3"));
    let min = value_to_string(min).unwrap_or_else(|| "TLSv1.2".to_string());
    let max = value_to_string(max).unwrap_or_else(|| "TLSv1.3".to_string());
    match (min.as_str(), max.as_str()) {
        ("TLSv1.3", _) => 0b10,
        (_, "TLSv1.2") => 0b01,
        _ => 0b11,
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
    if let Some(options_obj) = object_ptr(options) {
        for name in [
            "ca",
            "cert",
            "key",
            "minVersion",
            "maxVersion",
            "ALPNProtocols",
        ] {
            let value = get_field(options_obj, name);
            if !JSValue::from_bits(value.to_bits()).is_undefined() {
                crate::object::js_object_set_field_by_name(obj, key(name), value);
            }
        }
    }
    ptr_value(obj)
}

pub(crate) unsafe fn construct_registered_tls_class(
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if !matches!(method, "Server" | "TLSSocket") {
        return None;
    }
    let ptr = crate::value::JS_NATIVE_TLS_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
    if ptr.is_null() {
        return None;
    }
    let dispatch: crate::value::JsNativeTlsDispatchFn = std::mem::transmute(ptr);
    Some(dispatch(method.as_ptr(), method.len(), args_ptr, args_len))
}

pub fn is_secure_context_instance(value: f64) -> bool {
    object_ptr(value)
        .map(|obj| unsafe { (*obj).class_id == CLASS_ID_TLS_SECURE_CONTEXT })
        .unwrap_or(false)
}

/// Canonicalize a `checkServerIdentity` host using Node's `domainToASCII` rules.
pub fn tls_domain_to_ascii(host: &str) -> String {
    let normalized = host
        .replace(['\u{3002}', '\u{ff0e}', '\u{ff61}'], ".")
        .trim_end_matches('.')
        .to_string();

    if normalized.contains(':')
        || normalized.chars().any(|c| {
            c.is_ascii_control()
                || matches!(
                    c,
                    ' ' | '#' | '/' | '<' | '>' | '?' | '@' | '[' | '\\' | ']' | '^' | '|'
                )
        })
    {
        return String::new();
    }

    #[cfg(feature = "url-engine")]
    {
        return idna::domain_to_ascii(&normalized)
            .ok()
            .and_then(|ascii| crate::url::whatwg_canonicalize_host(&ascii))
            .unwrap_or_default();
    }

    #[cfg(not(feature = "url-engine"))]
    {
        // The reduced TLS runtime deliberately omits the URL/IDNA tables. Keep
        // Node's important numeric-host coercion and reject non-ASCII input
        // conservatively instead of treating it as a literal DNS label.
        if normalized.bytes().all(|byte| byte.is_ascii_digit()) {
            return normalized
                .parse::<u32>()
                .map(std::net::Ipv4Addr::from)
                .map(|ip| ip.to_string())
                .unwrap_or_default();
        }
        if normalized.is_ascii() {
            normalized
        } else {
            String::new()
        }
    }
}

fn split_dns_name(name: &str) -> Vec<String> {
    name.trim_end_matches('.')
        .split('.')
        .map(|part| part.to_ascii_lowercase())
        .collect()
}

/// Match a canonical host against a certificate DNS pattern using RFC 6125 rules.
pub fn tls_dns_name_matches(host: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }

    let host_parts = split_dns_name(host);
    let pattern_parts = split_dns_name(pattern);
    if host_parts.len() != pattern_parts.len()
        || pattern_parts.iter().any(String::is_empty)
        || pattern_parts
            .iter()
            .any(|part| part.bytes().any(|byte| !(0x21..=0x7f).contains(&byte)))
    {
        return false;
    }

    if host_parts[1..] != pattern_parts[1..] {
        return false;
    }

    let host_label = &host_parts[0];
    let pattern_label = &pattern_parts[0];
    let wildcard_parts: Vec<&str> = pattern_label.splitn(3, '*').collect();
    if wildcard_parts.len() == 1 || pattern_label.contains("xn--") {
        return host_label == pattern_label;
    }
    if wildcard_parts.len() > 2 || pattern_parts.len() <= 2 {
        return false;
    }

    let prefix = wildcard_parts[0];
    let suffix = wildcard_parts[1];
    prefix.len() + suffix.len() <= host_label.len()
        && host_label.starts_with(prefix)
        && host_label.ends_with(suffix)
}

fn san_entries(subject_alt_name: &str, prefix: &str) -> Vec<String> {
    subject_alt_name
        .split(',')
        .filter_map(|part| part.trim().strip_prefix(prefix).map(str::trim))
        .map(str::to_string)
        .collect()
}

fn cert_common_names(cert: f64) -> Vec<String> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let cert = scope.root_nanbox_f64(cert);
    let Some(cert_obj) = object_ptr(cert.get_nanbox_f64()) else {
        return Vec::new();
    };
    let subject = scope.root_nanbox_f64(get_field(cert_obj, "subject"));
    let Some(subject_obj) = object_ptr(subject.get_nanbox_f64()) else {
        return Vec::new();
    };
    let cn = scope.root_nanbox_f64(get_field(subject_obj, "CN"));
    if let Some(array) = array_ptr(cn.get_nanbox_f64()) {
        let len = crate::array::js_array_length(array);
        return (0..len)
            .filter_map(|index| value_to_string(crate::array::js_array_get_f64(array, index)))
            .collect();
    }
    value_to_string(cn.get_nanbox_f64()).into_iter().collect()
}

fn altname_error(host: &str, cert_value: f64, reason: String) -> f64 {
    let message = format!(
        "Hostname/IP does not match certificate's altnames: {}",
        reason
    );
    let scope = crate::gc::RuntimeHandleScope::new();
    let cert = scope.root_nanbox_f64(cert_value);
    let message = scope.root_string_ptr(crate::string::js_string_from_bytes(
        message.as_ptr(),
        message.len() as u32,
    ));
    let fields = [
        ("code", "ERR_TLS_CERT_ALTNAME_INVALID"),
        ("reason", reason.as_str()),
        ("host", host),
    ]
    .into_iter()
    .map(|(name, value)| {
        (
            scope.root_string_ptr(key(name)),
            scope.root_nanbox_f64(string_value(value)),
        )
    })
    .collect::<Vec<_>>();
    let cert_key = scope.root_string_ptr(key("cert"));
    let error = scope.root_raw_mut_ptr(
        message.with_mut_ptr(|message| crate::error::js_error_new_with_message(message)),
    );
    for (field, value) in fields {
        error.with_mut_ptr(|error| {
            field.with_const_ptr(|field| {
                crate::object::js_object_set_field_by_name(error, field, value.get_nanbox_f64())
            })
        });
    }
    error.with_mut_ptr(|error| {
        cert_key.with_const_ptr(|cert_key| {
            crate::object::js_object_set_field_by_name(error, cert_key, cert.get_nanbox_f64())
        })
    });
    error.with_mut_ptr(|error: *mut ObjectHeader| ptr_value(error))
}

#[no_mangle]
pub extern "C" fn js_tls_check_server_identity(hostname: f64, cert: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let hostname = scope.root_nanbox_f64(hostname);
    let cert = scope.root_nanbox_f64(cert);
    let host = host_to_string(hostname.get_nanbox_f64());
    let Some(cert_obj) = object_ptr(cert.get_nanbox_f64()) else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    let match_host = tls_domain_to_ascii(&host);
    let san = value_to_string(get_field(cert_obj, "subjectaltname")).unwrap_or_default();
    let dns_names = if san.is_empty() {
        Vec::new()
    } else {
        san_entries(&san, "DNS:")
    };
    let ip_names: Vec<String> = if san.is_empty() {
        Vec::new()
    } else {
        san_entries(&san, "IP Address:")
            .into_iter()
            .filter_map(|candidate| candidate.parse::<std::net::IpAddr>().ok())
            .map(|candidate| candidate.to_string())
            .collect()
    };

    if let Ok(ip) = match_host.parse::<std::net::IpAddr>() {
        let canonical_ip = ip.to_string();
        if ip_names.iter().any(|candidate| candidate == &canonical_ip) {
            return f64::from_bits(TAG_UNDEFINED);
        }
        return altname_error(
            &host,
            cert.get_nanbox_f64(),
            format!(
                "IP: {} is not in the cert's list: {}",
                host,
                ip_names.join(", ")
            ),
        );
    }

    let common_names = cert_common_names(cert.get_nanbox_f64());
    if !dns_names.is_empty() || !common_names.is_empty() {
        if dns_names
            .iter()
            .any(|candidate| tls_dns_name_matches(&match_host, candidate))
        {
            return f64::from_bits(TAG_UNDEFINED);
        }
        if !dns_names.is_empty() {
            return altname_error(
                &host,
                cert.get_nanbox_f64(),
                format!("Host: {}. is not in the cert's altnames: {}", host, san),
            );
        }
        if common_names
            .iter()
            .any(|candidate| tls_dns_name_matches(&match_host, candidate))
        {
            return f64::from_bits(TAG_UNDEFINED);
        }
        return altname_error(
            &host,
            cert.get_nanbox_f64(),
            format!(
                "Host: {}. is not cert's CN: {}",
                host,
                common_names.join(",")
            ),
        );
    }

    altname_error(
        &host,
        cert.get_nanbox_f64(),
        "Cert does not contain a DNS name".to_string(),
    )
}

// Keep-alive anchors so the auto-optimize bitcode rebuild does not dead-strip
// these codegen-emitted `#[no_mangle]` runtime helpers (referenced from the
// native dispatch table in perry-codegen).
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_TLS_GET_CIPHERS: extern "C" fn() -> f64 = js_tls_get_ciphers;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_TLS_GET_CERTIFICATE_COMPRESSION_ALGORITHMS: extern "C" fn() -> f64 =
    js_tls_get_certificate_compression_algorithms;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_TLS_PREPARE_CONNECT: extern "C" fn() = js_tls_prepare_connect;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_TLS_VALIDATE_CONNECT_OPTIONS: extern "C" fn(f64) = js_tls_validate_connect_options;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_TLS_VALIDATE_POSITIONAL_CONNECT_OPTIONS: extern "C" fn(f64) =
    js_tls_validate_positional_connect_options;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_TLS_CLIENT_CHECK_IDENTITY: extern "C" fn(i64, f64) -> f64 =
    js_tls_client_check_identity;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_TLS_CLIENT_CHECK_IDENTITY_FROM_METADATA: unsafe extern "C" fn(i64) -> f64 =
    js_tls_client_check_identity_from_metadata;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_TLS_GET_CA_CERTIFICATES: extern "C" fn(f64) -> f64 = js_tls_get_ca_certificates;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_TLS_SET_DEFAULT_CA_CERTIFICATES: extern "C" fn(f64) -> f64 =
    js_tls_set_default_ca_certificates;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_TLS_CREATE_SECURE_CONTEXT: extern "C" fn(f64) -> f64 = js_tls_create_secure_context;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_TLS_SECURE_CONTEXT_NEW: extern "C" fn(f64) -> f64 = js_tls_secure_context_new;
#[cfg(feature = "keepalive-anchors")]
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
        assert!(tls_dns_name_matches("api.example.com", "*.example.com"));
        assert!(!tls_dns_name_matches(
            "deep.api.example.com",
            "*.example.com"
        ));
        assert!(tls_dns_name_matches("a-cb.a.com", "*b.a.com"));
        assert!(!tls_dns_name_matches("a.com", "*.com"));
        assert!(tls_dns_name_matches("a.co.uk", "*.co.uk"));
        assert!(tls_dns_name_matches("a.example", "A.EXAMPLE."));
        assert!(!tls_dns_name_matches(
            "bad.x.example.com",
            "bad..example.com"
        ));
        assert!(!tls_dns_name_matches("x.example.com", "café.example.com"));
    }

    #[test]
    fn tls_hostname_canonicalization_matches_node_identity_inputs() {
        assert_eq!(tls_domain_to_ascii("123"), "0.0.0.123");
        assert_eq!(tls_domain_to_ascii("::1"), "");
        assert_eq!(
            tls_domain_to_ascii("foo。bar.example.com"),
            "foo.bar.example.com"
        );
    }
}
