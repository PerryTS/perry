//! Bcrypt password hashing module
//!
//! Native implementation of the 'bcrypt' npm package using the Rust bcrypt crate.
//! Provides secure password hashing and verification.

use perry_runtime::{js_string_from_bytes, JSValue, StringHeader};

use crate::common::async_bridge::{queue_promise_resolution, spawn};

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

/// Hash a password with the given cost factor
/// bcrypt.hash(password, saltRounds) -> Promise<string>
#[no_mangle]
pub unsafe extern "C" fn js_bcrypt_hash(
    password_ptr: *const StringHeader,
    salt_rounds: f64,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;

    let password = match string_from_header(password_ptr) {
        Some(s) => s,
        None => {
            let err_msg = "Password is null or invalid UTF-8";
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            let err_bits = JSValue::pointer(err_str as *const u8).bits();
            queue_promise_resolution(promise_ptr, false, err_bits);
            return promise;
        }
    };

    let cost = salt_rounds as u32;

    // Spawn async task for hashing (bcrypt is CPU-intensive)
    spawn(async move {
        let result = tokio::task::spawn_blocking(move || bcrypt::hash(password, cost)).await;

        match result {
            Ok(Ok(hash)) => {
                let hash_str = js_string_from_bytes(hash.as_ptr(), hash.len() as u32);
                let result_bits = JSValue::pointer(hash_str as *const u8).bits();
                queue_promise_resolution(promise_ptr, true, result_bits);
            }
            Ok(Err(e)) => {
                let err_msg = format!("Bcrypt error: {}", e);
                let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                let err_bits = JSValue::pointer(err_str as *const u8).bits();
                queue_promise_resolution(promise_ptr, false, err_bits);
            }
            Err(e) => {
                let err_msg = format!("Task error: {}", e);
                let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                let err_bits = JSValue::pointer(err_str as *const u8).bits();
                queue_promise_resolution(promise_ptr, false, err_bits);
            }
        }
    });

    promise
}

/// Compare a password with a hash
/// bcrypt.compare(password, hash) -> Promise<boolean>
#[no_mangle]
pub unsafe extern "C" fn js_bcrypt_compare(
    password_ptr: *const StringHeader,
    hash_ptr: *const StringHeader,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;

    let password = match string_from_header(password_ptr) {
        Some(s) => s,
        None => {
            let err_msg = "Password is null or invalid UTF-8";
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            let err_bits = JSValue::pointer(err_str as *const u8).bits();
            queue_promise_resolution(promise_ptr, false, err_bits);
            return promise;
        }
    };

    let hash = match string_from_header(hash_ptr) {
        Some(s) => s,
        None => {
            let err_msg = "Hash is null or invalid UTF-8";
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            let err_bits = JSValue::pointer(err_str as *const u8).bits();
            queue_promise_resolution(promise_ptr, false, err_bits);
            return promise;
        }
    };

    // Spawn async task for verification (bcrypt is CPU-intensive)
    spawn(async move {
        let result = tokio::task::spawn_blocking(move || bcrypt::verify(password, &hash)).await;

        match result {
            Ok(Ok(matches)) => {
                // Return boolean as f64 (1.0 for true, 0.0 for false)
                let result_bits = if matches {
                    1.0f64.to_bits()
                } else {
                    0.0f64.to_bits()
                };
                queue_promise_resolution(promise_ptr, true, result_bits);
            }
            Ok(Err(e)) => {
                let err_msg = format!("Bcrypt verify error: {}", e);
                let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                let err_bits = JSValue::pointer(err_str as *const u8).bits();
                queue_promise_resolution(promise_ptr, false, err_bits);
            }
            Err(e) => {
                let err_msg = format!("Task error: {}", e);
                let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                let err_bits = JSValue::pointer(err_str as *const u8).bits();
                queue_promise_resolution(promise_ptr, false, err_bits);
            }
        }
    });

    promise
}

/// Generate a salt with the given cost factor
/// bcrypt.genSalt(rounds) -> Promise<string>
#[no_mangle]
pub unsafe extern "C" fn js_bcrypt_gen_salt(rounds: f64) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;
    let cost = rounds as u32;

    // Spawn async task
    spawn(async move {
        let result = tokio::task::spawn_blocking(move || {
            // Generate a random salt with the given cost
            // The bcrypt crate doesn't expose salt generation directly,
            // so we generate a dummy hash and extract the salt prefix
            let dummy = bcrypt::hash("", cost);
            match dummy {
                Ok(h) => {
                    // bcrypt hash format: $2b$XX$<22-char-salt><31-char-hash>
                    // We return the full salt portion including the prefix
                    if h.len() >= 29 {
                        Ok(h[..29].to_string())
                    } else {
                        Err("Invalid hash format".to_string())
                    }
                }
                Err(e) => Err(format!("{}", e)),
            }
        })
        .await;

        match result {
            Ok(Ok(salt)) => {
                let salt_str = js_string_from_bytes(salt.as_ptr(), salt.len() as u32);
                let result_bits = JSValue::pointer(salt_str as *const u8).bits();
                queue_promise_resolution(promise_ptr, true, result_bits);
            }
            Ok(Err(e)) => {
                let err_str = js_string_from_bytes(e.as_ptr(), e.len() as u32);
                let err_bits = JSValue::pointer(err_str as *const u8).bits();
                queue_promise_resolution(promise_ptr, false, err_bits);
            }
            Err(e) => {
                let err_msg = format!("Task error: {}", e);
                let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                let err_bits = JSValue::pointer(err_str as *const u8).bits();
                queue_promise_resolution(promise_ptr, false, err_bits);
            }
        }
    });

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
