use crate::ffi::js_string_from_bytes;
use std::ffi::c_void;

extern "C" {
    fn js_nanbox_string(ptr: i64) -> f64;
}

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

// Security framework functions
extern "C" {
    fn SecItemAdd(attributes: *const c_void, result: *mut *const c_void) -> i32;
    fn SecItemCopyMatching(query: *const c_void, result: *mut *const c_void) -> i32;
    fn SecItemUpdate(query: *const c_void, attributes_to_update: *const c_void) -> i32;
    fn SecItemDelete(query: *const c_void) -> i32;
}

/// Create a keychain query dictionary for a given key.
unsafe fn make_query(key: &str) -> objc2::rc::Retained<objc2::runtime::AnyObject> {
    let dict_cls = objc2::runtime::AnyClass::get(c"NSMutableDictionary").unwrap();
    let dict: objc2::rc::Retained<objc2::runtime::AnyObject> = objc2::msg_send![dict_cls, new];

    // kSecClass = kSecClassGenericPassword
    let sec_class_key: *const c_void = kSecClass;
    let sec_class_val: *const c_void = kSecClassGenericPassword;
    let _: () = objc2::msg_send![&*dict, setObject: sec_class_val as *const objc2::runtime::AnyObject, forKey: sec_class_key as *const objc2::runtime::AnyObject];

    // kSecAttrAccount = key
    let attr_account_key: *const c_void = kSecAttrAccount;
    let ns_key = objc2_foundation::NSString::from_str(key);
    let _: () = objc2::msg_send![&*dict, setObject: &*ns_key, forKey: attr_account_key as *const objc2::runtime::AnyObject];

    // kSecAttrService = "perry"
    let attr_service_key: *const c_void = kSecAttrService;
    let ns_service = objc2_foundation::NSString::from_str("perry");
    let _: () = objc2::msg_send![&*dict, setObject: &*ns_service, forKey: attr_service_key as *const objc2::runtime::AnyObject];

    dict
}

// Link against Security framework constants (data symbols, not functions)
extern "C" {
    static kSecClass: *const c_void;
    static kSecClassGenericPassword: *const c_void;
    static kSecAttrAccount: *const c_void;
    static kSecAttrService: *const c_void;
    static kSecValueData: *const c_void;
    static kSecReturnData: *const c_void;
    static kSecMatchLimit: *const c_void;
    static kSecMatchLimitOne: *const c_void;
}

/// Save a key-value pair to the keychain.
/// Uses SecItemUpdate to preserve the ACL (avoids keychain prompts on code signature changes).
/// Falls back to SecItemAdd if the item doesn't exist yet.
pub fn save(key_ptr: *const u8, value_ptr: *const u8) {
    let key = str_from_header(key_ptr);
    let value = str_from_header(value_ptr);

    unsafe {
        let value_data: objc2::rc::Retained<objc2::runtime::AnyObject> = {
            let ns_str = objc2_foundation::NSString::from_str(value);
            objc2::msg_send![&*ns_str, dataUsingEncoding: 4u64] // NSUTF8StringEncoding = 4
        };
        let value_data_key: *const c_void = kSecValueData;

        // Try to update existing entry first (preserves ACL)
        let query = make_query(key);
        let dict_cls = objc2::runtime::AnyClass::get(c"NSMutableDictionary").unwrap();
        let update_dict: objc2::rc::Retained<objc2::runtime::AnyObject> =
            objc2::msg_send![dict_cls, new];
        let _: () = objc2::msg_send![&*update_dict, setObject: &*value_data, forKey: value_data_key as *const objc2::runtime::AnyObject];

        let status = SecItemUpdate(
            &*query as *const _ as *const c_void,
            &*update_dict as *const _ as *const c_void,
        );

        // errSecItemNotFound = -25300: item doesn't exist yet, add it
        if status == -25300 {
            let add_dict = make_query(key);
            let _: () = objc2::msg_send![&*add_dict, setObject: &*value_data, forKey: value_data_key as *const objc2::runtime::AnyObject];
            SecItemAdd(
                &*add_dict as *const _ as *const c_void,
                std::ptr::null_mut(),
            );
        }
    }
}

/// Get a value from the keychain. Returns NaN-boxed string or TAG_UNDEFINED.
pub fn get(key_ptr: *const u8) -> f64 {
    let key = str_from_header(key_ptr);

    unsafe {
        let dict = make_query(key);

        // Add kSecReturnData = true
        let return_data_key: *const c_void = kSecReturnData;
        let cf_true: *const objc2::runtime::AnyObject = objc2::msg_send![
            objc2::runtime::AnyClass::get(c"NSNumber").unwrap(), numberWithBool: true
        ];
        let _: () = objc2::msg_send![&*dict, setObject: cf_true, forKey: return_data_key as *const objc2::runtime::AnyObject];

        // Add kSecMatchLimit = kSecMatchLimitOne
        let limit_key: *const c_void = kSecMatchLimit;
        let limit_one: *const c_void = kSecMatchLimitOne;
        let _: () = objc2::msg_send![&*dict, setObject: limit_one as *const objc2::runtime::AnyObject, forKey: limit_key as *const objc2::runtime::AnyObject];

        let mut result: *const c_void = std::ptr::null();
        let status = SecItemCopyMatching(&*dict as *const _ as *const c_void, &mut result);

        if status == 0 && !result.is_null() {
            // Result is NSData
            let data = result as *const objc2::runtime::AnyObject;
            let bytes: *const u8 = objc2::msg_send![data, bytes];
            let length: usize = objc2::msg_send![data, length];

            let str_ptr = js_string_from_bytes(bytes, length as u32);
            js_nanbox_string(str_ptr as i64)
        } else {
            f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
        }
    }
}

/// Delete a value from the keychain.
pub fn delete(key_ptr: *const u8) {
    let key = str_from_header(key_ptr);
    unsafe {
        let query = make_query(key);
        SecItemDelete(&*query as *const _ as *const c_void);
    }
}
