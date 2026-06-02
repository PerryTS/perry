//! Native bindings for the npm `argon2` package.
//!
//! Sixth wrapper port under #466 Phase 5 (#466 step 6). Uses
//! perry-ffi v0.5.1's async surface — same recipe as bcrypt.

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use perry_ffi::{
    alloc_string, read_string, spawn_blocking, JsPromise, JsString, Promise, StringHeader,
};

/// `argon2.hash(password) -> Promise<string>` — Argon2id with
/// default parameters.
///
/// # Safety
///
/// `password_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_argon2_hash(password_ptr: *const StringHeader) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    let handle = JsString::from_raw(password_ptr as *mut StringHeader);
    let Some(password) = read_string(handle).map(String::from) else {
        promise.reject_string("Invalid password");
        return raw;
    };

    spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        match argon2.hash_password(password.as_bytes(), &salt) {
            Ok(hash) => promise.resolve_string(&hash.to_string()),
            Err(e) => promise.reject_string(&format!("Failed to hash password: {}", e)),
        }
    });
    raw
}

/// `argon2.hashSync(password) -> string`.
///
/// # Safety
///
/// `password_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_argon2_hash_sync(
    password_ptr: *const StringHeader,
) -> *mut StringHeader {
    let handle = JsString::from_raw(password_ptr as *mut StringHeader);
    let Some(password) = read_string(handle) else {
        return std::ptr::null_mut();
    };
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    match argon2.hash_password(password.as_bytes(), &salt) {
        Ok(hash) => alloc_string(&hash.to_string()).as_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// `argon2.verify(hash, password) -> Promise<boolean>`.
///
/// # Safety
///
/// Both pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_argon2_verify(
    hash_ptr: *const StringHeader,
    password_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    let hash_handle = JsString::from_raw(hash_ptr as *mut StringHeader);
    let Some(hash_str) = read_string(hash_handle).map(String::from) else {
        promise.reject_string("Invalid hash");
        return raw;
    };
    let pw_handle = JsString::from_raw(password_ptr as *mut StringHeader);
    let Some(password) = read_string(pw_handle).map(String::from) else {
        promise.reject_string("Invalid password");
        return raw;
    };

    spawn_blocking(move || {
        let parsed_hash = match PasswordHash::new(&hash_str) {
            Ok(h) => h,
            Err(e) => {
                promise.reject_string(&format!("Invalid hash format: {}", e));
                return;
            }
        };
        let argon2 = Argon2::default();
        let is_valid = argon2
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok();
        promise.resolve_bool(is_valid);
    });
    raw
}

/// `argon2.verifySync(hash, password) -> boolean`.
///
/// # Safety
///
/// Both pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_argon2_verify_sync(
    hash_ptr: *const StringHeader,
    password_ptr: *const StringHeader,
) -> i32 {
    let hash_handle = JsString::from_raw(hash_ptr as *mut StringHeader);
    let Some(hash_str) = read_string(hash_handle) else {
        return 0;
    };
    let pw_handle = JsString::from_raw(password_ptr as *mut StringHeader);
    let Some(password) = read_string(pw_handle) else {
        return 0;
    };
    let parsed = match PasswordHash::new(hash_str) {
        Ok(h) => h,
        Err(_) => return 0,
    };
    if Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
    {
        1
    } else {
        0
    }
}

/// `argon2.needsRehash(hash) -> boolean` — true if the hash uses
/// an algorithm other than argon2id (mirrors perry-stdlib's
/// existing heuristic).
///
/// # Safety
///
/// `hash_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_argon2_needs_rehash(hash_ptr: *const StringHeader) -> i32 {
    let handle = JsString::from_raw(hash_ptr as *mut StringHeader);
    let Some(hash_str) = read_string(handle) else {
        return 1;
    };
    match PasswordHash::new(hash_str) {
        Ok(parsed) => {
            if parsed.algorithm.as_str() != "argon2id" {
                1
            } else {
                0
            }
        }
        Err(_) => 1,
    }
}
