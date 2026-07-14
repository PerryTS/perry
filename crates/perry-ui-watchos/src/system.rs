//! FFI exports: system preferences (NSUserDefaults), keychain (SecItem),
//! and locale — watchOS implementations.
//!
//! Ported from the iOS implementations (`perry-ui-ios/src/ffi/camera.rs`
//! preferences + `security_notifications.rs` keychain/locale). Same
//! Foundation / Security.framework APIs exist on watchOS; the link line
//! already carries both frameworks.
//!
//! arm64_32 caution: watchOS device builds may be ILP32 (32-bit pointers).
//! All runtime externs below match the real `perry-runtime` signatures
//! (`js_string_from_bytes(ptr, len: u32) -> *mut u8`) and pointers travel
//! as pointers/`usize` internally — widening to `i64` only happens at the
//! NaN-box / FFI boundary via zero-extending casts.

use crate::str_from_header;

/// NaN-boxed `undefined` (matches `perry-runtime`'s TAG_UNDEFINED).
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: u32) -> *mut u8;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

// =============================================================================
// Preferences (NSUserDefaults — thread-safe, no main-thread hop needed)
// =============================================================================

/// Set a preference value (UserDefaults).
#[no_mangle]
pub extern "C" fn perry_system_preferences_set(key_ptr: i64, value: f64) {
    let key = str_from_header(key_ptr as *const u8);
    let bits = value.to_bits();
    unsafe {
        let defaults_cls = objc2::runtime::AnyClass::get(c"NSUserDefaults").unwrap();
        let defaults: *mut objc2::runtime::AnyObject =
            objc2::msg_send![defaults_cls, standardUserDefaults];
        let ns_key = objc2_foundation::NSString::from_str(key);
        if (bits >> 48) == 0x7FFF {
            // NaN-boxed string payload
            let str_ptr = js_nanbox_get_pointer(value) as usize as *const u8;
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
    let key = str_from_header(key_ptr as *const u8);
    unsafe {
        let defaults_cls = objc2::runtime::AnyClass::get(c"NSUserDefaults").unwrap();
        let defaults: *mut objc2::runtime::AnyObject =
            objc2::msg_send![defaults_cls, standardUserDefaults];
        let ns_key = objc2_foundation::NSString::from_str(key);
        let obj: *mut objc2::runtime::AnyObject =
            objc2::msg_send![defaults, objectForKey: &*ns_key];
        if obj.is_null() {
            return f64::from_bits(TAG_UNDEFINED);
        }
        if let Some(str_cls) = objc2::runtime::AnyClass::get(c"NSString") {
            let is_string: bool = objc2::msg_send![obj, isKindOfClass: str_cls];
            if is_string {
                let ns_str: &objc2_foundation::NSString =
                    &*(obj as *const objc2_foundation::NSString);
                let rust_str = ns_str.to_string();
                let bytes = rust_str.as_bytes();
                let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
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
                                    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
                                return js_nanbox_string(str_ptr as i64);
                            }
                        }
                    }
                }
            }
        }
        f64::from_bits(TAG_UNDEFINED)
    }
}

// =============================================================================
// Keychain (SecItem API — Security.framework is on the watch link line)
// =============================================================================

extern "C" {
    fn SecItemAdd(attributes: *const std::ffi::c_void, result: *mut *const std::ffi::c_void)
        -> i32;
    fn SecItemCopyMatching(
        query: *const std::ffi::c_void,
        result: *mut *const std::ffi::c_void,
    ) -> i32;
    fn SecItemUpdate(query: *const std::ffi::c_void, attrs: *const std::ffi::c_void) -> i32;
    fn SecItemDelete(query: *const std::ffi::c_void) -> i32;
    static kSecClass: *const std::ffi::c_void;
    static kSecClassGenericPassword: *const std::ffi::c_void;
    static kSecAttrAccount: *const std::ffi::c_void;
    static kSecAttrService: *const std::ffi::c_void;
    static kSecValueData: *const std::ffi::c_void;
    static kSecReturnData: *const std::ffi::c_void;
    static kSecMatchLimit: *const std::ffi::c_void;
    static kSecMatchLimitOne: *const std::ffi::c_void;
}

unsafe fn keychain_make_query(key: &str) -> objc2::rc::Retained<objc2::runtime::AnyObject> {
    let dict_cls = objc2::runtime::AnyClass::get(c"NSMutableDictionary").unwrap();
    let dict: objc2::rc::Retained<objc2::runtime::AnyObject> = objc2::msg_send![dict_cls, new];
    let _: () = objc2::msg_send![&*dict, setObject: kSecClassGenericPassword as *const objc2::runtime::AnyObject, forKey: kSecClass as *const objc2::runtime::AnyObject];
    let ns_key = objc2_foundation::NSString::from_str(key);
    let _: () = objc2::msg_send![&*dict, setObject: &*ns_key, forKey: kSecAttrAccount as *const objc2::runtime::AnyObject];
    let ns_service = objc2_foundation::NSString::from_str("perry");
    let _: () = objc2::msg_send![&*dict, setObject: &*ns_service, forKey: kSecAttrService as *const objc2::runtime::AnyObject];
    dict
}

