// FFI: device/screen stubs + audio capture (WASAPI) + OS version + device model.
use crate::audio;

// =============================================================================
// Device / screen stubs (iOS-only on macOS, stubs everywhere else)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_get_screen_width() -> f64 {
    0.0
}

#[no_mangle]
pub extern "C" fn perry_get_screen_height() -> f64 {
    0.0
}

#[no_mangle]
pub extern "C" fn perry_get_scale_factor() -> f64 {
    0.0
}

/// Layout-change callback registration — stub on every platform
/// (matches macOS shape). Real on-resize plumbing would wire WM_SIZE
/// → callback dispatch; for now apps poll dimensions via
/// `perry_get_screen_width` / `perry_get_screen_height`.
#[no_mangle]
pub extern "C" fn perry_on_layout_change(_callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_get_orientation() -> i64 {
    0
}

/// perry_get_device_idiom() → "desktop" (raw `*mut StringHeader`,
/// NaN-boxed by the `ReturnKind::Str` dispatch row). Was the bare
/// numeric phone code 0.0.
#[no_mangle]
pub extern "C" fn perry_get_device_idiom() -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    }
    unsafe { js_string_from_bytes(b"desktop".as_ptr(), 7) }
}

// Audio capture (WASAPI)
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
/// Bug-report-flow utility: stable OS-version string. Windows stub —
/// native impl will use `GetVersionEx` / `RtlGetVersion`.
#[no_mangle]
pub extern "C" fn perry_system_get_os_version() -> i64 {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_get_os_version",
        "Windows getOSVersion (RtlGetVersion) not yet implemented",
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
