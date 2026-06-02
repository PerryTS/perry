//! Native bindings for the npm `bcrypt` package.
//!
//! First async-wrapper port under #466 Phase 5 — exercises the
//! `JsPromise` + `spawn_blocking` surface that perry-ffi grew in
//! v0.5.1. Functionally identical to
//! `crates/perry-stdlib/src/bcrypt.rs` modulo the eprintln! debug
//! lines that have been on the perry-stdlib copy since v0.5.0.

use perry_ffi::{
    alloc_string, nanbox_string_bits, read_string, spawn_blocking, JsPromise, JsString, Promise,
    StringHeader,
};

/// `bcrypt.hash(password, saltRounds) -> Promise<string>` — hash a
/// password with the requested cost factor. Spawns the actual
/// hashing onto Perry's shared blocking pool so the main thread
/// stays responsive.
///
/// # Safety
///
/// `password_ptr` must be null or point to a Perry-runtime
/// `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_bcrypt_hash(
    password_ptr: *const StringHeader,
    salt_rounds: f64,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    let password_handle = JsString::from_raw(password_ptr as *mut StringHeader);
    let Some(password) = read_string(password_handle).map(String::from) else {
        promise.reject_string("Password is null or invalid UTF-8");
        return raw;
    };

    let cost = salt_rounds as u32;
    spawn_blocking(move || match bcrypt::hash(&password, cost) {
        Ok(hash) => promise.resolve_string(&hash),
        Err(e) => promise.reject_string(&format!("Bcrypt error: {}", e)),
    });
    raw
}

/// `bcrypt.compare(password, hash) -> Promise<boolean>`.
///
/// # Safety
///
/// Both pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_bcrypt_compare(
    password_ptr: *const StringHeader,
    hash_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    let password_handle = JsString::from_raw(password_ptr as *mut StringHeader);
    let Some(password) = read_string(password_handle).map(String::from) else {
        promise.reject_string("Password is null or invalid UTF-8");
        return raw;
    };
    let hash_handle = JsString::from_raw(hash_ptr as *mut StringHeader);
    let Some(hash) = read_string(hash_handle).map(String::from) else {
        promise.reject_string("Hash is null or invalid UTF-8");
        return raw;
    };

    spawn_blocking(move || match bcrypt::verify(&password, &hash) {
        Ok(matches) => promise.resolve_bool(matches),
        Err(e) => promise.reject_string(&format!("Bcrypt verify error: {}", e)),
    });
    raw
}

/// `bcrypt.genSalt(rounds) -> Promise<string>`.
///
/// The `bcrypt` crate doesn't expose salt generation directly, so
/// we follow perry-stdlib's existing trick: hash an empty string
/// with the requested cost, return the 29-character prefix
/// (`$2b$XX$<22-char-salt>`).
#[no_mangle]
pub extern "C" fn js_bcrypt_gen_salt(rounds: f64) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    let cost = rounds as u32;
    spawn_blocking(move || match bcrypt::hash("", cost) {
        Ok(h) if h.len() >= 29 => promise.resolve_string(&h[..29]),
        Ok(_) => promise.reject_string("Invalid hash format"),
        Err(e) => promise.reject_string(&format!("{}", e)),
    });
    raw
}

/// `bcrypt.hashSync(password, saltRounds) -> string` — synchronous
/// variant. Returned i64 carries pre-NaN-boxed string bits so the
/// codegen `bitcast(F64, i64)` fall-through produces a correctly
/// tagged JSValue. Same trick perry-stdlib's hashSync uses.
///
/// # Safety
///
/// `password_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_bcrypt_hash_sync(
    password_ptr: *const StringHeader,
    salt_rounds: f64,
) -> i64 {
    let handle = JsString::from_raw(password_ptr as *mut StringHeader);
    let Some(password) = read_string(handle) else {
        return 0;
    };
    let cost = salt_rounds as u32;
    match bcrypt::hash(password, cost) {
        Ok(hash) => {
            let s = alloc_string(&hash);
            nanbox_string_bits(s.as_raw()) as i64
        }
        Err(_) => 0,
    }
}

/// `bcrypt.compareSync(password, hash) -> boolean`.
///
/// # Safety
///
/// Both pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_bcrypt_compare_sync(
    password_ptr: *const StringHeader,
    hash_ptr: *const StringHeader,
) -> f64 {
    let pw_handle = JsString::from_raw(password_ptr as *mut StringHeader);
    let Some(password) = read_string(pw_handle) else {
        return 0.0;
    };
    let hash_handle = JsString::from_raw(hash_ptr as *mut StringHeader);
    let Some(hash) = read_string(hash_handle) else {
        return 0.0;
    };
    match bcrypt::verify(password, hash) {
        Ok(true) => 1.0,
        _ => 0.0,
    }
}
