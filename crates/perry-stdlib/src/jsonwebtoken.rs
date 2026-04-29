//! JSON Web Token module (jsonwebtoken compatible)
//!
//! Native implementation of the 'jsonwebtoken' npm package.
//! Provides JWT sign, verify, and decode functionality.

use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use perry_runtime::{js_string_from_bytes, StringHeader};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

/// Generic claims structure that can hold any JSON
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    #[serde(flatten)]
    data: HashMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exp: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    iat: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nbf: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sub: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    iss: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    aud: Option<String>,
}

const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// Shared signing logic — parse payload, apply expiry, encode with given algorithm/key.
/// `kid_ptr` is optional (null = no `kid` header field). Returns a NaN-boxed string i64,
/// or 0 on error.
unsafe fn sign_common(
    payload_ptr: *const StringHeader,
    expires_in_secs: f64,
    algorithm: Algorithm,
    key: &EncodingKey,
    kid_ptr: *const StringHeader,
) -> i64 {
    let payload_json = match string_from_header(payload_ptr) {
        Some(p) => p,
        None => return 0,
    };

    let mut claims: Claims = match serde_json::from_str(&payload_json) {
        Ok(c) => c,
        Err(_) => Claims {
            data: HashMap::new(),
            exp: None,
            iat: None,
            nbf: None,
            sub: None,
            iss: None,
            aud: None,
        },
    };

    if expires_in_secs > 0.0 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        claims.exp = Some(now + expires_in_secs as u64);
        if claims.iat.is_none() {
            claims.iat = Some(now);
        }
    }

    let mut header = Header::new(algorithm);
    if !kid_ptr.is_null() {
        if let Some(kid) = string_from_header(kid_ptr) {
            if !kid.is_empty() {
                header.kid = Some(kid);
            }
        }
    }

    match encode(&header, &claims, key) {
        Ok(token) => {
            let ptr = js_string_from_bytes(token.as_ptr(), token.len() as u32);
            (STRING_TAG | (ptr as u64 & POINTER_MASK)) as i64
        }
        Err(_) => 0,
    }
}

/// Sign a payload to create a JWT (HS256)
/// jwt.sign(payload, secret) -> string
/// jwt.sign(payload, secret, options) -> string
///
/// `kid_ptr` may be null when no `keyid` is provided in options.
#[no_mangle]
pub unsafe extern "C" fn js_jwt_sign(
    payload_ptr: *const StringHeader,
    secret_ptr: *const StringHeader,
    expires_in_secs: f64,
    kid_ptr: *const StringHeader,
) -> i64 {
    let secret = match string_from_header(secret_ptr) {
        Some(s) => s,
        None => return 0,
    };
    let key = EncodingKey::from_secret(secret.as_bytes());
    sign_common(
        payload_ptr,
        expires_in_secs,
        Algorithm::HS256,
        &key,
        kid_ptr,
    )
}

/// Sign a payload to create a JWT (ES256)
/// `pem_ptr` must contain a PKCS#8 PEM-encoded EC private key (P-256 curve).
/// jwt.sign(payload, ecPrivateKeyPem, { algorithm: 'ES256', keyid: '...' }) -> string
///
/// Used by APNs (Apple Push Notification service) provider tokens — APNs requires
/// `kid` in the JWT header to identify which `.p8` key was used to sign.
#[no_mangle]
pub unsafe extern "C" fn js_jwt_sign_es256(
    payload_ptr: *const StringHeader,
    pem_ptr: *const StringHeader,
    expires_in_secs: f64,
    kid_ptr: *const StringHeader,
) -> i64 {
    let pem = match string_from_header(pem_ptr) {
        Some(p) => p,
        None => return 0,
    };
    let key = match EncodingKey::from_ec_pem(pem.as_bytes()) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("[jwt-sign-es256] invalid EC PEM key: {}", e);
            return 0;
        }
    };
    sign_common(
        payload_ptr,
        expires_in_secs,
        Algorithm::ES256,
        &key,
        kid_ptr,
    )
}

