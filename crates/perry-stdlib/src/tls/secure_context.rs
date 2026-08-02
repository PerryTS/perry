//! SecureContext construction plus the CA / PEM catalog helpers for `node:tls`.
//!
//! Split out of `tls.rs` to keep that file under the 2000-line lint cap
//! (`scripts/check_file_size.sh`). Items are moved verbatim; the entry points
//! consumed by the parent module and by `tls::module_api` are widened to
//! `pub(super)` so the original call sites keep resolving.

use std::io::Cursor;
use std::sync::{Mutex, OnceLock};

use base64::{engine::general_purpose, Engine as _};
use perry_runtime::array::js_array_get_f64;
use perry_runtime::{js_array_length, js_nanbox_pointer, js_object_alloc};

use super::{
    is_array_value, js_is_undefined_or_null, object_field, pointer_addr, set_field, set_str_field,
    string_array, throw_error, throw_type_error, undefined, value_to_string,
};

static ROOT_CERTIFICATES: OnceLock<Vec<String>> = OnceLock::new();
static DEFAULT_CA_CERTIFICATES: OnceLock<Mutex<Option<Vec<String>>>> = OnceLock::new();
static NEXT_SECURE_CONTEXT_ID: OnceLock<Mutex<i64>> = OnceLock::new();

#[derive(Default)]
struct PemScan {
    valid: usize,
    had_pem_boundary: bool,
    had_parse_error: bool,
}

fn der_to_pem(der: &[u8]) -> String {
    let encoded = general_purpose::STANDARD.encode(der);
    let mut pem = String::from("-----BEGIN CERTIFICATE-----\n");
    for chunk in encoded.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        pem.push('\n');
    }
    pem.push_str("-----END CERTIFICATE-----\n");
    pem
}

fn load_native_certificates() -> Vec<String> {
    let native = rustls_native_certs::load_native_certs();
    let mut out = Vec::with_capacity(native.certs.len());
    for cert in native.certs {
        out.push(der_to_pem(cert.as_ref()));
    }
    out
}

pub(super) fn root_certificates() -> &'static Vec<String> {
    ROOT_CERTIFICATES.get_or_init(load_native_certificates)
}

pub(super) fn ca_store() -> &'static Mutex<Option<Vec<String>>> {
    DEFAULT_CA_CERTIFICATES.get_or_init(|| Mutex::new(None))
}

pub(super) unsafe fn cert_list_from_array_value(value: f64) -> Result<Vec<String>, ()> {
    if !is_array_value(value) {
        return Err(());
    }
    let Some(addr) = pointer_addr(value) else {
        return Err(());
    };
    let arr = addr as *const perry_runtime::ArrayHeader;
    let len = js_array_length(arr);
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        let item = js_array_get_f64(arr, i);
        let Some(s) = value_to_string(item) else {
            return Err(());
        };
        out.push(s);
    }
    Ok(out)
}

fn scan_pem_certificates(pems: &[String]) -> PemScan {
    let mut scan = PemScan::default();
    for pem in pems {
        if pem.contains("-----BEGIN CERTIFICATE-----") {
            scan.had_pem_boundary = true;
        }
        let mut cursor = Cursor::new(pem.as_bytes());
        for cert in rustls_pemfile::certs(&mut cursor) {
            match cert {
                Ok(_) => scan.valid += 1,
                Err(_) => scan.had_parse_error = true,
            }
        }
    }
    scan
}

pub(super) fn validate_ca_list_for_set(pems: &[String]) {
    if pems.is_empty() {
        return;
    }
    let scan = scan_pem_certificates(pems);
    if scan.valid > 0 {
        return;
    }
    if scan.had_pem_boundary || scan.had_parse_error {
        throw_error(
            "error:0488000D:PEM routines::ASN1 lib",
            "ERR_OSSL_PEM_ASN1_LIB",
        );
    }
    throw_error(
        "No valid certificates found in the provided array",
        "ERR_CRYPTO_OPERATION_FAILED",
    );
}

fn validate_ca_list_for_context(pems: &[String]) {
    if pems.is_empty() {
        return;
    }
    let scan = scan_pem_certificates(pems);
    if scan.valid > 0 {
        return;
    }
    if scan.had_pem_boundary || scan.had_parse_error {
        throw_error(
            "error:0488000D:PEM routines::ASN1 lib",
            "ERR_OSSL_PEM_ASN1_LIB",
        );
    }
    throw_error(
        "No valid certificates found in the provided array",
        "ERR_CRYPTO_OPERATION_FAILED",
    );
}

fn validate_tls_version(value: f64, label: &str) {
    if js_is_undefined_or_null(value) {
        return;
    }
    let text = unsafe { value_to_string(value).unwrap_or_default() };
    match text.as_str() {
        "TLSv1.2" | "TLSv1.3" => {}
        _ => {
            let adjective = if label == "minVersion" {
                "minimum"
            } else {
                "maximum"
            };
            throw_type_error(
                &format!("{text:?} is not a valid {adjective} TLS protocol version"),
                "ERR_TLS_INVALID_PROTOCOL_VERSION",
            );
        }
    }
}

unsafe fn ca_list_from_value(value: f64) -> Result<Vec<String>, ()> {
    if js_is_undefined_or_null(value) {
        return Ok(Vec::new());
    }
    if is_array_value(value) {
        return cert_list_from_array_value(value);
    }
    let Some(s) = value_to_string(value) else {
        return Err(());
    };
    Ok(vec![s])
}

fn next_secure_context_id() -> i64 {
    let lock = NEXT_SECURE_CONTEXT_ID.get_or_init(|| Mutex::new(1));
    let mut guard = lock.lock().unwrap();
    let id = *guard;
    *guard += 1;
    id
}

unsafe fn constructor_value(name: &str) -> f64 {
    let module = b"tls";
    let prop = name.as_bytes();
    perry_runtime::object::js_native_module_property_by_name(
        module.as_ptr(),
        module.len(),
        prop.as_ptr(),
        prop.len(),
    )
}

pub(super) unsafe fn make_secure_context(options: f64) -> f64 {
    let min_version = if js_is_undefined_or_null(options) {
        undefined()
    } else {
        object_field(options, "minVersion")
    };
    let max_version = if js_is_undefined_or_null(options) {
        undefined()
    } else {
        object_field(options, "maxVersion")
    };
    validate_tls_version(min_version, "minVersion");
    validate_tls_version(max_version, "maxVersion");

    let ca_value = if js_is_undefined_or_null(options) {
        undefined()
    } else {
        object_field(options, "ca")
    };
    let ca = ca_list_from_value(ca_value).unwrap_or_else(|_| {
        throw_type_error(
            "The \"ca\" option must be a string, Buffer, or an array of those values",
            "ERR_INVALID_ARG_TYPE",
        )
    });
    validate_ca_list_for_context(&ca);

    let obj = js_object_alloc(0, 0);
    set_field(obj, "context", next_secure_context_id() as f64);
    set_field(obj, "_secureContext", next_secure_context_id() as f64);
    set_str_field(obj, "minVersion", "TLSv1.2");
    set_str_field(obj, "maxVersion", "TLSv1.3");
    set_field(obj, "constructor", constructor_value("SecureContext"));
    if !ca.is_empty() {
        let ca_arr = string_array(&ca);
        set_field(obj, "ca", js_nanbox_pointer(ca_arr as i64));
    }
    js_nanbox_pointer(obj as i64)
}
