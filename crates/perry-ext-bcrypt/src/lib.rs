//! Native bindings for the npm `bcrypt` package.
//!
//! First async-wrapper port under #466 Phase 5. Since turnloop P4 it is the
//! reference consumer of the perry-ffi async **ABI v2** ([`perry_ffi::pool`]):
//! the hashing runs on turnloop's shared bounded pool and the JS string is
//! built on the thread that owns the heap.
//!
//! That second half is not cosmetic. The v1 version called
//! `promise.resolve_string(&hash)` from *inside* the `spawn_blocking` closure,
//! i.e. it allocated a `StringHeader` on a tokio blocking-pool thread — the
//! arena hazard #1824 describes, which perry-stdlib's copy had already worked
//! around with a deferred converter and this crate had not. Under v2 the split
//! is a trait bound: `work` is `Send`, `JsPromise` never crosses.
//!
//! Functionally identical to `crates/perry-stdlib/src/bcrypt.rs` modulo the
//! eprintln! debug lines that have been on the perry-stdlib copy since v0.5.0.

use perry_ffi::{
    alloc_string, nanbox_string_bits, pool, read_string, JsPromise, JsString, Promise, StringHeader,
};

/// Settle `promise` from one pool outcome. Every async entry point below
/// funnels through this so a cancelled or panicking job can never leave the
/// awaiter hanging (turnloop DESIGN D4).
fn settle_string(promise: JsPromise, outcome: pool::Outcome<Result<String, String>>, what: &str) {
    match outcome {
        pool::Outcome::Done(Ok(value)) => promise.resolve_string(&value),
        pool::Outcome::Done(Err(message)) => promise.reject_string(&message),
        pool::Outcome::Cancelled => promise.reject_string(&format!("{what} was cancelled")),
        pool::Outcome::Failed => promise.reject_string(&format!("{what} failed")),
    }
}

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
    pool::submit_or_run_inline(
        move || bcrypt::hash(&password, cost).map_err(|e| format!("Bcrypt error: {}", e)),
        move |outcome| settle_string(promise, outcome, "bcrypt.hash"),
    );
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

    pool::submit_or_run_inline(
        move || bcrypt::verify(&password, &hash).map_err(|e| format!("Bcrypt verify error: {}", e)),
        move |outcome| match outcome {
            pool::Outcome::Done(Ok(matches)) => promise.resolve_bool(matches),
            pool::Outcome::Done(Err(message)) => promise.reject_string(&message),
            pool::Outcome::Cancelled => promise.reject_string("bcrypt.compare was cancelled"),
            pool::Outcome::Failed => promise.reject_string("bcrypt.compare failed"),
        },
    );
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
    pool::submit_or_run_inline(
        move || match bcrypt::hash("", cost) {
            Ok(h) if h.len() >= 29 => Ok(h[..29].to_string()),
            Ok(_) => Err("Invalid hash format".to_string()),
            Err(e) => Err(format!("{}", e)),
        },
        move |outcome| settle_string(promise, outcome, "bcrypt.genSalt"),
    );
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
