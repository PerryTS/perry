use objc2::msg_send;
use objc2::rc::Retained;
use objc2_foundation::NSString;

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

/// Read the current text from the system clipboard (UIPasteboard).
/// Returns a NaN-boxed string (f64) or TAG_UNDEFINED if empty.
pub fn read() -> f64 {
    unsafe {
        let pasteboard: *const objc2::runtime::AnyObject = msg_send![
            objc2::runtime::AnyClass::get(c"UIPasteboard").unwrap(),
            generalPasteboard
        ];
        let text: *const NSString = msg_send![pasteboard, string];
        if !text.is_null() {
            let rust_str = (*text).to_string();
            let bytes = rust_str.as_bytes();
            let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
            js_nanbox_string(str_ptr as i64)
        } else {
            // Return TAG_UNDEFINED
            f64::from_bits(0x7FFC_0000_0000_0001)
        }
    }
}

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

/// Write text to the system clipboard (UIPasteboard).
pub fn write(text_ptr: *const u8) {
    let text = str_from_header(text_ptr);
    unsafe {
        let pasteboard: *const objc2::runtime::AnyObject = msg_send![
            objc2::runtime::AnyClass::get(c"UIPasteboard").unwrap(),
            generalPasteboard
        ];
        let ns_string = NSString::from_str(text);
        let _: () = msg_send![pasteboard, setString: &*ns_string];
    }
}
