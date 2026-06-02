//! Geolocation + image picker stubs (issue #552).
//!
//! This platform does not implement the new geolocation/image-picker FFI
//! surface. The error-callback variants invoke the user's `onError` /
//! permission-status callback with `"unsupported-platform"`; the image
//! picker callback receives an empty array.

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: u32) -> *mut u8;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_array_alloc(capacity: u32) -> *mut core::ffi::c_void;
    fn js_nanbox_pointer(ptr: i64) -> f64;
}

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

unsafe fn unsupported_string() -> f64 {
    let bytes = b"unsupported-platform";
    let s = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    js_nanbox_string(s as i64)
}

#[no_mangle]
pub extern "C" fn perry_system_geolocation_get_current(_on_success: f64, on_error: f64) {
    unsafe {
        if on_error.to_bits() == TAG_UNDEFINED {
            return;
        }
        let nb = unsupported_string();
        let cb = js_nanbox_get_pointer(on_error) as *const u8;
        js_closure_call1(cb, nb);
    }
}

#[no_mangle]
pub extern "C" fn perry_system_geolocation_watch(_callback: f64) -> f64 {
    0.0
}

#[no_mangle]
pub extern "C" fn perry_system_geolocation_stop_watch(_id: f64) {}

#[no_mangle]
pub extern "C" fn perry_system_geolocation_request_permission(callback: f64) {
    unsafe {
        let nb = unsupported_string();
        let cb = js_nanbox_get_pointer(callback) as *const u8;
        js_closure_call1(cb, nb);
    }
}

#[no_mangle]
pub extern "C" fn perry_system_image_picker_pick(
    _max_count: f64,
    _allow_multiple: f64,
    callback: f64,
) {
    unsafe {
        let arr = js_array_alloc(0);
        let nb = js_nanbox_pointer(arr as i64);
        let cb = js_nanbox_get_pointer(callback) as *const u8;
        js_closure_call1(cb, nb);
    }
}

// ---- perry/background (issue #538) — no-op stubs on Windows. ----
//
// Win32 has no Perry-callable equivalent for "wake up an otherwise-not-
// running desktop app on a schedule" that doesn't require either:
//   - admin elevation (Task Scheduler API — `ITaskService::NewTask` plus
//     `RegisterTaskDefinition` requires the calling user to have Schedule
//     Tasks rights; ordinary user-installed apps can register but the
//     trigger only fires while the user is logged in, and the
//     registration step itself prompts UAC),
//   - or the app to be a packaged WinUI 3 / UWP app (MSIX) that declares
//     `<BackgroundTasks>` extensions in its appxmanifest and provides a
//     dedicated background-task handler component. Perry-compiled apps
//     are plain Win32 executables, not MSIX packages.
// A Perry Windows app that wants periodic refresh while the app IS
// running can use `setInterval()` — the OS-managed wake-up-while-
// suspended contract is unavailable to plain Win32. `registerTask`
// records nothing; `schedule` and `cancel` are silent no-ops.
#[no_mangle]
pub extern "C" fn perry_background_register_task(_identifier_ptr: i64, _handler: f64) {}
#[no_mangle]
pub extern "C" fn perry_background_schedule(
    _identifier_ptr: i64,
    _kind_ptr: i64,
    _earliest_start_ms: f64,
    _requires_network: f64,
    _requires_charging: f64,
) {
}
#[no_mangle]
pub extern "C" fn perry_background_cancel(_identifier_ptr: i64) {}
