//! Cross-platform updater primitives.
//!
//! Module of `perry-updater` — no I/O on the executable itself, just semver
//! compare, hash + Ed25519 verification, and sentinel state. Per-OS install
//! / relaunch / path resolution lives in the sibling `desktop` module.

use perry_runtime::buffer::BufferHeader;
use perry_runtime::{js_string_from_bytes, StringHeader};

use base64::Engine as _;
use sha2::{Digest, Sha256};

/// Verify an Ed25519 signature over `payload` in-crate via `ed25519-dalek`
/// (already a hard dependency for the sign side in `cli_manifest.rs`).
///
/// This used to extern-call perry-stdlib's `js_crypto_ed25519_verify`, but
/// the `perry` CLI links perry-updater WITHOUT perry-stdlib: MSVC `link.exe`
/// rejects the unresolved extern (LNK2019) even though GNU/Mach-O linkers
/// dead-strip the reference — which is how the Windows CLI build broke while
/// Unix builds stayed green. Verifying in-crate removes the cross-crate
/// link-time coupling. Semantics match the stdlib primitive exactly:
/// non-strict `Verifier::verify`, 64-byte signature, 32-byte public key.
fn verify_ed25519(payload: &[u8], sig_bytes: &[u8], pk_bytes: &[u8]) -> bool {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    if sig_bytes.len() != 64 || pk_bytes.len() != 32 {
        return false;
    }
    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(sig_bytes);
    let mut pk_arr = [0u8; 32];
    pk_arr.copy_from_slice(pk_bytes);
    let Ok(verifying_key) = VerifyingKey::from_bytes(&pk_arr) else {
        return false;
    };
    verifying_key
        .verify(payload, &Signature::from_bytes(&sig_arr))
        .is_ok()
}

// ============================================================================
// Helpers
// ============================================================================

