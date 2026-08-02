//! Module-level `node:tls` surface: the cipher catalogs, `checkServerIdentity`,
//! the CA-certificate accessors, `convertALPNProtocols` and the `tls.*` native
//! dispatch table.
//!
//! Split out of `tls.rs` to keep that file under the 2000-line lint cap
//! (`scripts/check_file_size.sh`). Items are moved verbatim; the `pub` entry
//! points are re-exported from the parent module so `crate::tls::…` (and the
//! `pub use tls::*` glob in `lib.rs`) keep resolving them unchanged.

use perry_runtime::array::js_array_get_f64;
use perry_runtime::{js_array_length, js_nanbox_pointer, js_object_alloc, JSValue, ObjectHeader};

use super::secure_context::{
    ca_store, cert_list_from_array_value, make_secure_context, root_certificates,
    validate_ca_list_for_set,
};
use super::{
    f64_from_raw_bits, is_array_value, js_is_undefined_or_null, js_tls_create_server,
    js_tls_tlssocket_constructor, nanbox_handle, nanbox_str, object_field, object_field_string,
    pointer_addr, record_tls_client_handle, set_field, set_str_field, static_string_array,
    string_array, throw_type_error, type_name, undefined, value_to_string,
    TLS_DISPATCH_MISSING_BITS,
};

const DEFAULT_CIPHERS: &str = concat!(
    "TLS_AES_256_GCM_SHA384:",
    "TLS_CHACHA20_POLY1305_SHA256:",
    "TLS_AES_128_GCM_SHA256:",
    "ECDHE-RSA-AES128-GCM-SHA256:",
    "ECDHE-ECDSA-AES128-GCM-SHA256:",
    "ECDHE-RSA-AES256-GCM-SHA384:",
    "ECDHE-ECDSA-AES256-GCM-SHA384:",
    "DHE-RSA-AES128-GCM-SHA256:",
    "ECDHE-RSA-AES128-SHA256:",
    "DHE-RSA-AES128-SHA256:",
    "ECDHE-RSA-AES256-SHA384:",
    "DHE-RSA-AES256-SHA384:",
    "ECDHE-RSA-AES256-SHA256:",
    "DHE-RSA-AES256-SHA256:",
    "HIGH:!aNULL:!eNULL:!EXPORT:!DES:!RC4:!MD5:!PSK:!SRP:!CAMELLIA"
);

