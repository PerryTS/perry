use super::*;

pub struct X509Handle {
    der: Vec<u8>,
    cert: x509_cert::Certificate,
    issuer_certificate: Option<Handle>,
}

/// Short attribute name for an X.500 DN OID, matching Node's subject /
/// issuer formatting (`CN`, `O`, `C`, …); falls back to the dotted OID.
pub(super) fn x509_attr_short_name(oid: &str) -> String {
    match oid {
        "2.5.4.3" => "CN",
        "2.5.4.6" => "C",
        "2.5.4.7" => "L",
        "2.5.4.8" => "ST",
        "2.5.4.10" => "O",
        "2.5.4.11" => "OU",
        "2.5.4.4" => "SN",
        "2.5.4.42" => "GN",
        "2.5.4.5" => "serialNumber",
        "2.5.4.9" => "STREET",
        "0.9.2342.19200300.100.1.25" => "DC",
        "1.2.840.113549.1.9.1" => "emailAddress",
        other => return other.to_string(),
    }
    .to_string()
}

/// Format an X.500 `Name` the way Node's `cert.subject` / `cert.issuer`
/// do: one `TYPE=value` per line, newline-joined, in encoding order.
pub(super) fn x509_format_name(name: &x509_cert::name::Name) -> String {
    let mut lines: Vec<String> = Vec::new();
    for rdn in name.0.iter() {
        for atv in rdn.0.iter() {
            let oid = atv.oid.to_string();
            let value = x509_attr_value(atv);
            lines.push(format!("{}={}", x509_attr_short_name(&oid), value));
        }
    }
    lines.join("\n")
}

fn x509_attr_value(atv: &x509_cert::attr::AttributeTypeAndValue) -> String {
    use x509_cert::der::Encode;

    // The value is an AttributeValue (ASN.1 Any); decode common string forms.
    // Fall back to its UTF-8 lossy DER tail.
    atv.value
        .decode_as::<x509_cert::der::asn1::Utf8StringRef>()
        .map(|s| s.as_str().to_string())
        .or_else(|_| {
            atv.value
                .decode_as::<x509_cert::der::asn1::PrintableStringRef>()
                .map(|s| s.as_str().to_string())
        })
        .or_else(|_| {
            atv.value
                .decode_as::<x509_cert::der::asn1::Ia5StringRef>()
                .map(|s| s.as_str().to_string())
        })
        .unwrap_or_else(|_| {
            let bytes = atv.value.to_der().unwrap_or_default();
            // Skip the 2-byte tag+len header when present.
            let tail = if bytes.len() > 2 {
                &bytes[2..]
            } else {
                &bytes[..]
            };
            String::from_utf8_lossy(tail).to_string()
        })
}

/// Format an X.509 validity `Time` as Node does — `MMM D HH:MM:SS YYYY GMT`
/// with a space-padded day (e.g. `Jan  1 00:00:00 2020 GMT`).
pub(super) fn x509_format_time(time: &x509_cert::time::Time) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let dt = time.to_date_time();
    let month = MONTHS
        .get((dt.month() as usize).saturating_sub(1))
        .copied()
        .unwrap_or("Jan");
    format!(
        "{} {:>2} {:02}:{:02}:{:02} {} GMT",
        month,
        dt.day(),
        dt.hour(),
        dt.minutes(),
        dt.seconds(),
        dt.year(),
    )
}

/// Uppercase colon-separated hex of a digest, matching Node's
/// `cert.fingerprint` / `.fingerprint256`.
pub(super) fn x509_colon_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(":")
}

fn x509_find_extension<'a>(
    cert: &'a x509_cert::Certificate,
    oid: &str,
) -> Option<&'a x509_cert::ext::Extension> {
    cert.tbs_certificate
        .extensions
        .as_ref()?
        .iter()
        .find(|ext| ext.extn_id.to_string() == oid)
}

