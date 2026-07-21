use super::*;

use crate::dns_resolver::{Answer, ResolvedRecord};

/// Deterministic-mode (`PERRY_DETERMINISTIC_NET=1`) loopback answers — the
/// pre-#4911 behavior, kept for reproducible parity fixtures.
pub(crate) fn deterministic_resolve_records(kind: RecordKind, name: &str) -> f64 {
    if !localhost_name(name) {
        return empty_array_value();
    }

    match kind {
        RecordKind::A => string_array_value(&["127.0.0.1"]),
        RecordKind::Aaaa => string_array_value(&["::1"]),
        RecordKind::Any => array_value_from_values(&[
            any_address_record("127.0.0.1", 0.0, "A"),
            any_address_record("::1", 0.0, "AAAA"),
        ]),
        RecordKind::Caa => empty_array_value(),
        RecordKind::Cname => string_array_value(&["localhost"]),
        RecordKind::Mx => array_value_from_values(&[mx_record("localhost", 0.0)]),
        RecordKind::Naptr => {
            array_value_from_values(&[naptr_record("", "", "", "localhost", 0.0, 0.0)])
        }
        RecordKind::Ns => string_array_value(&["localhost"]),
        RecordKind::Ptr => string_array_value(&["localhost"]),
        RecordKind::Soa => soa_record("localhost", "root.localhost", 1.0, 0.0, 0.0, 0.0, 0.0),
        RecordKind::Srv => array_value_from_values(&[srv_record("localhost", 0.0, 0.0, 0.0)]),
        RecordKind::Tlsa => array_value_from_values(&[tlsa_record(0.0, 0.0, 0.0, "")]),
        RecordKind::Txt => array_value_from_values(&[string_array_value(&["localhost"])]),
    }
}

/// Build the Node-shaped JS value for `kind` from real resolved records.
/// `resolveSoa` yields a single object; every other family yields an array.
pub(crate) fn build_resolve_value(kind: RecordKind, records: &[ResolvedRecord]) -> f64 {
    match kind {
        RecordKind::A | RecordKind::Aaaa => {
            let values: Vec<f64> = records
                .iter()
                .filter_map(|r| match &r.data {
                    Answer::Addr(ip) => Some(str_value(&ip.to_string())),
                    _ => None,
                })
                .collect();
            array_value_from_values(&values)
        }
        RecordKind::Cname | RecordKind::Ns | RecordKind::Ptr => {
            let values: Vec<f64> = records
                .iter()
                .filter_map(|r| match &r.data {
                    Answer::Name(name) => Some(str_value(name)),
                    _ => None,
                })
                .collect();
            array_value_from_values(&values)
        }
        RecordKind::Mx => {
            let values: Vec<f64> = records
                .iter()
                .filter_map(|r| match &r.data {
                    Answer::Mx(mx) => Some(mx_record(&mx.exchange, mx.priority as f64)),
                    _ => None,
                })
                .collect();
            array_value_from_values(&values)
        }
        RecordKind::Txt => {
            let values: Vec<f64> = records
                .iter()
                .filter_map(|r| match &r.data {
                    Answer::Txt(chunks) => {
                        let strs: Vec<&str> = chunks.iter().map(String::as_str).collect();
                        Some(string_array_value(&strs))
                    }
                    _ => None,
                })
                .collect();
            array_value_from_values(&values)
        }
        RecordKind::Soa => records
            .iter()
            .find_map(|r| match &r.data {
                Answer::Soa(soa) => Some(soa_record(
                    &soa.nsname,
                    &soa.hostmaster,
                    soa.serial as f64,
                    soa.refresh as f64,
                    soa.retry as f64,
                    soa.expire as f64,
                    soa.minttl as f64,
                )),
                _ => None,
            })
            .unwrap_or_else(empty_array_value),
        RecordKind::Srv => {
            let values: Vec<f64> = records
                .iter()
                .filter_map(|r| match &r.data {
                    Answer::Srv(srv) => Some(srv_record(
                        &srv.name,
                        srv.port as f64,
                        srv.priority as f64,
                        srv.weight as f64,
                    )),
                    _ => None,
                })
                .collect();
            array_value_from_values(&values)
        }
        RecordKind::Naptr => {
            let values: Vec<f64> = records
                .iter()
                .filter_map(|r| match &r.data {
                    Answer::Naptr(n) => Some(naptr_record(
                        &n.flags,
                        &n.service,
                        &n.regexp,
                        &n.replacement,
                        n.order as f64,
                        n.preference as f64,
                    )),
                    _ => None,
                })
                .collect();
            array_value_from_values(&values)
        }
        RecordKind::Tlsa => {
            let values: Vec<f64> = records
                .iter()
                .filter_map(|r| match &r.data {
                    Answer::Tlsa(t) => Some(tlsa_record(
                        t.usage as f64,
                        t.selector as f64,
                        t.matching_type as f64,
                        &hex_encode(&t.certificate),
                    )),
                    _ => None,
                })
                .collect();
            array_value_from_values(&values)
        }
        RecordKind::Caa => {
            let values: Vec<f64> = records
                .iter()
                .filter_map(|r| match &r.data {
                    Answer::Caa(caa) => {
                        Some(caa_record(caa.critical as f64, &caa.field, &caa.value))
                    }
                    _ => None,
                })
                .collect();
            array_value_from_values(&values)
        }
        RecordKind::Any => {
            let values: Vec<f64> = records.iter().map(build_any_record).collect();
            array_value_from_values(&values)
        }
    }
}

