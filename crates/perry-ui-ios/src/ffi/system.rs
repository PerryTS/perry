//! FFI exports: system APIs, audio (AVAudioEngine)
//!
//! Extracted from `lib.rs` for file-size hygiene. No behavior changes.

use crate::*;
use core::ffi::{c_char, CStr};

// =============================================================================
// System APIs (perry/system module)
// =============================================================================

// Emit a first-call no-op-stub diagnostic through perry-runtime's stable
// C-ABI shim (`perry_stub_warn_ffi`) rather than the hash-mangled Rust path
// `perry_runtime::stub_diag::perry_stub_warn`. Under the `geisterhand`
// feature this UI lib and the linked runtime can be built with different
// Cargo feature sets, which makes Rust-mangled symbols unresolvable at link
// time (#1311); a `#[no_mangle]` symbol resolves regardless. Mirrors how
// perry-ui-macos reaches `perry_app_group_suite_name`. `name`/`reason`/`issue`
// are `'static` C-string literals.
fn stub_warn(name: &CStr, reason: &CStr, issue: Option<&CStr>) {
    extern "C" {
        fn perry_stub_warn_ffi(name: *const c_char, reason: *const c_char, issue: *const c_char);
    }
    unsafe {
        perry_stub_warn_ffi(
            name.as_ptr(),
            reason.as_ptr(),
            issue.map_or(std::ptr::null(), CStr::as_ptr),
        );
    }
}

/// #917 — system share sheet (text). MVP stub on iOS: emits a
/// first-call warning. The native implementation (issue follow-up)
/// will wrap `UIActivityViewController` with the text in
/// `activityItems`, anchored to the key window's root view
/// controller. Kept stub-shaped so the symbol exists on every
/// platform — apps can compile-test against the API.
#[no_mangle]
pub extern "C" fn perry_system_share_text(_text_ptr: i64, _title_ptr: i64) {
    stub_warn(
        c"perry_system_share_text",
        c"iOS UIActivityViewController not yet implemented (#917 follow-up)",
        Some(c"#917"),
    );
}

/// #917 — system share sheet (URL). MVP stub on iOS.
#[no_mangle]
pub extern "C" fn perry_system_share_url(_url_ptr: i64, _title_ptr: i64) {
    stub_warn(
        c"perry_system_share_url",
        c"iOS UIActivityViewController not yet implemented (#917 follow-up)",
        Some(c"#917"),
    );
}

// #675 + #1178 — App Group / cross-process shared storage on iOS.
//
// Backed by `NSUserDefaults(suiteName:)`. The suite name is baked
// into `main()`'s prelude by the CLI from `[ios] app_group` in
// perry.toml (see `perry-runtime::app_group::perry_app_group_init`)
// so widget extensions sharing the same App Group container can read
// keys written here. When no suite is configured we emit a one-shot
// stub-warn diagnostic naming the missing `[ios] app_group` key so
// developers see why the widget can't see the value, rather than a
// silent in-process HashMap that lies about cross-process behavior.
fn app_group_str_from_header_ios(ptr: *const u8) -> &'static str {
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

/// Resolve the configured App Group suite, warning once per process
/// when no `[ios] app_group` was baked in. Returns `None` so the
/// caller can short-circuit before reaching for `UserDefaults`.
fn app_group_suite_ios() -> Option<objc2::rc::Retained<objc2_foundation::NSString>> {
    // Reach the baked-in suite name through perry-runtime's stable C-ABI
    // accessor instead of the hash-mangled Rust `configured_suite_name()`, so
    // the symbol resolves even when this UI lib and the linked runtime were
    // built with different Cargo feature sets (#1311). Mirrors perry-ui-macos.
    extern "C" {
        fn perry_app_group_suite_name(out_len: *mut i32) -> *const u8;
    }
    unsafe {
        let mut len: i32 = 0;
        let ptr = perry_app_group_suite_name(&mut len as *mut i32);
        if ptr.is_null() || len <= 0 {
            return None;
        }
        let slice = std::slice::from_raw_parts(ptr, len as usize);
        let suite = std::str::from_utf8(slice).ok()?;
        Some(objc2_foundation::NSString::from_str(suite))
    }
}

fn warn_app_group_not_configured(symbol: &CStr) {
    stub_warn(
        symbol,
        c"App Group not configured. Add `[ios] app_group = \"group.com.example.shared\"` to perry.toml (#1178).",
        Some(c"#1178"),
    );
}

unsafe fn app_group_defaults_ios() -> *mut objc2::runtime::AnyObject {
    let Some(suite) = app_group_suite_ios() else {
        return std::ptr::null_mut();
    };
    let cls = objc2::runtime::AnyClass::get(c"NSUserDefaults").unwrap();
    let alloc: *mut objc2::runtime::AnyObject = objc2::msg_send![cls, alloc];
    let defaults: *mut objc2::runtime::AnyObject =
        objc2::msg_send![alloc, initWithSuiteName: &*suite];
    defaults
}

