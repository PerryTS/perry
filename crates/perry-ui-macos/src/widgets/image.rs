use objc2::msg_send;
use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadOnly};
use objc2_app_kit::{NSImage, NSImageView, NSView};
use objc2_foundation::{MainThreadMarker, NSString};

/// Extract a &str from a *const StringHeader pointer.
pub(crate) fn str_from_header(ptr: *const u8) -> &'static str {
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
            Ok(bytes) if !bytes.is_empty() => {
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

/// Create an NSImageView whose bytes are fetched from a URL (or `data:`
/// URI) asynchronously and applied on the main thread (#635). `alt`,
/// when non-empty, becomes the view's accessibility description.
/// Returns the widget handle immediately; the image appears once the
/// fetch resolves.
pub fn create_url(url_ptr: *const u8, alt_ptr: *const u8) -> i64 {
    let url = str_from_header(url_ptr).to_string();
    let alt = str_from_header(alt_ptr);
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    unsafe {
        let image_view: Retained<NSImageView> = msg_send![
            NSImageView::alloc(mtm), initWithFrame: objc2_core_foundation::CGRect::new(
                objc2_core_foundation::CGPoint::new(0.0, 0.0),
                objc2_core_foundation::CGSize::new(64.0, 64.0),
            )
        ];

        // ScaleAxesIndependently lets the contained NSImage stretch to
        // the view's bounds. Callers can override with `imageSetSize`.
        let _: () = msg_send![&*image_view, setImageScaling: 0i64];

        if !alt.is_empty() {
            let ns_alt = NSString::from_str(alt);
            let _: () = msg_send![&*image_view, setAccessibilityLabel: &*ns_alt];
        }

        let view: Retained<NSView> = Retained::cast_unchecked(image_view);
        let handle = super::register_widget(view);

        if !url.is_empty() {
            load_remote(handle, url);
        }

        handle
    }
}

/// Background-fetch + main-thread-apply for a remote URL. Uses Rust
/// std::thread for the off-main-thread hop and `dispatch_async_f` to
/// the main queue for the apply.
fn load_remote(handle: i64, url: String) {
    extern "C" {
        static _dispatch_main_q: std::ffi::c_void;
        fn dispatch_async_f(
            queue: *const std::ffi::c_void,
            context: *mut std::ffi::c_void,
            work: unsafe extern "C" fn(*mut std::ffi::c_void),
        );
    }

    std::thread::spawn(move || {
        let bytes_opt: Option<Vec<u8>> = unsafe {
            let url_cls = match objc2::runtime::AnyClass::get(c"NSURL") {
                Some(c) => c,
                None => return,
            };
            let ns: Retained<NSString> = NSString::from_str(&url);
            let nsurl: *mut objc2::runtime::AnyObject = msg_send![url_cls, URLWithString: &*ns];
            if nsurl.is_null() {
                return;
            }
            let data_cls = match objc2::runtime::AnyClass::get(c"NSData") {
                Some(c) => c,
                None => return,
            };
            let data: *mut objc2::runtime::AnyObject =
                msg_send![data_cls, dataWithContentsOfURL: nsurl];
            if data.is_null() {
                return;
            }
            let len: usize = msg_send![data, length];
            if len == 0 {
                return;
            }
            let raw: *const u8 = msg_send![data, bytes];
            if raw.is_null() {
                return;
            }
            Some(std::slice::from_raw_parts(raw, len).to_vec())
        };

        let bytes = match bytes_opt {
            Some(b) => b,
            None => return,
        };

        struct Apply {
            handle: i64,
            bytes: Vec<u8>,
        }
        let apply = Box::into_raw(Box::new(Apply { handle, bytes }));

        unsafe extern "C" fn finish(ctx: *mut std::ffi::c_void) {
            let _ = std::panic::catch_unwind(|| {
                let p = unsafe { Box::from_raw(ctx as *mut Apply) };
                unsafe {
                    let view = match super::get_widget(p.handle) {
                        Some(v) => v,
                        None => return,
                    };
                    let ns_data_cls = match objc2::runtime::AnyClass::get(c"NSData") {
                        Some(c) => c,
                        None => return,
                    };
                    let ns_data: *mut objc2::runtime::AnyObject = msg_send![
                        ns_data_cls,
                        dataWithBytes: p.bytes.as_ptr() as *const std::ffi::c_void,
                        length: p.bytes.len()
                    ];
                    if ns_data.is_null() {
                        return;
                    }
                    let image_cls = match objc2::runtime::AnyClass::get(c"NSImage") {
                        Some(c) => c,
                        None => return,
                    };
                    let img: *mut objc2::runtime::AnyObject = msg_send![image_cls, alloc];
                    let img: *mut objc2::runtime::AnyObject = msg_send![img, initWithData: ns_data];
                    if img.is_null() {
                        return;
                    }
                    let _: () = msg_send![&*view, setImage: img];
                }
            });
        }

        unsafe {
            dispatch_async_f(
                &_dispatch_main_q as *const _ as *const std::ffi::c_void,
                apply as *mut std::ffi::c_void,
                finish,
            );
        }
    });
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
