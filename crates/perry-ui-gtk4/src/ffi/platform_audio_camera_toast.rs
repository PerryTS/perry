// FFI: Platform detection wrappers (__wrapper_perry_*), audio capture,
// camera stubs, cross-platform toast + reactive setText.
use crate::{app, audio, widgets};

// =============================================================================
// Platform Detection — __wrapper_perry_* required by Perry codegen
//
// Perry codegen emits calls to __wrapper_perry_X for every `declare function
// perry_X(...)` in TypeScript. These wrappers follow Perry's calling convention:
//   first param: i64 closure_ptr (ignored for FFI wrappers)
//   remaining params: f64 (NaN-boxed)
//   return: f64 (NaN-boxed string, plain f64, or TAG_UNDEFINED)
// =============================================================================

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

/// TAG_UNDEFINED — returned by void-returning wrappers.
const TAG_UNDEFINED: f64 = unsafe { std::mem::transmute(0x7FFC_0000_0000_0001_u64) };

fn nanbox_static_str(s: &'static [u8]) -> f64 {
    let ptr = unsafe { js_string_from_bytes(s.as_ptr(), s.len() as i64) };
    unsafe { js_nanbox_string(ptr as i64) }
}

/// perry_get_platform() → "linux"
#[no_mangle]
pub extern "C" fn __wrapper_perry_get_platform(_closure_ptr: i64) -> f64 {
    nanbox_static_str(b"linux")
}

/// perry_get_screen_width() → 1920 (desktop default; layout mode computed once at startup)
#[no_mangle]
pub extern "C" fn __wrapper_perry_get_screen_width(_closure_ptr: i64) -> f64 {
    1920.0
}

/// perry_get_screen_height() → 1080 (desktop default)
#[no_mangle]
pub extern "C" fn __wrapper_perry_get_screen_height(_closure_ptr: i64) -> f64 {
    1080.0
}

/// perry_get_scale_factor() → 1.0 (non-HiDPI default)
#[no_mangle]
pub extern "C" fn __wrapper_perry_get_scale_factor(_closure_ptr: i64) -> f64 {
    1.0
}

/// perry_get_orientation() → "landscape" (desktop is always landscape)
#[no_mangle]
pub extern "C" fn __wrapper_perry_get_orientation(_closure_ptr: i64) -> f64 {
    nanbox_static_str(b"landscape")
}

/// perry_has_hardware_keyboard() → true (desktop always has a keyboard)
#[no_mangle]
pub extern "C" fn __wrapper_perry_has_hardware_keyboard(_closure_ptr: i64) -> f64 {
    1.0
}

thread_local! {
    static RESIZE_CALLBACK: std::cell::RefCell<Option<f64>> = std::cell::RefCell::new(None);
}

/// perry_on_resize(callback) — store callback; called with (width, height) on resize.
#[no_mangle]
pub extern "C" fn __wrapper_perry_on_resize(_closure_ptr: i64, callback: f64) -> f64 {
    RESIZE_CALLBACK.with(|rc| {
        *rc.borrow_mut() = Some(callback);
    });
    TAG_UNDEFINED
}

/// perry_on_orientation_change(callback) — no-op on desktop (orientation never changes).
#[no_mangle]
pub extern "C" fn __wrapper_perry_on_orientation_change(_closure_ptr: i64, _callback: f64) -> f64 {
    TAG_UNDEFINED
}

/// perry_ui_poll_open_file() -> i64 — stub for Linux (macOS "Open With" not applicable).
/// Returns an empty string pointer; the IDE's checkOpenFileRequests() polls this every 500ms.
#[no_mangle]
pub extern "C" fn perry_ui_poll_open_file() -> i64 {
    unsafe { js_string_from_bytes(std::ptr::null(), 0) as i64 }
}

/// perry_get_device_idiom() → "desktop" — Linux is always a desktop (not
/// phone or pad). Returns a raw `*mut StringHeader` (i64); the
/// `ReturnKind::Str` dispatch row NaN-boxes it with STRING_TAG. (The
/// dispatch row declares no arguments — the previous `_closure_ptr`
/// parameter was a copy-paste leftover.)
#[no_mangle]
pub extern "C" fn perry_get_device_idiom() -> i64 {
    unsafe { js_string_from_bytes(b"desktop".as_ptr(), 7) as i64 }
}

