//! System APIs — open_url, dark mode, preferences, keychain, notifications (Win32)

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_get_string_pointer_unified(value: f64) -> *const u8;
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

/// Safe wrapper for other modules.
pub fn js_get_string_pointer_unified_safe(value: f64) -> *const u8 {
    unsafe { js_get_string_pointer_unified(value) }
}

fn prefs_dir() -> PathBuf {
    let appdata = std::env::var("APPDATA")
        .unwrap_or_else(|_| std::env::var("HOME").unwrap_or_else(|_| ".".to_string()));
    PathBuf::from(appdata).join("Perry")
}

thread_local! {
    static PREFS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    static PREFS_LOADED: RefCell<bool> = RefCell::new(false);
    static KEYCHAIN: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    static KEYCHAIN_LOADED: RefCell<bool> = RefCell::new(false);
}

fn ensure_prefs_loaded() {
    PREFS_LOADED.with(|loaded| {
        if !*loaded.borrow() {
            *loaded.borrow_mut() = true;
            let path = prefs_dir().join("prefs.ini");
            if let Ok(contents) = std::fs::read_to_string(&path) {
                PREFS.with(|p| {
                    let mut prefs = p.borrow_mut();
                    for line in contents.lines() {
                        if let Some((k, v)) = line.split_once('=') {
                            prefs.insert(k.to_string(), v.to_string());
                        }
                    }
                });
            }
        }
    });
}

fn save_prefs() {
    let dir = prefs_dir();
    let _ = std::fs::create_dir_all(&dir);
    PREFS.with(|p| {
        let prefs = p.borrow();
        let mut content = String::new();
        for (k, v) in prefs.iter() {
            content.push_str(k);
            content.push('=');
            content.push_str(v);
            content.push('\n');
        }
        let _ = std::fs::write(dir.join("prefs.ini"), content);
    });
}

fn ensure_keychain_loaded() {
    KEYCHAIN_LOADED.with(|loaded| {
        if !*loaded.borrow() {
            *loaded.borrow_mut() = true;
            let path = prefs_dir().join("keychain");
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
    let dir = prefs_dir();
    let _ = std::fs::create_dir_all(&dir);
    KEYCHAIN.with(|k| {
        let kc = k.borrow();
        let mut content = String::new();
        for (key, val) in kc.iter() {
            content.push_str(key);
            content.push('=');
            content.push_str(val);
            content.push('\n');
        }
        let _ = std::fs::write(dir.join("keychain"), content);
    });
}

/// Open a URL in the default browser.
pub fn open_url(url_ptr: *const u8) {
    let url = str_from_header(url_ptr);
    #[cfg(target_os = "windows")]
    {
        use windows::core::PCWSTR;
        use windows::Win32::UI::Shell::ShellExecuteW;
        let url_wide: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
        let open_wide: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            ShellExecuteW(
                None,
                PCWSTR(open_wide.as_ptr()),
                PCWSTR(url_wide.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                windows::Win32::UI::WindowsAndMessaging::SW_SHOW,
            );
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
}

/// Check if dark mode is active.
pub fn is_dark_mode() -> i64 {
    #[cfg(target_os = "windows")]
    {
        use windows::core::PCWSTR;
        use windows::Win32::System::Registry::*;
        unsafe {
            let key_wide: Vec<u16> =
                "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
            let value_wide: Vec<u16> = "AppsUseLightTheme"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let mut hkey = HKEY::default();
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(key_wide.as_ptr()),
                0,
                KEY_READ,
                &mut hkey,
            )
            .is_ok()
            {
                let mut data: u32 = 1;
                let mut size = std::mem::size_of::<u32>() as u32;
                if RegQueryValueExW(
                    hkey,
                    PCWSTR(value_wide.as_ptr()),
                    None,
                    None,
                    Some(&mut data as *mut u32 as *mut u8),
                    Some(&mut size),
                )
                .is_ok()
                {
                    let _ = RegCloseKey(hkey);
                    return if data == 0 { 1 } else { 0 };
                }
                let _ = RegCloseKey(hkey);
            }
        }
        0
    }

    #[cfg(not(target_os = "windows"))]
    0
}

/// Set a preference value.
pub fn preferences_set(key_ptr: *const u8, value: f64) {
    ensure_prefs_loaded();
    let key = str_from_header(key_ptr);
    let str_ptr = unsafe { js_get_string_pointer_unified(value) };
    let val_str = if !str_ptr.is_null() {
        str_from_header(str_ptr).to_string()
    } else {
        format!("{}", value)
    };
    PREFS.with(|p| {
        p.borrow_mut().insert(key.to_string(), val_str);
    });
    save_prefs();
}

/// Get a preference value.
pub fn preferences_get(key_ptr: *const u8) -> f64 {
    ensure_prefs_loaded();
    let key = str_from_header(key_ptr);
    PREFS.with(|p| {
        let prefs = p.borrow();
        if let Some(val) = prefs.get(key) {
            if let Ok(n) = val.parse::<f64>() {
                n
            } else {
                let bytes = val.as_bytes();
                let str_ptr = unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64) };
                unsafe { js_nanbox_string(str_ptr as i64) }
            }
        } else {
            f64::from_bits(0x7FFC_0000_0000_0001)
        }
    })
}

/// Save to keychain.
pub fn keychain_save(key_ptr: *const u8, value_ptr: *const u8) {
    ensure_keychain_loaded();
    let key = str_from_header(key_ptr);
    let value = str_from_header(value_ptr);
    KEYCHAIN.with(|k| {
        k.borrow_mut().insert(key.to_string(), value.to_string());
    });
    save_keychain();
}

/// Get from keychain.
pub fn keychain_get(key_ptr: *const u8) -> f64 {
    ensure_keychain_loaded();
    let key = str_from_header(key_ptr);
    KEYCHAIN.with(|k| {
        let kc = k.borrow();
        if let Some(val) = kc.get(key) {
            let bytes = val.as_bytes();
            let str_ptr = unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64) };
            unsafe { js_nanbox_string(str_ptr as i64) }
        } else {
            f64::from_bits(0x7FFC_0000_0000_0001)
        }
    })
}

/// Delete from keychain.
pub fn keychain_delete(key_ptr: *const u8) {
    ensure_keychain_loaded();
    let key = str_from_header(key_ptr);
    KEYCHAIN.with(|k| {
        k.borrow_mut().remove(key);
    });
    save_keychain();
}

/// Send a notification.
pub fn notification_send(title_ptr: *const u8, body_ptr: *const u8) {
    let _title = str_from_header(title_ptr);
    let _body = str_from_header(body_ptr);

    #[cfg(target_os = "windows")]
    {
        // Win32 notification via Shell_NotifyIconW is complex.
        // For now, use a simple MessageBox as fallback.
        use windows::core::PCWSTR;
        use windows::Win32::UI::WindowsAndMessaging::*;
        let title_wide: Vec<u16> = _title.encode_utf16().chain(std::iter::once(0)).collect();
        let body_wide: Vec<u16> = _body.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            MessageBoxW(
                None,
                PCWSTR(body_wide.as_ptr()),
                PCWSTR(title_wide.as_ptr()),
                MB_OK | MB_ICONINFORMATION,
            );
        }
    }
}
