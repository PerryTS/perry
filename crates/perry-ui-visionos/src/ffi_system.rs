//! System APIs: share, app groups, open URL, location, audio, camera,
//! dark-mode, preferences, border/shadow/opacity, text font family,
//! QR code, folder/save dialogs, poll-open-file, overlay stubs,
//! state textfield binding, alerts, sheet stubs, screen detection,
//! app lifecycle, and toolbar stubs. Behavior is unchanged from the
//! pre-split `lib.rs`.

use super::*;

// =============================================================================
// System APIs (perry/system module)
// =============================================================================

/// #917 — system share sheet stub on visionOS. UIActivityViewController
/// is available but its presentation model on visionOS is different
/// (window-anchored rather than view-anchored). Stub + first-call
/// warning; the visionOS native impl is tracked under #917 follow-up.
#[no_mangle]
pub extern "C" fn perry_system_share_text(_text_ptr: i64, _title_ptr: i64) {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_share_text",
        "visionOS share sheet not yet implemented (#917 follow-up)",
        Some("#917"),
    );
}
#[no_mangle]
pub extern "C" fn perry_system_share_url(_url_ptr: i64, _title_ptr: i64) {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_share_url",
        "visionOS share sheet not yet implemented (#917 follow-up)",
        Some("#917"),
    );
}

// #675 — App Group stubs on visionOS. Same UserDefaults(suiteName:)
// path as iOS will work; tracked as #675 follow-up.
#[no_mangle]
pub extern "C" fn perry_system_app_group_set(_key_ptr: i64, _value_ptr: i64) {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_app_group_set",
        "visionOS App Group not implemented (#675 follow-up)",
        Some("#675"),
    );
}
#[no_mangle]
pub extern "C" fn perry_system_app_group_get(_key_ptr: i64) -> i64 {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_app_group_get",
        "visionOS App Group not implemented (#675 follow-up)",
        Some("#675"),
    );
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    }
    unsafe { js_string_from_bytes(std::ptr::null(), 0) }
}
#[no_mangle]
pub extern "C" fn perry_system_app_group_delete(_key_ptr: i64) {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_app_group_delete",
        "visionOS App Group not implemented (#675 follow-up)",
        Some("#675"),
    );
}

/// Open a URL in the default browser/app.
#[no_mangle]
pub extern "C" fn perry_system_open_url(url_ptr: i64) {
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
    let url_str = str_from_header(url_ptr as *const u8);
    unsafe {
        let ns_url_str = objc2_foundation::NSString::from_str(url_str);
        let url_cls = objc2::runtime::AnyClass::get(c"NSURL").unwrap();
        let url: *mut objc2::runtime::AnyObject =
            objc2::msg_send![url_cls, URLWithString: &*ns_url_str];
        if !url.is_null() {
            let app_cls = objc2::runtime::AnyClass::get(c"UIApplication").unwrap();
            let app: *mut objc2::runtime::AnyObject = objc2::msg_send![app_cls, sharedApplication];
            let _: () = objc2::msg_send![app, openURL: url];
        }
    }
}

/// Request one-shot location. Callback receives (lat, lon) or (NaN, NaN) on error.
#[no_mangle]
pub extern "C" fn perry_system_request_location(callback: f64) {
    location::request_location(callback);
}

// =============================================================================
// Audio (perry/system) — AVAudioEngine-based microphone capture
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_system_audio_start() -> i64 {
    audio::start()
}

#[no_mangle]
pub extern "C" fn perry_system_audio_stop() {
    audio::stop()
}

#[no_mangle]
pub extern "C" fn perry_system_audio_get_level() -> f64 {
    audio::get_level()
}

#[no_mangle]
pub extern "C" fn perry_system_audio_get_peak() -> f64 {
    audio::get_peak()
}

#[no_mangle]
pub extern "C" fn perry_system_audio_get_waveform(count: f64) -> f64 {
    audio::get_waveform(count)
}

#[no_mangle]
pub extern "C" fn perry_system_get_device_model() -> i64 {
    audio::get_device_model()
}
/// Bug-report-flow utility: stable OS-version string. visionOS stub.
#[no_mangle]
pub extern "C" fn perry_system_get_os_version() -> i64 {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_get_os_version",
        "visionOS getOSVersion not yet implemented",
        None,
    );
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    }
    unsafe { js_string_from_bytes(std::ptr::null(), 0) }
}
#[no_mangle]
pub extern "C" fn perry_system_audio_set_output_filename(filename_ptr: i64) {
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
    let filename = str_from_header(filename_ptr as *const u8);
    audio::set_output_filename(filename);
}
#[no_mangle]
pub extern "C" fn perry_system_audio_start_recording() {
    audio::start_recording();
}
#[no_mangle]
pub extern "C" fn perry_system_audio_stop_recording() {
    audio::stop_recording();
}

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