#[no_mangle]
pub extern "C" fn perry_system_keychain_save(key_ptr: i64, value_ptr: i64) {
    let key = str_from_header(key_ptr as *const u8);
    let value = str_from_header(value_ptr as *const u8);
    unsafe {
        let value_data: objc2::rc::Retained<objc2::runtime::AnyObject> = {
            let ns_str = objc2_foundation::NSString::from_str(value);
            // 4 = NSUTF8StringEncoding; NSStringEncoding is NSUInteger
            // (32-bit on arm64_32), so pass usize, not u64.
            objc2::msg_send![&*ns_str, dataUsingEncoding: 4usize]
        };
        // Try update first
        let query = keychain_make_query(key);
        let dict_cls = objc2::runtime::AnyClass::get(c"NSMutableDictionary").unwrap();
        let update: objc2::rc::Retained<objc2::runtime::AnyObject> =
            objc2::msg_send![dict_cls, new];
        let _: () = objc2::msg_send![&*update, setObject: &*value_data, forKey: kSecValueData as *const objc2::runtime::AnyObject];
        let status = SecItemUpdate(
            &*query as *const _ as *const std::ffi::c_void,
            &*update as *const _ as *const std::ffi::c_void,
        );
        if status == -25300 {
            // errSecItemNotFound
            let add = keychain_make_query(key);
            let _: () = objc2::msg_send![&*add, setObject: &*value_data, forKey: kSecValueData as *const objc2::runtime::AnyObject];
            SecItemAdd(
                &*add as *const _ as *const std::ffi::c_void,
                std::ptr::null_mut(),
            );
        }
    }
}

#[no_mangle]
pub extern "C" fn perry_system_keychain_get(key_ptr: i64) -> f64 {
    let key = str_from_header(key_ptr as *const u8);
    unsafe {
        let dict = keychain_make_query(key);
        let cf_true: *const objc2::runtime::AnyObject = objc2::msg_send![
            objc2::runtime::AnyClass::get(c"NSNumber").unwrap(), numberWithBool: true
        ];
        let _: () = objc2::msg_send![&*dict, setObject: cf_true, forKey: kSecReturnData as *const objc2::runtime::AnyObject];
        let _: () = objc2::msg_send![&*dict, setObject: kSecMatchLimitOne as *const objc2::runtime::AnyObject, forKey: kSecMatchLimit as *const objc2::runtime::AnyObject];
        let mut result: *const std::ffi::c_void = std::ptr::null();
        let status =
            SecItemCopyMatching(&*dict as *const _ as *const std::ffi::c_void, &mut result);
        if status == 0 && !result.is_null() {
            let data = result as *const objc2::runtime::AnyObject;
            let bytes: *const u8 = objc2::msg_send![data, bytes];
            let length: usize = objc2::msg_send![data, length];
            let str_ptr = js_string_from_bytes(bytes, length as u32);
            // SecItemCopyMatching + kSecReturnData follows the Core Foundation
            // Copy Rule: the returned CFData is +1 and owned by us. Release it
            // now that its bytes are copied into the JS string (CFData is
            // toll-free bridged to NSData, so an objc `release` is the
            // CFRelease equivalent) — otherwise every keychain read leaks it.
            let _: () = objc2::msg_send![data, release];
            js_nanbox_string(str_ptr as i64)
        } else {
            f64::from_bits(TAG_UNDEFINED)
        }
    }
}

#[no_mangle]
pub extern "C" fn perry_system_keychain_delete(key_ptr: i64) {
    let key = str_from_header(key_ptr as *const u8);
    unsafe {
        let query = keychain_make_query(key);
        SecItemDelete(&*query as *const _ as *const std::ffi::c_void);
    }
}

// =============================================================================
// Locale
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_system_get_locale() -> i64 {
    unsafe {
        // Use currentLocale.languageCode — reflects the actual device language setting
        let ns_locale: *mut objc2::runtime::AnyObject = objc2::msg_send![
            objc2::runtime::AnyClass::get(c"NSLocale").unwrap(),
            currentLocale
        ];
        let lang_code: *mut objc2::runtime::AnyObject = objc2::msg_send![ns_locale, languageCode];
        if lang_code.is_null() {
            let fallback = b"en";
            return js_string_from_bytes(fallback.as_ptr(), 2) as i64;
        }
        let utf8: *const u8 = objc2::msg_send![lang_code, UTF8String];
        let len = libc::strlen(utf8 as *const libc::c_char);
        let code_len = if len >= 2 { 2 } else { len };
        js_string_from_bytes(utf8, code_len as u32) as i64
    }
}
