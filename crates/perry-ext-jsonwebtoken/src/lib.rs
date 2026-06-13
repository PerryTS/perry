//! Native bindings for the npm `jsonwebtoken` package.
//!
//! Sync wrapper — no async/await, no Promise. Uses only the
//! perry-ffi v0.5 string surface. Functionally identical to
//! `crates/perry-stdlib/src/jsonwebtoken.rs`. Seventh wrapper port
//! under #466 Phase 5.

use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use perry_ffi::{alloc_string, nanbox_string_bits, read_string, JsString, StringHeader};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Generic claims structure that can hold any JSON. Mirrors the
/// shape `perry-stdlib::jsonwebtoken` uses so encoded / decoded
/// tokens are byte-compatible.
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

unsafe fn read_str(ptr: *const StringHeader) -> Option<String> {
    let handle = JsString::from_raw(ptr as *mut StringHeader);
    read_string(handle).map(String::from)
}

/// Shared signing logic — parse payload, apply expiry, encode with
/// the given algorithm/key. `kid_ptr` is optional (null = no `kid`
/// header field). Returns a NaN-boxed string i64, or 0 on error.
unsafe fn sign_common(
    payload_ptr: *const StringHeader,
    expires_in_secs: f64,
    algorithm: Algorithm,
    key: &EncodingKey,
    kid_ptr: *const StringHeader,
) -> i64 {
    let Some(payload_json) = read_str(payload_ptr) else {
        return 0;
    };

    let mut claims: Claims = serde_json::from_str(&payload_json).unwrap_or_else(|_| Claims {
        data: HashMap::new(),
        exp: None,
        iat: None,
        nbf: None,
        sub: None,
        iss: None,
        aud: None,
    });

    if expires_in_secs > 0.0 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        claims.exp = Some(now + expires_in_secs as u64);
        if claims.iat.is_none() {
            claims.iat = Some(now);
        }
    }

    let mut header = Header::new(algorithm);
    if !kid_ptr.is_null() {
        if let Some(kid) = read_str(kid_ptr) {
            if !kid.is_empty() {
                header.kid = Some(kid);
            }
        }
    }

    match encode(&header, &claims, key) {
        Ok(token) => {
            let s = alloc_string(&token);
            nanbox_string_bits(s.as_raw()) as i64
        }
        Err(_) => 0,
    }
}

/// `jwt.sign(payload, secret)` — HS256.
///
/// # Safety
///
/// All pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_jwt_sign(
    payload_ptr: *const StringHeader,
    secret_ptr: *const StringHeader,
    expires_in_secs: f64,
    kid_ptr: *const StringHeader,
) -> i64 {
    let Some(secret) = read_str(secret_ptr) else {
        return 0;
    };
    sign_common(
        payload_ptr,
        expires_in_secs,
        Algorithm::HS256,
        &EncodingKey::from_secret(secret.as_bytes()),
        kid_ptr,
    )
}

/// `jwt.sign(payload, ecPrivateKeyPem, { algorithm: 'ES256' })` —
/// PKCS#8 PEM-encoded EC P-256 private key. Used by APNs.
///
/// # Safety
///
/// All pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_jwt_sign_es256(
    payload_ptr: *const StringHeader,
    pem_ptr: *const StringHeader,
    expires_in_secs: f64,
    kid_ptr: *const StringHeader,
) -> i64 {
    let Some(pem) = read_str(pem_ptr) else {
        return 0;
    };
    let Ok(key) = EncodingKey::from_ec_pem(pem.as_bytes()) else {
        return 0;
    };
    sign_common(
        payload_ptr,
        expires_in_secs,
        Algorithm::ES256,
        &key,
        kid_ptr,
    )
}

/// `jwt.sign(payload, rsaPrivateKeyPem, { algorithm: 'RS256' })` —
/// PKCS#8 PEM-encoded RSA private key. Used by FCM.
///
/// # Safety
///
/// All pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_jwt_sign_rs256(
    payload_ptr: *const StringHeader,
    pem_ptr: *const StringHeader,
    expires_in_secs: f64,
    kid_ptr: *const StringHeader,
) -> i64 {
    let Some(pem) = read_str(pem_ptr) else {
        return 0;
    };
    let Ok(key) = EncodingKey::from_rsa_pem(pem.as_bytes()) else {
        return 0;
    };
    sign_common(
        payload_ptr,
        expires_in_secs,
        Algorithm::RS256,
        &key,
        kid_ptr,
    )
}

/// `jwt.verify(token, secret)` — HS256. Returns the claims as a
/// JSON string.
///
/// # Safety
///
/// `token_ptr` and `secret_ptr` must be null or Perry-runtime
/// `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_jwt_verify(
    token_ptr: *const StringHeader,
    secret_ptr: *const StringHeader,
) -> *mut StringHeader {
    let Some(token) = read_str(token_ptr) else {
        return std::ptr::null_mut();
    };
    let Some(secret) = read_str(secret_ptr) else {
        return std::ptr::null_mut();
    };

    let key = DecodingKey::from_secret(secret.as_bytes());
    let mut validation = Validation::new(Algorithm::HS256);
    // Match Node's `jsonwebtoken`: validate the `exp` claim whenever it is
    // present (so expired tokens are rejected), but do not *require* exp — a
    // token that legitimately omits expiry still verifies. `required_spec_claims`
    // stays empty for the latter; `validate_exp = true` enforces the former.
    //
    // This previously read `validate_exp = false`, which accepted expired
    // tokens indefinitely (GHSA-5324-c68v-8w62 / CVE-2026-53777) — the same
    // bug already fixed in crates/perry-stdlib/src/jsonwebtoken.rs.
    validation.required_spec_claims = std::collections::HashSet::new();
    validation.validate_exp = true;

    match decode::<Claims>(&token, &key, &validation) {
        Ok(token_data) => {
            let json = serde_json::to_string(&token_data.claims).unwrap_or_else(|_| "{}".into());
            alloc_string(&json).as_raw()
        }
        Err(_) => std::ptr::null_mut(),
    }
}

