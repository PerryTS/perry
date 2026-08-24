//! Module-level `node:tls` surface: the cipher catalogs, `checkServerIdentity`,
//! the CA-certificate accessors, `convertALPNProtocols` and the `tls.*` native
//! dispatch table.
//!
//! Split out of `tls.rs` to keep that file under the 2000-line lint cap
//! (`scripts/check_file_size.sh`). Items are moved verbatim; the `pub` entry
//! points are re-exported from the parent module so `crate::tls::…` (and the
//! `pub use tls::*` glob in `lib.rs`) keep resolving them unchanged.

use perry_runtime::array::js_array_get_f64;
use perry_runtime::{js_array_length, js_nanbox_pointer, JSValue, ObjectHeader};

use super::{
    f64_from_raw_bits, is_array_value, js_is_undefined_or_null, js_tls_create_server,
    js_tls_tlssocket_constructor, nanbox_handle, object_field, object_field_string, pointer_addr,
    record_tls_client_handle, set_field, throw_type_error, undefined, value_to_string,
    TLS_DISPATCH_MISSING_BITS,
};

#[cfg(feature = "bundled-net")]
unsafe fn dispatch_tls_connect(arg1: f64, arg2: f64, arg3: f64, arg4: f64) -> i64 {
    crate::net::js_tls_connect(arg1, arg2, arg3, arg4)
}

#[cfg(all(not(feature = "bundled-net"), feature = "external-net-tls"))]
unsafe fn dispatch_tls_connect(arg1: f64, arg2: f64, arg3: f64, arg4: f64) -> i64 {
    unsafe extern "C" {
        fn js_tls_connect(arg1: f64, arg2: f64, arg3: f64, arg4: f64) -> i64;
    }
    js_tls_connect(arg1, arg2, arg3, arg4)
}

#[cfg(not(any(feature = "bundled-net", feature = "external-net-tls")))]
unsafe fn dispatch_tls_connect(_arg1: f64, _arg2: f64, _arg3: f64, _arg4: f64) -> i64 {
    0
}

fn split_subject_alt_names(san: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for part in san.split(',') {
        let item = part.trim();
        if let Some(rest) = item.strip_prefix("DNS:") {
            out.push(("DNS".to_string(), rest.trim().to_string()));
        } else if let Some(rest) = item.strip_prefix("IP Address:") {
            out.push(("IP".to_string(), rest.trim().to_string()));
        }
    }
    out
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
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let cert = scope.root_nanbox_f64(cert);
    let message = scope.root_string_ptr(perry_runtime::string::js_string_from_bytes(
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
            scope.root_string_ptr(perry_runtime::string::js_string_from_bytes(
                name.as_ptr(),
                name.len() as u32,
            )),
            scope.root_nanbox_f64(super::nanbox_str(value)),
        )
    })
    .collect::<Vec<_>>();
    let cert_key = scope.root_string_ptr(perry_runtime::string::js_string_from_bytes(
        b"cert".as_ptr(),
        4,
    ));
    let error = scope.root_raw_mut_ptr(perry_runtime::error::js_error_new_with_message(
        message.get_raw_mut_ptr(),
    ));
    for (key, value) in &fields {
        perry_runtime::object::js_object_set_field_by_name(
            error.get_raw_mut_ptr::<ObjectHeader>(),
            key.get_raw_const_ptr(),
            value.get_nanbox_f64(),
        );
    }
    perry_runtime::object::js_object_set_field_by_name(
        error.get_raw_mut_ptr::<ObjectHeader>(),
        cert_key.get_raw_const_ptr(),
        cert.get_nanbox_f64(),
    );
    js_nanbox_pointer(error.get_raw_mut_ptr::<ObjectHeader>() as i64)
}

pub unsafe extern "C" fn js_tls_get_ciphers() -> *mut perry_runtime::ArrayHeader {
    let value = perry_runtime::tls::js_tls_get_ciphers();
    pointer_addr(value).unwrap_or(0) as *mut perry_runtime::ArrayHeader
}

