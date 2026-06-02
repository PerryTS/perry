//! FFI exports: camera (AVCaptureSession)
//!
//! Extracted from `lib.rs` for file-size hygiene. No behavior changes.

use crate::*;

// =============================================================================
// Camera (perry/ui) — AVCaptureSession-based camera capture
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_camera_create() -> i64 {
    camera::create()
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_start(handle: i64) {
    camera::start(handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_stop(handle: i64) {
    camera::stop(handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_freeze(handle: i64) {
    camera::freeze(handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_unfreeze(handle: i64) {
    camera::unfreeze(handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_sample_color(x: f64, y: f64) -> f64 {
    camera::sample_color(x, y)
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_set_on_tap(handle: i64, callback: f64) {
    camera::set_on_tap(handle, callback)
}

/// Check if dark mode is active. Returns 1 if dark, 0 if light.
#[no_mangle]
pub extern "C" fn perry_system_is_dark_mode() -> i64 {
    unsafe {
        let tc_cls = objc2::runtime::AnyClass::get(c"UITraitCollection").unwrap();
        let tc: *mut objc2::runtime::AnyObject = objc2::msg_send![tc_cls, currentTraitCollection];
        if tc.is_null() {
            return 0;
        }
        let style: i64 = objc2::msg_send![tc, userInterfaceStyle];
        if style == 2 {
            1
        } else {
            0
        } // 2 = UIUserInterfaceStyleDark
    }
}

/// Set a preference value (UserDefaults).
#[no_mangle]
pub extern "C" fn perry_system_preferences_set(key_ptr: i64, value: f64) {
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
    extern "C" {
        fn js_nanbox_get_pointer(value: f64) -> i64;
    }
    let key = str_from_header(key_ptr as *const u8);
    let bits = value.to_bits();
    unsafe {
        let defaults_cls = objc2::runtime::AnyClass::get(c"NSUserDefaults").unwrap();
        let defaults: *mut objc2::runtime::AnyObject =
            objc2::msg_send![defaults_cls, standardUserDefaults];
        let ns_key = objc2_foundation::NSString::from_str(key);
        if (bits >> 48) == 0x7FFF {
            let str_ptr = js_nanbox_get_pointer(value) as *const u8;
            let s = str_from_header(str_ptr);
            let ns_str = objc2_foundation::NSString::from_str(s);
            let _: () = objc2::msg_send![defaults, setObject: &*ns_str, forKey: &*ns_key];
        } else {
            let ns_num: objc2::rc::Retained<objc2::runtime::AnyObject> = objc2::msg_send![
                objc2::runtime::AnyClass::get(c"NSNumber").unwrap(), numberWithDouble: value
            ];
            let _: () = objc2::msg_send![defaults, setObject: &*ns_num, forKey: &*ns_key];
        }
    }
}

/// Get a preference value (UserDefaults). Returns NaN-boxed value or TAG_UNDEFINED.
#[no_mangle]
pub extern "C" fn perry_system_preferences_get(key_ptr: i64) -> f64 {
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
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
        fn js_nanbox_string(ptr: i64) -> f64;
    }
    let key = str_from_header(key_ptr as *const u8);
    unsafe {
        let defaults_cls = objc2::runtime::AnyClass::get(c"NSUserDefaults").unwrap();
        let defaults: *mut objc2::runtime::AnyObject =
            objc2::msg_send![defaults_cls, standardUserDefaults];
        let ns_key = objc2_foundation::NSString::from_str(key);
        let obj: *mut objc2::runtime::AnyObject =
            objc2::msg_send![defaults, objectForKey: &*ns_key];
        if obj.is_null() {
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }
        if let Some(str_cls) = objc2::runtime::AnyClass::get(c"NSString") {
            let is_string: bool = objc2::msg_send![obj, isKindOfClass: str_cls];
            if is_string {
                let ns_str: &objc2_foundation::NSString =
                    &*(obj as *const objc2_foundation::NSString);
                let rust_str = ns_str.to_string();
                let bytes = rust_str.as_bytes();
                let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
                return js_nanbox_string(str_ptr as i64);
            }
        }
        if let Some(num_cls) = objc2::runtime::AnyClass::get(c"NSNumber") {
            let is_number: bool = objc2::msg_send![obj, isKindOfClass: num_cls];
            if is_number {
                let val: f64 = objc2::msg_send![obj, doubleValue];
                return val;
            }
        }
        // NSArray: return first element as string (for AppleLanguages etc.)
        if let Some(arr_cls) = objc2::runtime::AnyClass::get(c"NSArray") {
            let is_array: bool = objc2::msg_send![obj, isKindOfClass: arr_cls];
            if is_array {
                let count: usize = objc2::msg_send![obj, count];
                if count > 0 {
                    let first: *mut objc2::runtime::AnyObject =
                        objc2::msg_send![obj, objectAtIndex: 0usize];
                    if !first.is_null() {
                        if let Some(str_cls2) = objc2::runtime::AnyClass::get(c"NSString") {
                            let is_str: bool = objc2::msg_send![first, isKindOfClass: str_cls2];
                            if is_str {
                                let ns_str: &objc2_foundation::NSString =
                                    &*(first as *const objc2_foundation::NSString);
                                let rust_str = ns_str.to_string();
                                let bytes = rust_str.as_bytes();
                                let str_ptr =
                                    js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
                                return js_nanbox_string(str_ptr as i64);
                            }
                        }
                    }
                }
            }
        }
        f64::from_bits(0x7FFC_0000_0000_0001)
    }
}

/// Set border color on a widget via its CALayer.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_border_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view) = widgets::get_widget(handle) {
        unsafe {
            let layer: *mut objc2::runtime::AnyObject = objc2::msg_send![&*view, layer];
            if !layer.is_null() {
                let cg_color = widgets::create_cg_color(r, g, b, a);
                let _: () = objc2::msg_send![layer, setBorderColor: cg_color];
                extern "C" {
                    fn CGColorRelease(color: *mut std::ffi::c_void);
                }
                CGColorRelease(cg_color);
            }
        }
    }
}

/// Set drop shadow on any widget via its CALayer (issue #185 Phase B).
/// Signature mirrors macOS: `(r,g,b,a)` shadow color (alpha → shadowOpacity
/// so a non-1 alpha doesn't double-multiply via the CGColor's alpha),
/// `blur` → shadowRadius, `(offset_x, offset_y)` → shadowOffset CGSize.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_shadow(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
    blur: f64,
    offset_x: f64,
    offset_y: f64,
) {
    if let Some(view) = widgets::get_widget(handle) {
        unsafe {
            let layer: *mut objc2::runtime::AnyObject = objc2::msg_send![&*view, layer];
            if !layer.is_null() {
                let cg_color = widgets::create_cg_color(r, g, b, 1.0);
                let _: () = objc2::msg_send![layer, setShadowColor: cg_color];
                extern "C" {
                    fn CGColorRelease(color: *mut std::ffi::c_void);
                }
                CGColorRelease(cg_color);
                let _: () = objc2::msg_send![layer, setShadowOpacity: a as f32];
                let _: () = objc2::msg_send![layer, setShadowRadius: blur];
                let offset = objc2_core_foundation::CGSize::new(offset_x, offset_y);
                let _: () = objc2::msg_send![layer, setShadowOffset: offset];
                // CALayer shadows are clipped by masksToBounds; ensure
                // off so corner-radius widgets still show shadow outside
                // the rounded edge.
                let _: () = objc2::msg_send![layer, setMasksToBounds: false];
            }
        }
    }
}

/// Set border width on a widget via its CALayer.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_border_width(handle: i64, width: f64) {
    if let Some(view) = widgets::get_widget(handle) {
        unsafe {
            let layer: *mut objc2::runtime::AnyObject = objc2::msg_send![&*view, layer];
            if !layer.is_null() {
                let _: () = objc2::msg_send![layer, setBorderWidth: width];
            }
        }
    }
}

/// Set edge insets (padding) on a UIStackView. No-op for other widget types.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_edge_insets(
    handle: i64,
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
) {
    if let Some(view) = widgets::get_widget(handle) {
        unsafe {
            let is_stack = if let Some(cls) = objc2::runtime::AnyClass::get(c"UIStackView") {
                use objc2_foundation::NSObjectProtocol;
                view.isKindOfClass(cls)
            } else {
                false
            };
            if is_stack {
                let _: () = objc2::msg_send![&*view, setLayoutMarginsRelativeArrangement: true];
                let insets = objc2_ui_kit::UIEdgeInsets {
                    top,
                    left,
                    bottom,
                    right,
                };
                let _: () = objc2::msg_send![&*view, setDirectionalLayoutMargins: insets];
            }
        }
    }
}

/// Set view opacity (alpha) in [0.0, 1.0].
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_opacity(handle: i64, alpha: f64) {
    if let Some(view) = widgets::get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setAlpha: alpha];
        }
    }
}

/// Set the font family on a Text widget.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_family(handle: i64, family_ptr: i64) {
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
    let family = str_from_header(family_ptr as *const u8);
    if let Some(view) = widgets::get_widget(handle) {
        unsafe {
            let size: f64 = objc2::msg_send![&*view, font];
            let size = 13.0f64; // Default size for iOS
            let font: objc2::rc::Retained<objc2::runtime::AnyObject> =
                if family == "monospaced" || family == "monospace" {
                    objc2::msg_send![
                        objc2::runtime::AnyClass::get(c"UIFont").unwrap(),
                        monospacedSystemFontOfSize: size,
                        weight: 0.0f64
                    ]
                } else {
                    let ns_name = objc2_foundation::NSString::from_str(family);
                    let raw_font: *mut objc2::runtime::AnyObject = objc2::msg_send![
                        objc2::runtime::AnyClass::get(c"UIFont").unwrap(),
                        fontWithName: &*ns_name,
                        size: size
                    ];
                    if raw_font.is_null() {
                        // Font not found — fall back to system font
                        objc2::msg_send![
                            objc2::runtime::AnyClass::get(c"UIFont").unwrap(),
                            systemFontOfSize: size
                        ]
                    } else {
                        objc2::rc::Retained::retain(raw_font).unwrap()
                    }
                };
            let _: () = objc2::msg_send![&*view, setFont: &*font];
        }
    }
}