/// One `resolveAny` element — Node tags each record with its `type`.
pub(crate) fn build_any_record(record: &ResolvedRecord) -> f64 {
    match &record.data {
        Answer::Addr(ip) => any_address_record(&ip.to_string(), record.ttl as f64, record.rtype),
        Answer::Name(name) => object_value(&[
            ("type", str_value(record.rtype)),
            ("value", str_value(name)),
        ]),
        Answer::Mx(mx) => object_value(&[
            ("type", str_value("MX")),
            ("exchange", str_value(&mx.exchange)),
            ("priority", mx.priority as f64),
        ]),
        Answer::Txt(chunks) => {
            let strs: Vec<&str> = chunks.iter().map(String::as_str).collect();
            object_value(&[
                ("type", str_value("TXT")),
                ("entries", string_array_value(&strs)),
            ])
        }
        Answer::Soa(soa) => object_value(&[
            ("type", str_value("SOA")),
            ("nsname", str_value(&soa.nsname)),
            ("hostmaster", str_value(&soa.hostmaster)),
            ("serial", soa.serial as f64),
            ("refresh", soa.refresh as f64),
            ("retry", soa.retry as f64),
            ("expire", soa.expire as f64),
            ("minttl", soa.minttl as f64),
        ]),
        Answer::Srv(srv) => object_value(&[
            ("type", str_value("SRV")),
            ("name", str_value(&srv.name)),
            ("port", srv.port as f64),
            ("priority", srv.priority as f64),
            ("weight", srv.weight as f64),
        ]),
        Answer::Naptr(n) => object_value(&[
            ("type", str_value("NAPTR")),
            ("flags", str_value(&n.flags)),
            ("service", str_value(&n.service)),
            ("regexp", str_value(&n.regexp)),
            ("replacement", str_value(&n.replacement)),
            ("order", n.order as f64),
            ("preference", n.preference as f64),
        ]),
        Answer::Tlsa(t) => object_value(&[
            ("type", str_value("TLSA")),
            ("usage", t.usage as f64),
            ("selector", t.selector as f64),
            ("matchingType", t.matching_type as f64),
            ("certificate", str_value(&hex_encode(&t.certificate))),
        ]),
        Answer::Caa(caa) => object_value(&[
            ("type", str_value("CAA")),
            ("critical", caa.critical as f64),
            (caa.field.as_str(), str_value(&caa.value)),
        ]),
    }
}
