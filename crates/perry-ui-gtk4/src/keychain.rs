use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
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

fn keychain_path() -> PathBuf {
    let config = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        format!("{}/.local/share", home)
    });
    PathBuf::from(config).join("perry").join("keychain")
}

thread_local! {
    static KEYCHAIN: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    static KEYCHAIN_LOADED: RefCell<bool> = RefCell::new(false);
}

fn ensure_loaded() {
    KEYCHAIN_LOADED.with(|loaded| {
        if !*loaded.borrow() {
            *loaded.borrow_mut() = true;
            let path = keychain_path();
            if let Ok(contents) = std::fs::read_to_string(&path) {
                KEYCHAIN.with(|k| {
                    let mut kc = k.borrow_mut();
                    for line in contents.lines() {
                        if let Some((key, val)) = line.split_once('=') {
                            kc.insert(key.to_string(), val.to_string());
                        }
                    }
                });
            }
        }
    });
}

fn save_keychain() {
    let path = keychain_path();
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    KEYCHAIN.with(|k| {
        let kc = k.borrow();
        let mut content = String::new();
        for (key, val) in kc.iter() {
            content.push_str(key);
            content.push('=');
            content.push_str(val);
            content.push('\n');
        }
        let _ = std::fs::write(&path, content);
    });
}

/// Save a value to the keychain.
pub fn save(key_ptr: *const u8, value_ptr: *const u8) {
    ensure_loaded();
    let key = str_from_header(key_ptr);
    let value = str_from_header(value_ptr);
    KEYCHAIN.with(|k| {
        k.borrow_mut().insert(key.to_string(), value.to_string());
    });
    save_keychain();
}

/// Get a value from the keychain. Returns NaN-boxed string or TAG_UNDEFINED.
pub fn get(key_ptr: *const u8) -> f64 {
    ensure_loaded();
    let key = str_from_header(key_ptr);
    KEYCHAIN.with(|k| {
        let kc = k.borrow();
        if let Some(val) = kc.get(key) {
            let bytes = val.as_bytes();
            let str_ptr = unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64) };
            unsafe { js_nanbox_string(str_ptr as i64) }
        } else {
            f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
        }
    })
}

/// Delete a value from the keychain.
pub fn delete(key_ptr: *const u8) {
    ensure_loaded();
    let key = str_from_header(key_ptr);
    KEYCHAIN.with(|k| {
        k.borrow_mut().remove(key);
    });
    save_keychain();
}
