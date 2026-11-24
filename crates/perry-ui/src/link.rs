//! What a native `Link` opens: the web schemes only. The platform open calls take more
//! (Windows runs a path, Linux opens a file), and a relative `href` has no page to
//! resolve against.

use perry_ffi::{closure_capture_f64, js_function_info, JsThis, JsValue, RawClosureHeader};
use url::Url;

/// A backend's `perry_system_open_url`.
pub type OpenUrl = extern "C" fn(i64);

/// Pass `href` to `open` when it is an http, https, mailto or tel URL.
pub fn open_link(href: &str, open: OpenUrl) {
    let Ok(url) = Url::parse(href) else { return };
    if matches!(url.scheme(), "http" | "https" | "mailto" | "tel") {
        open(perry_ffi::alloc_string(href).as_raw() as i64);
    }
}

/// A NaN-boxed closure that opens `href`. Its target is leaked: widgets and their
/// callbacks live for the whole process.
pub fn open_link_callback(href: String, open: OpenUrl) -> f64 {
    let target = Box::into_raw(Box::new((href, open)));
    let closure = perry_ffi::alloc_closure(js_function_info!(open_link_body, 0), 1);
    unsafe { perry_ffi::set_closure_capture_f64(closure, 0, target as usize as f64) };
    f64::from_bits(JsValue::from_object_ptr(closure).bits())
}

unsafe extern "C" fn open_link_body(closure: *const RawClosureHeader, _: JsThis) -> f64 {
    let target = unsafe { closure_capture_f64(closure, 0) } as usize as *const (String, OpenUrl);
    let (href, open) = unsafe { &*target };
    open_link(href, *open);
    f64::from_bits(JsValue::UNDEFINED.bits())
}
