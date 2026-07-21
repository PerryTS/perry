use super::*;

use crate::object::{js_object_alloc, js_object_set_field_by_name};

pub(crate) fn object_value(fields: &[(&str, f64)]) -> f64 {
    let obj = js_object_alloc(0, fields.len() as u32);
    for (name, value) in fields {
        js_object_set_field_by_name(obj, key(name), *value);
    }
    boxed_pointer(obj as *const u8)
}

pub(crate) fn mx_record(exchange: &str, priority: f64) -> f64 {
    object_value(&[("exchange", str_value(exchange)), ("priority", priority)])
}

pub(crate) fn any_address_record(address: &str, ttl: f64, record_type: &str) -> f64 {
    object_value(&[
        ("address", str_value(address)),
        ("ttl", ttl),
        ("type", str_value(record_type)),
    ])
}

pub(crate) fn naptr_record(
    flags: &str,
    service: &str,
    regexp: &str,
    replacement: &str,
    order: f64,
    preference: f64,
) -> f64 {
    object_value(&[
        ("flags", str_value(flags)),
        ("service", str_value(service)),
        ("regexp", str_value(regexp)),
        ("replacement", str_value(replacement)),
        ("order", order),
        ("preference", preference),
    ])
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn soa_record(
    nsname: &str,
    hostmaster: &str,
    serial: f64,
    refresh: f64,
    retry: f64,
    expire: f64,
    minttl: f64,
) -> f64 {
    object_value(&[
        ("nsname", str_value(nsname)),
        ("hostmaster", str_value(hostmaster)),
        ("serial", serial),
        ("refresh", refresh),
        ("retry", retry),
        ("expire", expire),
        ("minttl", minttl),
    ])
}

pub(crate) fn srv_record(name: &str, port: f64, priority: f64, weight: f64) -> f64 {
    object_value(&[
        ("name", str_value(name)),
        ("port", port),
        ("priority", priority),
        ("weight", weight),
    ])
}

pub(crate) fn tlsa_record(usage: f64, selector: f64, matching_type: f64, certificate: &str) -> f64 {
    object_value(&[
        ("usage", usage),
        ("selector", selector),
        ("matchingType", matching_type),
        ("certificate", str_value(certificate)),
    ])
}

pub(crate) fn caa_record(critical: f64, field: &str, value: &str) -> f64 {
    object_value(&[("critical", critical), (field, str_value(value))])
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}
