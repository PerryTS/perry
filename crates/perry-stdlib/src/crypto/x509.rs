use super::*;

pub struct X509Handle {
    der: Vec<u8>,
    cert: x509_cert::Certificate,
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
    use x509_cert::der::Encode;
    let mut lines: Vec<String> = Vec::new();
    for rdn in name.0.iter() {
        for atv in rdn.0.iter() {
            let oid = atv.oid.to_string();
            // The value is an AttributeValue (ASN.1 Any); decode common
            // string forms. Fall back to its UTF-8 lossy DER tail.
            let value = atv
                .value
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
                });
            lines.push(format!("{}={}", x509_attr_short_name(&oid), value));
        }
    }
    lines.join("\n")
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

/// `new crypto.X509Certificate(pem | der)` — parse and register a handle.
/// Accepts a PEM string or a DER Buffer/Uint8Array. Returns `undefined`
/// on a parse failure (Node throws; the stub degrades to undefined).
///
/// # Safety
/// `input_ptr` must be a valid string/buffer pointer (the codegen-unboxed
/// constructor argument).
#[no_mangle]
pub unsafe extern "C" fn js_crypto_x509_new(input_ptr: i64) -> f64 {
    use x509_cert::der::{Decode, DecodePem, Encode};
    let bytes = bytes_from_ptr(input_ptr);
    let cert = if bytes.starts_with(b"-----BEGIN") {
        match x509_cert::Certificate::from_pem(&bytes) {
            Ok(c) => c,
            Err(_) => return nanbox_undefined(),
        }
    } else {
        match x509_cert::Certificate::from_der(&bytes) {
            Ok(c) => c,
            Err(_) => return nanbox_undefined(),
        }
    };
    let der = match cert.to_der() {
        Ok(d) => d,
        Err(_) => return nanbox_undefined(),
    };
    let handle: Handle = register_handle(X509Handle { der, cert });
    f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
}

/// Read-only property dispatch for an X509Certificate handle.
pub unsafe fn dispatch_x509_property(handle: i64, property: &str) -> f64 {
    use sha1::Sha1;
    use sha2::{Digest, Sha256};
    let h = match get_handle_mut::<X509Handle>(handle) {
        Some(h) => h,
        None => return nanbox_undefined(),
    };
    let string_f64 = |s: &str| -> f64 {
        let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
        f64::from_bits(JSValue::string_ptr(ptr).bits())
    };
    let tbs = &h.cert.tbs_certificate;
    match property {
        "subject" => string_f64(&x509_format_name(&tbs.subject)),
        "issuer" => string_f64(&x509_format_name(&tbs.issuer)),
        "validFrom" => string_f64(&x509_format_time(&tbs.validity.not_before)),
        "validTo" => string_f64(&x509_format_time(&tbs.validity.not_after)),
        "serialNumber" => {
            let hex_str: String = tbs
                .serial_number
                .as_bytes()
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect();
            string_f64(&hex_str)
        }
        "fingerprint" => {
            let digest = Sha1::digest(&h.der);
            string_f64(&x509_colon_hex(&digest))
        }
        "fingerprint256" => {
            let digest = Sha256::digest(&h.der);
            string_f64(&x509_colon_hex(&digest))
        }
        "ca" => {
            // BasicConstraints (id-ce 2.5.29.19) cA flag.
            let is_ca = x509_basic_constraints_ca(&h.cert);
            f64::from_bits(if is_ca {
                0x7FFC_0000_0000_0004
            } else {
                0x7FFC_0000_0000_0003
            })
        }
        "raw" => {
            let buf = alloc_buffer_from_slice(&h.der);
            f64::from_bits(0x7FFD_0000_0000_0000u64 | ((buf as u64) & 0x0000_FFFF_FFFF_FFFF))
        }
        _ => nanbox_undefined(),
    }
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