fn x509_format_general_name(name: &x509_cert::ext::pkix::name::GeneralName) -> Option<String> {
    use x509_cert::ext::pkix::name::GeneralName;

    match name {
        GeneralName::DnsName(value) => Some(format!("DNS:{}", value.as_str())),
        GeneralName::Rfc822Name(value) => Some(format!("email:{}", value.as_str())),
        GeneralName::UniformResourceIdentifier(value) => Some(format!("URI:{}", value.as_str())),
        GeneralName::IpAddress(value) => {
            let bytes = value.as_bytes();
            match bytes.len() {
                4 => Some(format!(
                    "IP Address:{}.{}.{}.{}",
                    bytes[0], bytes[1], bytes[2], bytes[3]
                )),
                16 => {
                    let segments: Vec<String> = bytes
                        .chunks_exact(2)
                        .map(|chunk| {
                            let segment = u16::from_be_bytes([chunk[0], chunk[1]]);
                            format!("{:x}", segment)
                        })
                        .collect();
                    Some(format!("IP Address:{}", segments.join(":")))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn x509_subject_alt_name(cert: &x509_cert::Certificate) -> Option<String> {
    let san = x509_decoded_subject_alt_name(cert)?;
    let names: Vec<String> = san.0.iter().filter_map(x509_format_general_name).collect();
    if names.is_empty() {
        None
    } else {
        Some(names.join(", "))
    }
}

fn x509_decoded_subject_alt_name(
    cert: &x509_cert::Certificate,
) -> Option<x509_cert::ext::pkix::SubjectAltName> {
    use x509_cert::der::Decode;

    let ext = x509_find_extension(cert, "2.5.29.17")?;
    x509_cert::ext::pkix::SubjectAltName::from_der(ext.extn_value.as_bytes()).ok()
}

fn x509_subject_common_names(cert: &x509_cert::Certificate) -> Vec<String> {
    let mut names = Vec::new();
    for rdn in cert.tbs_certificate.subject.0.iter() {
        for atv in rdn.0.iter() {
            if atv.oid.to_string() == "2.5.4.3" {
                names.push(x509_attr_value(atv));
            }
        }
    }
    names
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum X509HostSubject {
    Default,
    Always,
    Never,
}

fn x509_subject_email_addresses(cert: &x509_cert::Certificate) -> Vec<String> {
    let mut addresses = Vec::new();
    for rdn in cert.tbs_certificate.subject.0.iter() {
        for atv in rdn.0.iter() {
            if atv.oid.to_string() == "1.2.840.113549.1.9.1" {
                addresses.push(x509_attr_value(atv));
            }
        }
    }
    addresses
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum X509CheckSubject {
    Default,
    Always,
    Never,
}

struct X509CheckHostOptions {
    subject: X509HostSubject,
    wildcards: bool,
    partial_wildcards: bool,
    multi_label_wildcards: bool,
}

impl Default for X509CheckHostOptions {
    fn default() -> Self {
        Self {
            subject: X509HostSubject::Default,
            wildcards: true,
            partial_wildcards: true,
            multi_label_wildcards: false,
        }
    }
}

unsafe fn x509_object_field(value: f64, name: &[u8]) -> Option<f64> {
    let js = JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        return None;
    }
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    Some(f64::from_bits(
        js_object_get_field_by_name(js.as_pointer::<ObjectHeader>(), key).bits(),
    ))
}

unsafe fn x509_options_bool(opts: f64, name: &[u8], default: bool) -> bool {
    let Some(value) = x509_object_field(opts, name) else {
        return default;
    };
    let js = JSValue::from_bits(value.to_bits());
    if js.is_bool() {
        js.as_bool()
    } else {
        default
    }
}

unsafe fn x509_check_host_options(args: &[f64]) -> X509CheckHostOptions {
    let mut options = X509CheckHostOptions::default();
    let opts = args
        .get(1)
        .copied()
        .unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits()));

    if let Some(subject) =
        x509_object_field(opts, b"subject").and_then(|value| string_from_jsvalue(value.to_bits()))
    {
        options.subject = match subject.as_str() {
            "always" => X509HostSubject::Always,
            "never" => X509HostSubject::Never,
            _ => X509HostSubject::Default,
        };
    }

    options.wildcards = x509_options_bool(opts, b"wildcards", true);
    options.partial_wildcards = x509_options_bool(opts, b"partialWildcards", true);
    options.multi_label_wildcards = x509_options_bool(opts, b"multiLabelWildcards", false);
    options
}

fn x509_label_count(value: &str) -> usize {
    value.split('.').filter(|label| !label.is_empty()).count()
}

fn x509_host_suffix_matches(host: &str, suffix: &str, options: &X509CheckHostOptions) -> bool {
    if x509_label_count(suffix) < 2 {
        return false;
    }
    if !host.ends_with(suffix) {
        return false;
    }
    let prefix = &host[..host.len() - suffix.len()];
    if !prefix.ends_with('.') {
        return false;
    }
    let prefix = &prefix[..prefix.len() - 1];
    if prefix.is_empty() {
        return false;
    }
    options.multi_label_wildcards || !prefix.contains('.')
}

fn x509_dns_name_matches(candidate: &str, host: &str, options: &X509CheckHostOptions) -> bool {
    let candidate = candidate.to_ascii_lowercase();
    let host = host.to_ascii_lowercase();
    if candidate == host {
        return true;
    }
    if !options.wildcards {
        return false;
    }

    if let Some(suffix) = candidate.strip_prefix("*.") {
        return x509_host_suffix_matches(&host, suffix, options);
    }

    if !options.partial_wildcards {
        return false;
    }
    let Some((candidate_label, candidate_suffix)) = candidate.split_once('.') else {
        return false;
    };
    let Some((host_label, host_suffix)) = host.split_once('.') else {
        return false;
    };
    if candidate_suffix != host_suffix || x509_label_count(candidate_suffix) < 2 {
        return false;
    }
    let Some(star_index) = candidate_label.find('*') else {
        return false;
    };
    if candidate_label[star_index + 1..].contains('*') {
        return false;
    }
    let prefix = &candidate_label[..star_index];
    let suffix = &candidate_label[star_index + 1..];
    !host_label.is_empty() && host_label.starts_with(prefix) && host_label.ends_with(suffix)
}

unsafe fn x509_check_subject_option(args: &[f64]) -> X509CheckSubject {
    let Some(options) = args.get(1) else {
        return X509CheckSubject::Default;
    };
    let Some(subject) = object_field_string(options.to_bits(), b"subject") else {
        return X509CheckSubject::Default;
    };
    match subject.as_str() {
        "always" => X509CheckSubject::Always,
        "never" => X509CheckSubject::Never,
        _ => X509CheckSubject::Default,
    }
}

fn x509_check_host_value(
    cert: &x509_cert::Certificate,
    name: &str,
    options: &X509CheckHostOptions,
) -> Option<String> {
    use x509_cert::ext::pkix::name::GeneralName;

    let mut saw_dns_san = false;
    if let Some(san) = x509_decoded_subject_alt_name(cert) {
        for general_name in san.0.iter() {
            let GeneralName::DnsName(value) = general_name else {
                continue;
            };
            saw_dns_san = true;
            let candidate = value.as_str();
            if x509_dns_name_matches(candidate, name, options) {
                return Some(candidate.to_string());
            }
        }
    }

    if options.subject == X509HostSubject::Never
        || (saw_dns_san && options.subject != X509HostSubject::Always)
    {
        return None;
    }

    x509_subject_common_names(cert)
        .into_iter()
        .find(|candidate| x509_dns_name_matches(candidate, name, options))
}

fn x509_check_email_value(
    cert: &x509_cert::Certificate,
    email: &str,
    subject: X509CheckSubject,
) -> Option<String> {
    use x509_cert::ext::pkix::name::GeneralName;

    let mut saw_email_san = false;
    if let Some(san) = x509_decoded_subject_alt_name(cert) {
        for general_name in san.0.iter() {
            let GeneralName::Rfc822Name(value) = general_name else {
                continue;
            };
            saw_email_san = true;
            let candidate = value.as_str();
            if candidate == email {
                return Some(candidate.to_string());
            }
        }
    }

    if subject == X509CheckSubject::Never || (saw_email_san && subject != X509CheckSubject::Always)
    {
        return None;
    }

    x509_subject_email_addresses(cert)
        .into_iter()
        .find(|candidate| candidate == email)
}

fn x509_check_ip_value(cert: &x509_cert::Certificate, ip: &str) -> Option<String> {
    use std::net::IpAddr;
    use x509_cert::ext::pkix::name::GeneralName;

    let parsed = ip.parse::<IpAddr>().unwrap_or_else(|_| {
        perry_runtime::fs::validate::throw_type_error_with_code(
            "Invalid IP",
            "ERR_INVALID_ARG_VALUE",
        )
    });
    let san = x509_decoded_subject_alt_name(cert)?;
    san.0.iter().find_map(|general_name| {
        let GeneralName::IpAddress(value) = general_name else {
            return None;
        };
        let bytes = value.as_bytes();
        match parsed {
            IpAddr::V4(addr) if bytes == addr.octets().as_slice() => Some(ip.to_string()),
            IpAddr::V6(addr) if bytes == addr.octets().as_slice() => Some(ip.to_string()),
            _ => None,
        }
    })
}

fn x509_extended_key_usage(cert: &x509_cert::Certificate) -> Option<Vec<String>> {
    use x509_cert::der::Decode;

    let ext = x509_find_extension(cert, "2.5.29.37")?;
    let key_usage =
        x509_cert::ext::pkix::ExtendedKeyUsage::from_der(ext.extn_value.as_bytes()).ok()?;
    let usages: Vec<String> = key_usage.0.iter().map(|oid| oid.to_string()).collect();
    if usages.is_empty() {
        None
    } else {
        Some(usages)
    }
}

fn x509_info_access_method_name(oid: &str) -> String {
    match oid {
        "1.3.6.1.5.5.7.48.1" => "OCSP".to_string(),
        "1.3.6.1.5.5.7.48.2" => "CA Issuers".to_string(),
        other => other.to_string(),
    }
}

fn x509_info_access(cert: &x509_cert::Certificate) -> Option<String> {
    use x509_cert::der::Decode;

    let ext = x509_find_extension(cert, "1.3.6.1.5.5.7.1.1")?;
    let info_access =
        x509_cert::ext::pkix::AuthorityInfoAccessSyntax::from_der(ext.extn_value.as_bytes())
            .ok()?;
    let lines: Vec<String> = info_access
        .0
        .iter()
        .filter_map(|desc| {
            let method = x509_info_access_method_name(&desc.access_method.to_string());
            let location = x509_format_general_name(&desc.access_location)?;
            Some(format!("{} - {}", method, location))
        })
        .collect();
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn x509_handle_arg(value: f64) -> Option<i64> {
    let bits = value.to_bits();
    if (bits & 0xFFFF_0000_0000_0000) != 0x7FFD_0000_0000_0000 {
        return None;
    }
    let handle = (bits & 0x0000_FFFF_FFFF_FFFF) as i64;
    crate::common::handle::with_handle::<X509Handle, bool, _>(handle, |_| true)
        .unwrap_or(false)
        .then_some(handle)
}

fn x509_throw_certificate_arg(value: f64) -> ! {
    let message = format!(
        "The \"otherCert\" argument must be an instance of X509Certificate. Received {}",
        perry_runtime::fs::validate::describe_received(value)
    );
    perry_runtime::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn x509_name_der(name: &x509_cert::name::Name) -> Option<Vec<u8>> {
    use x509_cert::der::Encode;

    name.to_der().ok()
}

fn x509_check_issued_value(cert: &x509_cert::Certificate, issuer: &x509_cert::Certificate) -> bool {
    use x509_cert::der::Encode;

    let Some(issuer_name) = x509_name_der(&cert.tbs_certificate.issuer) else {
        return false;
    };
    let Some(subject_name) = x509_name_der(&issuer.tbs_certificate.subject) else {
        return false;
    };
    if issuer_name != subject_name {
        return false;
    }

    let Some(alg) = x509_rsa_signature_digest(cert) else {
        return false;
    };
    let Some((_, public_key)) = x509_rsa_public_key(issuer) else {
        return false;
    };
    let Some(signature_bytes) = cert.signature.as_bytes() else {
        return false;
    };
    let Ok(signature) = RsaPkcs1v15Signature::try_from(signature_bytes) else {
        return false;
    };
    let Ok(tbs_der) = cert.tbs_certificate.to_der() else {
        return false;
    };

    verify_rsa_data(alg, public_key, &tbs_der, &signature)
}

fn x509_string_f64(s: &str) -> f64 {
    let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

unsafe fn x509_string_array_f64(items: &[String]) -> f64 {
    let mut arr = perry_runtime::js_array_alloc(items.len() as u32);
    for item in items {
        let s = js_string_from_bytes(item.as_ptr(), item.len() as u32);
        arr = perry_runtime::js_array_push(arr, JSValue::string_ptr(s));
    }
    nanbox_ptr(arr)
}

fn x509_time_millis(time: &x509_cert::time::Time) -> f64 {
    let dt = time.to_date_time();
    let Some(date) =
        chrono::NaiveDate::from_ymd_opt(dt.year() as i32, dt.month() as u32, dt.day() as u32)
    else {
        return f64::NAN;
    };
    let Some(time) =
        chrono::NaiveTime::from_hms_opt(dt.hour() as u32, dt.minutes() as u32, dt.seconds() as u32)
    else {
        return f64::NAN;
    };
    let dt = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
        date.and_time(time),
        chrono::Utc,
    );
    dt.timestamp_millis() as f64
}

fn x509_der_to_pem(der: &[u8]) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(der);
    let mut pem = String::from("-----BEGIN CERTIFICATE-----\n");
    for chunk in b64.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        pem.push('\n');
    }
    pem.push_str("-----END CERTIFICATE-----\n");
    pem
}

fn x509_public_key_pem(spki_der: &[u8]) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(spki_der);
    let mut pem = String::from("-----BEGIN PUBLIC KEY-----\n");
    for chunk in b64.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        pem.push('\n');
    }
    pem.push_str("-----END PUBLIC KEY-----\n");
    pem
}

unsafe fn x509_public_key_value(handle: &X509Handle) -> f64 {
    use x509_cert::der::Encode;

    let spki_der = match handle.cert.tbs_certificate.subject_public_key_info.to_der() {
        Ok(der) => der,
        Err(_) => return nanbox_undefined(),
    };
    let pem = x509_public_key_pem(&spki_der);
    let ptr = js_string_from_bytes(pem.as_ptr(), pem.len() as u32);
    if let Some(asym_type) = classify_public_key_surrogate(&pem) {
        mark_keyobject_string(ptr, KeyKind::Public, asym_type);
    }
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn x509_signature_algorithm_oid(cert: &x509_cert::Certificate) -> String {
    cert.signature_algorithm.oid.to_string()
}

fn x509_signature_algorithm_name(cert: &x509_cert::Certificate) -> Option<&'static str> {
    match x509_signature_algorithm_oid(cert).as_str() {
        "1.2.840.113549.1.1.5" => Some("sha1WithRSAEncryption"),
        "1.2.840.113549.1.1.11" => Some("sha256WithRSAEncryption"),
        "1.2.840.113549.1.1.12" => Some("sha384WithRSAEncryption"),
        "1.2.840.113549.1.1.13" => Some("sha512WithRSAEncryption"),
        "1.2.840.10045.4.3.2" => Some("ecdsa-with-SHA256"),
        "1.2.840.10045.4.3.3" => Some("ecdsa-with-SHA384"),
        "1.2.840.10045.4.3.4" => Some("ecdsa-with-SHA512"),
        _ => None,
    }
}

fn x509_rsa_signature_digest(cert: &x509_cert::Certificate) -> Option<RsaDigestKind> {
    match cert.signature_algorithm.oid.to_string().as_str() {
        "1.2.840.113549.1.1.11" => Some(RsaDigestKind::Sha256),
        "1.2.840.113549.1.1.12" => Some(RsaDigestKind::Sha384),
        "1.2.840.113549.1.1.13" => Some(RsaDigestKind::Sha512),
        _ => None,
    }
}

unsafe fn x509_name_legacy_object(name: &x509_cert::name::Name) -> f64 {
    let attr_count = name.0.iter().map(|rdn| rdn.0.len()).sum::<usize>() as u32;
    let obj = js_object_alloc(0, attr_count);
    for rdn in name.0.iter() {
        for atv in rdn.0.iter() {
            let key = x509_attr_short_name(&atv.oid.to_string());
            let value = x509_attr_value(atv);
            set_object_string_field(obj, key.as_bytes(), &value);
        }
    }
    nanbox_ptr(obj)
}

fn x509_rsa_public_key(cert: &x509_cert::Certificate) -> Option<(Vec<u8>, RsaPublicKey)> {
    use x509_cert::der::Encode;

    let spki_der = cert.tbs_certificate.subject_public_key_info.to_der().ok()?;
    let pem = x509_public_key_pem(&spki_der);
    let public_key = parse_rsa_public_key_pem(&pem)?;
    Some((spki_der, public_key))
}

fn x509_serial_number_hex(cert: &x509_cert::Certificate) -> String {
    cert.tbs_certificate
        .serial_number
        .as_bytes()
        .iter()
        .map(|b| format!("{:02X}", b))
        .collect()
}

unsafe fn set_undefined_field(obj: *mut ObjectHeader, name: &[u8]) {
    set_object_value_field(obj, name, nanbox_undefined());
}

fn x509_bool_f64(value: bool) -> f64 {
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

    f64::from_bits(if value { TAG_TRUE } else { TAG_FALSE })
}

unsafe fn x509_to_legacy_object(handle: &X509Handle) -> f64 {
    use sha1::Sha1;
    use sha2::{Digest, Sha256, Sha512};

    let obj = js_object_alloc(0, 20);
    let tbs = &handle.cert.tbs_certificate;

    set_object_value_field(obj, b"subject", x509_name_legacy_object(&tbs.subject));
    set_object_value_field(obj, b"issuer", x509_name_legacy_object(&tbs.issuer));
    match x509_subject_alt_name(&handle.cert) {
        Some(value) => set_object_string_field(obj, b"subjectaltname", &value),
        None => set_undefined_field(obj, b"subjectaltname"),
    }
    set_undefined_field(obj, b"infoAccess");
    set_object_value_field(
        obj,
        b"ca",
        x509_bool_f64(x509_basic_constraints_ca(&handle.cert)),
    );

    if let Some((spki_der, public_key)) = x509_rsa_public_key(&handle.cert) {
        set_object_string_field(
            obj,
            b"modulus",
            &hex::encode_upper(public_key.n().to_bytes_be()),
        );
        set_object_string_field(
            obj,
            b"exponent",
            &format!("0x{}", public_key.e().to_str_radix(16)),
        );
        set_object_value_field(
            obj,
            b"pubkey",
            nanbox_ptr(alloc_buffer_from_slice(&spki_der)),
        );
        set_object_value_field(obj, b"bits", public_key.n().bits() as f64);
    } else {
        set_undefined_field(obj, b"modulus");
        set_undefined_field(obj, b"exponent");
        set_undefined_field(obj, b"pubkey");
        set_undefined_field(obj, b"bits");
    }

    set_object_string_field(
        obj,
        b"valid_from",
        &x509_format_time(&tbs.validity.not_before),
    );
    set_object_string_field(obj, b"valid_to", &x509_format_time(&tbs.validity.not_after));
    set_object_string_field(
        obj,
        b"fingerprint",
        &x509_colon_hex(&Sha1::digest(&handle.der)),
    );
    set_object_string_field(
        obj,
        b"fingerprint256",
        &x509_colon_hex(&Sha256::digest(&handle.der)),
    );
    set_object_string_field(
        obj,
        b"fingerprint512",
        &x509_colon_hex(&Sha512::digest(&handle.der)),
    );
    match x509_extended_key_usage(&handle.cert) {
        Some(values) => {
            set_object_value_field(obj, b"ext_key_usage", x509_string_array_f64(&values))
        }
        None => set_undefined_field(obj, b"ext_key_usage"),
    }
    set_object_string_field(obj, b"serialNumber", &x509_serial_number_hex(&handle.cert));
    set_object_value_field(
        obj,
        b"raw",
        nanbox_ptr(alloc_buffer_from_slice(&handle.der)),
    );
    set_undefined_field(obj, b"asn1Curve");
    set_undefined_field(obj, b"nistCurve");

    nanbox_ptr(obj)
}

unsafe fn x509_asymmetric_key_meta(value: f64) -> Option<(u8, u8)> {
    let bits = value.to_bits();
    let js = JSValue::from_bits(bits);
    if !js.is_any_string() || (bits >> 48) as u16 != 0x7FFF {
        return None;
    }
    let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return None;
    }
    perry_runtime::buffer::asymmetric_key_meta(ptr as usize)
}

enum X509PkeyKind {
    Public,
    Private(u8),
    Secret,
}

unsafe fn x509_pkey_kind(value: f64) -> Option<X509PkeyKind> {
    let bits = value.to_bits();
    let js = JSValue::from_bits(bits);
    if js.is_any_string() && (bits >> 48) as u16 == 0x7FFF {
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
        if !ptr.is_null() && (ptr as usize) >= 0x1000 {
            return perry_runtime::buffer::asymmetric_key_meta(ptr as usize).map(
                |(kind, asym_type)| {
                    if kind == 2 {
                        X509PkeyKind::Private(asym_type)
                    } else {
                        X509PkeyKind::Public
                    }
                },
            );
        }
    }
    if js.is_pointer() {
        let addr = bits & 0x0000_FFFF_FFFF_FFFF;
        if addr >= 0x1000 && perry_runtime::buffer::is_secret_key(addr as usize) {
            return Some(X509PkeyKind::Secret);
        }
    }
    None
}

fn throw_x509_pkey_type_error(value: f64) -> ! {
    let message = format!(
        "The \"pkey\" argument must be an instance of KeyObject. Received {}",
        perry_runtime::fs::validate::describe_received(value)
    );
    perry_runtime::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn throw_x509_pkey_value_error(kind: u8) -> ! {
    let received = if kind == 2 {
        "PrivateKeyObject {}"
    } else {
        "KeyObject {}"
    };
    let message = format!("The argument 'pkey' is invalid. Received {received}");
    perry_runtime::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE")
}

fn throw_x509_pkey_value_error_typed(kind: X509PkeyKind) -> ! {
    let received = match kind {
        X509PkeyKind::Public => "PublicKeyObject {}",
        X509PkeyKind::Private(_) => "PrivateKeyObject {}",
        X509PkeyKind::Secret => "SecretKeyObject {}",
    };
    let message = format!("The argument 'pkey' is invalid. Received {received}");
    perry_runtime::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE")
}

unsafe fn x509_verify_value(handle: &X509Handle, args: &[f64]) -> f64 {
    use x509_cert::der::Encode;

    let key_value = args
        .first()
        .copied()
        .unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits()));
    let Some((kind, _asym_type)) = x509_asymmetric_key_meta(key_value) else {
        throw_x509_pkey_type_error(key_value);
    };
    if kind != 1 {
        throw_x509_pkey_value_error(kind);
    }

    let alg = match x509_rsa_signature_digest(&handle.cert) {
        Some(alg) => alg,
        None => return js_bool(false),
    };
    let pem = match crypto_key_input_to_public_pem(key_value.to_bits()) {
        Some(pem) => pem,
        None => return js_bool(false),
    };
    let public_key = match parse_rsa_public_key_pem(&pem) {
        Some(key) => key,
        None => return js_bool(false),
    };
    let tbs_der = match handle.cert.tbs_certificate.to_der() {
        Ok(der) => der,
        Err(_) => return js_bool(false),
    };
    let signature = match handle
        .cert
        .signature
        .as_bytes()
        .and_then(|bytes| RsaPkcs1v15Signature::try_from(bytes).ok())
    {
        Some(signature) => signature,
        None => return js_bool(false),
    };
    js_bool(verify_rsa_data(alg, public_key, &tbs_der, &signature))
}

