use crate::ffi::js_string_from_bytes;
use crate::*;

// =============================================================================
// Location (perry/system) — stub on macOS, iOS only
// =============================================================================

/// Request one-shot location.
#[no_mangle]
pub extern "C" fn perry_system_request_location(callback: f64) {
    location::request_location(callback);
}

// ---- Geolocation + image picker (issue #552) ----
#[no_mangle]
pub extern "C" fn perry_system_geolocation_get_current(on_success: f64, on_error: f64) {
    geolocation::get_current(on_success, on_error);
}
#[no_mangle]
pub extern "C" fn perry_system_geolocation_watch(callback: f64) -> f64 {
    geolocation::watch(callback)
}
#[no_mangle]
pub extern "C" fn perry_system_geolocation_stop_watch(id: f64) {
    geolocation::stop_watch(id);
}
#[no_mangle]
pub extern "C" fn perry_system_geolocation_request_permission(callback: f64) {
    geolocation::request_permission(callback);
}
#[no_mangle]
pub extern "C" fn perry_system_image_picker_pick(
    max_count: f64,
    allow_multiple: f64,
    callback: f64,
) {
    image_picker::pick(max_count, allow_multiple, callback);
}

// ---- In-app screen capture (issue #918) ----
/// Capture the key window as a PNG and return a base64-encoded string.
/// Returns an empty string (interned via `js_string_from_bytes`) if no key
/// window is available (e.g. headless CLI builds) or capture fails.
#[no_mangle]
pub extern "C" fn perry_system_take_screenshot() -> i64 {
    use base64::Engine as _;
    unsafe {
        let mut len: usize = 0;
        let ptr = crate::screenshot::perry_ui_screenshot_capture(&mut len as *mut usize);
        if ptr.is_null() || len == 0 {
            return js_string_from_bytes(std::ptr::null(), 0) as i64;
        }
        let bytes = std::slice::from_raw_parts(ptr, len);
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        // perry_ui_screenshot_capture allocates with libc::malloc; release it.
        libc::free(ptr as *mut libc::c_void);
        js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32) as i64
    }
}

/// #1475 — safe-area insets. macOS windows have no status bar / home
/// indicator, so report all-zero insets. Keeps the symbol present so
/// `getSafeAreaInsets()` links on the host build.
#[no_mangle]
pub extern "C" fn perry_system_get_safe_area_insets() -> f64 {
    extern "C" {
        fn perry_safe_area_insets_make(top: f64, right: f64, bottom: f64, left: f64) -> f64;
    }
    unsafe { perry_safe_area_insets_make(0.0, 0.0, 0.0, 0.0) }
}

// ---- Network reachability (issue #582) ----
#[no_mangle]
pub extern "C" fn perry_system_network_get_status(callback: f64) {
    network::get_status(callback);
}
#[no_mangle]
pub extern "C" fn perry_system_network_on_change(callback: f64) -> f64 {
    network::on_change(callback)
}
#[no_mangle]
pub extern "C" fn perry_system_network_stop_on_change(id: f64) {
    network::stop_on_change(id);
}

// ---- Deep links (issue #583) ----
#[no_mangle]
pub extern "C" fn perry_system_app_on_open_url(callback: f64) {
    deeplinks::set_handler(callback);
}
#[no_mangle]
pub extern "C" fn perry_system_app_get_launch_url() -> i64 {
    let s = deeplinks::launch_url();
    let bytes = s.as_bytes();
    unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) as i64 }
}

// ---- perry/background (issue #538) — NSBackgroundActivityScheduler ----
#[no_mangle]
pub extern "C" fn perry_background_register_task(identifier_ptr: i64, handler: f64) {
    background::register_task(identifier_ptr as *const u8, handler);
}
#[no_mangle]
pub extern "C" fn perry_background_schedule(
    identifier_ptr: i64,
    kind_ptr: i64,
    earliest_start_ms: f64,
    requires_network: f64,
    requires_charging: f64,
) {
    background::schedule(
        identifier_ptr as *const u8,
        kind_ptr as *const u8,
        earliest_start_ms,
        requires_network,
        requires_charging,
    );
}
#[no_mangle]
pub extern "C" fn perry_background_cancel(identifier_ptr: i64) {
    background::cancel(identifier_ptr as *const u8);
}