/// `jwt.decode(token)` — split-and-base64-decode the payload, no
/// signature verification.
///
/// # Safety
///
/// `token_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_jwt_decode(token_ptr: *const StringHeader) -> *mut StringHeader {
    let Some(token) = read_str(token_ptr) else {
        return std::ptr::null_mut();
    };

    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return std::ptr::null_mut();
    }

    use base64::Engine;
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let Ok(payload_bytes) = engine.decode(parts[1]) else {
        return std::ptr::null_mut();
    };
    let Ok(payload_json) = String::from_utf8(payload_bytes) else {
        return std::ptr::null_mut();
    };
    if serde_json::from_str::<serde_json::Value>(&payload_json).is_err() {
        return std::ptr::null_mut();
    }
    alloc_string(&payload_json).as_raw()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(handle: i64) -> String {
        const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
        let raw = (handle as u64 & POINTER_MASK) as *mut StringHeader;
        read_string(unsafe { JsString::from_raw(raw) })
            .map(String::from)
            .unwrap_or_default()
    }

    fn ps(p: *mut StringHeader) -> Option<String> {
        if p.is_null() {
            return None;
        }
        read_string(unsafe { JsString::from_raw(p) }).map(String::from)
    }

    #[test]
    fn sign_then_verify_round_trip() {
        let payload = alloc_string(r#"{"sub":"1234","name":"Alice"}"#);
        let secret = alloc_string("supersecret");
        let token_bits = unsafe {
            js_jwt_sign(
                payload.as_raw() as *const _,
                secret.as_raw() as *const _,
                3600.0,
                std::ptr::null(),
            )
        };
        assert_ne!(token_bits, 0, "sign returned zero");
        let token = s(token_bits);
        assert!(
            token.starts_with("eyJ"),
            "JWT should start with eyJ: {}",
            token
        );

        let token_handle = alloc_string(&token);
        let claims_ptr = unsafe {
            js_jwt_verify(
                token_handle.as_raw() as *const _,
                alloc_string("supersecret").as_raw() as *const _,
            )
        };
        let claims = ps(claims_ptr).expect("verify returned non-null");
        assert!(claims.contains("\"name\":\"Alice\""), "got: {}", claims);
        assert!(claims.contains("\"sub\":\"1234\""), "got: {}", claims);
    }

    #[test]
    fn verify_with_wrong_secret_returns_null() {
        let payload = alloc_string(r#"{"sub":"x"}"#);
        let token_bits = unsafe {
            js_jwt_sign(
                payload.as_raw() as *const _,
                alloc_string("right").as_raw() as *const _,
                0.0,
                std::ptr::null(),
            )
        };
        let token = s(token_bits);
        let token_handle = alloc_string(&token);
        let result = unsafe {
            js_jwt_verify(
                token_handle.as_raw() as *const _,
                alloc_string("wrong").as_raw() as *const _,
            )
        };
        assert!(result.is_null(), "wrong secret should fail verify");
    }

    #[test]
    fn decode_skips_signature_check() {
        // Decode unverified — even with a wrong secret, decode
        // returns the payload. Used by clients that just need to
        // peek at the claims (e.g. `exp`) before deciding whether
        // to refresh.
        let payload = alloc_string(r#"{"role":"admin"}"#);
        let token_bits = unsafe {
            js_jwt_sign(
                payload.as_raw() as *const _,
                alloc_string("k").as_raw() as *const _,
                0.0,
                std::ptr::null(),
            )
        };
        let token = s(token_bits);
        let result_ptr = unsafe { js_jwt_decode(alloc_string(&token).as_raw() as *const _) };
        let claims = ps(result_ptr).expect("decode non-null");
        assert!(claims.contains("\"role\":\"admin\""), "got: {}", claims);
    }

    #[test]
    fn verify_rejects_expired_token() {
        // Regression for #5066 / GHSA-5324-c68v-8w62: expired token must be rejected.
        let secret = "supersecret";
        let expired_claims = Claims {
            data: std::collections::HashMap::new(),
            exp: Some(1),
            iat: None,
            nbf: None,
            sub: Some("1234".into()),
            iss: None,
            aud: None,
        };
        let token = encode(
            &Header::new(Algorithm::HS256),
            &expired_claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .expect("encode expired token");
        let token_handle = alloc_string(&token);
        let result = unsafe {
            js_jwt_verify(
                token_handle.as_raw() as *const _,
                alloc_string(secret).as_raw() as *const _,
            )
        };
        assert!(
            result.is_null(),
            "expired token must be rejected, got claims: {:?}",
            ps(result)
        );
    }

    #[test]
    fn verify_accepts_token_without_exp() {
        // Node parity: token omitting exp must still verify (required_spec_claims empty).
        let secret = "supersecret";
        let claims = Claims {
            data: std::collections::HashMap::new(),
            exp: None,
            iat: None,
            nbf: None,
            sub: Some("1234".into()),
            iss: None,
            aud: None,
        };
        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .expect("encode no-exp token");
        let token_handle = alloc_string(&token);
        let result = unsafe {
            js_jwt_verify(
                token_handle.as_raw() as *const _,
                alloc_string(secret).as_raw() as *const _,
            )
        };
        assert!(!result.is_null(), "token without exp must still verify");
    }
}
