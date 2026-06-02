//! Network reachability stubs (issue #582).
//!
//! tvOS Apple TVs are typically wired (or always-on Wi-Fi) and the Network
//! framework is available, but Perry's tvOS surface ships UI-only today.
//! Until a real `NWPathMonitor` impl lands, these stubs report `connected =
//! true` with `kind = "unknown"` and suppress change events. Apps that need
//! true reachability detection should target iOS / macOS / Android.

extern "C" {
    fn js_closure_call2(closure: *const u8, arg0: f64, arg1: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: u32) -> *mut u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

unsafe fn nb_str(s: &str) -> f64 {
    let bytes = s.as_bytes();
    let p = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    js_nanbox_string(p as i64)
}

#[no_mangle]
pub extern "C" fn perry_system_network_get_status(callback: f64) {
    unsafe {
        let cb = js_nanbox_get_pointer(callback) as *const u8;
        if cb.is_null() {
            return;
        }
        js_closure_call2(cb, f64::from_bits(TAG_TRUE), nb_str("unknown"));
    }
}

#[no_mangle]
pub extern "C" fn perry_system_network_on_change(_callback: f64) -> f64 {
    0.0
}

#[no_mangle]
pub extern "C" fn perry_system_network_stop_on_change(_id: f64) {}
