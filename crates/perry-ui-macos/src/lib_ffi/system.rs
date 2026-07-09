use crate::*;

// =============================================================================
// System APIs (perry/system module)
// =============================================================================

/// #917 — system share sheet (text). Wraps `NSSharingServicePicker`
/// anchored to the key window's content view. `title` is currently
/// dropped on macOS (Cocoa's picker derives its label from the
/// item type); kept in the signature for cross-platform symmetry
/// with iOS/Android. Both args are Perry string pointers.
#[no_mangle]
pub extern "C" fn perry_system_share_text(text_ptr: i64, _title_ptr: i64) {
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
    let text = str_from_header(text_ptr as *const u8);
    if text.is_empty() {
        return;
    }
    unsafe {
        let ns_text = objc2_foundation::NSString::from_str(text);
        let arr_cls = objc2::runtime::AnyClass::get(c"NSArray").unwrap();
        let items: *mut objc2::runtime::AnyObject =
            objc2::msg_send![arr_cls, arrayWithObject: &*ns_text];
        present_sharing_picker(items);
    }
}

/// #917 — system share sheet (URL). Same shape as `shareText` but
/// wraps the value as `NSURL` so the picker offers
/// Safari / Reading List / Add to Bookmarks alongside Messages / Mail.
#[no_mangle]
pub extern "C" fn perry_system_share_url(url_ptr: i64, _title_ptr: i64) {
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
    let url_str = str_from_header(url_ptr as *const u8);
    if url_str.is_empty() {
        return;
    }
    unsafe {
        let ns_str = objc2_foundation::NSString::from_str(url_str);
        let url_cls = objc2::runtime::AnyClass::get(c"NSURL").unwrap();
        let url: *mut objc2::runtime::AnyObject =
            objc2::msg_send![url_cls, URLWithString: &*ns_str];
        let arr_cls = objc2::runtime::AnyClass::get(c"NSArray").unwrap();
        let items: *mut objc2::runtime::AnyObject = if url.is_null() {
            // Malformed URL → fall back to sharing as plain text.
            objc2::msg_send![arr_cls, arrayWithObject: &*ns_str]
        } else {
            objc2::msg_send![arr_cls, arrayWithObject: url]
        };
        present_sharing_picker(items);
    }
}

/// Build an `NSSharingServicePicker` for `items` and present it
/// anchored to the key window's content-view bounds. Common helper
/// so `shareText` and `shareUrl` share the presentation logic.
unsafe fn present_sharing_picker(items: *mut objc2::runtime::AnyObject) {
    let picker_cls = objc2::runtime::AnyClass::get(c"NSSharingServicePicker").unwrap();
    let alloc: *mut objc2::runtime::AnyObject = objc2::msg_send![picker_cls, alloc];
    let picker: *mut objc2::runtime::AnyObject = objc2::msg_send![alloc, initWithItems: items];
    if picker.is_null() {
        return;
    }
    let app_cls = objc2::runtime::AnyClass::get(c"NSApplication").unwrap();
    let app: *mut objc2::runtime::AnyObject = objc2::msg_send![app_cls, sharedApplication];
    let key_window: *mut objc2::runtime::AnyObject = objc2::msg_send![app, keyWindow];
    if key_window.is_null() {
        return;
    }
    let content_view: *mut objc2::runtime::AnyObject = objc2::msg_send![key_window, contentView];
    if content_view.is_null() {
        return;
    }
    let bounds: objc2_foundation::NSRect = objc2::msg_send![content_view, bounds];
    // showRelativeToRect:ofView:preferredEdge: — anchor to the
    // content view's bounds, preferred edge = NSRectEdgeMinY (1)
    // so the popover renders above the view.
    const NS_RECT_EDGE_MIN_Y: u64 = 1;
    let _: () = objc2::msg_send![
        picker,
        showRelativeToRect: bounds,
        ofView: content_view,
        preferredEdge: NS_RECT_EDGE_MIN_Y
    ];
}