// =============================================================================
// Audio (perry/system) — AVAudioEngine-based microphone capture
// =============================================================================

/// Start audio capture. Returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn perry_system_audio_start() -> i64 {
    audio::start()
}

/// Stop audio capture.
#[no_mangle]
pub extern "C" fn perry_system_audio_stop() {
    audio::stop()
}

/// Get current smoothed dB(A) level.
#[no_mangle]
pub extern "C" fn perry_system_audio_get_level() -> f64 {
    audio::get_level()
}

/// Get current peak sample amplitude.
#[no_mangle]
pub extern "C" fn perry_system_audio_get_peak() -> f64 {
    audio::get_peak()
}

/// Get recent dB samples for waveform rendering.
#[no_mangle]
pub extern "C" fn perry_system_audio_get_waveform(count: f64) -> f64 {
    audio::get_waveform(count)
}

/// Get device model identifier string.
#[no_mangle]
pub extern "C" fn perry_system_get_device_model() -> i64 {
    audio::get_device_model()
}

/// Bug-report-flow utility: stable OS-version string. Uses
/// `[[NSProcessInfo processInfo] operatingSystemVersionString]`
/// which returns a human-readable form like `"Version 14.5
/// (Build 23F79)"` — preserved as-is so triage can grep the build
/// number too. Returned as a Perry-managed string.
#[no_mangle]
pub extern "C" fn perry_system_get_os_version() -> i64 {
    unsafe {
        let cls = objc2::runtime::AnyClass::get(c"NSProcessInfo").unwrap();
        let info: *mut objc2::runtime::AnyObject = objc2::msg_send![cls, processInfo];
        if info.is_null() {
            return js_string_from_bytes(std::ptr::null(), 0) as i64;
        }
        let s: *mut objc2::runtime::AnyObject =
            objc2::msg_send![info, operatingSystemVersionString];
        if s.is_null() {
            return js_string_from_bytes(std::ptr::null(), 0) as i64;
        }
        let utf8_ptr: *const u8 = objc2::msg_send![s, UTF8String];
        if utf8_ptr.is_null() {
            return js_string_from_bytes(std::ptr::null(), 0) as i64;
        }
        let utf8_len: usize = objc2::msg_send![s, lengthOfBytesUsingEncoding: 4u64];
        if utf8_len == 0 {
            return js_string_from_bytes(std::ptr::null(), 0) as i64;
        }
        js_string_from_bytes(utf8_ptr, utf8_len as u32) as i64
    }
}

/// Set output filename for audio recording.
#[no_mangle]
pub extern "C" fn perry_system_audio_set_output_filename(filename_ptr: i64) {
    fn str_from_header(ptr: *const u8) -> &'static str {
        if ptr.is_null() {
            return "";
        }
        unsafe {
            let header = ptr as *const crate::string_header::StringHeader;
            let len = (*header).byte_len as usize;
            let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
        }
    }
    let filename = str_from_header(filename_ptr as *const u8);
    audio::set_output_filename(filename);
}

/// Start audio recording.
#[no_mangle]
pub extern "C" fn perry_system_audio_start_recording() {
    audio::start_recording();
}

/// Stop audio recording and save to file.
#[no_mangle]
pub extern "C" fn perry_system_audio_stop_recording() {
    audio::stop_recording();
}

/// Get the icon for a file/application at the given path. Returns a widget handle (NSImageView).
#[no_mangle]
pub extern "C" fn perry_system_get_app_icon(path_ptr: i64) -> i64 {
    app::get_app_icon(path_ptr as *const u8)
}

#[no_mangle]
pub extern "C" fn perry_system_get_locale() -> i64 {
    unsafe {
        let ns_locale: *mut objc2::runtime::AnyObject = objc2::msg_send![
            objc2::runtime::AnyClass::get(c"NSLocale").unwrap(),
            currentLocale
        ];
        let lang_code: *mut objc2::runtime::AnyObject = objc2::msg_send![ns_locale, languageCode];
        if lang_code.is_null() {
            let fallback = b"en";
            return js_string_from_bytes(fallback.as_ptr(), 2) as i64;
        }
        let utf8: *const u8 = objc2::msg_send![lang_code, UTF8String];
        let len = libc::strlen(utf8 as *const i8);
        let code_len = if len >= 2 { 2 } else { len };
        js_string_from_bytes(utf8, code_len as u32) as i64
    }
}