#[no_mangle]
pub extern "C" fn perry_system_app_group_set(key_ptr: i64, value_ptr: i64) {
    let key = app_group_str_from_header_ios(key_ptr as *const u8);
    let value = app_group_str_from_header_ios(value_ptr as *const u8);
    if key.is_empty() {
        return;
    }
    unsafe {
        let defaults = app_group_defaults_ios();
        if defaults.is_null() {
            warn_app_group_not_configured(c"perry_system_app_group_set");
            return;
        }
        let ns_key = objc2_foundation::NSString::from_str(key);
        let ns_value = objc2_foundation::NSString::from_str(value);
        let _: () = objc2::msg_send![defaults, setObject: &*ns_value, forKey: &*ns_key];
        let _: () = objc2::msg_send![defaults, synchronize];
    }
}
#[no_mangle]
pub extern "C" fn perry_system_app_group_get(key_ptr: i64) -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    }
    let empty = || unsafe { js_string_from_bytes(std::ptr::null(), 0) };
    let key = app_group_str_from_header_ios(key_ptr as *const u8);
    if key.is_empty() {
        return empty();
    }
    unsafe {
        let defaults = app_group_defaults_ios();
        if defaults.is_null() {
            warn_app_group_not_configured(c"perry_system_app_group_get");
            return empty();
        }
        let ns_key = objc2_foundation::NSString::from_str(key);
        let value: *mut objc2::runtime::AnyObject =
            objc2::msg_send![defaults, stringForKey: &*ns_key];
        if value.is_null() {
            return empty();
        }
        let utf8_ptr: *const u8 = objc2::msg_send![value, UTF8String];
        if utf8_ptr.is_null() {
            return empty();
        }
        // NSUTF8StringEncoding = 4
        let utf8_len: usize = objc2::msg_send![value, lengthOfBytesUsingEncoding: 4u64];
        if utf8_len == 0 {
            return empty();
        }
        js_string_from_bytes(utf8_ptr, utf8_len as i32)
    }
}
#[no_mangle]
pub extern "C" fn perry_system_app_group_delete(key_ptr: i64) {
    let key = app_group_str_from_header_ios(key_ptr as *const u8);
    if key.is_empty() {
        return;
    }
    unsafe {
        let defaults = app_group_defaults_ios();
        if defaults.is_null() {
            warn_app_group_not_configured(c"perry_system_app_group_delete");
            return;
        }
        let ns_key = objc2_foundation::NSString::from_str(key);
        let _: () = objc2::msg_send![defaults, removeObjectForKey: &*ns_key];
        let _: () = objc2::msg_send![defaults, synchronize];
    }
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
/// Returns an empty string if no key window is available (e.g. before the
/// scene is attached, in tests, or in CLI builds) or capture fails.
#[no_mangle]
pub extern "C" fn perry_system_take_screenshot() -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: u32) -> *mut u8;
    }
    use base64::Engine as _;
    unsafe {
        let mut len: usize = 0;
        let ptr = crate::screenshot::perry_ui_screenshot_capture(&mut len as *mut usize);
        if ptr.is_null() || len == 0 {
            return js_string_from_bytes(std::ptr::null(), 0) as i64;
        }
        let bytes = std::slice::from_raw_parts(ptr, len);
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        libc::free(ptr as *mut libc::c_void);
        js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32) as i64
    }
}

/// #1475 — safe-area insets. Reads `UIWindow.safeAreaInsets` from the key
/// window and returns `{ top, right, bottom, left }` (points). Falls back to
/// all-zero when no key window is attached yet (e.g. before the scene
/// connects, in tests, or in headless CLI builds).
#[no_mangle]
pub extern "C" fn perry_system_get_safe_area_insets() -> f64 {
    extern "C" {
        fn perry_safe_area_insets_make(top: f64, right: f64, bottom: f64, left: f64) -> f64;
    }
    let mut insets = crate::widgets::vstack::UIEdgeInsets {
        top: 0.0,
        left: 0.0,
        bottom: 0.0,
        right: 0.0,
    };
    unsafe {
        if let Some(app_cls) = objc2::runtime::AnyClass::get(c"UIApplication") {
            let app: *mut objc2::runtime::AnyObject = objc2::msg_send![app_cls, sharedApplication];
            if !app.is_null() {
                let key_window: *mut objc2::runtime::AnyObject = objc2::msg_send![app, keyWindow];
                if !key_window.is_null() {
                    insets = objc2::msg_send![key_window, safeAreaInsets];
                }
            }
        }
        perry_safe_area_insets_make(insets.top, insets.right, insets.bottom, insets.left)
    }
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
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: u32) -> *mut u8;
    }
    unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) as i64 }
}

// ---- perry/background (issue #538) — BGTaskScheduler ----
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
/// Bug-report-flow utility: stable OS-version string. MVP stub on
/// iOS; native impl will use `[[UIDevice currentDevice] systemVersion]`.
#[no_mangle]
pub extern "C" fn perry_system_get_os_version() -> i64 {
    stub_warn(
        c"perry_system_get_os_version",
        c"iOS getOSVersion not yet implemented (UIDevice.systemVersion follow-up)",
        Some(c"#918"),
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
