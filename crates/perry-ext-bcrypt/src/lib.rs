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
//! The bundled stdlib build re-exports this crate (`perry-stdlib/src/bcrypt.rs`),
//! so both link modes share one provider per symbol.

use perry_ffi::{
    alloc_string, pool, read_string, JsPromise, JsString, JsValue, Promise, StringHeader,
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

/// `bcrypt.hash(password, saltOrRounds) -> Promise<string>`, with the same
/// argument handling as [`js_bcrypt_hash_sync`]. The hashing runs on Perry's
/// shared blocking pool so the main thread stays responsive.
///
/// # Safety
///
/// `password_ptr` must be null or point to a Perry-runtime
/// `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_bcrypt_hash(
    password_ptr: *const StringHeader,
    salt_or_rounds: i64,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    let password_handle = JsString::from_raw(password_ptr as *mut StringHeader);
    let Some(password) = read_string(password_handle).map(String::from) else {
        promise.reject_string("Password is null or invalid UTF-8");
        return raw;
    };

    let Some(salt_or_rounds) = SaltOrRounds::read(salt_or_rounds) else {
        promise.reject_string("data and salt arguments required");
        return raw;
    };
    pool::submit_or_run_inline(
        move || salt_or_rounds.hash(&password).ok_or_else(|| "Invalid salt".to_string()),
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

/// The cost a `rounds` argument names, as node's `genSaltSync` normalizes it:
/// a missing (`undefined`, NaN) or zero argument means the default of 10, and
/// any other value is clamped to bcrypt's 4..=31.
fn rounds_or_default(rounds: f64) -> u32 {
    if rounds.is_finite() && rounds != 0.0 {
        rounds.clamp(4.0, 31.0) as u32
    } else {
        10
    }
}

/// A `$2b$<cost>$<22-char-salt>` salt. The `bcrypt` crate exposes no salt
/// generator, so the random salt comes from a minimum-cost hash; its cost
/// never runs, it is only written into the prefix.
fn gen_salt(cost: u32) -> Result<String, String> {
    bcrypt::hash_with_result("", 4)
        .map(|parts| format!("$2b${cost:02}${}", parts.get_salt()))
        .map_err(|e| e.to_string())
}

/// Hash `password` with a `$2a$`/`$2b$` salt string (or a whole hash, whose
/// first 29 bytes are its salt), as node's `hashSync(data, salt)` does.
fn hash_with_salt_string(password: &str, salt: &str) -> Option<String> {
    let prefix = salt.get(..29)?;
    let version = match &prefix.as_bytes()[1..3] {
        b"2a" => bcrypt::Version::TwoA,
        b"2b" => bcrypt::Version::TwoB,
        _ => return None,
    };
    // `HashParts` parses only a whole 60-byte hash: pad the salt with an
    // all-zero digest to read its cost and raw salt bytes.
    let parts: bcrypt::HashParts = format!("{prefix}{}", ".".repeat(31)).parse().ok()?;
    bcrypt::hash_with_salt(password, parts.get_cost(), parts.get_salt_raw())
        .ok()
        .map(|hash| hash.format_for_version(version))
}

/// A `hash`/`hashSync` second argument: a salt string, or a cost that gets a
/// fresh salt.
enum SaltOrRounds {
    Salt(String),
    Rounds(u32),
}

impl SaltOrRounds {
    /// Reads the argument's NaN-boxed bits (`NA_JSV`); `None` for any other type.
    fn read(bits: i64) -> Option<Self> {
        let value = JsValue::from_bits(bits as u64);
        if value.is_any_string() {
            value.to_owned_string().map(Self::Salt)
        } else if value.is_number() {
            Some(Self::Rounds(rounds_or_default(value.to_number())))
        } else {
            None
        }
    }

    fn hash(&self, password: &str) -> Option<String> {
        match self {
            Self::Salt(salt) => hash_with_salt_string(password, salt),
            Self::Rounds(cost) => bcrypt::hash(password, *cost).ok(),
        }
    }
}

/// `bcrypt.genSalt(rounds?) -> Promise<string>`.
#[no_mangle]
pub extern "C" fn js_bcrypt_gen_salt(rounds: f64) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    let cost = rounds_or_default(rounds);
    pool::submit_or_run_inline(
        move || gen_salt(cost),
        move |outcome| settle_string(promise, outcome, "bcrypt.genSalt"),
    );
    raw
}

/// `bcrypt.genSaltSync(rounds?) -> string`. Returns the raw `StringHeader`
/// (the dispatch row's `NR_STR` boxes it); null on failure reads as `null`.
#[no_mangle]
pub extern "C" fn js_bcrypt_gen_salt_sync(rounds: f64) -> *mut StringHeader {
    match gen_salt(rounds_or_default(rounds)) {
        Ok(salt) => alloc_string(&salt).as_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// `bcrypt.hashSync(password, saltOrRounds) -> string`. `salt_or_rounds` is
/// the argument's NaN-boxed bits (`NA_JSV`): a salt string, or a cost that
/// gets a fresh salt. Returns the raw `StringHeader` (the dispatch row's
/// `NR_STR` boxes it); null on failure reads as `null`.
///
/// # Safety
///
/// `password_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_bcrypt_hash_sync(
    password_ptr: *const StringHeader,
    salt_or_rounds: i64,
) -> *mut StringHeader {
    let handle = JsString::from_raw(password_ptr as *mut StringHeader);
    let Some(password) = read_string(handle) else {
        return std::ptr::null_mut();
    };
    match SaltOrRounds::read(salt_or_rounds).and_then(|arg| arg.hash(password)) {
        Some(hash) => alloc_string(&hash).as_raw(),
        None => std::ptr::null_mut(),
    }
}

/// `bcrypt.compareSync(password, hash) -> boolean`, as a NaN-boxed boolean
/// (the dispatch row's `NR_F64` passes it through).
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
    let hash_handle = JsString::from_raw(hash_ptr as *mut StringHeader);
    let matches = match (read_string(pw_handle), read_string(hash_handle)) {
        (Some(password), Some(hash)) => bcrypt::verify(password, hash).unwrap_or(false),
        _ => false,
    };
    f64::from_bits(JsValue::from_bool(matches).bits())
}