/// perry/system hapticPlay — documented no-op: Vision Pro exposes no
/// app-facing haptic engine (UIFeedbackGenerator is unavailable on
/// visionOS; the API contract is "no-op on platforms without one", so
/// no stub warning).
#[no_mangle]
pub extern "C" fn perry_system_haptic_play(_type_ptr: i64) {}

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
/// Mirrors the iOS / tvOS / macOS twin: `(r,g,b,a)` shadow color (alpha →
/// shadowOpacity), `blur` → shadowRadius, `(offset_x, offset_y)` →
/// shadowOffset CGSize.
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

// =============================================================================
// QR Code
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_qrcode_create(data_ptr: i64, size: f64) -> i64 {
    widgets::qrcode::create(data_ptr as *const u8, size)
}

#[no_mangle]
pub extern "C" fn perry_ui_qrcode_set_data(handle: i64, data_ptr: i64) {
    widgets::qrcode::set_data(handle, data_ptr as *const u8);
}

// =============================================================================
// Folder Dialog
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_open_folder_dialog(callback: f64) {
    // iOS: UIDocumentPickerViewController for directories — stub for now
    file_dialog::open_dialog(callback);
}

// =============================================================================
// Save File Dialog
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_save_file_dialog(
    _callback: f64,
    _default_name: i64,
    _allowed_types: i64,
) {
    // iOS: UIDocumentPickerViewController needed — stub for now
}

// =============================================================================
// Poll Open File (stub — iOS uses URL schemes / UIDocumentBrowser instead)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_poll_open_file() -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    }
    unsafe { js_string_from_bytes(std::ptr::null(), 0) }
}

// =============================================================================
// Overlay (stub — iOS uses different approach)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_widget_add_overlay(_parent_handle: i64, _child_handle: i64) {
    // Stub — iOS would use addSubview directly
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_overlay_frame(
    _handle: i64,
    _x: f64,
    _y: f64,
    _w: f64,
    _h: f64,
) {
    // Stub
}

// =============================================================================
// State TextField Binding
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_state_bind_textfield(state_handle: i64, textfield_handle: i64) {
    state::bind_textfield(state_handle, textfield_handle);
}

// =============================================================================
// Alert Dialog
// =============================================================================

/// Issue #708 — visionOS UIAlertController via the shared iOS-shape impl.
/// `buttons` is F64 (NaN-boxed pointer) per the canonical ABI in
/// perry-dispatch; the pre-existing `i64` signature here was ABI-broken.
#[no_mangle]
pub extern "C" fn perry_ui_alert(title_ptr: i64, message_ptr: i64, buttons: f64, callback: f64) {
    extern "C" {
        fn js_nanbox_get_pointer(value: f64) -> i64;
    }
    let buttons_ptr = unsafe { js_nanbox_get_pointer(buttons) };
    widgets::alert::show(
        title_ptr as *const u8,
        message_ptr as *const u8,
        buttons_ptr,
        callback,
    );
}

/// Issue #708 — visionOS simple OK-only alert.
#[no_mangle]
pub extern "C" fn perry_ui_alert_simple(title_ptr: i64, message_ptr: i64) {
    widgets::alert::show_simple(title_ptr as *const u8, message_ptr as *const u8);
}

// =============================================================================
// Sheet
// =============================================================================

// #1033: signature aligned with the perry-dispatch row
// `[Widget, F64, F64]` and the TS surface `sheetCreate(body, w, h)`.
#[no_mangle]
pub extern "C" fn perry_ui_sheet_create(_body: i64, _width: f64, _height: f64) -> i64 {
    0 // stub
}

