//! Bcrypt password hashing module
//!
//! Native implementation of the 'bcrypt' npm package using the Rust bcrypt crate.
//! Provides secure password hashing and verification.

use perry_runtime::{js_string_from_bytes, JSValue, StringHeader};

use crate::common::async_bridge::{pool_for_promise_deferred, queue_promise_resolution};
use crate::common::string_from_header;

/// Hash a password with the given cost factor
/// bcrypt.hash(password, saltRounds) -> Promise<string>
#[no_mangle]
pub unsafe extern "C" fn js_bcrypt_hash(
    password_ptr: *const StringHeader,
    salt_rounds: f64,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new_cross_thread();
    let promise_ptr = promise as usize;

    let password = match string_from_header(password_ptr) {
        Some(s) => s,
        None => {
            let err_msg = "Password is null or invalid UTF-8";
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            let err_bits = JSValue::string_ptr(err_str).bits();
            queue_promise_resolution(promise_ptr, false, err_bits);
            return promise;
        }
    };

    let cost = salt_rounds as u32;

    // turnloop P4: the hash runs on turnloop's shared blocking pool, and the
    // JS string is built on the thread that owns the heap. The arena rule
    // (#1292/#1824) is now a trait bound — `work` is `Send`, and a
    // `StringHeader` is not — rather than a comment the next author has to
    // read.
    pool_for_promise_deferred(
        promise as *mut u8,
        move || bcrypt::hash(password, cost).map_err(|e| format!("Bcrypt error: {}", e)),
        move |hash: String| {
            let hash_str = js_string_from_bytes(hash.as_ptr(), hash.len() as u32);
            JSValue::string_ptr(hash_str).bits()
        },
    );

    promise
}

/// Compare a password with a hash
/// bcrypt.compare(password, hash) -> Promise<boolean>
#[no_mangle]
pub unsafe extern "C" fn js_bcrypt_compare(
    password_ptr: *const StringHeader,
    hash_ptr: *const StringHeader,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new_cross_thread();
    let promise_ptr = promise as usize;

    let password = match string_from_header(password_ptr) {
        Some(s) => s,
        None => {
            let err_msg = "Password is null or invalid UTF-8";
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            let err_bits = JSValue::string_ptr(err_str).bits();
            queue_promise_resolution(promise_ptr, false, err_bits);
            return promise;
        }
    };

    let hash = match string_from_header(hash_ptr) {
        Some(s) => s,
        None => {
            let err_msg = "Hash is null or invalid UTF-8";
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            let err_bits = JSValue::string_ptr(err_str).bits();
            queue_promise_resolution(promise_ptr, false, err_bits);
            return promise;
        }
    };

    // turnloop P4: verification is the same CPU cost as hashing, so it goes
    // to the pool too. The result is a plain boolean, which needs no arena.
    pool_for_promise_deferred(
        promise as *mut u8,
        move || bcrypt::verify(password, &hash).map_err(|e| format!("Bcrypt verify error: {}", e)),
        // Deliberately the same encoding the tokio path used: the resolution
        // is the f64 1.0/0.0, not a JS boolean. That is a pre-existing quirk of
        // `bcrypt.compare` (`=== true` is false against it) and changing it is
        // a behaviour change, not a migration.
        move |matches: bool| {
            if matches {
                1.0f64.to_bits()
            } else {
                0.0f64.to_bits()
            }
        },
    );

    promise
}

/// The cost a `rounds` argument names. `genSalt()` with no argument reaches
/// here as `undefined` (NaN), and node's default is 10.
fn rounds_or_default(rounds: f64) -> u32 {
    if rounds.is_finite() {
        rounds as u32
    } else {
        10
    }
}

/// A salt for `cost`. The bcrypt crate does not expose salt generation, so a
/// dummy hash is generated and its salt prefix taken
/// (`$2b$XX$<22-char salt>`).
fn gen_salt(cost: u32) -> Result<String, String> {
    let hashed = bcrypt::hash("", cost).map_err(|e| format!("{}", e))?;
    if hashed.len() >= 29 {
        Ok(hashed[..29].to_string())
    } else {
        Err("Invalid hash format".to_string())
    }
}

/// Generate a salt with the given cost factor
/// bcrypt.genSalt(rounds?) -> Promise<string>
#[no_mangle]
pub unsafe extern "C" fn js_bcrypt_gen_salt(rounds: f64) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new_cross_thread();
    let cost = rounds_or_default(rounds);

    // turnloop P4: on the pool with the other two.
    pool_for_promise_deferred(
        promise as *mut u8,
        move || gen_salt(cost),
        move |salt: String| {
            let salt_str = js_string_from_bytes(salt.as_ptr(), salt.len() as u32);
            JSValue::string_ptr(salt_str).bits()
        },
    );

    promise
}

/// bcrypt.genSaltSync(rounds?) -> string. Returns the raw `StringHeader` (the
/// dispatch row's `NR_STR` boxes it); null on failure reads as `null`.
#[no_mangle]
pub extern "C" fn js_bcrypt_gen_salt_sync(rounds: f64) -> *mut StringHeader {
    match gen_salt(rounds_or_default(rounds)) {
        Ok(salt) => js_string_from_bytes(salt.as_ptr(), salt.len() as u32),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Hash a password synchronously
/// bcrypt.hashSync(password, saltRounds) -> string. Returns the raw
/// `StringHeader` (the dispatch row's `NR_STR` boxes it); null on failure
/// reads as `null`.
#[no_mangle]
pub unsafe extern "C" fn js_bcrypt_hash_sync(
    password_ptr: *const StringHeader,
    salt_rounds: f64,
) -> *mut StringHeader {
    let Some(password) = string_from_header(password_ptr) else {
        return std::ptr::null_mut();
    };
    match bcrypt::hash(password, salt_rounds as u32) {
        Ok(hash) => js_string_from_bytes(hash.as_ptr(), hash.len() as u32),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Compare a password with a hash synchronously
/// bcrypt.compareSync(password, hash) -> boolean, as a NaN-boxed boolean (the
/// dispatch row's `NR_F64` passes it through).
#[no_mangle]
pub unsafe extern "C" fn js_bcrypt_compare_sync(
    password_ptr: *const StringHeader,
    hash_ptr: *const StringHeader,
) -> f64 {
    let matches = match (
        string_from_header(password_ptr),
        string_from_header(hash_ptr),
    ) {
        (Some(password), Some(hash)) => bcrypt::verify(&password, &hash).unwrap_or(false),
        _ => false,
    };
    f64::from_bits(JSValue::bool(matches).bits())
}
