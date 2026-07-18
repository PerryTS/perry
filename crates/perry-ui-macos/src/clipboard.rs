use crate::ffi::js_string_from_bytes;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2_app_kit::NSPasteboard;
use objc2_foundation::NSString;

extern "C" {
    fn js_nanbox_string(ptr: i64) -> f64;
}

/// Read the current text from the system clipboard.
/// Returns a NaN-boxed string (f64) or TAG_UNDEFINED if empty.
pub fn read() -> f64 {
    unsafe {
        let pasteboard = NSPasteboard::generalPasteboard();
        let ns_type = NSString::from_str("public.utf8-plain-text");
        let result: Option<Retained<NSString>> = msg_send![&*pasteboard, stringForType: &*ns_type];
        if let Some(text) = result {
            let rust_str = text.to_string();
            let bytes = rust_str.as_bytes();
            let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            js_nanbox_string(str_ptr as i64)
        } else {
            // Return TAG_UNDEFINED
            f64::from_bits(0x7FFC_0000_0000_0001)
        }
    }
}

/// Extract a &str from a *const StringHeader pointer.
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

/// Write text to the system clipboard.
pub fn write(text_ptr: *const u8) {
    let text = str_from_header(text_ptr);
    unsafe {
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        let ns_type = NSString::from_str("public.utf8-plain-text");
        let ns_string = NSString::from_str(text);
        let types = objc2_foundation::NSArray::from_retained_slice(&[ns_type]);
        pasteboard.declareTypes_owner(&types, None);
        let _: bool = msg_send![&*pasteboard, setString: &*ns_string, forType: &*objc2_foundation::NSString::from_str("public.utf8-plain-text")];
    }
}
