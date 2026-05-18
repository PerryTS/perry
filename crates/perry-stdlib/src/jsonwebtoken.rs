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
    // jsonwebtoken's `EncodingKey::from_ec_pem` only accepts PKCS#8
    // (`-----BEGIN PRIVATE KEY-----`). openssl's default
    // `ecparam -genkey -name prime256v1` emits SEC1
    // (`-----BEGIN EC PRIVATE KEY-----`), which is the form most users
    // start with. Convert SEC1 → PKCS#8 transparently so both PEM
    // forms work. Same ergonomic story as the verify side's
    // `ec_pem_to_public_pem` helper.
    let pkcs8_pem = if pem.contains("EC PRIVATE KEY") {
        use p256::pkcs8::EncodePrivateKey;
        match p256::SecretKey::from_sec1_pem(&pem)
            .ok()
            .and_then(|k| k.to_pkcs8_pem(Default::default()).ok())
        {
            Some(p) => p.to_string(),
            None => {
                eprintln!("[jwt-sign-es256] could not convert SEC1 EC PEM to PKCS#8");
                return 0;
            }
        }
    } else {
        pem
    };
    let key = match EncodingKey::from_ec_pem(pkcs8_pem.as_bytes()) {
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

/// Shared verify path — runs the decode + returns claims as JSON, or
/// a null pointer on any failure. `debug` mirrors the gating in
/// `js_jwt_verify` (perry#924) so all three verify entry points emit
/// the same `[jwt-verify]` log lines under `PERRY_DEBUG=1`.
unsafe fn verify_decode(
    token: &str,
    key: &DecodingKey,
    algorithm: Algorithm,
    debug: bool,
) -> *mut StringHeader {
    let mut validation = Validation::new(algorithm);
    // Don't require exp claim - tokens may not have expiry set
    validation.required_spec_claims = std::collections::HashSet::new();
    validation.validate_exp = false;

    match decode::<Claims>(token, key, &validation) {
        Ok(token_data) => {
            let json =
                serde_json::to_string(&token_data.claims).unwrap_or_else(|_| "{}".to_string());
            if debug {
                eprintln!(
                    "[jwt-verify] success, claims={}",
                    &json[..json.len().min(80)]
                );
            }
            js_string_from_bytes(json.as_ptr(), json.len() as u32)
        }
        Err(e) => {
            if debug {
                eprintln!("[jwt-verify] error: {}", e);
            }
            std::ptr::null_mut()
        }
    }
}

/// Verify and decode an HS256 JWT
/// jwt.verify(token, secret) -> object (payload)
#[no_mangle]
pub unsafe extern "C" fn js_jwt_verify(
    token_ptr: *const StringHeader,
    secret_ptr: *const StringHeader,
) -> *mut StringHeader {
    // perry#924: all `[jwt-verify]` eprintln!s are gated behind
    // `PERRY_DEBUG=1`. Authenticated production services call
    // `jwt.verify` per request, so the previous unconditional logging
    // (token length + secret length + claims/error) flooded stderr and
    // also leaked the secret length, narrowing the cracking surface
    // when paired with a known JWT structure. The application layer
    // already logs 401s at a useful granularity.
    let debug = std::env::var_os("PERRY_DEBUG").is_some();

    let token = match string_from_header(token_ptr) {
        Some(t) => t,
        None => {
            if debug {
                eprintln!("[jwt-verify] token_ptr is null or invalid");
            }
            return std::ptr::null_mut();
        }
    };

    let secret = match string_from_header(secret_ptr) {
        Some(s) => s,
        None => {
            if debug {
                eprintln!("[jwt-verify] secret_ptr is null or invalid");
            }
            return std::ptr::null_mut();
        }
    };

    let key = DecodingKey::from_secret(secret.as_bytes());
    verify_decode(&token, &key, Algorithm::HS256, debug)
}

/// Coerce an EC PEM (public *or* private, SEC1 or PKCS#8) into a
/// PKCS#8 PUBLIC KEY PEM that `DecodingKey::from_ec_pem` accepts.
/// Mirrors Node's `jsonwebtoken` ergonomics: the user can pass the
/// same PEM to `sign` and `verify` without having to extract the
/// public key separately. perry#927 follow-up — without this, ES256
/// `verify` rejected the very PEM the matching `sign` accepted,
/// breaking the shop-admin auth path even after the JSON-parse
/// return-shape fix.
fn ec_pem_to_public_pem(pem: &str) -> Option<String> {
    use p256::pkcs8::{DecodePrivateKey, EncodePublicKey};

    if pem.contains("PUBLIC KEY") {
        return Some(pem.to_string());
    }

    // Try PKCS#8 private (`-----BEGIN PRIVATE KEY-----`) first,
    // then SEC1 (`-----BEGIN EC PRIVATE KEY-----`).
    let secret = p256::SecretKey::from_pkcs8_pem(pem)
        .or_else(|_| p256::SecretKey::from_sec1_pem(pem))
        .ok()?;
    secret
        .public_key()
        .to_public_key_pem(Default::default())
        .ok()
}

/// Verify and decode an ES256 JWT.
/// `pem_ptr` may contain either a PUBLIC key PEM (SPKI) or the
/// matching PRIVATE key PEM (PKCS#8 or SEC1) — the latter is
/// auto-converted via `ec_pem_to_public_pem` so callers can reuse
/// their signing key.
/// jwt.verify(token, pem, { algorithms: ['ES256'] }) -> object
#[no_mangle]
pub unsafe extern "C" fn js_jwt_verify_es256(
    token_ptr: *const StringHeader,
    pem_ptr: *const StringHeader,
) -> *mut StringHeader {
    let debug = std::env::var_os("PERRY_DEBUG").is_some();

    let token = match string_from_header(token_ptr) {
        Some(t) => t,
        None => {
            if debug {
                eprintln!("[jwt-verify-es256] token_ptr is null or invalid");
            }
            return std::ptr::null_mut();
        }
    };

    let pem = match string_from_header(pem_ptr) {
        Some(p) => p,
        None => {
            if debug {
                eprintln!("[jwt-verify-es256] pem_ptr is null or invalid");
            }
            return std::ptr::null_mut();
        }
    };

    let public_pem = match ec_pem_to_public_pem(&pem) {
        Some(p) => p,
        None => {
            if debug {
                eprintln!("[jwt-verify-es256] could not derive EC public key from PEM");
            }
            return std::ptr::null_mut();
        }
    };

    let key = match DecodingKey::from_ec_pem(public_pem.as_bytes()) {
        Ok(k) => k,
        Err(e) => {
            if debug {
                eprintln!("[jwt-verify-es256] invalid EC PEM key: {}", e);
            }
            return std::ptr::null_mut();
        }
    };

    verify_decode(&token, &key, Algorithm::ES256, debug)
}

/// Coerce an RSA PEM (public *or* private, PKCS#1 or PKCS#8) into a
/// PEM that `DecodingKey::from_rsa_pem` accepts. Matches Node's
/// `jsonwebtoken` behavior of accepting either side of the keypair
/// on verify.
fn rsa_pem_to_public_pem(pem: &str) -> Option<String> {
    use rsa::pkcs1::EncodeRsaPublicKey;
    use rsa::pkcs8::{DecodePrivateKey, EncodePublicKey};

    if pem.contains("PUBLIC KEY") {
        // Either PKCS#1 `RSA PUBLIC KEY` or PKCS#8 `PUBLIC KEY` —
        // both consumed directly by `DecodingKey::from_rsa_pem`.
        return Some(pem.to_string());
    }

    // Try PKCS#8 (`-----BEGIN PRIVATE KEY-----`) then PKCS#1
    // (`-----BEGIN RSA PRIVATE KEY-----`).
    let priv_key = rsa::RsaPrivateKey::from_pkcs8_pem(pem)
        .or_else(|_| {
            use rsa::pkcs1::DecodeRsaPrivateKey;
            rsa::RsaPrivateKey::from_pkcs1_pem(pem)
        })
        .ok()?;
    let pub_key = priv_key.to_public_key();
    pub_key
        .to_public_key_pem(Default::default())
        .ok()
        .or_else(|| pub_key.to_pkcs1_pem(Default::default()).ok())
}

/// Verify and decode an RS256 JWT.
/// `pem_ptr` may contain either a PUBLIC key PEM (PKCS#1 or PKCS#8)
/// or the matching PRIVATE key PEM (auto-converted via
/// `rsa_pem_to_public_pem`).
/// jwt.verify(token, pem, { algorithms: ['RS256'] }) -> object
#[no_mangle]
pub unsafe extern "C" fn js_jwt_verify_rs256(
    token_ptr: *const StringHeader,
    pem_ptr: *const StringHeader,
) -> *mut StringHeader {
    let debug = std::env::var_os("PERRY_DEBUG").is_some();

    let token = match string_from_header(token_ptr) {
        Some(t) => t,
        None => {
            if debug {
                eprintln!("[jwt-verify-rs256] token_ptr is null or invalid");
            }
            return std::ptr::null_mut();
        }
    };

    let pem = match string_from_header(pem_ptr) {
        Some(p) => p,
        None => {
            if debug {
                eprintln!("[jwt-verify-rs256] pem_ptr is null or invalid");
            }
            return std::ptr::null_mut();
        }
    };

    let public_pem = match rsa_pem_to_public_pem(&pem) {
        Some(p) => p,
        None => {
            if debug {
                eprintln!("[jwt-verify-rs256] could not derive RSA public key from PEM");
            }
            return std::ptr::null_mut();
        }
    };

    let key = match DecodingKey::from_rsa_pem(public_pem.as_bytes()) {
        Ok(k) => k,
        Err(e) => {
            if debug {
                eprintln!("[jwt-verify-rs256] invalid RSA PEM key: {}", e);
            }
            return std::ptr::null_mut();
        }
    };

    verify_decode(&token, &key, Algorithm::RS256, debug)
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

#[cfg(all(test, unix))]
mod tests {
    //! perry#924 regression tests — `jwt.verify` MUST be silent on the
    //! happy path. We exercise the real `js_jwt_verify` FFI in a
    //! subprocess (spawning the current test binary with a sentinel
    //! env var) because cargo-test's harness installs a Rust-level
    //! stderr capture that intercepts `eprintln!` before fd 2, making
    //! in-process `dup2`-style capture vacuously pass. Subprocess
    //! stderr is unaffected and gives us a real byte stream to count
    //! lines against.
    //!
    //! Before the fix:
    //!   • valid token: 3 stderr lines (`token_len=…` + `success, claims=…`)
    //!   • invalid token: 2 stderr lines (`token_len=…` + `error: …`)
    //! After the fix (no `PERRY_DEBUG`):
    //!   • valid token: 0 stderr lines
    //!   • invalid token: 0 stderr lines
    //! With `PERRY_DEBUG=1`: original verbose output is restored.
    use super::*;
    use perry_runtime::js_string_from_bytes;
    use std::process::{Command, Stdio};

    /// Sentinel env var: when set, the targeted helper test runs the
    /// FFI in this process (which is a subprocess of the real test)
    /// and exits so the subprocess produces a clean, uncaptured
    /// stderr stream for the parent test to inspect. Spawning is done
    /// via `--exact …::__perry_924_helper --nocapture --quiet` so
    /// only the helper test runs and harness stderr capture is off.
    const HELPER_ENV: &str = "PERRY_924_HELPER";

    /// Hidden helper test — invoked by the real tests via subprocess.
    /// When `PERRY_924_HELPER` is set, exec the requested FFI scenario
    /// and exit. Otherwise no-op (so a normal `cargo test` run just
    /// records this as a trivially-passing test).
    #[test]
    fn __perry_924_helper() {
        let Ok(mode) = std::env::var(HELPER_ENV) else {
            return;
        };
        unsafe { run_helper(&mode) };
        std::process::exit(0);
    }

    unsafe fn run_helper(mode: &str) {
        unsafe fn mk(s: &str) -> *mut StringHeader {
            js_string_from_bytes(s.as_ptr(), s.len() as u32)
        }

        match mode {
            "valid" => {
                // Mint a real HS256 token, then verify it. Success
                // path → must not eprintln (unless PERRY_DEBUG set
                // by parent).
                let payload = mk(r#"{"sub":"1234","name":"Alice"}"#);
                let secret = mk("supersecret");
                let token_bits = js_jwt_sign(
                    payload as *const _,
                    secret as *const _,
                    0.0,
                    std::ptr::null(),
                );
                assert_ne!(token_bits, 0);
                let raw = (token_bits as u64 & POINTER_MASK) as *mut StringHeader;
                let len = (*raw).byte_len as usize;
                let data_ptr = (raw as *const u8).add(std::mem::size_of::<StringHeader>());
                let token_bytes = std::slice::from_raw_parts(data_ptr, len);
                let token_str = std::str::from_utf8(token_bytes).unwrap().to_string();

                let token = mk(&token_str);
                let secret2 = mk("supersecret");
                let result = js_jwt_verify(token as *const _, secret2 as *const _);
                assert!(!result.is_null(), "verify must succeed on a valid token");
            }
            "invalid" => {
                // Garbage input → verify must fail silently (no log
                // unless PERRY_DEBUG set).
                let token = mk("not-a-jwt");
                let secret = mk("supersecret");
                let result = js_jwt_verify(token as *const _, secret as *const _);
                assert!(result.is_null(), "verify must fail on garbage");
            }
            other => panic!("unknown helper mode: {}", other),
        }
    }

    fn spawn_helper(mode: &str, debug: bool) -> std::process::Output {
        let exe = std::env::current_exe().expect("current_exe");
        let mut cmd = Command::new(exe);
        cmd.arg("--exact")
            .arg("jsonwebtoken::tests::__perry_924_helper")
            .arg("--nocapture")
            .arg("--quiet")
            .env(HELPER_ENV, mode)
            .env_remove("PERRY_DEBUG")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if debug {
            cmd.env("PERRY_DEBUG", "1");
        }
        cmd.output().expect("spawn helper")
    }

    #[test]
    fn verify_valid_token_is_silent() {
        let out = spawn_helper("valid", false);
        assert!(out.status.success(), "helper exited non-zero: {:?}", out);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.is_empty(),
            "jwt.verify on a valid token must not log to stderr (perry#924); got: {:?}",
            stderr
        );
    }

    #[test]
    fn verify_invalid_token_is_silent() {
        let out = spawn_helper("invalid", false);
        assert!(out.status.success(), "helper exited non-zero: {:?}", out);
        let stderr = String::from_utf8_lossy(&out.stderr);
        // Application code (e.g. authMiddleware) already logs the
        // 401 — stdlib must not duplicate. One line maximum if we
        // ever decide a single error-class summary is worth it.
        let lines = stderr.lines().count();
        assert!(
            lines == 0,
            "jwt.verify on invalid input must be silent (perry#924), got {} lines: {:?}",
            lines,
            stderr
        );
        assert!(
            !stderr.contains("[jwt-verify]"),
            "no `[jwt-verify]` line may appear without PERRY_DEBUG; got: {:?}",
            stderr
        );
    }

    #[test]
    fn verify_logs_under_perry_debug() {
        let out = spawn_helper("valid", true);
        assert!(out.status.success(), "helper exited non-zero: {:?}", out);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("[jwt-verify] success"),
            "PERRY_DEBUG=1 must restore verbose logging; got: {:?}",
            stderr
        );
    }
}