unsafe fn x509_check_private_key_value(handle: &X509Handle, args: &[f64]) -> f64 {
    let key_value = args
        .first()
        .copied()
        .unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits()));
    let kind = match x509_pkey_kind(key_value) {
        Some(kind) => kind,
        None => throw_x509_pkey_type_error(key_value),
    };
    let X509PkeyKind::Private(asym_type) = kind else {
        throw_x509_pkey_value_error_typed(kind);
    };
    if asym_type != 1 {
        return js_bool(false);
    }

    let pem = match crypto_key_input_to_private_pem(key_value.to_bits()) {
        Some(pem) => pem,
        None => return js_bool(false),
    };
    let private_key = match parse_rsa_private_key_pem(&pem) {
        Some(key) => key,
        None => return js_bool(false),
    };
    let cert_public_key = match x509_rsa_public_key(&handle.cert) {
        Some((_, key)) => key,
        None => return js_bool(false),
    };
    let private_public_key = RsaPublicKey::from(&private_key);
    js_bool(
        cert_public_key.n() == private_public_key.n()
            && cert_public_key.e() == private_public_key.e(),
    )
}

fn x509_handle_value(handle: Handle) -> f64 {
    f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
}

fn throw_x509_parse_error(message: &str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = perry_runtime::error::js_error_new_with_message(msg);
    perry_runtime::exception::js_throw(perry_runtime::value::js_nanbox_pointer(err as i64))
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn decode_x509_pem(bytes: &[u8]) -> Option<Vec<u8>> {
    const BEGIN: &[u8] = b"-----BEGIN CERTIFICATE-----";
    const END: &[u8] = b"-----END CERTIFICATE-----";

    let start = bytes.strip_prefix(BEGIN)?;
    let end = find_bytes(start, END)?;
    let mut body = Vec::with_capacity(end);
    for &byte in &start[..end] {
        if !matches!(byte, b'\r' | b'\n' | b'\t' | b' ') {
            body.push(byte);
        }
    }
    if body.is_empty() {
        return None;
    }
    base64::engine::general_purpose::STANDARD.decode(body).ok()
}

fn complete_der_sequence_len(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 2 || bytes[0] != 0x30 {
        return None;
    }
    let first_len = bytes[1];
    if first_len & 0x80 == 0 {
        return Some(2 + first_len as usize);
    }

    let len_bytes = (first_len & 0x7f) as usize;
    if len_bytes == 0 || len_bytes > std::mem::size_of::<usize>() || bytes.len() < 2 + len_bytes {
        return None;
    }
    let mut payload_len = 0usize;
    for &byte in &bytes[2..2 + len_bytes] {
        payload_len = payload_len.checked_shl(8)?.checked_add(byte as usize)?;
    }
    (2 + len_bytes).checked_add(payload_len)
}

fn is_complete_der_sequence(bytes: &[u8]) -> bool {
    matches!(complete_der_sequence_len(bytes), Some(len) if len == bytes.len())
}

/// `new crypto.X509Certificate(pem | der)` — parse and register a handle.
/// Accepts a PEM string or a DER Buffer/Uint8Array. Invalid input throws
/// a catchable Error instead of returning an undefined sentinel.
///
/// # Safety
/// `input_ptr` must be a valid string/buffer pointer (the codegen-unboxed
/// constructor argument).
#[no_mangle]
pub unsafe extern "C" fn js_crypto_x509_new(input_ptr: i64) -> f64 {
    use x509_cert::der::{Decode, Encode};
    let bytes = bytes_from_ptr(input_ptr);
    let der = if bytes.starts_with(b"-----BEGIN") {
        match decode_x509_pem(&bytes) {
            Some(der) if is_complete_der_sequence(&der) => der,
            _ => throw_x509_parse_error("error:0480006C:PEM routines::no start line"),
        }
    } else {
        if !is_complete_der_sequence(&bytes) {
            throw_x509_parse_error("error:0680007B:asn1 encoding routines::header too long");
        }
        bytes
    };
    let cert = match x509_cert::Certificate::from_der(&der) {
        Ok(c) => c,
        Err(_) => throw_x509_parse_error("error:0680007B:asn1 encoding routines::header too long"),
    };
    let der = match cert.to_der() {
        Ok(d) => d,
        Err(_) => throw_x509_parse_error("error:0680007B:asn1 encoding routines::header too long"),
    };
    let handle: Handle = register_handle(X509Handle {
        der,
        cert,
        issuer_certificate: None,
    });
    x509_handle_value(handle)
}

/// Read-only property dispatch for an X509Certificate handle.
pub unsafe fn dispatch_x509_property(handle: i64, property: &str) -> f64 {
    use sha1::Sha1;
    use sha2::{Digest, Sha256, Sha512};
    if matches!(
        property,
        "toString"
            | "toJSON"
            | "toLegacyObject"
            | "checkHost"
            | "checkEmail"
            | "checkIP"
            | "verify"
            | "checkPrivateKey"
            | "checkIssued"
    ) {
        return dispatch_x509_method_property(handle, property);
    }
    let h = match get_handle_mut::<X509Handle>(handle) {
        Some(h) => h,
        None => return nanbox_undefined(),
    };
    let tbs = &h.cert.tbs_certificate;
    match property {
        "subject" => x509_string_f64(&x509_format_name(&tbs.subject)),
        "issuer" => x509_string_f64(&x509_format_name(&tbs.issuer)),
        "validFrom" => x509_string_f64(&x509_format_time(&tbs.validity.not_before)),
        "validTo" => x509_string_f64(&x509_format_time(&tbs.validity.not_after)),
        "validFromDate" => perry_runtime::date::js_date_new_from_timestamp(x509_time_millis(
            &tbs.validity.not_before,
        )),
        "validToDate" => perry_runtime::date::js_date_new_from_timestamp(x509_time_millis(
            &tbs.validity.not_after,
        )),
        "serialNumber" => x509_string_f64(&x509_serial_number_hex(&h.cert)),
        "signatureAlgorithm" => match x509_signature_algorithm_name(&h.cert) {
            Some(value) => x509_string_f64(value),
            None => nanbox_undefined(),
        },
        "signatureAlgorithmOid" => x509_string_f64(&x509_signature_algorithm_oid(&h.cert)),
        "fingerprint" => {
            let digest = Sha1::digest(&h.der);
            x509_string_f64(&x509_colon_hex(&digest))
        }
        "fingerprint256" => {
            let digest = Sha256::digest(&h.der);
            x509_string_f64(&x509_colon_hex(&digest))
        }
        "fingerprint512" => {
            let digest = Sha512::digest(&h.der);
            x509_string_f64(&x509_colon_hex(&digest))
        }
        "subjectAltName" => match x509_subject_alt_name(&h.cert) {
            Some(value) => x509_string_f64(&value),
            None => nanbox_undefined(),
        },
        "keyUsage" => match x509_extended_key_usage(&h.cert) {
            Some(values) => x509_string_array_f64(&values),
            None => nanbox_undefined(),
        },
        "infoAccess" => match x509_info_access(&h.cert) {
            Some(value) => x509_string_f64(&value),
            None => nanbox_undefined(),
        },
        "ca" => {
            // BasicConstraints (id-ce 2.5.29.19) cA flag.
            x509_bool_f64(x509_basic_constraints_ca(&h.cert))
        }
        "raw" => {
            let buf = alloc_buffer_from_slice(&h.der);
            f64::from_bits(0x7FFD_0000_0000_0000u64 | ((buf as u64) & 0x0000_FFFF_FFFF_FFFF))
        }
        "publicKey" => x509_public_key_value(h),
        "issuerCertificate" => h
            .issuer_certificate
            .map(x509_handle_value)
            .unwrap_or_else(nanbox_undefined),
        _ => nanbox_undefined(),
    }
}

pub unsafe fn dispatch_x509_method(handle: i64, method: &str, args: &[f64]) -> f64 {
    let h = match get_handle_mut::<X509Handle>(handle) {
        Some(h) => h,
        None => return nanbox_undefined(),
    };
    match method {
        "toString" | "toJSON" => x509_string_f64(&x509_der_to_pem(&h.der)),
        "toLegacyObject" => x509_to_legacy_object(h),
        "checkHost" => {
            let options = x509_check_host_options(args);
            match x509_check_host_value(&h.cert, &x509_required_string_arg(args, "name"), &options)
            {
                Some(value) => x509_string_f64(&value),
                None => nanbox_undefined(),
            }
        }
        "checkEmail" => {
            let subject = x509_check_subject_option(args);
            match x509_check_email_value(&h.cert, &x509_required_string_arg(args, "email"), subject)
            {
                Some(value) => x509_string_f64(&value),
                None => nanbox_undefined(),
            }
        }
        "checkIP" => match x509_check_ip_value(&h.cert, &x509_required_string_arg(args, "ip")) {
            Some(value) => x509_string_f64(&value),
            None => nanbox_undefined(),
        },
        "verify" => x509_verify_value(h, args),
        "checkPrivateKey" => x509_check_private_key_value(h, args),
        "checkIssued" => {
            let value = args
                .first()
                .copied()
                .unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits()));
            let Some(other_handle) = x509_handle_arg(value) else {
                x509_throw_certificate_arg(value);
            };
            let cert = h.cert.clone();
            let Some(other_cert) =
                get_handle_mut::<X509Handle>(other_handle).map(|other| other.cert.clone())
            else {
                x509_throw_certificate_arg(value);
            };
            js_bool(x509_check_issued_value(&cert, &other_cert))
        }
        _ => nanbox_undefined(),
    }
}

