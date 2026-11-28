//! What a native `Link` opens: the web schemes only. The platform open calls take more
//! (Windows runs a path, Linux opens a file), and a relative `href` has no page to
//! resolve against.

use perry_ffi::{closure_capture_f64, js_function_info, JsThis, JsValue, RawClosureHeader};
use url::Url;

/// A backend's `perry_system_open_url`.
pub type OpenUrl = extern "C" fn(i64);

/// The URL that was checked, not the text it was parsed from: the parser drops tabs and
/// newlines and reads `\` as `/`, and a platform may read them another way.
fn web_url(href: &str) -> Option<String> {
    let url = Url::parse(href).ok()?;
    matches!(url.scheme(), "http" | "https" | "mailto" | "tel").then(|| url.into())
}

/// Pass `href` to `open` when it is an http, https, mailto or tel URL.
pub fn open_link(href: &str, open: OpenUrl) {
    if let Some(url) = web_url(href) {
        open(perry_ffi::alloc_string(&url).as_raw() as i64);
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

#[cfg(test)]
mod tests {
    use super::web_url;

    #[test]
    fn opens_the_parsed_url_of_a_web_scheme_only() {
        for (href, opened) in [
            ("https://a.test/x?y#z", Some("https://a.test/x?y#z")),
            ("mailto:a@b.test", Some("mailto:a@b.test")),
            ("tel:+34600000000", Some("tel:+34600000000")),
            ("  HtTpS://a.test/\n", Some("https://a.test/")),
            ("ht\ttps://a.te\nst/b\rc", Some("https://a.test/bc")),
            ("https:\\\\a.test\\b", Some("https://a.test/b")),
            ("https://a.test/b\0c", Some("https://a.test/b%00c")),
            ("mailto:a\0b", Some("mailto:a%00b")),
            ("java\tscript:alert(1)", None),
            ("\x01javascript:alert(1)", None),
            ("file:///etc/hosts", None),
            ("C:\\Windows\\notepad.exe", None),
            ("\\\\host\\share\\x.exe", None),
            ("/about", None),
            ("", None),
        ] {
            assert_eq!(web_url(href).as_deref(), opened, "{href:?}");
        }
    }
}