/// Extract a Rust String from a raw `*const StringHeader` pointer (passed as
/// `i64` over the FFI). The pointer comes from
/// `js_get_string_pointer_unified` on the codegen side, so SSO is already
/// materialized to a heap StringHeader by the time we read it here.
unsafe fn extract_str(ptr_val: i64) -> Option<String> {
    if ptr_val == 0 || (ptr_val as usize) < 0x1000 {
        return None;
    }
    let ptr = ptr_val as *const StringHeader;
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

fn alloc_string(s: &str) -> *mut StringHeader {
    js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

// ============================================================================
// Semver compare
// ============================================================================

/// Compare two semver versions. Returns -1 if `current < candidate` (an update is available),
/// 0 if equal, 1 if `current > candidate`, and -2 if either string fails to parse.
#[no_mangle]
pub extern "C" fn perry_updater_compare_versions(current_val: i64, candidate_val: i64) -> i64 {
    let current = match unsafe { extract_str(current_val) } {
        Some(s) => s,
        None => return -2,
    };
    let candidate = match unsafe { extract_str(candidate_val) } {
        Some(s) => s,
        None => return -2,
    };

    let cur = match semver::Version::parse(&current) {
        Ok(v) => v,
        Err(_) => return -2,
    };
    let cand = match semver::Version::parse(&candidate) {
        Ok(v) => v,
        Err(_) => return -2,
    };

    match cur.cmp(&cand) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

// ============================================================================
// Hash verify
// ============================================================================

/// Verify the SHA-256 of a file against an expected lowercase hex digest.
/// Returns 1 on match, 0 on any failure (file missing, mismatch, unreadable).
#[no_mangle]
pub extern "C" fn perry_updater_verify_hash(file_path_val: i64, expected_hex_val: i64) -> i64 {
    let path = match unsafe { extract_str(file_path_val) } {
        Some(s) => s,
        None => return 0,
    };
    let expected = match unsafe { extract_str(expected_hex_val) } {
        Some(s) => s.to_lowercase(),
        None => return 0,
    };

    let actual = match compute_sha256_hex(&path) {
        Some(h) => h,
        None => return 0,
    };

    if actual == expected {
        1
    } else {
        0
    }
}

fn compute_sha256_hex(path: &str) -> Option<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(hex::encode(hasher.finalize()))
}

/// Like `verify_hash`, but returns the actual hex digest of the file (or empty
/// string on failure). Useful for the TS layer when it wants to log the
/// computed hash on mismatch.
#[no_mangle]
pub extern "C" fn perry_updater_compute_file_sha256(file_path_val: i64) -> *mut StringHeader {
    let path = match unsafe { extract_str(file_path_val) } {
        Some(s) => s,
        None => return alloc_string(""),
    };
    let hex = compute_sha256_hex(&path).unwrap_or_default();
    alloc_string(&hex)
}

// ============================================================================
// Signature verify
// ============================================================================

/// Verify an Ed25519 signature over the SHA-256 digest of a file.
///
/// The signed payload is the **raw 32-byte SHA-256 digest** of the binary —
/// not the hex string, not the file bytes. This must match how the developer's
/// signing tool produces signatures. Documented at the manifest spec level.
///
/// **Why plain Ed25519 over a pre-hash, not Ed25519ph**: this is the Tauri
/// convention and we follow it deliberately. Ed25519ph (the prehashed
/// variant from RFC 8032) was designed for streaming signers that can't
/// hold the full message in memory — pure Ed25519 internally hashes its
/// own input, so signing a 100MB binary directly with pure Ed25519 would
/// require buffering. We sidestep that by hashing the binary ourselves
/// (SHA-256 is streamable) and signing the 32-byte digest with pure
/// Ed25519. Functionally equivalent to Ed25519ph for our use case, but
/// avoids the domain-separation prefix Ed25519ph adds (which would make
/// signatures incompatible with non-Tauri-shaped tooling). Reviewed
/// against RFC 8032 §6 — the construction "external SHA-256, sign the
/// digest with Ed25519" is a well-trodden pattern, equivalent in
/// strength to Ed25519ph as long as the digest binding is unambiguous
/// (which the manifest schema enforces).
///
/// `sig_b64`: base64-encoded 64-byte Ed25519 signature.
/// `pubkey_b64`: base64-encoded 32-byte Ed25519 public key.
/// Returns 1 on valid signature, 0 on any error.
#[no_mangle]
pub extern "C" fn perry_updater_verify_signature(
    file_path_val: i64,
    sig_b64_val: i64,
    pubkey_b64_val: i64,
) -> i64 {
    let path = match unsafe { extract_str(file_path_val) } {
        Some(s) => s,
        None => return 0,
    };
    let sig_b64 = match unsafe { extract_str(sig_b64_val) } {
        Some(s) => s,
        None => return 0,
    };
    let pubkey_b64 = match unsafe { extract_str(pubkey_b64_val) } {
        Some(s) => s,
        None => return 0,
    };

    let sig_bytes = match base64::engine::general_purpose::STANDARD.decode(sig_b64.trim()) {
        Ok(b) => b,
        Err(_) => return 0,
    };
    let pk_bytes = match base64::engine::general_purpose::STANDARD.decode(pubkey_b64.trim()) {
        Ok(b) => b,
        Err(_) => return 0,
    };

    if sig_bytes.len() != 64 || pk_bytes.len() != 32 {
        return 0;
    }

    // Compute file digest (32 bytes raw) — streamed, so we never hold the
    // whole binary in memory.
    use std::io::Read;
    let mut file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(_) => return 0,
    };
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = match file.read(&mut buf) {
            Ok(n) => n,
            Err(_) => return 0,
        };
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();

    if verify_ed25519(&digest, &sig_bytes, &pk_bytes) {
        1
    } else {
        0
    }
}

/// Issue #229: version-bound Ed25519 signature verification.
///
/// **The signed payload is `SHA256(binary) || version_string_utf8`** —
/// 32 bytes of digest followed by the UTF-8 bytes of the version
/// string. This binds the version into the signature so an on-path
/// attacker can't replay a previously-signed older binary as a "new
/// version" by serving a manifest that pairs the old binary's URL +
/// signature with a higher version number. (Pre-v2 the signed payload
/// was just the 32-byte digest, leaving the version unauthenticated.)
///
/// The `version` argument MUST be the same byte sequence the manifest
/// claims for this asset. Sign-side tooling produces signatures by
/// computing `payload = sha256(binary) || version.as_bytes()` and
/// signing that with Ed25519. Verify-side recomputes the same payload
/// and runs the same verification — any tampering with either the
/// version string or the binary causes the signature to fail.
///
/// Manifest schemaVersion 2+ uses this path. v1 manifests stay on
/// `perry_updater_verify_signature` (digest-only) for backwards
/// compatibility; the @perry/updater TS wrapper dispatches based on
/// schemaVersion.
///
/// `sig_b64`: base64-encoded 64-byte Ed25519 signature.
/// `pubkey_b64`: base64-encoded 32-byte Ed25519 public key.
/// `version`: UTF-8 version string (e.g. "1.2.3" — exact bytes).
/// Returns 1 on valid signature, 0 on any error.
#[no_mangle]
pub extern "C" fn perry_updater_verify_signature_v2(
    file_path_val: i64,
    sig_b64_val: i64,
    pubkey_b64_val: i64,
    version_val: i64,
) -> i64 {
    let path = match unsafe { extract_str(file_path_val) } {
        Some(s) => s,
        None => return 0,
    };
    let sig_b64 = match unsafe { extract_str(sig_b64_val) } {
        Some(s) => s,
        None => return 0,
    };
    let pubkey_b64 = match unsafe { extract_str(pubkey_b64_val) } {
        Some(s) => s,
        None => return 0,
    };
    let version = match unsafe { extract_str(version_val) } {
        Some(s) => s,
        None => return 0,
    };
    if version.is_empty() {
        // Empty version would let an attacker forge a v2 manifest that
        // matches a v1 signature byte-for-byte (since `digest || ""`
        // == `digest`). Refuse the empty case to keep the v1 / v2
        // payload spaces fully disjoint.
        return 0;
    }

    let sig_bytes = match base64::engine::general_purpose::STANDARD.decode(sig_b64.trim()) {
        Ok(b) => b,
        Err(_) => return 0,
    };
    let pk_bytes = match base64::engine::general_purpose::STANDARD.decode(pubkey_b64.trim()) {
        Ok(b) => b,
        Err(_) => return 0,
    };

    if sig_bytes.len() != 64 || pk_bytes.len() != 32 {
        return 0;
    }

    // Stream the file digest exactly like v1, then build the
    // version-bound payload by appending the UTF-8 version bytes.
    use std::io::Read;
    let mut file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(_) => return 0,
    };
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = match file.read(&mut buf) {
            Ok(n) => n,
            Err(_) => return 0,
        };
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();

    // payload = digest_bytes (32) || version_utf8 (variable)
    let mut payload = Vec::with_capacity(32 + version.len());
    payload.extend_from_slice(&digest);
    payload.extend_from_slice(version.as_bytes());

    if verify_ed25519(&payload, &sig_bytes, &pk_bytes) {
        1
    } else {
        0
    }
}