const NODE_TLS_CIPHERS: &[&str] = &[
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

fn split_subject_alt_names(san: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for part in san.split(',') {
        let item = part.trim();
        if let Some(rest) = item.strip_prefix("DNS:") {
            out.push(("DNS".to_string(), rest.trim().to_string()));
        } else if let Some(rest) = item.strip_prefix("IP Address:") {
            out.push(("IP".to_string(), rest.trim().to_string()));
        } else if let Some(rest) = item.strip_prefix("IP:") {
            out.push(("IP".to_string(), rest.trim().to_string()));
        }
    }
    out
}

fn hostname_is_ip(host: &str) -> bool {
    host.parse::<std::net::IpAddr>().is_ok()
}

fn dns_matches(pattern: &str, host: &str) -> bool {
    let pattern = pattern.trim_end_matches('.').to_ascii_lowercase();
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if pattern == host {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix("*.") {
        let Some(rest) = host.strip_suffix(suffix) else {
            return false;
        };
        return rest.ends_with('.') && rest[..rest.len().saturating_sub(1)].find('.').is_none();
    }
    false
}

unsafe fn cn_values(subject_value: f64) -> Vec<String> {
    let cn = object_field(subject_value, "CN");
    if js_is_undefined_or_null(cn) {
        return Vec::new();
    }
    if is_array_value(cn) {
        if let Some(addr) = pointer_addr(cn) {
            let arr = addr as *const perry_runtime::ArrayHeader;
            let len = js_array_length(arr);
            let mut out = Vec::new();
            for i in 0..len {
                if let Some(s) = value_to_string(js_array_get_f64(arr, i)) {
                    out.push(s);
                }
            }
            return out;
        }
    }
    value_to_string(cn).into_iter().collect()
}

unsafe fn make_altname_error(reason: String, host: &str, cert: f64) -> f64 {
    let message = format!("Hostname/IP does not match certificate's altnames: {reason}");
    let obj = js_object_alloc(perry_runtime::error::CLASS_ID_ERROR, 0);
    set_str_field(obj, "name", "Error");
    set_str_field(obj, "message", &message);
    set_str_field(obj, "code", "ERR_TLS_CERT_ALTNAME_INVALID");
    set_str_field(obj, "reason", &reason);
    set_str_field(obj, "host", host);
    set_field(obj, "cert", cert);
    js_nanbox_pointer(obj as i64)
}

pub unsafe extern "C" fn js_tls_get_ciphers() -> *mut perry_runtime::ArrayHeader {
    static_string_array(NODE_TLS_CIPHERS)
}

pub unsafe extern "C" fn js_tls_root_certificates() -> *mut perry_runtime::ArrayHeader {
    string_array(root_certificates())
}

pub unsafe extern "C" fn js_tls_get_ca_certificates(
    type_bits: i64,
) -> *mut perry_runtime::ArrayHeader {
    let value = f64_from_raw_bits(type_bits);
    let kind = if js_is_undefined_or_null(value) {
        "default".to_string()
    } else {
        let jsv = JSValue::from_bits(value.to_bits());
        if !jsv.is_any_string() {
            throw_type_error(
                &format!(
                    "The \"type\" argument must be of type string. Received type {}",
                    type_name(value)
                ),
                "ERR_INVALID_ARG_TYPE",
            );
        }
        value_to_string(value).unwrap_or_default()
    };

    match kind.as_str() {
        "default" => {
            if let Some(certs) = ca_store().lock().unwrap().clone() {
                string_array(&certs)
            } else {
                string_array(root_certificates())
            }
        }
        "system" | "bundled" => string_array(root_certificates()),
        "extra" => string_array(&[]),
        _ => throw_type_error(
            &format!("The argument 'type' is invalid. Received {kind:?}"),
            "ERR_INVALID_ARG_VALUE",
        ),
    }
}

pub unsafe extern "C" fn js_tls_set_default_ca_certificates(certs_bits: i64) -> f64 {
    let value = f64_from_raw_bits(certs_bits);
    if !is_array_value(value) {
        throw_type_error(
            &format!(
                "The \"certs\" argument must be an instance of Array. Received type {}",
                type_name(value)
            ),
            "ERR_INVALID_ARG_TYPE",
        );
    }
    let certs = cert_list_from_array_value(value).unwrap_or_else(|_| {
        throw_type_error(
            "The \"certs\" argument must contain strings or Buffer-like values",
            "ERR_INVALID_ARG_TYPE",
        )
    });
    validate_ca_list_for_set(&certs);
    *ca_store().lock().unwrap() = Some(certs);
    undefined()
}

pub unsafe extern "C" fn js_tls_check_server_identity(hostname_bits: i64, cert_bits: i64) -> f64 {
    let hostname_value = f64_from_raw_bits(hostname_bits);
    let cert = f64_from_raw_bits(cert_bits);
    let host = value_to_string(hostname_value).unwrap_or_default();
    let host_is_ip = hostname_is_ip(&host);
    let san = object_field_string(cert, "subjectaltname").unwrap_or_default();
    let san_entries = split_subject_alt_names(&san);
    let ip_names: Vec<String> = san_entries
        .iter()
        .filter(|(kind, _)| kind == "IP")
        .map(|(_, value)| value.clone())
        .collect();
    let dns_names: Vec<String> = san_entries
        .iter()
        .filter(|(kind, _)| kind == "DNS")
        .map(|(_, value)| value.clone())
        .collect();

    if host_is_ip {
        if ip_names.iter().any(|ip| ip == &host) {
            return undefined();
        }
        let reason = format!(
            "IP: {host} is not in the cert's list: {}",
            ip_names.join(", ")
        );
        return make_altname_error(reason, &host, cert);
    }

    if !dns_names.is_empty() {
        if dns_names.iter().any(|pattern| dns_matches(pattern, &host)) {
            return undefined();
        }
        let reason = format!("Host: {host}. is not in the cert's altnames: {san}");
        return make_altname_error(reason, &host, cert);
    }

    let subject = object_field(cert, "subject");
    let cns = cn_values(subject);
    if cns.iter().any(|cn| dns_matches(cn, &host)) {
        return undefined();
    }
    let reason = if cns.is_empty() {
        format!("Host: {host}. is not cert's CN: ")
    } else {
        format!("Host: {host}. is not cert's CN: {}", cns.join(","))
    };
    make_altname_error(reason, &host, cert)
}

pub unsafe extern "C" fn js_tls_create_secure_context(options_bits: i64) -> f64 {
    make_secure_context(f64_from_raw_bits(options_bits))
}

pub unsafe extern "C" fn js_tls_secure_context_constructor(options_bits: i64) -> f64 {
    make_secure_context(f64_from_raw_bits(options_bits))
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_convert_alpn_protocols(protocols: f64, out: f64) -> f64 {
    let mut encoded = Vec::new();
    if is_array_value(protocols) {
        let array =
            JSValue::from_bits(protocols.to_bits()).as_pointer::<perry_runtime::ArrayHeader>();
        let length = js_array_length(array);
        for index in 0..length {
            let protocol = js_array_get_f64(array, index);
            if !JSValue::from_bits(protocol.to_bits()).is_any_string() {
                throw_type_error(
                    "The \"protocols\" argument must contain only strings",
                    "ERR_INVALID_ARG_TYPE",
                );
            }
            let protocol = value_to_string(protocol).unwrap_or_default();
            if protocol.len() > u8::MAX as usize {
                perry_runtime::fs::validate::throw_range_error_named(
                    "ALPN protocol names must not exceed 255 bytes",
                    "ERR_OUT_OF_RANGE",
                );
            }
            encoded.push(protocol.len() as u8);
            encoded.extend_from_slice(protocol.as_bytes());
        }
    } else if let Some(addr) = pointer_addr(protocols) {
        if perry_runtime::buffer::is_registered_buffer(addr)
            && !perry_runtime::buffer::is_any_array_buffer(addr)
        {
            let data = perry_runtime::buffer::js_native_buffer_data_ptr(protocols);
            let length = perry_runtime::buffer::js_native_buffer_byte_len(protocols);
            if !data.is_null() && length != 0 {
                encoded.extend_from_slice(std::slice::from_raw_parts(data, length));
            }
        } else if perry_runtime::typedarray::lookup_typed_array_kind(addr).is_some() {
            let mut length = 0u32;
            let data =
                perry_runtime::buffer::js_value_buffer_or_typedarray_data(protocols, &mut length);
            if !data.is_null() && length != 0 {
                encoded.extend_from_slice(std::slice::from_raw_parts(data, length as usize));
            }
        } else {
            return undefined();
        }
    } else {
        // Node's internal helper ignores non-array/non-view values and leaves
        // the target object untouched.
        return undefined();
    }

    let Some(out_addr) = pointer_addr(out) else {
        throw_type_error(
            "The \"out\" argument must be of type object",
            "ERR_INVALID_ARG_TYPE",
        );
    };
    let buffer = perry_runtime::buffer::js_buffer_alloc(encoded.len() as i32, 0);
    if !encoded.is_empty() {
        std::ptr::copy_nonoverlapping(
            encoded.as_ptr(),
            perry_runtime::buffer::buffer_data_mut(buffer),
            encoded.len(),
        );
    }
    set_field(
        out_addr as *mut ObjectHeader,
        "ALPNProtocols",
        js_nanbox_pointer(buffer as i64),
    );
    undefined()
}

pub unsafe extern "C" fn js_tls_native_dispatch(
    method_ptr: *const u8,
    method_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let method =
        std::str::from_utf8(std::slice::from_raw_parts(method_ptr, method_len)).unwrap_or("");
    let arg = |idx: usize| -> f64 {
        if idx < args_len && !args_ptr.is_null() {
            *args_ptr.add(idx)
        } else {
            undefined()
        }
    };
    match method {
        "getCiphers" => js_nanbox_pointer(js_tls_get_ciphers() as i64),
        "rootCertificates" => js_nanbox_pointer(js_tls_root_certificates() as i64),
        "getCACertificates" => {
            js_nanbox_pointer(js_tls_get_ca_certificates(arg(0).to_bits() as i64) as i64)
        }
        "setDefaultCACertificates" => js_tls_set_default_ca_certificates(arg(0).to_bits() as i64),
        "checkServerIdentity" => {
            js_tls_check_server_identity(arg(0).to_bits() as i64, arg(1).to_bits() as i64)
        }
        "convertALPNProtocols" => js_tls_convert_alpn_protocols(arg(0), arg(1)),
        "connect" => {
            // Pass the args through raw — js_tls_connect resolves Node's
            // `connect(options[, cb])` / `connect(port[, host][, options][,
            // cb])` overloads plus the legacy positional form itself (#4971).
            let handle = crate::net::js_tls_connect(arg(0), arg(1), arg(2), arg(3));
            if handle == 0 {
                // Unresolvable args (e.g. no port) — undefined beats a
                // NaN-boxed null pointer that every later method call
                // trips over (#4971).
                return undefined();
            }
            record_tls_client_handle(handle);
            nanbox_handle(handle)
        }
        "createServer" | "Server" => nanbox_handle(js_tls_create_server(
            arg(0).to_bits() as i64,
            arg(1).to_bits() as i64,
        )),
        "TLSSocket" => nanbox_handle(js_tls_tlssocket_constructor(
            arg(0).to_bits() as i64,
            arg(1).to_bits() as i64,
        )),
        "createSecureContext" | "SecureContext" => {
            js_tls_create_secure_context(arg(0).to_bits() as i64)
        }
        "$DEFAULT_CIPHERS" => nanbox_str(DEFAULT_CIPHERS),
        _ => f64::from_bits(TLS_DISPATCH_MISSING_BITS),
    }
}
