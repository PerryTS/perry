extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

/// Open a file dialog (stub — UIDocumentPickerVC not yet implemented).
/// Calls callback with TAG_UNDEFINED immediately.
pub fn open_dialog(callback: f64) {
    unsafe {
        let closure_ptr = js_nanbox_get_pointer(callback) as *const u8;
        // Call callback with TAG_UNDEFINED (user "cancelled")
        js_closure_call1(closure_ptr, f64::from_bits(0x7FFC_0000_0000_0001));
    }
}