// #675 + #1178 — App Group / cross-process shared storage on macOS.
//
// Backed by `NSUserDefaults(suiteName:)`. Resolution order:
//   1. `[ios] app_group` / `[macos] app_group` baked in from
//      `perry.toml` (the #1178 path — codegen calls
//      `perry_app_group_init` in `main`'s prelude).
//   2. `PERRY_APP_GROUP` environment override (predates #1178; kept
//      for dev/CI flows that flip the suite per-run without
//      regenerating the binary).
//   3. `"group.perryapp"` fallback so the dev flow still works
//      without any config — no entitlement → the suite simply lives
//      under `~/Library/Preferences/group.perryapp.plist` instead of
//      inside the shared App Group container.

fn app_group_suite() -> objc2::rc::Retained<objc2_foundation::NSString> {
    // perry-ui-macos intentionally avoids a Cargo dep on perry-runtime
    // (see `string_header.rs`'s comment). Reach for the suite name via
    // the C-ABI shim instead.
    extern "C" {
        fn perry_app_group_suite_name(out_len: *mut i32) -> *const u8;
    }
    let baked = unsafe {
        let mut len: i32 = 0;
        let ptr = perry_app_group_suite_name(&mut len as *mut i32);
        if ptr.is_null() || len <= 0 {
            None
        } else {
            let slice = std::slice::from_raw_parts(ptr, len as usize);
            std::str::from_utf8(slice).ok().map(str::to_string)
        }
    };
    let suite = baked
        .or_else(|| std::env::var("PERRY_APP_GROUP").ok())
        .unwrap_or_else(|| "group.perryapp".to_string());
    objc2_foundation::NSString::from_str(&suite)
}

unsafe fn app_group_defaults() -> *mut objc2::runtime::AnyObject {
    let cls = objc2::runtime::AnyClass::get(c"NSUserDefaults").unwrap();
    let alloc: *mut objc2::runtime::AnyObject = objc2::msg_send![cls, alloc];
    let suite = app_group_suite();
    let defaults: *mut objc2::runtime::AnyObject =
        objc2::msg_send![alloc, initWithSuiteName: &*suite];
    defaults
}

fn appgroup_str_from_header(ptr: *const u8) -> &'static str {
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

/// #675 — set a key in the App Group's `NSUserDefaults` suite.
#[no_mangle]
pub extern "C" fn perry_system_app_group_set(key_ptr: i64, value_ptr: i64) {
    let key = appgroup_str_from_header(key_ptr as *const u8);
    let value = appgroup_str_from_header(value_ptr as *const u8);
    if key.is_empty() {
        return;
    }
    unsafe {
        let defaults = app_group_defaults();
        if defaults.is_null() {
            return;
        }
        let ns_key = objc2_foundation::NSString::from_str(key);
        let ns_value = objc2_foundation::NSString::from_str(value);
        let _: () = objc2::msg_send![defaults, setObject: &*ns_value, forKey: &*ns_key];
        let _: () = objc2::msg_send![defaults, synchronize];
    }
}

/// #675 — read a key from the App Group suite. Returns the empty
/// string when the key is absent (matches the contract documented in
/// the issue: "" if absent).
#[no_mangle]
pub extern "C" fn perry_system_app_group_get(key_ptr: i64) -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    }
    let empty = || unsafe { js_string_from_bytes(std::ptr::null(), 0) };
    let key = appgroup_str_from_header(key_ptr as *const u8);
    if key.is_empty() {
        return empty();
    }
    unsafe {
        let defaults = app_group_defaults();
        if defaults.is_null() {
            return empty();
        }
        let ns_key = objc2_foundation::NSString::from_str(key);
        let value: *mut objc2::runtime::AnyObject =
            objc2::msg_send![defaults, stringForKey: &*ns_key];
        if value.is_null() {
            return empty();
        }
        // Convert NSString -> UTF-8 bytes.
        let utf8_ptr: *const u8 = objc2::msg_send![value, UTF8String];
        if utf8_ptr.is_null() {
            return empty();
        }
        let utf8_len: usize = objc2::msg_send![value, lengthOfBytesUsingEncoding: 4u64]; // NSUTF8StringEncoding = 4
        if utf8_len == 0 {
            return empty();
        }
        js_string_from_bytes(utf8_ptr, utf8_len as i32)
    }
}