unsafe fn x509_required_string_arg(args: &[f64], arg_name: &str) -> String {
    let value = args
        .first()
        .copied()
        .unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits()));
    if !JSValue::from_bits(value.to_bits()).is_any_string() {
        let message = format!(
            "The \"{}\" argument must be of type string. Received {}",
            arg_name,
            perry_runtime::fs::validate::describe_received(value)
        );
        perry_runtime::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    string_from_jsvalue(value.to_bits()).unwrap_or_default()
}

pub unsafe fn dispatch_x509_method_property(handle: i64, property: &str) -> f64 {
    let name_bytes: &'static [u8] = match property {
        "toString" => b"toString",
        "toJSON" => b"toJSON",
        "toLegacyObject" => b"toLegacyObject",
        "checkHost" => b"checkHost",
        "checkEmail" => b"checkEmail",
        "checkIP" => b"checkIP",
        "verify" => b"verify",
        "checkPrivateKey" => b"checkPrivateKey",
        "checkIssued" => b"checkIssued",
        _ => return nanbox_undefined(),
    };
    let this_f64 =
        f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF));
    extern "C" {
        fn js_class_method_bind(
            instance: f64,
            method_name_ptr: *const u8,
            method_name_len: usize,
        ) -> f64;
    }
    js_class_method_bind(this_f64, name_bytes.as_ptr(), name_bytes.len())
}

