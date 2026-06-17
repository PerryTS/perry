use super::*;

fn js_bool(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

fn normalize_name(name: &str) -> String {
    name.chars().flat_map(char::to_uppercase).collect()
}

unsafe fn algorithm_name(bits: u64) -> Option<String> {
    if let Some(name) = string_from_jsvalue(bits) {
        return Some(name);
    }
    object_field_string(bits, b"name")
}

unsafe fn algorithm_is_object(bits: u64) -> bool {
    string_from_jsvalue(bits).is_none() && strip_ptr(bits) >= 0x1000
}

unsafe fn algorithm_curve(bits: u64) -> Option<String> {
    object_field_string(bits, b"namedCurve").map(|curve| normalize_name(&curve))
}

unsafe fn supports_generate_key(algorithm_bits: u64, algorithm: &str) -> bool {
    let object_form = algorithm_is_object(algorithm_bits);
    match algorithm {
        "ED25519" | "ED448" | "X25519" | "X448" | "KMAC128" | "KMAC256" | "ML-KEM-512"
        | "ML-KEM-768" | "ML-KEM-1024" => true,
        "CHACHA20-POLY1305" => true,
        "HMAC" | "AES-GCM" | "AES-CBC" | "AES-CTR" | "AES-KW" => object_form,
        "ECDSA" | "ECDH" => {
            object_form
                && algorithm_curve(algorithm_bits)
                    .as_deref()
                    .and_then(parse_ec_named_curve)
                    .is_some()
        }
        _ => false,
    }
}

unsafe fn supports_import_key(algorithm_bits: u64, algorithm: &str) -> bool {
    match algorithm {
        "AES-GCM" | "AES-CBC" | "AES-CTR" | "AES-KW" | "AES-OCB" | "CHACHA20-POLY1305"
        | "PBKDF2" | "HKDF" | "ARGON2D" | "ARGON2I" | "ARGON2ID" | "ED25519" | "ED448"
        | "X25519" | "X448" | "KMAC128" | "KMAC256" | "ML-KEM-512" | "ML-KEM-768"
        | "ML-KEM-1024" => true,
        "HMAC" => algorithm_is_object(algorithm_bits),
        "ECDSA" | "ECDH" => algorithm_curve(algorithm_bits)
            .as_deref()
            .and_then(parse_ec_named_curve)
            .is_some(),
        "RSA-OAEP" | "RSA-PSS" | "RSASSA-PKCS1-V1_5" => algorithm_is_object(algorithm_bits),
        _ => false,
    }
}

fn supports_export_key(algorithm: &str) -> bool {
    matches!(
        algorithm,
        "HMAC"
            | "AES-GCM"
            | "AES-CBC"
            | "AES-CTR"
            | "AES-KW"
            | "CHACHA20-POLY1305"
            | "AES-OCB"
            | "ECDSA"
            | "ECDH"
            | "ED25519"
            | "ED448"
            | "X25519"
            | "X448"
            | "KMAC128"
            | "KMAC256"
            | "ML-KEM-512"
            | "ML-KEM-768"
            | "ML-KEM-1024"
            | "RSA-OAEP"
            | "RSA-PSS"
            | "RSASSA-PKCS1-V1_5"
    )
}

/// Static `SubtleCrypto.supports(operation, algorithm, length?)` support
/// detection. This reports Perry's implemented WebCrypto subset in the same
/// operation contexts Node's static method uses; modern algorithms that still
/// lack execution support remain `false` rather than being advertised early.
#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_supports(
    operation_bits: f64,
    algorithm_bits: f64,
    _length_bits: f64,
) -> f64 {
    let Some(operation) = string_from_jsvalue(operation_bits.to_bits()) else {
        return js_bool(false);
    };
    let Some(algorithm) = algorithm_name(algorithm_bits.to_bits()) else {
        return js_bool(false);
    };
    let operation = normalize_name(&operation);
    let algorithm = normalize_name(&algorithm);
    let supported = match operation.as_str() {
        "DIGEST" => matches!(
            algorithm.as_str(),
            "SHA-1" | "SHA-256" | "SHA-384" | "SHA-512"
        ),
        "SIGN" | "VERIFY" => {
            matches!(
                algorithm.as_str(),
                "HMAC" | "ED25519" | "ED448" | "RSASSA-PKCS1-V1_5"
            )
        }
        "ENCRYPT" | "DECRYPT" => {
            algorithm == "RSA-OAEP"
                || (algorithm == "CHACHA20-POLY1305"
                    && object_field_bytes(algorithm_bits.to_bits(), b"iv")
                        .map(|iv| iv.len() == 12)
                        .unwrap_or(false)
                    && object_field_number(algorithm_bits.to_bits(), b"tagLength")
                        .map(|tag_length| tag_length == 128)
                        .unwrap_or(true))
        }
        "GENERATEKEY" => supports_generate_key(algorithm_bits.to_bits(), &algorithm),
        "IMPORTKEY" => supports_import_key(algorithm_bits.to_bits(), &algorithm),
        "EXPORTKEY" => supports_export_key(&algorithm),
        "ENCAPSULATEBITS" | "DECAPSULATEBITS" => matches!(
            algorithm.as_str(),
            "ML-KEM-512" | "ML-KEM-768" | "ML-KEM-1024"
        ),
        _ => false,
    };
    js_bool(supported)
}