/// Sign a payload to create a JWT (RS256)
/// `pem_ptr` must contain a PKCS#8 PEM-encoded RSA private key.
/// jwt.sign(payload, rsaPrivateKeyPem, { algorithm: 'RS256', keyid: '...' }) -> string
///
/// Used by FCM (Firebase Cloud Messaging) OAuth assertions.
#[no_mangle]
pub unsafe extern "C" fn js_jwt_sign_rs256(
    payload_ptr: *const StringHeader,
    pem_ptr: *const StringHeader,
    expires_in_secs: f64,
    kid_ptr: *const StringHeader,
) -> i64 {
    let pem = match string_from_header(pem_ptr) {
        Some(p) => p,
        None => return 0,
    };
    let key = match EncodingKey::from_rsa_pem(pem.as_bytes()) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("[jwt-sign-rs256] invalid RSA PEM key: {}", e);
            return 0;
        }
    };
    sign_common(
        payload_ptr,
        expires_in_secs,
        Algorithm::RS256,
        &key,
        kid_ptr,
    )
}

/// Verify and decode a JWT
/// jwt.verify(token, secret) -> object (payload)
#[no_mangle]
pub unsafe extern "C" fn js_jwt_verify(
    token_ptr: *const StringHeader,
    secret_ptr: *const StringHeader,
) -> *mut StringHeader {
    let token = match string_from_header(token_ptr) {
        Some(t) => t,
        None => {
            eprintln!("[jwt-verify] token_ptr is null or invalid");
            return std::ptr::null_mut();
        }
    };

    let secret = match string_from_header(secret_ptr) {
        Some(s) => s,
        None => {
            eprintln!("[jwt-verify] secret_ptr is null or invalid");
            return std::ptr::null_mut();
        }
    };

    eprintln!(
        "[jwt-verify] token_len={} secret_len={}",
        token.len(),
        secret.len()
    );

    let key = DecodingKey::from_secret(secret.as_bytes());
    let mut validation = Validation::new(Algorithm::HS256);
    // Don't require exp claim - tokens may not have expiry set
    validation.required_spec_claims = std::collections::HashSet::new();
    validation.validate_exp = false;

    match decode::<Claims>(&token, &key, &validation) {
        Ok(token_data) => {
            // Return the claims as JSON
            let json =
                serde_json::to_string(&token_data.claims).unwrap_or_else(|_| "{}".to_string());
            eprintln!(
                "[jwt-verify] success, claims={}",
                &json[..json.len().min(80)]
            );
            js_string_from_bytes(json.as_ptr(), json.len() as u32)
        }
        Err(e) => {
            eprintln!("[jwt-verify] error: {}", e);
            std::ptr::null_mut()
        }
    }
}

/// Decode a JWT without verification (just parse the payload)
/// jwt.decode(token) -> object (payload)
#[no_mangle]
pub unsafe extern "C" fn js_jwt_decode(token_ptr: *const StringHeader) -> *mut StringHeader {
    let token = match string_from_header(token_ptr) {
        Some(t) => t,
        None => return std::ptr::null_mut(),
    };

    // Split the token into parts
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return std::ptr::null_mut();
    }

    // Decode the payload (second part)
    use base64::Engine;
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;

    match engine.decode(parts[1]) {
        Ok(payload_bytes) => {
            match String::from_utf8(payload_bytes) {
                Ok(payload_json) => {
                    // Validate it's valid JSON and return it
                    if serde_json::from_str::<serde_json::Value>(&payload_json).is_ok() {
                        js_string_from_bytes(payload_json.as_ptr(), payload_json.len() as u32)
                    } else {
                        std::ptr::null_mut()
                    }
                }
                Err(_) => std::ptr::null_mut(),
            }
        }
        Err(_) => std::ptr::null_mut(),
    }
}