/// Extract the BasicConstraints `cA` flag (default false when absent).
pub(super) fn x509_basic_constraints_ca(cert: &x509_cert::Certificate) -> bool {
    use x509_cert::der::Decode;
    let Some(exts) = cert.tbs_certificate.extensions.as_ref() else {
        return false;
    };
    for ext in exts.iter() {
        if ext.extn_id.to_string() == "2.5.29.19" {
            if let Ok(bc) =
                x509_cert::ext::pkix::BasicConstraints::from_der(ext.extn_value.as_bytes())
            {
                return bc.ca;
            }
        }
    }
    false
}

pub(super) const DH_DEFAULT_PRIME_HEX: &str = concat!(
    "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD1",
    "29024E088A67CC74020BBEA63B139B22514A08798E3404DD",
    "EF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245",
    "E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7ED",
    "EE386BFB5A899FA5AE9F24117C4B1FE649286651ECE65381",
    "FFFFFFFFFFFFFFFF"
);

pub(super) fn dh_default_prime() -> Vec<u8> {
    hex::decode(DH_DEFAULT_PRIME_HEX).unwrap_or_else(|_| vec![0xff; 128])
}

pub(super) fn dh_default_generator() -> Vec<u8> {
    vec![2]
}

pub(super) fn bigint_to_padded_bytes(n: &RsaBigUint, len: usize) -> Vec<u8> {
    let mut bytes = n.to_bytes_be();
    if bytes.len() > len {
        bytes = bytes[bytes.len() - len..].to_vec();
    } else if bytes.len() < len {
        let mut padded = vec![0u8; len - bytes.len()];
        padded.extend_from_slice(&bytes);
        bytes = padded;
    }
    bytes
}