// ============================================================================
// Sentinel state
// ============================================================================

/// Write a sentinel JSON payload to a path. Caller passes the full JSON string;
/// schema is owned by the TS layer (`@perry/updater`). Returns 1 on
/// success, 0 on any IO failure.
///
/// Creates the parent directory if missing. The write is atomic: writes to
/// `<path>.tmp` then renames over `<path>`.
#[no_mangle]
pub extern "C" fn perry_updater_write_sentinel(
    sentinel_path_val: i64,
    json_payload_val: i64,
) -> i64 {
    let path = match unsafe { extract_str(sentinel_path_val) } {
        Some(s) => s,
        None => return 0,
    };
    let payload = match unsafe { extract_str(json_payload_val) } {
        Some(s) => s,
        None => return 0,
    };

    if let Some(parent) = std::path::Path::new(&path).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }

    let tmp = format!("{}.tmp", path);
    if std::fs::write(&tmp, payload.as_bytes()).is_err() {
        return 0;
    }
    if std::fs::rename(&tmp, &path).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return 0;
    }
    1
}

/// Read the sentinel JSON payload from a path. Returns the contents as a
/// string, or empty string if the file is missing/unreadable. The TS layer
/// parses the JSON.
#[no_mangle]
pub extern "C" fn perry_updater_read_sentinel(sentinel_path_val: i64) -> *mut StringHeader {
    let path = match unsafe { extract_str(sentinel_path_val) } {
        Some(s) => s,
        None => return alloc_string(""),
    };
    let contents = std::fs::read_to_string(&path).unwrap_or_default();
    alloc_string(&contents)
}

