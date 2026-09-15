//! Native bindings for the npm `argon2` package.
//!
//! Sixth wrapper port under #466 Phase 5 (#466 step 6). Since turnloop P4 it
//! uses the perry-ffi async **ABI v2** ([`perry_ffi::pool`]) — same recipe as
//! bcrypt: the derivation runs on turnloop's shared bounded pool, and the JS
//! string is built on the thread that owns the heap rather than on the worker
//! (the #1824 arena hazard the v1 shape had).

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use perry_ffi::{alloc_string, pool, read_string, JsPromise, JsString, Promise, StringHeader};
use rand_core::OsRng;

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

    pool::submit_or_run_inline(
        move || {
            let salt = SaltString::generate(&mut OsRng);
            Argon2::default()
                .hash_password(password.as_bytes(), &salt)
                .map(|hash| hash.to_string())
                .map_err(|e| format!("Failed to hash password: {}", e))
        },
        move |outcome| match outcome {
            pool::Outcome::Done(Ok(hash)) => promise.resolve_string(&hash),
            pool::Outcome::Done(Err(message)) => promise.reject_string(&message),
            pool::Outcome::Cancelled => promise.reject_string("argon2.hash was cancelled"),
            pool::Outcome::Failed => promise.reject_string("argon2.hash failed"),
        },
    );
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

    pool::submit_or_run_inline(
        move || -> Result<bool, String> {
            let parsed_hash =
                PasswordHash::new(&hash_str).map_err(|e| format!("Invalid hash format: {}", e))?;
            Ok(Argon2::default()
                .verify_password(password.as_bytes(), &parsed_hash)
                .is_ok())
        },
        move |outcome| match outcome {
            pool::Outcome::Done(Ok(is_valid)) => promise.resolve_bool(is_valid),
            pool::Outcome::Done(Err(message)) => promise.reject_string(&message),
            pool::Outcome::Cancelled => promise.reject_string("argon2.verify was cancelled"),
            pool::Outcome::Failed => promise.reject_string("argon2.verify failed"),
        },
    );
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