/// #675 — remove a key from the App Group suite.
#[no_mangle]
pub extern "C" fn perry_system_app_group_delete(key_ptr: i64) {
    let key = appgroup_str_from_header(key_ptr as *const u8);
    if key.is_empty() {
        return;
    }
    unsafe {
        let defaults = app_group_defaults();
        if defaults.is_null() {
            return;
        }
        let ns_key = objc2_foundation::NSString::from_str(key);
        let _: () = objc2::msg_send![defaults, removeObjectForKey: &*ns_key];
        let _: () = objc2::msg_send![defaults, synchronize];
    }
}

/// Open a URL in the default browser/app.
#[no_mangle]
pub extern "C" fn perry_system_open_url(url_ptr: i64) {
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
    let url_str = str_from_header(url_ptr as *const u8);
    unsafe {
        let ns_url_str = objc2_foundation::NSString::from_str(url_str);
        let url_cls = objc2::runtime::AnyClass::get(c"NSURL").unwrap();
        let url: *mut objc2::runtime::AnyObject =
            objc2::msg_send![url_cls, URLWithString: &*ns_url_str];
        if !url.is_null() {
            let workspace_cls = objc2::runtime::AnyClass::get(c"NSWorkspace").unwrap();
            let workspace: *mut objc2::runtime::AnyObject =
                objc2::msg_send![workspace_cls, sharedWorkspace];
            let _: bool = objc2::msg_send![workspace, openURL: url];
        }
    }
}

/// Check if dark mode is active. Returns 1 if dark, 0 if light.
#[no_mangle]
pub extern "C" fn perry_system_is_dark_mode() -> i64 {
    unsafe {
        // Method 1: NSUserDefaults — works before the window exists and for
        // explicit Dark mode. Returns nil for Auto mode.
        let defaults_cls = objc2::runtime::AnyClass::get(c"NSUserDefaults").unwrap();
        let defaults: *mut objc2::runtime::AnyObject =
            objc2::msg_send![defaults_cls, standardUserDefaults];
        let key = objc2_foundation::NSString::from_str("AppleInterfaceStyle");
        let style: *mut objc2::runtime::AnyObject = objc2::msg_send![defaults, stringForKey: &*key];
        if !style.is_null() {
            let dark_str = objc2_foundation::NSString::from_str("Dark");
            let is_dark: bool = objc2::msg_send![style, isEqualToString: &*dark_str];
            if is_dark {
                return 1;
            }
        }

        // Method 2: NSApp.effectiveAppearance — works once the app is initialized.
        let app_cls = objc2::runtime::AnyClass::get(c"NSApplication").unwrap();
        let app: *mut objc2::runtime::AnyObject = objc2::msg_send![app_cls, sharedApplication];
        let appearance: *mut objc2::runtime::AnyObject = objc2::msg_send![app, effectiveAppearance];
        if !appearance.is_null() {
            let name: *mut objc2::runtime::AnyObject = objc2::msg_send![appearance, name];
            if !name.is_null() {
                let dark_name = objc2_foundation::NSString::from_str("NSAppearanceNameDarkAqua");
                let is_dark: bool = objc2::msg_send![name, isEqualToString: &*dark_name];
                if is_dark {
                    return 1;
                }
            }
        }
        0
    }
}

