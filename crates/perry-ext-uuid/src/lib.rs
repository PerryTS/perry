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

/// Parse a NaN-boxed namespace argument into a `Uuid`. The shim only
/// supports the string-UUID namespace form (`v5(name, '6ba7…')`), which
/// covers the `uuid.v5.DNS`/`uuid.v5.URL` constants and the overwhelming
/// majority of real usage. A non-string / unparseable namespace falls
/// back to the nil UUID rather than crashing — the array-namespace form
/// is only reachable via `perry.compilePackages` (real source).
unsafe fn parse_namespace(ns_ptr: *const StringHeader) -> Uuid {
    let handle = JsString::from_raw(ns_ptr as *mut StringHeader);
    read_string(handle)
        .and_then(|s| Uuid::parse_str(s).ok())
        .unwrap_or_else(Uuid::nil)
}

/// `uuid.v5(name, namespace)` — SHA-1 name-based UUID.
///
/// # Safety
///
/// `name_ptr` / `ns_ptr` must be null or Perry-runtime `StringHeader`
/// pointers.
#[no_mangle]
pub unsafe extern "C" fn js_uuid_v5(
    name_ptr: *const StringHeader,
    ns_ptr: *const StringHeader,
) -> *mut StringHeader {
    let name = read_string(JsString::from_raw(name_ptr as *mut StringHeader)).unwrap_or("");
    let namespace = parse_namespace(ns_ptr);
    let uuid = Uuid::new_v5(&namespace, name.as_bytes());
    alloc_string(&uuid.to_string()).as_raw()
}

/// `uuid.v3(name, namespace)` — MD5 name-based UUID.
///
/// # Safety
///
/// `name_ptr` / `ns_ptr` must be null or Perry-runtime `StringHeader`
/// pointers.
#[no_mangle]
pub unsafe extern "C" fn js_uuid_v3(
    name_ptr: *const StringHeader,
    ns_ptr: *const StringHeader,
) -> *mut StringHeader {
    let name = read_string(JsString::from_raw(name_ptr as *mut StringHeader)).unwrap_or("");
    let namespace = parse_namespace(ns_ptr);
    let uuid = Uuid::new_v3(&namespace, name.as_bytes());
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

    #[test]
    fn v5_matches_the_reference_vector() {
        // `v5('perry', '6ba7b810-9dad-11d1-80b4-00c04fd430c8')` — the
        // exact value Node's `uuid` produces (issue #5197).
        let name = alloc_string("perry");
        let ns = alloc_string("6ba7b810-9dad-11d1-80b4-00c04fd430c8");
        let id =
            read_handle(unsafe { js_uuid_v5(name.as_raw() as *const _, ns.as_raw() as *const _) });
        assert_eq!(id, "6cb3836f-339d-52d8-acc6-8751229b61cf");
        let id_handle = alloc_string(&id);
        assert_eq!(
            unsafe { js_uuid_version(id_handle.as_raw() as *const _) },
            5.0
        );
    }

    #[test]
    fn v3_matches_the_reference_vector() {
        // `v3('perry', '6ba7b810-9dad-11d1-80b4-00c04fd430c8')`.
        let name = alloc_string("perry");
        let ns = alloc_string("6ba7b810-9dad-11d1-80b4-00c04fd430c8");
        let id =
            read_handle(unsafe { js_uuid_v3(name.as_raw() as *const _, ns.as_raw() as *const _) });
        assert_eq!(id, "3533df6e-72b1-3859-a772-7410b3d2f9c2");
        let id_handle = alloc_string(&id);
        assert_eq!(
            unsafe { js_uuid_version(id_handle.as_raw() as *const _) },
            3.0
        );
    }
}
