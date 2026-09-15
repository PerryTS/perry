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

/// Generate a salt with the given cost factor
/// bcrypt.genSalt(rounds) -> Promise<string>
#[no_mangle]
pub unsafe extern "C" fn js_bcrypt_gen_salt(rounds: f64) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new_cross_thread();
    let cost = rounds as u32;

    // turnloop P4: on the pool with the other two.
    pool_for_promise_deferred(
        promise as *mut u8,
        move || {
            // The bcrypt crate does not expose salt generation, so a dummy
            // hash is generated and its salt prefix taken. Unchanged from the
            // tokio version; only where it runs has moved.
            let hashed = bcrypt::hash("", cost).map_err(|e| format!("{}", e))?;
            if hashed.len() >= 29 {
                // bcrypt format: $2b$XX$<22-char salt><31-char hash>
                Ok(hashed[..29].to_string())
            } else {
                Err("Invalid hash format".to_string())
            }
        },
        move |salt: String| {
            let salt_str = js_string_from_bytes(salt.as_ptr(), salt.len() as u32);
            JSValue::string_ptr(salt_str).bits()
        },
    );

    promise
}

/// Hash a password synchronously
/// bcrypt.hashSync(password, saltRounds) -> string
#[no_mangle]
pub unsafe extern "C" fn js_bcrypt_hash_sync(
    password_ptr: *const StringHeader,
    salt_rounds: f64,
) -> i64 {
    eprintln!(
        "[bcrypt-sync] hash_sync called, password_ptr={:?} salt_rounds={}",
        password_ptr, salt_rounds
    );
    let password = match string_from_header(password_ptr) {
        Some(s) => s,
        None => {
            eprintln!("[bcrypt-sync] password_ptr is null or invalid UTF-8");
            return 0;
        }
    };
    eprintln!(
        "[bcrypt-sync] password len={} cost={}",
        password.len(),
        salt_rounds as u32
    );

    let cost = salt_rounds as u32;

    match bcrypt::hash(password, cost) {
        Ok(hash) => {
            eprintln!("[bcrypt-sync] hash success, hash_len={}", hash.len());
            let ptr = js_string_from_bytes(hash.as_ptr(), hash.len() as u32);
            // Pre-NaN-box the string pointer so that even if the codegen falls through
            // to bitcast(F64, i64), the result is a correctly tagged string value.
            // js_nanbox_string is idempotent, so this is also safe if the codegen
            // applies it again.
            const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
            const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
            (STRING_TAG | (ptr as u64 & POINTER_MASK)) as i64
        }
        Err(e) => {
            eprintln!("[bcrypt-sync] hash error: {}", e);
            0
        }
    }
}

/// Compare a password with a hash synchronously
/// bcrypt.compareSync(password, hash) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_bcrypt_compare_sync(
    password_ptr: *const StringHeader,
    hash_ptr: *const StringHeader,
) -> f64 {
    eprintln!(
        "[bcrypt-cmp] compare_sync called, password_ptr={:?} hash_ptr={:?}",
        password_ptr, hash_ptr
    );
    let password = match string_from_header(password_ptr) {
        Some(s) => s,
        None => {
            eprintln!("[bcrypt-cmp] password_ptr is null or invalid");
            return 0.0;
        }
    };

    let hash = match string_from_header(hash_ptr) {
        Some(s) => s,
        None => {
            eprintln!("[bcrypt-cmp] hash_ptr is null or invalid");
            return 0.0;
        }
    };

    eprintln!(
        "[bcrypt-cmp] password len={} hash_prefix={}",
        password.len(),
        &hash[..hash.len().min(15)]
    );
    match bcrypt::verify(&password, &hash) {
        Ok(true) => {
            eprintln!("[bcrypt-cmp] match=true");
            1.0
        }
        Ok(false) => {
            eprintln!("[bcrypt-cmp] match=false");
            0.0
        }
        Err(e) => {
            eprintln!("[bcrypt-cmp] verify error: {}", e);
            0.0
        }
    }
}
