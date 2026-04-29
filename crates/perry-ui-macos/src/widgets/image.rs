use objc2::msg_send;
use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadOnly};
use objc2_app_kit::{NSImage, NSImageView, NSView};
use objc2_foundation::{MainThreadMarker, NSString};

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

/// Create an NSImageView displaying an SF Symbol by name. Returns widget handle.
pub fn create_symbol(name_ptr: *const u8) -> i64 {
    let name = str_from_header(name_ptr);
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    unsafe {
        let ns_name = NSString::from_str(name);
        let image: Option<Retained<NSImage>> = msg_send![
            objc2::runtime::AnyClass::get(c"NSImage").unwrap(),
            imageWithSystemSymbolName: &*ns_name,
            accessibilityDescription: std::ptr::null::<NSString>()
        ];

        let image_view: Retained<NSImageView> = msg_send![
            NSImageView::alloc(mtm), initWithFrame: objc2_core_foundation::CGRect::new(
                objc2_core_foundation::CGPoint::new(0.0, 0.0),
                objc2_core_foundation::CGSize::new(24.0, 24.0),
            )
        ];

        if let Some(img) = image {
            let _: () = msg_send![&*image_view, setImage: &*img];
        }

        let view: Retained<NSView> = Retained::cast_unchecked(image_view);
        super::register_widget(view)
    }
}

/// Create an NSImageView displaying an image loaded from a file path. Returns widget handle.
pub fn create_file(path_ptr: *const u8) -> i64 {
    let path = str_from_header(path_ptr);
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    // Resolve relative paths against the .app bundle.
    // Try multiple locations: bundle resource dir (Contents/Resources/),
    // executable dir (Contents/MacOS/ or flat bundle), and cwd.
    let resolved = if !path.starts_with('/') {
        let mut found = None;
        // 1. Try NSBundle.mainBundle.resourcePath (Contents/Resources/)
        if found.is_none() {
            let bundle_class = objc2::runtime::AnyClass::get(c"NSBundle").unwrap();
            let bundle: *mut objc2::runtime::AnyObject =
                unsafe { msg_send![bundle_class, mainBundle] };
            if !bundle.is_null() {
                let res_path: Option<Retained<NSString>> =
                    unsafe { msg_send![bundle, resourcePath] };
                if let Some(rp) = res_path {
                    let rp_str = rp.to_string();
                    let candidate = std::path::PathBuf::from(&rp_str).join(path);
                    if candidate.exists() {
                        found = Some(candidate.to_string_lossy().to_string());
                    }
                }
            }
        }
        // 2. Try executable's directory (flat .app bundles or dev mode)
        if found.is_none() {
            if let Ok(exe) = std::env::current_exe() {
                if let Some(exe_dir) = exe.parent() {
                    let candidate = exe_dir.join(path);
                    if candidate.exists() {
                        found = Some(candidate.to_string_lossy().to_string());
                    }
                }
            }
        }
        found.unwrap_or_else(|| path.to_string())
    } else {
        path.to_string()
    };

    unsafe {
        // Read file bytes with Rust, then create NSImage from NSData
        let image_obj: *mut objc2::runtime::AnyObject = match std::fs::read(&resolved) {
            Ok(bytes) if bytes.len() > 0 => {
                let ns_data_cls = objc2::runtime::AnyClass::get(c"NSData").unwrap();
                let ns_data: *mut objc2::runtime::AnyObject = msg_send![
                    ns_data_cls, dataWithBytes: bytes.as_ptr() as *const std::ffi::c_void, length: bytes.len()
                ];
                if ns_data.is_null() {
                    std::ptr::null_mut()
                } else {
                    let image_cls = objc2::runtime::AnyClass::get(c"NSImage").unwrap();
                    let img: *mut objc2::runtime::AnyObject = msg_send![image_cls, alloc];
                    msg_send![img, initWithData: ns_data]
                }
            }
            _ => std::ptr::null_mut(),
        };

        let image_view: Retained<NSImageView> = msg_send![
            NSImageView::alloc(mtm), initWithFrame: objc2_core_foundation::CGRect::new(
                objc2_core_foundation::CGPoint::new(0.0, 0.0),
                objc2_core_foundation::CGSize::new(64.0, 64.0),
            )
        ];

        if !image_obj.is_null() {
            let _: () = msg_send![&*image_view, setImage: image_obj];
        }

        let view: Retained<NSView> = Retained::cast_unchecked(image_view);
        super::register_widget(view)
    }
}

/// Set the frame size of an image widget.
pub fn set_size(handle: i64, width: f64, height: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            // Resize the NSImage itself so intrinsic content size matches the desired display size
            let image: *mut objc2::runtime::AnyObject = msg_send![&*view, image];
            if !image.is_null() {
                let img_size = objc2_core_foundation::CGSize::new(width, height);
                let _: () = msg_send![image, setSize: img_size];
            }
            let size = objc2_core_foundation::CGSize::new(width, height);
            let _: () = msg_send![&*view, setFrameSize: size];
        }
    }
}

/// Set the content tint color of an image widget (for SF Symbols).
pub fn set_tint(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let color: *mut objc2::runtime::AnyObject = msg_send![
                objc2::runtime::AnyClass::get(c"NSColor").unwrap(),
                colorWithRed: r, green: g, blue: b, alpha: a
            ];
            let _: () = msg_send![&*view, setContentTintColor: color];
        }
    }
}
