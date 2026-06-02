use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{MainThreadMarker, NSString};
use objc2_ui_kit::UIView;

extern "C" {
    static _dispatch_main_q: std::ffi::c_void;
    fn dispatch_async_f(
        queue: *const std::ffi::c_void,
        context: *mut std::ffi::c_void,
        work: unsafe extern "C" fn(*mut std::ffi::c_void),
    );
    fn dispatch_get_global_queue(identifier: i64, flags: u64) -> *const std::ffi::c_void;
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

pub fn create_symbol(name_ptr: *const u8) -> i64 {
    let name = str_from_header(name_ptr);
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let ns_name = NSString::from_str(name);
        let image_cls = objc2::runtime::AnyClass::get(c"UIImage").unwrap();
        let image: *mut objc2::runtime::AnyObject =
            msg_send![image_cls, systemImageNamed: &*ns_name];
        let iv_cls = objc2::runtime::AnyClass::get(c"UIImageView").unwrap();
        let obj: *mut AnyObject = msg_send![iv_cls, alloc];
        let obj: *mut AnyObject = msg_send![obj, initWithImage: image];
        let image_view: Retained<UIView> = Retained::retain(obj as *mut UIView).unwrap();
        super::register_widget(image_view)
    }
}

pub fn create_file(path_ptr: *const u8) -> i64 {
    let path = str_from_header(path_ptr);
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        // Resolve relative paths against the app bundle's resource directory
        let resolved = if !path.starts_with('/') {
            let bundle_cls = objc2::runtime::AnyClass::get(c"NSBundle").unwrap();
            let main_bundle: *mut AnyObject = msg_send![bundle_cls, mainBundle];
            let res_path: *mut AnyObject = msg_send![main_bundle, resourcePath];
            if !res_path.is_null() {
                let res_str: *const AnyObject = msg_send![res_path, UTF8String];
                let c_str = std::ffi::CStr::from_ptr(res_str as *const i8);
                format!("{}/{}", c_str.to_str().unwrap_or(""), path)
            } else {
                path.to_string()
            }
        } else {
            path.to_string()
        };
        let ns_path = NSString::from_str(&resolved);
        let image_cls = objc2::runtime::AnyClass::get(c"UIImage").unwrap();
        let image: *mut AnyObject = msg_send![image_cls, imageWithContentsOfFile: &*ns_path];
        if image.is_null() {
            eprintln!(
                "[perry-ui-ios] ImageFile: failed to load image at path: {}",
                resolved
            );
        }
        let iv_cls = objc2::runtime::AnyClass::get(c"UIImageView").unwrap();
        let obj: *mut AnyObject = msg_send![iv_cls, alloc];
        let obj: *mut AnyObject = msg_send![obj, initWithImage: image];
        if obj.is_null() {
            // Image not found — create an empty UIImageView instead of crashing
            eprintln!(
                "[perry-ui-ios] ImageFile: initWithImage returned nil for path: {}",
                resolved
            );
            let obj: *mut AnyObject = msg_send![iv_cls, new];
            let image_view: Retained<UIView> = Retained::retain(obj as *mut UIView).unwrap();
            return super::register_widget(image_view);
        }
        // Set content mode to ScaleAspectFit so the image scales properly with constraints
        let _: () = msg_send![obj, setContentMode: 1i64]; // UIViewContentModeScaleAspectFit
        let image_view: Retained<UIView> = Retained::retain(obj as *mut UIView).unwrap();
        super::register_widget(image_view)
    }
}

