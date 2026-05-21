//! FFI exports: Keychain (SecItem) + notifications
//!
//! Extracted from `lib.rs` for file-size hygiene. No behavior changes.

use crate::*;

// =============================================================================
// Keychain (iOS — uses SecItem API with data protection keychain)
// =============================================================================

fn keychain_str_from_header(ptr: *const u8) -> &'static str {
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
    let key = keychain_str_from_header(key_ptr as *const u8);
    let value = keychain_str_from_header(value_ptr as *const u8);
    unsafe {
        let value_data: objc2::rc::Retained<objc2::runtime::AnyObject> = {
            let ns_str = objc2_foundation::NSString::from_str(value);
            objc2::msg_send![&*ns_str, dataUsingEncoding: 4u64]
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
    let key = keychain_str_from_header(key_ptr as *const u8);
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
            extern "C" {
                fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
                fn js_nanbox_string(ptr: i64) -> f64;
            }
            let str_ptr = js_string_from_bytes(bytes, length as i64);
            js_nanbox_string(str_ptr as i64)
        } else {
            f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
        }
    }
}

#[no_mangle]
pub extern "C" fn perry_system_keychain_delete(key_ptr: i64) {
    let key = keychain_str_from_header(key_ptr as *const u8);
    unsafe {
        let query = keychain_make_query(key);
        SecItemDelete(&*query as *const _ as *const std::ffi::c_void);
    }
}

// =============================================================================
// Notifications
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_system_notification_send(title_ptr: i64, body_ptr: i64) {
    notifications::send(title_ptr as *const u8, body_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_system_notification_register_remote(callback: f64) {
    notifications::register_remote(callback);
}

#[no_mangle]
pub extern "C" fn perry_system_notification_on_receive(callback: f64) {
    notifications::on_receive(callback);
}

/// Background-delivery handler (#98). The closure registered here fires from
/// `application:didReceiveRemoteNotification:fetchCompletionHandler:`; iOS's
/// completion handler is invoked once the user's returned Promise settles.
#[no_mangle]
pub extern "C" fn perry_system_notification_on_background_receive(callback: f64) {
    notifications::on_background_receive(callback);
}

#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_interval(
    id_ptr: i64,
    title_ptr: i64,
    body_ptr: i64,
    seconds: f64,
    repeats: f64,
) {
    notifications::schedule_interval(
        id_ptr as *const u8,
        title_ptr as *const u8,
        body_ptr as *const u8,
        seconds,
        repeats,
    );
}

#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_calendar(
    id_ptr: i64,
    title_ptr: i64,
    body_ptr: i64,
    timestamp_ms: f64,
) {
    notifications::schedule_calendar(
        id_ptr as *const u8,
        title_ptr as *const u8,
        body_ptr as *const u8,
        timestamp_ms,
    );
}

#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_location(
    id_ptr: i64,
    title_ptr: i64,
    body_ptr: i64,
    lat: f64,
    lon: f64,
    radius: f64,
) {
    notifications::schedule_location(
        id_ptr as *const u8,
        title_ptr as *const u8,
        body_ptr as *const u8,
        lat,
        lon,
        radius,
    );
}

#[no_mangle]
pub extern "C" fn perry_system_notification_cancel(id_ptr: i64) {
    notifications::cancel(id_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_system_notification_on_tap(callback: f64) {
    notifications::set_on_tap(callback);
}

#[no_mangle]
pub extern "C" fn perry_system_get_locale() -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    }
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
        let len = libc::strlen(utf8 as *const i8);
        let code_len = if len >= 2 { 2 } else { len };
        js_string_from_bytes(utf8, code_len as i64) as i64
    }
}
