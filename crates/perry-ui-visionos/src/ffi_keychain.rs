//! Keychain FFI exports. Uses the SecItem API with the data-protection
//! keychain. Behavior is unchanged from the pre-split `lib.rs`.

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