// Audio capture (PulseAudio simple API)
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
/// Bug-report-flow utility: stable OS-version string. GTK4/Linux
/// stub — native impl would read `/etc/os-release` or
/// `uname -r`.
#[no_mangle]
pub extern "C" fn perry_system_get_os_version() -> i64 {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_get_os_version",
        "Linux getOSVersion (/etc/os-release) not yet implemented",
        None,
    );
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    }
    unsafe { js_string_from_bytes(std::ptr::null(), 0) }
}
#[no_mangle]
pub extern "C" fn perry_system_audio_set_output_filename(filename_ptr: i64) {
    let filename_handle = unsafe { perry_ffi::JsString::from_raw(filename_ptr as *mut _) };
    let filename = perry_ffi::read_string(filename_handle).unwrap_or("");
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
#[no_mangle]
pub extern "C" fn perry_system_audio_register_callback(callback: f64) {
    use std::io::{self, Write};
    writeln!(
        io::stderr(),
        "[LIB] perry_system_audio_register_callback called! callback={}",
        callback
    )
    .unwrap();
    audio::register_audio_callback(callback);
}
#[no_mangle]
pub extern "C" fn perry_system_audio_unregister_callback() {
    audio::unregister_audio_callback();
}

/// hone_get_documents_dir() — iOS sandbox documents dir stub.
/// Returns empty string; only reachable on iOS (__platform__ === 1), which is dead code on Linux.
#[no_mangle]
pub extern "C" fn __wrapper_hone_get_documents_dir(_closure_ptr: i64) -> f64 {
    nanbox_static_str(b"")
}

/// hone_get_app_files_dir() — Android app files dir stub.
/// Returns empty string; only reachable on Android (__platform__ === 2), dead code on Linux.
#[no_mangle]
pub extern "C" fn __wrapper_hone_get_app_files_dir(_closure_ptr: i64) -> f64 {
    nanbox_static_str(b"")
}

// --- CameraView widget (GTK4 / GStreamer implementation, issue #191) ---
// On Linux we drive a `v4l2src → videoconvert → appsink` GStreamer pipeline
// for live preview and per-frame callbacks (`crate::camera`). iOS / Android
// keep their own AVFoundation / CameraX backends.

#[no_mangle]
pub extern "C" fn perry_ui_camera_create() -> i64 {
    crate::camera::create()
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_start(handle: i64) {
    crate::camera::start(handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_stop(handle: i64) {
    crate::camera::stop(handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_freeze(handle: i64) {
    crate::camera::freeze(handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_unfreeze(handle: i64) {
    crate::camera::unfreeze(handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_sample_color(x: f64, y: f64) -> f64 {
    crate::camera::sample_color(x, y)
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_set_on_tap(handle: i64, callback: f64) {
    crate::camera::set_on_tap(handle, callback)
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_register_frame_callback(handle: i64, callback: f64) {
    crate::camera::register_frame_callback(handle, callback)
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_unregister_frame_callback(handle: i64) {
    crate::camera::unregister_frame_callback(handle)
}

// --- Cross-platform toast + reactive setText (Phase 2 v3.3) ---

/// Show a brief slide-down toast banner on the main window.
/// msg_ptr is a raw StringHeader pointer (NaN-boxed string, unboxed to i64 by codegen).
#[no_mangle]
pub extern "C" fn perry_ui_show_toast(msg_ptr: i64) {
    let msg = app::str_from_header(msg_ptr as *const u8);
    widgets::toast::show_toast(msg);
}

/// Create a Text (GtkLabel) widget and register it under a string id so that
/// perry_ui_set_text can update it imperatively later.
/// Returns the widget handle (1-based i64, NaN-boxed by codegen).
#[no_mangle]
pub extern "C" fn perry_ui_text_create_with_id(text_ptr: i64, id_ptr: i64) -> i64 {
    let handle = widgets::text::create(text_ptr as *const u8);
    let id = app::str_from_header(id_ptr as *const u8);
    widgets::text_registry::register(id, handle);
    handle
}

/// Update the label of a Text widget previously registered via
/// perry_ui_text_create_with_id.
#[no_mangle]
pub extern "C" fn perry_ui_set_text(id_ptr: i64, value_ptr: i64) {
    let id = app::str_from_header(id_ptr as *const u8);
    let value = app::str_from_header(value_ptr as *const u8);
    widgets::text_registry::set_text_for_id(id, value);
}