pub(super) fn dh_public_from_private(
    prime: &[u8],
    generator: &[u8],
    private_key: &[u8],
) -> Vec<u8> {
    let p = RsaBigUint::from_bytes_be(prime);
    let g = RsaBigUint::from_bytes_be(generator);
    let x = RsaBigUint::from_bytes_be(private_key);
    let y = g.modpow(&x, &p);
    bigint_to_padded_bytes(&y, prime.len())
}

pub(super) fn dh_secret(prime: &[u8], private_key: &[u8], other_public_key: &[u8]) -> Vec<u8> {
    let p = RsaBigUint::from_bytes_be(prime);
    let x = RsaBigUint::from_bytes_be(private_key);
    let y = RsaBigUint::from_bytes_be(other_public_key);
    let s = y.modpow(&x, &p);
    bigint_to_padded_bytes(&s, prime.len())
}

pub(super) fn dh_random_private_key(prime: &[u8]) -> Vec<u8> {
    let p = RsaBigUint::from_bytes_be(prime);
    let two = RsaBigUint::from(2u32);
    let mut rng = rand::thread_rng();
    for _ in 0..128 {
        let mut bytes = vec![0u8; prime.len()];
        rng.fill_bytes(&mut bytes);
        let x = RsaBigUint::from_bytes_be(&bytes);
        if x > two && x < p {
            return bigint_to_padded_bytes(&x, prime.len());
        }
    }
    let fallback = RsaBigUint::from(3u32);
    bigint_to_padded_bytes(&fallback, prime.len())
}

