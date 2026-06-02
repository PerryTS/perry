//! `net.isIP` / `isIPv4` / `isIPv6` (issue #811) + the Happy-Eyeballs
//! auto-select-family default accessors. Pure string/global-flag helpers
//! with no socket state. Split out of `lib.rs` (#1852) to keep that file
//! under the 2000-line gate.

use crate::string_from_header_i64;
use perry_ffi::JsValue;

extern "C" {
    fn js_net_validate_default_auto_select_family(value: f64) -> i32;
    fn js_net_validate_default_auto_select_family_attempt_timeout(value: f64) -> i32;
}

/// Issue #811 — `net.isIP(s)` returns 0/4/6 (number).
fn classify_ip(s: &str) -> i32 {
    if is_ipv4_str(s) {
        4
    } else if is_ipv6_str(s) {
        6
    } else {
        0
    }
}

fn is_ipv4_str(s: &str) -> bool {
    s.parse::<std::net::Ipv4Addr>().is_ok()
}

fn is_ipv6_str(s: &str) -> bool {
    if s.contains('[') || s.contains(']') {
        return false;
    }
    let address = match s.split_once('%') {
        Some((address, zone)) => {
            if zone.is_empty()
                || zone.contains('%')
                || !zone
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b':'))
            {
                return false;
            }
            address
        }
        None => s,
    };
    address.parse::<std::net::Ipv6Addr>().is_ok()
}

#[no_mangle]
pub unsafe extern "C" fn js_net_is_ip(s_ptr: i64) -> f64 {
    let kind = match string_from_header_i64(s_ptr) {
        Some(s) => classify_ip(&s),
        None => 0,
    };
    f64::from_bits(JsValue::from_number(kind as f64).0)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_is_ipv4(s_ptr: i64) -> f64 {
    let is = match string_from_header_i64(s_ptr) {
        Some(s) => is_ipv4_str(&s),
        None => false,
    };
    f64::from_bits(JsValue::from_bool(is).0)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_is_ipv6(s_ptr: i64) -> f64 {
    let is = match string_from_header_i64(s_ptr) {
        Some(s) => is_ipv6_str(&s),
        None => false,
    };
    f64::from_bits(JsValue::from_bool(is).0)
}

// Happy-Eyeballs (auto-select-family) defaults. Process-wide globals
// that `getDefault*` reads and `setDefault*` writes.
static AUTO_SELECT_FAMILY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
// Node's current default is 500ms (raised from 250 in v20.x); pin to
// 500 so byte-for-byte parity holds against `node --experimental-strip-types`.
static AUTO_SELECT_FAMILY_ATTEMPT_TIMEOUT_MS: std::sync::atomic::AtomicI32 =
    std::sync::atomic::AtomicI32::new(500);

#[no_mangle]
pub unsafe extern "C" fn js_net_get_default_auto_select_family() -> f64 {
    let v = AUTO_SELECT_FAMILY.load(std::sync::atomic::Ordering::Relaxed);
    f64::from_bits(JsValue::from_bool(v).0)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_set_default_auto_select_family(val_f64: f64) -> f64 {
    let val = unsafe { js_net_validate_default_auto_select_family(val_f64) } != 0;
    AUTO_SELECT_FAMILY.store(val, std::sync::atomic::Ordering::Relaxed);
    f64::from_bits(JsValue::UNDEFINED.0)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_get_default_auto_select_family_attempt_timeout() -> f64 {
    let v = AUTO_SELECT_FAMILY_ATTEMPT_TIMEOUT_MS.load(std::sync::atomic::Ordering::Relaxed);
    f64::from_bits(JsValue::from_number(v as f64).0)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_set_default_auto_select_family_attempt_timeout(ms_f64: f64) -> f64 {
    let ms = unsafe { js_net_validate_default_auto_select_family_attempt_timeout(ms_f64) };
    AUTO_SELECT_FAMILY_ATTEMPT_TIMEOUT_MS.store(ms, std::sync::atomic::Ordering::Relaxed);
    f64::from_bits(JsValue::UNDEFINED.0)
}