/// Delete the sentinel file. Returns 1 on success or if the file didn't exist
/// to begin with, 0 on any other IO error.
#[no_mangle]
pub extern "C" fn perry_updater_clear_sentinel(sentinel_path_val: i64) -> i64 {
    let path = match unsafe { extract_str(sentinel_path_val) } {
        Some(s) => s,
        None => return 0,
    };
    match std::fs::remove_file(&path) {
        Ok(_) => 1,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => 1,
        Err(_) => 0,
    }
}

// ============================================================================
// Buffer / digest helpers (used when the TS side already has the file bytes)
// ============================================================================

/// Compute SHA-256 over a Buffer and return a Buffer holding the 32-byte digest.
/// Useful when TS downloaded the file bytes and wants to verify before writing.
#[no_mangle]
pub extern "C" fn perry_updater_sha256_buffer(buf_ptr: i64) -> *mut BufferHeader {
    if buf_ptr == 0 {
        return std::ptr::null_mut();
    }
    unsafe {
        let buf = buf_ptr as *const BufferHeader;
        let len = (*buf).length as usize;
        let data = (buf as *const u8).add(std::mem::size_of::<BufferHeader>());
        let bytes = std::slice::from_raw_parts(data, len);

        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let digest = hasher.finalize();

        let out = perry_runtime::buffer::buffer_alloc(32);
        if out.is_null() {
            return out;
        }
        (*out).length = 32;
        let dst = perry_runtime::buffer::buffer_data_mut(out);
        std::ptr::copy_nonoverlapping(digest.as_ptr(), dst, 32);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    // Note: the signature tests exercise the production `verify_ed25519`
    // path directly (ed25519-dalek is a real dependency now) — no in-test
    // verifier stub is needed. Coverage still spans file read, SHA
    // streaming, base64 decode, and payload construction end-to-end.

    fn make_str(s: &str) -> i64 {
        // Tests pass the raw *StringHeader pointer as i64 — same convention
        // codegen uses (it extracts via js_get_string_pointer_unified before
        // passing into our FFIs).
        js_string_from_bytes(s.as_ptr(), s.len() as u32) as i64
    }

    fn read_str(p: *mut StringHeader) -> String {
        unsafe {
            if p.is_null() {
                return String::new();
            }
            let len = (*p).byte_len as usize;
            let data = (p as *const u8).add(std::mem::size_of::<StringHeader>());
            let bytes = std::slice::from_raw_parts(data, len);
            std::str::from_utf8(bytes).unwrap_or("").to_string()
        }
    }

    #[test]
    fn semver_compare_basic() {
        assert_eq!(
            perry_updater_compare_versions(make_str("1.0.0"), make_str("1.0.1")),
            -1
        );
        assert_eq!(
            perry_updater_compare_versions(make_str("1.0.0"), make_str("1.0.0")),
            0
        );
        assert_eq!(
            perry_updater_compare_versions(make_str("2.0.0"), make_str("1.9.9")),
            1
        );
    }

    #[test]
    fn semver_compare_prerelease() {
        // 1.0.0-beta < 1.0.0
        assert_eq!(
            perry_updater_compare_versions(make_str("1.0.0-beta"), make_str("1.0.0")),
            -1
        );
    }

    #[test]
    fn semver_compare_invalid() {
        assert_eq!(
            perry_updater_compare_versions(make_str("notaversion"), make_str("1.0.0")),
            -2
        );
    }

    #[test]
    fn hash_verify_roundtrip() {
        let dir = std::env::temp_dir().join(format!("perry-updater-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("payload.bin");
        std::fs::write(&file, b"hello, perry").unwrap();

        let mut hasher = Sha256::new();
        hasher.update(b"hello, perry");
        let expected = hex::encode(hasher.finalize());

        let path_str = file.to_string_lossy().to_string();
        assert_eq!(
            perry_updater_verify_hash(make_str(&path_str), make_str(&expected)),
            1,
            "hash should match"
        );
        assert_eq!(
            perry_updater_verify_hash(make_str(&path_str), make_str("0".repeat(64).as_str())),
            0,
            "wrong hash should be rejected"
        );
        assert_eq!(
            perry_updater_verify_hash(make_str("/nonexistent/path"), make_str(&expected)),
            0
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn signature_verify_roundtrip() {
        let dir =
            std::env::temp_dir().join(format!("perry-updater-sig-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("payload.bin");
        let body = b"signed binary contents";
        std::fs::write(&file, body).unwrap();

        // Generate a test keypair (NEVER commit a real one).
        let mut seed = [0u8; 32];
        for (i, b) in seed.iter_mut().enumerate() {
            *b = i as u8;
        }
        let signing = SigningKey::from_bytes(&seed);
        let verifying = signing.verifying_key();

        let mut h = Sha256::new();
        h.update(body);
        let digest = h.finalize();
        let sig = signing.sign(&digest);

        let sig_b64 = base64::engine::general_purpose::STANDARD.encode(sig.to_bytes());
        let pk_b64 = base64::engine::general_purpose::STANDARD.encode(verifying.to_bytes());

        let path_str = file.to_string_lossy().to_string();
        assert_eq!(
            perry_updater_verify_signature(
                make_str(&path_str),
                make_str(&sig_b64),
                make_str(&pk_b64),
            ),
            1,
            "valid signature should verify"
        );

        // Tamper the body — same path, new contents — sig must now reject.
        std::fs::write(&file, b"TAMPERED contents").unwrap();
        assert_eq!(
            perry_updater_verify_signature(
                make_str(&path_str),
                make_str(&sig_b64),
                make_str(&pk_b64),
            ),
            0,
            "tampered file must fail"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Issue #229: version-bound signature verify (`_v2`).
    ///
    /// Three assertions:
    ///   1. `(binary, sig_v2, version)` triple verifies cleanly when the
    ///      signature was made over `digest || version`.
    ///   2. **Old-binary replay defense**: if an attacker pairs a
    ///      previously-signed v1 binary with a manifest claiming a higher
    ///      version, the v2 verify path rejects it (because the original
    ///      sig was made over digest only, not digest||"99.0.0").
    ///   3. Empty-string version rejection (would reduce to v1 payload).
    #[test]
    fn signature_verify_v2_roundtrip_and_replay_defense() {
        let dir =
            std::env::temp_dir().join(format!("perry-updater-sig-v2-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("payload.bin");
        let body = b"v2 signed binary contents";
        std::fs::write(&file, body).unwrap();

        // Test keypair (NEVER commit a real one).
        let mut seed = [0u8; 32];
        for (i, b) in seed.iter_mut().enumerate() {
            *b = (i + 100) as u8; // distinct from v1 test seed
        }
        let signing = SigningKey::from_bytes(&seed);
        let verifying = signing.verifying_key();

        let mut h = Sha256::new();
        h.update(body);
        let digest = h.finalize();

        // Build v2 payload: digest || version_utf8
        let version = "1.2.3";
        let mut v2_payload = Vec::with_capacity(32 + version.len());
        v2_payload.extend_from_slice(&digest);
        v2_payload.extend_from_slice(version.as_bytes());
        let v2_sig = signing.sign(&v2_payload);
        let v2_sig_b64 = base64::engine::general_purpose::STANDARD.encode(v2_sig.to_bytes());

        let pk_b64 = base64::engine::general_purpose::STANDARD.encode(verifying.to_bytes());
        let path_str = file.to_string_lossy().to_string();

        // (1) Happy path: v2 sig + matching version.
        assert_eq!(
            perry_updater_verify_signature_v2(
                make_str(&path_str),
                make_str(&v2_sig_b64),
                make_str(&pk_b64),
                make_str(version),
            ),
            1,
            "v2 signature with matching version should verify"
        );

        // (2) Old-binary replay defense: a v1-style sig (digest only —
        // no version bound in) must FAIL v2 verify, even when the
        // version label happens to match.
        let v1_sig = signing.sign(&digest);
        let v1_sig_b64 = base64::engine::general_purpose::STANDARD.encode(v1_sig.to_bytes());
        assert_eq!(
            perry_updater_verify_signature_v2(
                make_str(&path_str),
                make_str(&v1_sig_b64),
                make_str(&pk_b64),
                make_str(version),
            ),
            0,
            "v1-style sig (digest only) must NOT verify against v2 path"
        );

        // (2b) Different version label fails verify.
        assert_eq!(
            perry_updater_verify_signature_v2(
                make_str(&path_str),
                make_str(&v2_sig_b64),
                make_str(&pk_b64),
                make_str("99.0.0"),
            ),
            0,
            "tampered version label must fail v2 verify"
        );

        // (2c) Tampered binary fails verify.
        std::fs::write(&file, b"TAMPERED v2 contents").unwrap();
        assert_eq!(
            perry_updater_verify_signature_v2(
                make_str(&path_str),
                make_str(&v2_sig_b64),
                make_str(&pk_b64),
                make_str(version),
            ),
            0,
            "tampered binary must fail v2 verify"
        );
        std::fs::write(&file, body).unwrap();

        // (3) Empty version is rejected up-front (would otherwise
        // collapse the v2 payload to digest-only and accept any v1
        // signature as a valid v2 signature).
        assert_eq!(
            perry_updater_verify_signature_v2(
                make_str(&path_str),
                make_str(&v2_sig_b64),
                make_str(&pk_b64),
                make_str(""),
            ),
            0,
            "empty version must be rejected"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sentinel_roundtrip() {
        let dir =
            std::env::temp_dir().join(format!("perry-updater-sent-test-{}", std::process::id()));
        let path = dir.join("subdir/updater.sentinel");
        let path_str = path.to_string_lossy().to_string();

        let payload = r#"{"prevExePath":"/tmp/old","stagedAt":"2026-04-27","restartCount":0,"state":"armed"}"#;

        assert_eq!(
            perry_updater_write_sentinel(make_str(&path_str), make_str(payload)),
            1
        );

        let read = read_str(perry_updater_read_sentinel(make_str(&path_str)));
        assert_eq!(read, payload);

        assert_eq!(perry_updater_clear_sentinel(make_str(&path_str)), 1);
        let after = read_str(perry_updater_read_sentinel(make_str(&path_str)));
        assert_eq!(after, "", "after clear, read should return empty");

        // Clearing twice is OK (idempotent).
        assert_eq!(perry_updater_clear_sentinel(make_str(&path_str)), 1);

        std::fs::remove_dir_all(&dir).ok();
    }
}