/// Set a preference value (UserDefaults). Supports strings and numbers.
#[no_mangle]
pub extern "C" fn perry_system_preferences_set(key_ptr: i64, value: f64) {
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
            // NaN-boxed string — extract string pointer
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

/// Get a preference value (UserDefaults). Returns NaN-boxed string, number, or TAG_UNDEFINED.
#[no_mangle]
pub extern "C" fn perry_system_preferences_get(key_ptr: i64) -> f64 {
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
            return f64::from_bits(0x7FFC_0000_0000_0001); // TAG_UNDEFINED
        }
        // Check if it's an NSString
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
        // Check if it's an NSNumber
        if let Some(num_cls) = objc2::runtime::AnyClass::get(c"NSNumber") {
            let is_number: bool = objc2::msg_send![obj, isKindOfClass: num_cls];
            if is_number {
                let val: f64 = objc2::msg_send![obj, doubleValue];
                return val;
            }
        }
        f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
    }
}

/// Play a haptic feedback effect (perry/system hapticPlay) via
/// NSHapticFeedbackManager — the Force Touch trackpad actuator. Only
/// fires on hardware with a haptic trackpad; AppKit makes it a silent
/// no-op elsewhere, which matches the API contract.
///
/// Pattern raw values verified against the macOS 26.5 SDK's
/// `AppKit/NSHapticFeedback.h`: Generic=0, Alignment=1, LevelChange=2.
/// PerformanceTime: Default=0, Now=1, DrawCompleted=2.
#[no_mangle]
pub extern "C" fn perry_system_haptic_play(type_ptr: i64) {
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
    let name = str_from_header(type_ptr as *const u8);
    // Semantic notification types get the stronger LevelChange pattern;
    // everything else (impacts, ticks, directions) maps to Generic.
    let pattern: i64 = match name {
        "success" | "warning" | "error" => 2, // NSHapticFeedbackPatternLevelChange
        _ => 0,                               // NSHapticFeedbackPatternGeneric
    };
    unsafe {
        if let Some(mgr_cls) = objc2::runtime::AnyClass::get(c"NSHapticFeedbackManager") {
            let performer: *mut objc2::runtime::AnyObject =
                objc2::msg_send![mgr_cls, defaultPerformer];
            if !performer.is_null() {
                // NSHapticFeedbackPerformanceTimeNow = 1
                let _: () = objc2::msg_send![
                    performer,
                    performFeedbackPattern: pattern,
                    performanceTime: 1i64
                ];
            }
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
            let header = ptr as *const crate::string_header::StringHeader;
            let len = (*header).byte_len as usize;
            let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
        }
    }
    let family = str_from_header(family_ptr as *const u8);
    if let Some(view) = widgets::get_widget(handle) {
        unsafe {
            let tf: &objc2_app_kit::NSTextField =
                &*(objc2::rc::Retained::as_ptr(&view) as *const objc2_app_kit::NSTextField);
            // Get current font size (default 13.0 if none)
            let current_font: Option<objc2::rc::Retained<objc2_app_kit::NSFont>> = tf.font();
            let size = current_font.as_ref().map(|f| f.pointSize()).unwrap_or(13.0);

            let font: objc2::rc::Retained<objc2_app_kit::NSFont> =
                if family == "monospaced" || family == "monospace" {
                    objc2::msg_send![
                        objc2::runtime::AnyClass::get(c"NSFont").unwrap(),
                        monospacedSystemFontOfSize: size as objc2_core_foundation::CGFloat,
                        weight: 0.0 as objc2_core_foundation::CGFloat
                    ]
                } else {
                    let ns_name = objc2_foundation::NSString::from_str(family);
                    let result: *mut objc2_app_kit::NSFont = objc2::msg_send![
                        objc2::runtime::AnyClass::get(c"NSFont").unwrap(),
                        fontWithName: &*ns_name,
                        size: size as objc2_core_foundation::CGFloat
                    ];
                    if result.is_null() {
                        // Fallback to system font
                        objc2::msg_send![
                            objc2::runtime::AnyClass::get(c"NSFont").unwrap(),
                            systemFontOfSize: size as objc2_core_foundation::CGFloat
                        ]
                    } else {
                        objc2::rc::Retained::retain(result).unwrap()
                    }
                };
            tf.setFont(Some(&font));
        }
    }
}