#[no_mangle]
pub extern "C" fn perry_ui_sheet_present(_sheet: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_sheet_dismiss(_sheet: i64) {}

// =============================================================================
// Screen Detection (iPad vs iPhone, orientation)
// =============================================================================

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

fn nanbox_static_str(s: &'static [u8]) -> f64 {
    let ptr = unsafe { js_string_from_bytes(s.as_ptr(), s.len() as i64) };
    unsafe { js_nanbox_string(ptr as i64) }
}

/// perry_get_screen_width() → logical width in points (e.g. 820 for iPad Air portrait)
#[no_mangle]
pub extern "C" fn perry_get_screen_width() -> f64 {
    unsafe {
        let screen_cls = objc2::runtime::AnyClass::get(c"UIScreen").unwrap();
        let main_screen: *mut objc2::runtime::AnyObject = objc2::msg_send![screen_cls, mainScreen];
        // UIScreen.bounds is orientation-aware since iOS 8
        let bounds: objc2_core_foundation::CGRect = objc2::msg_send![main_screen, bounds];
        bounds.size.width
    }
}

/// perry_get_screen_height() → logical height in points
#[no_mangle]
pub extern "C" fn perry_get_screen_height() -> f64 {
    unsafe {
        let screen_cls = objc2::runtime::AnyClass::get(c"UIScreen").unwrap();
        let main_screen: *mut objc2::runtime::AnyObject = objc2::msg_send![screen_cls, mainScreen];
        let bounds: objc2_core_foundation::CGRect = objc2::msg_send![main_screen, bounds];
        bounds.size.height
    }
}

/// perry_get_scale_factor() → device pixel ratio (e.g. 2.0 for iPad, 3.0 for iPhone Pro)
#[no_mangle]
pub extern "C" fn perry_get_scale_factor() -> f64 {
    unsafe {
        let screen_cls = objc2::runtime::AnyClass::get(c"UIScreen").unwrap();
        let main_screen: *mut objc2::runtime::AnyObject = objc2::msg_send![screen_cls, mainScreen];
        let scale: f64 = objc2::msg_send![main_screen, scale];
        scale
    }
}

/// perry_get_orientation() → "landscape" or "portrait"
#[no_mangle]
pub extern "C" fn perry_get_orientation() -> f64 {
    unsafe {
        let screen_cls = objc2::runtime::AnyClass::get(c"UIScreen").unwrap();
        let main_screen: *mut objc2::runtime::AnyObject = objc2::msg_send![screen_cls, mainScreen];
        let bounds: objc2_core_foundation::CGRect = objc2::msg_send![main_screen, bounds];
        if bounds.size.width > bounds.size.height {
            nanbox_static_str(b"landscape")
        } else {
            nanbox_static_str(b"portrait")
        }
    }
}

/// perry_get_device_idiom() → 0 = phone, 1 = pad
/// Uses UIDevice.model string comparison (more reliable than userInterfaceIdiom
/// which can return 0 before full UIApplication init on iOS 26 simulator).
#[no_mangle]
pub extern "C" fn perry_get_device_idiom() -> f64 {
    unsafe {
        let device_cls = objc2::runtime::AnyClass::get(c"UIDevice").unwrap();
        let current: *mut objc2::runtime::AnyObject = objc2::msg_send![device_cls, currentDevice];

        // Check UIDevice.model — returns @"iPad" on iPad, @"iPhone" on iPhone
        let model: *mut objc2::runtime::AnyObject = objc2::msg_send![current, model];
        let utf8: *const u8 = objc2::msg_send![model, UTF8String];
        if !utf8.is_null() {
            // "iPad" starts with 'i' (0x69) then 'P' (0x50)
            // "iPhone" starts with 'i' (0x69) then 'P' (0x50) too...
            // Actually: "iPad" has 4 chars, "iPhone" has 6 chars
            // Check 3rd char: 'a' (0x61) for iPad vs 'h' (0x68) for iPhone
            let third = *utf8.add(2);
            if third == b'a' {
                // "iPad"
                return 1.0;
            }
        }
        0.0
    }
}

// =============================================================================
// App Lifecycle
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_app_on_terminate(_callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_on_activate(_callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_icon(_path_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_size(_app: i64, _w: f64, _h: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_frameless(_app_handle: i64, _value: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_level(_app_handle: i64, _value_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_transparent(_app_handle: i64, _value: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_vibrancy(_app_handle: i64, _value_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_activation_policy(_app_handle: i64, _value_ptr: i64) {}

/// Issue #1280 — windowState is a desktop concept; visionOS volumes/windows
/// have a different model. Stub keeps the linker happy.
#[no_mangle]
pub extern "C" fn perry_ui_app_set_window_state(_app_handle: i64, _value_ptr: i64) {}

// =============================================================================
// Toolbar
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_toolbar_create() -> i64 {
    0 // stub
}

#[no_mangle]
pub extern "C" fn perry_ui_toolbar_add_item(
    _toolbar: i64,
    _label: i64,
    _icon: i64,
    _callback: f64,
) {
}

#[no_mangle]
pub extern "C" fn perry_ui_toolbar_attach(_toolbar: i64) {}
