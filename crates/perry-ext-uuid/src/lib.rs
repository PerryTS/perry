//! Native bindings for the npm `uuid` package.
//!
//! Functionally identical to `crates/perry-stdlib/src/uuid.rs`. Only
//! depends on [`perry_ffi`] — third wrapper port under #466 Phase 5.

use perry_ffi::{alloc_string, read_string, JsString, StringHeader};
use uuid::Uuid;

/// `uuid.v4()` — random UUID.
#[no_mangle]
pub extern "C" fn js_uuid_v4() -> *mut StringHeader {
    let uuid = Uuid::new_v4();
    alloc_string(&uuid.to_string()).as_raw()
}

/// `uuid.v1()` — timestamp + node-id UUID. Node id is random
/// (Perry doesn't introspect the host MAC).
#[no_mangle]
pub extern "C" fn js_uuid_v1() -> *mut StringHeader {
    let ts = uuid::Timestamp::now(uuid::NoContext);
    let uuid = Uuid::new_v1(ts, &[0x01, 0x23, 0x45, 0x67, 0x89, 0xab]);
    alloc_string(&uuid.to_string()).as_raw()
}

/// `uuid.v7()` — Unix-timestamp UUID.
#[no_mangle]
pub extern "C" fn js_uuid_v7() -> *mut StringHeader {
    let uuid = Uuid::now_v7();
    alloc_string(&uuid.to_string()).as_raw()
}

/// `uuid.validate(str) -> boolean` — encoded as `1.0` / `0.0`
/// because the Perry FFI ABI carries booleans as f64.
///
/// # Safety
///
/// `str_ptr` must be null or a Perry-runtime `StringHeader` pointer.
#[no_mangle]
pub unsafe extern "C" fn js_uuid_validate(str_ptr: *const StringHeader) -> f64 {
    let handle = JsString::from_raw(str_ptr as *mut StringHeader);
    let Some(s) = read_string(handle) else {
        return 0.0;
    };
    if Uuid::parse_str(s).is_ok() {
        1.0
    } else {
        0.0
    }
}

/// `uuid.version(str) -> number` — version digit, or `NaN` if the
/// input isn't a valid UUID.
///
/// # Safety
///
/// `str_ptr` must be null or a Perry-runtime `StringHeader` pointer.
#[no_mangle]
pub unsafe extern "C" fn js_uuid_version(str_ptr: *const StringHeader) -> f64 {
    let handle = JsString::from_raw(str_ptr as *mut StringHeader);
    let Some(s) = read_string(handle) else {
        return f64::NAN;
    };
    match Uuid::parse_str(s) {
        Ok(uuid) => uuid.get_version_num() as f64,
        Err(_) => f64::NAN,
    }
}

/// `uuid.NIL` — all-zeros sentinel UUID, as a string.
#[no_mangle]
pub extern "C" fn js_uuid_nil() -> *mut StringHeader {
    alloc_string(&Uuid::nil().to_string()).as_raw()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_handle(handle: *mut StringHeader) -> String {
        read_string(unsafe { JsString::from_raw(handle) })
            .expect("non-null")
            .to_string()
    }

    #[test]
    fn v4_is_36_chars_with_dashes() {
        let s = read_handle(js_uuid_v4());
        assert_eq!(s.len(), 36);
        assert_eq!(s.chars().filter(|c| *c == '-').count(), 4);
    }

    #[test]
    fn v1_v7_round_trip_through_validate_and_version() {
        // Spelled out per-version because `extern "C" fn` doesn't
        // coerce to plain `fn` without a wrapper closure, and the
        // payoff of compressing two assertions isn't worth one.
        for (s, want_ver) in [
            (read_handle(js_uuid_v1()), 1.0),
            (read_handle(js_uuid_v7()), 7.0),
        ] {
            let s_handle = alloc_string(&s);
            let valid = unsafe { js_uuid_validate(s_handle.as_raw() as *const _) };
            assert_eq!(valid, 1.0, "{} should validate", s);
            let ver = unsafe { js_uuid_version(s_handle.as_raw() as *const _) };
            assert_eq!(ver, want_ver, "{} version", s);
        }
    }

    #[test]
    fn validate_rejects_garbage() {
        let s = alloc_string("not a uuid");
        let valid = unsafe { js_uuid_validate(s.as_raw() as *const _) };
        assert_eq!(valid, 0.0);
    }

    #[test]
    fn version_returns_nan_for_garbage() {
        let s = alloc_string("not a uuid");
        let ver = unsafe { js_uuid_version(s.as_raw() as *const _) };
        assert!(ver.is_nan());
    }

    #[test]
    fn nil_is_all_zeros_with_dashes() {
        let s = read_handle(js_uuid_nil());
        assert_eq!(s, "00000000-0000-0000-0000-000000000000");
    }
}
