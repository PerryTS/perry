//! Argon2 module
//!
//! Native implementation of the 'argon2' npm package.
//! Provides secure password hashing using Argon2id algorithm.

use crate::common::async_bridge::pool_for_promise_deferred;
use crate::common::async_bridge::reject_promise_later;
use crate::common::string_from_header_lossy as string_from_header;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use perry_runtime::{js_promise_new_cross_thread, js_string_from_bytes, Promise, StringHeader};

/// argon2.hash(password) -> Promise<string>
///
/// Hash a password using Argon2id with default parameters.
#[no_mangle]
pub unsafe extern "C" fn js_argon2_hash(password_ptr: *const StringHeader) -> *mut Promise {
    let promise = js_promise_new_cross_thread();

    let password = match string_from_header(password_ptr) {
        Some(p) => p,
        None => {
            reject_promise_later(promise as *mut u8, "Invalid password".to_string());
            return promise;
        }
    };

    // turnloop P4: argon2 is the phase's clearest case. The hash used to run
    // *inline* inside an async block on the shared current-thread runtime —
    // that is, on the thread that owns the JS heap — so `argon2.hash()` stalled
    // the event loop for the whole derivation and only the resolution was
    // deferred. It now runs on turnloop's shared blocking pool, and the JS
    // string is built on the owning thread where allocation is legal.
    pool_for_promise_deferred(
        promise as *mut u8,
        move || {
            let salt = SaltString::generate(&mut OsRng);
            Argon2::default()
                .hash_password(password.as_bytes(), &salt)
                .map(|hash| hash.to_string())
                .map_err(|e| format!("Failed to hash password: {}", e))
        },
        move |hash: String| {
            let ptr = js_string_from_bytes(hash.as_ptr(), hash.len() as u32);
            perry_runtime::JSValue::string_ptr(ptr).bits()
        },
    );

    promise
}

/// argon2.hashSync(password) -> string
///
/// Synchronously hash a password using Argon2id.
#[no_mangle]
pub unsafe extern "C" fn js_argon2_hash_sync(
    password_ptr: *const StringHeader,
) -> *mut StringHeader {
    let password = match string_from_header(password_ptr) {
        Some(p) => p,
        None => return std::ptr::null_mut(),
    };

    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();

    match argon2.hash_password(password.as_bytes(), &salt) {
        Ok(hash) => {
            let hash_str = hash.to_string();
            js_string_from_bytes(hash_str.as_ptr(), hash_str.len() as u32)
        }
        Err(_) => std::ptr::null_mut(),
    }
}

/// argon2.verify(hash, password) -> Promise<boolean>
///
/// Verify a password against an Argon2 hash.
#[no_mangle]
pub unsafe extern "C" fn js_argon2_verify(
    hash_ptr: *const StringHeader,
    password_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new_cross_thread();

    let hash_str = match string_from_header(hash_ptr) {
        Some(h) => h,
        None => {
            reject_promise_later(promise as *mut u8, "Invalid hash".to_string());
            return promise;
        }
    };

    let password = match string_from_header(password_ptr) {
        Some(p) => p,
        None => {
            reject_promise_later(promise as *mut u8, "Invalid password".to_string());
            return promise;
        }
    };

    // turnloop P4: same move as `hash` above — the verification is the same
    // memory-hard derivation and cost the event loop the same stall.
    pool_for_promise_deferred(
        promise as *mut u8,
        move || -> Result<bool, String> {
            let parsed_hash =
                PasswordHash::new(&hash_str).map_err(|e| format!("Invalid hash format: {}", e))?;
            Ok(Argon2::default()
                .verify_password(password.as_bytes(), &parsed_hash)
                .is_ok())
        },
        move |is_valid: bool| perry_runtime::JSValue::bool(is_valid).bits(),
    );

    promise
}

/// argon2.verifySync(hash, password) -> boolean
///
/// Synchronously verify a password against an Argon2 hash.
#[no_mangle]
pub unsafe extern "C" fn js_argon2_verify_sync(
    hash_ptr: *const StringHeader,
    password_ptr: *const StringHeader,
) -> i32 {
    let hash_str = match string_from_header(hash_ptr) {
        Some(h) => h,
        None => return 0,
    };

    let password = match string_from_header(password_ptr) {
        Some(p) => p,
        None => return 0,
    };

    let parsed_hash = match PasswordHash::new(&hash_str) {
        Ok(h) => h,
        Err(_) => return 0,
    };

    let argon2 = Argon2::default();
    if argon2
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
    {
        1
    } else {
        0
    }
}

/// argon2.needsRehash(hash) -> boolean
///
/// Check if a hash needs to be rehashed (e.g., due to outdated parameters).
#[no_mangle]
pub unsafe extern "C" fn js_argon2_needs_rehash(hash_ptr: *const StringHeader) -> i32 {
    let hash_str = match string_from_header(hash_ptr) {
        Some(h) => h,
        None => return 1,
    };

    // Parse the hash to check its parameters
    match PasswordHash::new(&hash_str) {
        Ok(parsed) => {
            // Check if algorithm is argon2id
            if parsed.algorithm.as_str() != "argon2id" {
                return 1;
            }
            // In a real implementation, we'd check memory cost, time cost, etc.
            // For now, we assume current defaults are acceptable
            0
        }
        Err(_) => 1, // Invalid hash needs rehashing
    }
}
