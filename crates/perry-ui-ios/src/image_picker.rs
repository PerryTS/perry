//! Photo-library image picker (issue #552).
//!
//! Currently a stub that immediately invokes the callback with an empty array.
//! Wiring a real PHPickerViewController + NSItemProvider.loadFileRepresentation
//! pipeline is tracked as a #552 follow-up — it requires a dynamic ObjC
//! delegate, block-based completion handlers, and async file copy out of the
//! sandboxed Photos asset URLs to NSTemporaryDirectory.

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_array_alloc(capacity: u32) -> *mut std::ffi::c_void;
    fn js_nanbox_pointer(ptr: i64) -> f64;
}

pub fn pick(_max_count: f64, _allow_multiple: f64, callback: f64) {
    unsafe {
        let arr = js_array_alloc(0);
        let nb = js_nanbox_pointer(arr as i64);
        let cb_ptr = js_nanbox_get_pointer(callback) as *const u8;
        js_closure_call1(cb_ptr, nb);
    }
}
