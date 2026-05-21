//! Auto-split from `crates/perry-ui-tvos/src/lib.rs`. See `ffi/mod.rs`.

#![allow(clippy::missing_safety_doc)]

use crate::*;

// =============================================================================
// System APIs (perry/system module)
// =============================================================================

/// #917 — system share sheet stub on tvOS. Apple's tvOS doesn't
/// expose UIActivityViewController; the typical tvOS share flow uses
/// AirDrop only and isn't programmatically invokable. Stub + first-
/// call warning.
#[no_mangle]
pub extern "C" fn perry_system_share_text(_text_ptr: i64, _title_ptr: i64) {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_share_text",
        "tvOS does not expose a programmatic share sheet (#917)",
        Some("#917"),
    );
}

/// #917 — system share sheet stub on tvOS.
#[no_mangle]
pub extern "C" fn perry_system_share_url(_url_ptr: i64, _title_ptr: i64) {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_share_url",
        "tvOS does not expose a programmatic share sheet (#917)",
        Some("#917"),
    );
}

// #675 — App Group / cross-process shared storage. MVP stubs on
// tvOS; tvOS user apps don't typically share data cross-process,
// and the matching real impl (UserDefaults(suiteName:)) isn't
// commonly used here. Stub + first-call warning so callers know.
#[no_mangle]
pub extern "C" fn perry_system_app_group_set(_key_ptr: i64, _value_ptr: i64) {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_app_group_set",
        "tvOS App Group not implemented (#675 follow-up)",
        Some("#675"),
    );
}
#[no_mangle]
pub extern "C" fn perry_system_app_group_get(_key_ptr: i64) -> i64 {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_app_group_get",
        "tvOS App Group not implemented (#675 follow-up)",
        Some("#675"),
    );
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    }
    unsafe { js_string_from_bytes(std::ptr::null(), 0) }
}
#[no_mangle]
pub extern "C" fn perry_system_app_group_delete(_key_ptr: i64) {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_app_group_delete",
        "tvOS App Group not implemented (#675 follow-up)",
        Some("#675"),
    );
}

/// Open a URL in the default browser/app.
#[no_mangle]
pub extern "C" fn perry_system_open_url(url_ptr: i64) {
    fn str_from_header(ptr: *const u8) -> &'static str {
        if ptr.is_null() {
            return "";
        }
        unsafe {
            let header = ptr as *const perry_runtime::string::StringHeader;
            let len = (*header).byte_len as usize;
            let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
        }
    }
    let url_str = str_from_header(url_ptr as *const u8);
    unsafe {
        let ns_url_str = objc2_foundation::NSString::from_str(url_str);
        let url_cls = objc2::runtime::AnyClass::get(c"NSURL").unwrap();
        let url: *mut objc2::runtime::AnyObject =
            objc2::msg_send![url_cls, URLWithString: &*ns_url_str];
        if !url.is_null() {
            let app_cls = objc2::runtime::AnyClass::get(c"UIApplication").unwrap();
            let app: *mut objc2::runtime::AnyObject = objc2::msg_send![app_cls, sharedApplication];
            let _: () = objc2::msg_send![app, openURL: url];
        }
    }
}

/// Request one-shot location. Callback receives (lat, lon) or (NaN, NaN) on error.
#[no_mangle]
pub extern "C" fn perry_system_request_location(callback: f64) {
    location::request_location(callback);
}

// =============================================================================
// Audio (perry/system) — AVAudioEngine-based microphone capture
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_system_audio_start() -> i64 {
    audio::start()
}

#[no_mangle]
pub extern "C" fn perry_system_audio_stop() {
    audio::stop()
}

#[no_mangle]
pub extern "C" fn perry_system_audio_get_level() -> f64 {
    audio::get_level()
}

#[no_mangle]
pub extern "C" fn perry_system_audio_get_peak() -> f64 {
    audio::get_peak()
}

#[no_mangle]
pub extern "C" fn perry_system_audio_get_waveform(count: f64) -> f64 {
    audio::get_waveform(count)
}

#[no_mangle]
pub extern "C" fn perry_system_get_device_model() -> i64 {
    audio::get_device_model()
}
/// Bug-report-flow utility: stable OS-version string. tvOS stub —
/// native impl can mirror iOS's `[[UIDevice currentDevice] systemVersion]`
/// (UIDevice is available on tvOS).
#[no_mangle]
pub extern "C" fn perry_system_get_os_version() -> i64 {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_get_os_version",
        "tvOS getOSVersion not yet implemented",
        None,
    );
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    }
    unsafe { js_string_from_bytes(std::ptr::null(), 0) }
}
#[no_mangle]
pub extern "C" fn perry_system_audio_set_output_filename(filename_ptr: i64) {
    fn str_from_header(ptr: *const u8) -> &'static str {
        if ptr.is_null() {
            return "";
        }
        unsafe {
            let header = ptr as *const perry_runtime::string::StringHeader;
            let len = (*header).byte_len as usize;
            let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
        }
    }
    let filename = str_from_header(filename_ptr as *const u8);
    audio::set_output_filename(filename);
}
#[no_mangle]
pub extern "C" fn perry_system_audio_start_recording() {
    audio::start_recording();
}
#[no_mangle]
pub extern "C" fn perry_system_audio_stop_recording() {
    audio::stop_recording();
}
