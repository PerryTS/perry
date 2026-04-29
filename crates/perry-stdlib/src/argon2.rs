//! Argon2 module
//!
//! Native implementation of the 'argon2' npm package.
//! Provides secure password hashing using Argon2id algorithm.

use crate::common::spawn_for_promise;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use perry_runtime::{js_promise_new, js_string_from_bytes, Promise, StringHeader};

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

/// argon2.hash(password) -> Promise<string>
///
/// Hash a password using Argon2id with default parameters.
#[no_mangle]
pub unsafe extern "C" fn js_argon2_hash(password_ptr: *const StringHeader) -> *mut Promise {
    let promise = js_promise_new();

    let password = match string_from_header(password_ptr) {
        Some(p) => p,
        None => {
            spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, _>("Invalid password".to_string())
            });
            return promise;
        }
    };

    spawn_for_promise(promise as *mut u8, async move {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();

        match argon2.hash_password(password.as_bytes(), &salt) {
            Ok(hash) => {
                let hash_str = hash.to_string();
                let ptr = js_string_from_bytes(hash_str.as_ptr(), hash_str.len() as u32);
                Ok(perry_runtime::JSValue::string_ptr(ptr).bits())
            }
            Err(e) => Err(format!("Failed to hash password: {}", e)),
        }
    });

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
    let promise = js_promise_new();

    let hash_str = match string_from_header(hash_ptr) {
        Some(h) => h,
        None => {
            spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, _>("Invalid hash".to_string())
            });
            return promise;
        }
    };

    let password = match string_from_header(password_ptr) {
        Some(p) => p,
        None => {
            spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, _>("Invalid password".to_string())
            });
            return promise;
        }
    };

    spawn_for_promise(promise as *mut u8, async move {
        let parsed_hash = match PasswordHash::new(&hash_str) {
            Ok(h) => h,
            Err(e) => return Err(format!("Invalid hash format: {}", e)),
        };

        let argon2 = Argon2::default();
        let is_valid = argon2
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok();

        Ok(perry_runtime::JSValue::bool(is_valid).bits())
    });

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