/// Create a UIImageView whose image is loaded from a URL (or data: URI) on
/// a background queue and applied on the main thread (#635). `alt`, when
/// non-empty, becomes the view's `accessibilityLabel`. Returns the widget
/// handle immediately; the image appears once the fetch resolves.
///
/// Mirrors the URL-loading shape of `image_gallery::load_remote` (which
/// is the existing precedent for async image fetch in this crate). Local
/// file paths are not handled here — use `ImageFile(path)` for those.
pub fn create_url(url_ptr: *const u8, alt_ptr: *const u8) -> i64 {
    let url = str_from_header(url_ptr).to_string();
    let alt = str_from_header(alt_ptr);
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let iv_cls = objc2::runtime::AnyClass::get(c"UIImageView").unwrap();
        let obj: *mut AnyObject = msg_send![iv_cls, alloc];
        let obj: *mut AnyObject = msg_send![obj, init];
        // UIViewContentModeScaleAspectFill = 2 — defaults to a "cover"
        // shape, which is what avatars/thumbnails want by default. Caller
        // can override via `imageSetSize(...)` + custom `setContentMode`.
        let _: () = msg_send![obj, setContentMode: 2i64];
        let _: () = msg_send![obj, setClipsToBounds: true];
        if !alt.is_empty() {
            let ns_alt = NSString::from_str(alt);
            let _: () = msg_send![obj, setAccessibilityLabel: &*ns_alt];
        }
        let image_view: Retained<UIView> = Retained::retain(obj as *mut UIView).unwrap();
        let handle = super::register_widget(image_view);
        if !url.is_empty() {
            load_remote(&url, obj);
        }
        handle
    }
}

/// Background-fetch + main-thread-apply for a remote URL (or data: URI).
/// Retain/release pair on `image_view` keeps the view alive across the
/// async hop. Failure paths silently leave the image unset (no crash).
fn load_remote(url: &str, image_view: *mut AnyObject) {
    let url_str = url.to_string();
    struct Pkg {
        url: String,
        view: *mut AnyObject,
    }
    unsafe {
        let _: *mut AnyObject = msg_send![image_view, retain];
    }
    let pkg = Box::into_raw(Box::new(Pkg {
        url: url_str,
        view: image_view,
    }));

    unsafe extern "C" fn worker(ctx: *mut std::ffi::c_void) {
        let _ = std::panic::catch_unwind(|| {
            let pkg = Box::from_raw(ctx as *mut Pkg);
            unsafe {
                let url_cls = objc2::runtime::AnyClass::get(c"NSURL").unwrap();
                let ns = NSString::from_str(&pkg.url);
                let nsurl: *mut AnyObject = msg_send![url_cls, URLWithString: &*ns];
                if nsurl.is_null() {
                    let _: () = msg_send![pkg.view, release];
                    return;
                }
                let data_cls = objc2::runtime::AnyClass::get(c"NSData").unwrap();
                let data: *mut AnyObject = msg_send![data_cls, dataWithContentsOfURL: nsurl];
                if data.is_null() {
                    let _: () = msg_send![pkg.view, release];
                    return;
                }
                let _: *mut AnyObject = msg_send![data, retain];
                struct Apply {
                    data: *mut AnyObject,
                    view: *mut AnyObject,
                }
                let apply = Box::into_raw(Box::new(Apply {
                    data,
                    view: pkg.view,
                }));
                unsafe extern "C" fn finish(ctx: *mut std::ffi::c_void) {
                    let _ = std::panic::catch_unwind(|| {
                        let p = Box::from_raw(ctx as *mut Apply);
                        unsafe {
                            let img_cls = objc2::runtime::AnyClass::get(c"UIImage").unwrap();
                            let img: *mut AnyObject = msg_send![img_cls, alloc];
                            let img: *mut AnyObject = msg_send![img, initWithData: p.data];
                            if !img.is_null() {
                                let _: () = msg_send![p.view, setImage: img];
                            }
                            let _: () = msg_send![p.data, release];
                            let _: () = msg_send![p.view, release];
                        }
                    });
                }
                dispatch_async_f(
                    &_dispatch_main_q as *const _ as *const std::ffi::c_void,
                    apply as *mut std::ffi::c_void,
                    finish,
                );
            }
        });
    }

    unsafe {
        let q = dispatch_get_global_queue(0, 0);
        dispatch_async_f(q, pkg as *mut std::ffi::c_void, worker);
    }
}

pub fn set_size(handle: i64, width: f64, height: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let frame = objc2_core_foundation::CGRect::new(
                objc2_core_foundation::CGPoint::new(0.0, 0.0),
                objc2_core_foundation::CGSize::new(width, height),
            );
            let _: () = msg_send![&*view, setFrame: frame];
        }
    }
}

pub fn set_tint(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let color_cls = objc2::runtime::AnyClass::get(c"UIColor").unwrap();
            let color: *mut objc2::runtime::AnyObject = msg_send![
                color_cls, colorWithRed: r as f64, green: g as f64, blue: b as f64, alpha: a as f64
            ];
            let _: () = msg_send![&*view, setTintColor: color];
        }
    }
}