pub(super) fn nanbox_ptr<T>(ptr: *mut T) -> f64 {
    f64::from_bits(0x7FFD_0000_0000_0000u64 | ((ptr as u64) & 0x0000_FFFF_FFFF_FFFF))
}

pub(super) fn arg_ptr(arg: f64) -> i64 {
    (arg.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64
}

pub(super) unsafe fn arg_bytes(args: &[f64], idx: usize) -> Vec<u8> {
    args.get(idx)
        .map(|arg| bytes_from_ptr(arg_ptr(*arg)))
        .unwrap_or_default()
}

pub(super) unsafe fn arg_string(args: &[f64], idx: usize) -> String {
    String::from_utf8(arg_bytes(args, idx)).unwrap_or_default()
}

pub(super) unsafe fn string_value(bytes: &[u8]) -> f64 {
    let s = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    nanbox_str(s)
}

pub(super) unsafe fn ecdh_output(bytes: &[u8], encoding: Option<&str>) -> f64 {
    if matches!(encoding, Some(enc) if enc.eq_ignore_ascii_case("hex")) {
        return string_value(hex::encode(bytes).as_bytes());
    }
    if matches!(encoding, Some(enc) if enc.eq_ignore_ascii_case("base64")) {
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        return string_value(encoded.as_bytes());
    }
    nanbox_ptr(alloc_buffer_from_slice(bytes))
}

pub(super) unsafe fn decode_ecdh_input(ptr: i64, encoding: &str) -> Vec<u8> {
    let bytes = bytes_from_ptr(ptr);
    if encoding.eq_ignore_ascii_case("hex") {
        let s = String::from_utf8(bytes).unwrap_or_default();
        return hex::decode(s).unwrap_or_default();
    }
    if encoding.eq_ignore_ascii_case("base64") {
        let s = String::from_utf8(bytes).unwrap_or_default();
        return base64::engine::general_purpose::STANDARD
            .decode(s.as_bytes())
            .unwrap_or_default();
    }
    bytes
}

pub(super) unsafe fn decode_crypto_value(value: f64, encoding: &str) -> Vec<u8> {
    decode_ecdh_input(arg_ptr(value), encoding)
}

pub(super) unsafe fn decode_hash_update_value(value: f64, encoding: &str) -> Vec<u8> {
    let bytes = bytes_from_ptr(arg_ptr(value));
    if encoding.eq_ignore_ascii_case("hex") {
        let s = String::from_utf8(bytes).unwrap_or_default();
        return hex::decode(s).unwrap_or_default();
    }
    if encoding.eq_ignore_ascii_case("base64") {
        let s = String::from_utf8(bytes).unwrap_or_default();
        return base64::engine::general_purpose::STANDARD
            .decode(s.as_bytes())
            .unwrap_or_default();
    }
    if encoding.eq_ignore_ascii_case("base64url") {
        let s = String::from_utf8(bytes).unwrap_or_default();
        return base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(s.as_bytes())
            .unwrap_or_default();
    }
    bytes
}

pub(super) unsafe fn decode_dh_prime_value(value: f64, encoding: &str) -> Vec<u8> {
    if value.is_finite() {
        return dh_default_prime();
    }
    let decoded = decode_crypto_value(value, encoding);
    if decoded.is_empty() {
        dh_default_prime()
    } else {
        decoded
    }
}

pub(super) unsafe fn decode_dh_generator_value(value: Option<f64>, encoding: &str) -> Vec<u8> {
    let Some(value) = value else {
        return dh_default_generator();
    };
    if value.is_finite() {
        let n = value as u64;
        if n == 0 {
            return dh_default_generator();
        }
        let bytes = RsaBigUint::from(n).to_bytes_be();
        return if bytes.is_empty() {
            dh_default_generator()
        } else {
            bytes
        };
    }
    let decoded = decode_crypto_value(value, encoding);
    if decoded.is_empty() {
        dh_default_generator()
    } else {
        decoded
    }
}

pub(super) fn generate_p256_secret_key() -> Option<P256SecretKey> {
    let mut rng = rand::thread_rng();
    for _ in 0..128 {
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);
        if let Ok(key) = P256SecretKey::from_slice(&bytes) {
            return Some(key);
        }
    }
    None
}

pub(super) fn p256_public_bytes(private_key: &P256SecretKey, format: &str) -> Vec<u8> {
    let compressed = format.eq_ignore_ascii_case("compressed");
    private_key
        .public_key()
        .to_encoded_point(compressed)
        .as_bytes()
        .to_vec()
}