pub unsafe extern "C" fn js_tls_root_certificates() -> *mut perry_runtime::ArrayHeader {
    let value = perry_runtime::tls::js_tls_root_certificates();
    pointer_addr(value).unwrap_or(0) as *mut perry_runtime::ArrayHeader
}

pub unsafe extern "C" fn js_tls_get_ca_certificates(
    type_bits: i64,
) -> *mut perry_runtime::ArrayHeader {
    let value = perry_runtime::tls::js_tls_get_ca_certificates(f64_from_raw_bits(type_bits));
    pointer_addr(value).unwrap_or(0) as *mut perry_runtime::ArrayHeader
}

pub unsafe extern "C" fn js_tls_set_default_ca_certificates(certs_bits: i64) -> f64 {
    perry_runtime::tls::js_tls_set_default_ca_certificates(f64_from_raw_bits(certs_bits))
}

pub unsafe extern "C" fn js_tls_check_server_identity(hostname_bits: i64, cert_bits: i64) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let hostname = scope.root_nanbox_f64(f64_from_raw_bits(hostname_bits));
    let cert = scope.root_nanbox_f64(f64_from_raw_bits(cert_bits));
    let host = value_to_string(hostname.get_nanbox_f64()).unwrap_or_default();
    let match_host = perry_runtime::tls::tls_domain_to_ascii(&host);
    let san = object_field_string(cert.get_nanbox_f64(), "subjectaltname").unwrap_or_default();
    let san_entries = split_subject_alt_names(&san);
    let ip_names: Vec<String> = san_entries
        .iter()
        .filter(|(kind, _)| kind == "IP")
        .filter_map(|(_, value)| value.parse::<std::net::IpAddr>().ok())
        .map(|value| value.to_string())
        .collect();
    let dns_names: Vec<String> = san_entries
        .iter()
        .filter(|(kind, _)| kind == "DNS")
        .map(|(_, value)| value.clone())
        .collect();

    if let Ok(ip) = match_host.parse::<std::net::IpAddr>() {
        if ip_names
            .iter()
            .any(|candidate| candidate == &ip.to_string())
        {
            return undefined();
        }
        let reason = format!(
            "IP: {host} is not in the cert's list: {}",
            ip_names.join(", ")
        );
        return make_altname_error(reason, &host, cert.get_nanbox_f64());
    }

    if !dns_names.is_empty() {
        if dns_names
            .iter()
            .any(|pattern| perry_runtime::tls::tls_dns_name_matches(&match_host, pattern))
        {
            return undefined();
        }
        let reason = format!("Host: {host}. is not in the cert's altnames: {san}");
        return make_altname_error(reason, &host, cert.get_nanbox_f64());
    }

    let subject = object_field(cert.get_nanbox_f64(), "subject");
    let cns = cn_values(subject);
    if cns
        .iter()
        .any(|cn| perry_runtime::tls::tls_dns_name_matches(&match_host, cn))
    {
        return undefined();
    }
    let reason = if cns.is_empty() {
        "Cert does not contain a DNS name".to_string()
    } else {
        format!("Host: {host}. is not cert's CN: {}", cns.join(","))
    };
    make_altname_error(reason, &host, cert.get_nanbox_f64())
}

pub unsafe extern "C" fn js_tls_create_secure_context(options_bits: i64) -> f64 {
    perry_runtime::tls::js_tls_create_secure_context(f64_from_raw_bits(options_bits))
}

pub unsafe extern "C" fn js_tls_secure_context_constructor(options_bits: i64) -> f64 {
    perry_runtime::tls::js_tls_secure_context_new(f64_from_raw_bits(options_bits))
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
        "getCertificateCompressionAlgorithms" => {
            perry_runtime::tls::js_tls_get_certificate_compression_algorithms()
        }
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
            let handle = dispatch_tls_connect(arg(0), arg(1), arg(2), arg(3));
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
        "$DEFAULT_CIPHERS" => perry_runtime::tls::tls_default_ciphers_value(),
        _ => f64::from_bits(TLS_DISPATCH_MISSING_BITS),
    }
}
