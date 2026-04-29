use gtk4::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_get_string_pointer_unified(value: f64) -> *const u8;
}

/// Safe wrapper for js_get_string_pointer_unified callable from other modules.
pub fn js_get_string_pointer_unified_safe(value: f64) -> *const u8 {
    unsafe { js_get_string_pointer_unified(value) }
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

fn prefs_path() -> PathBuf {
    let config = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        format!("{}/.config", home)
    });
    PathBuf::from(config).join("perry")
}

thread_local! {
    /// In-memory preferences cache (persisted to disk)
    static PREFS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    static PREFS_LOADED: RefCell<bool> = RefCell::new(false);
}

fn ensure_prefs_loaded() {
    PREFS_LOADED.with(|loaded| {
        if !*loaded.borrow() {
            *loaded.borrow_mut() = true;
            let path = prefs_path().join("prefs.ini");
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
    let dir = prefs_path();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("prefs.ini");
    PREFS.with(|p| {
        let prefs = p.borrow();
        let mut content = String::new();
        for (k, v) in prefs.iter() {
            content.push_str(k);
            content.push('=');
            content.push_str(v);
            content.push('\n');
        }
        let _ = std::fs::write(&path, content);
    });
}

/// Open a URL using the default browser.
pub fn open_url(url_ptr: *const u8) {
    let url = str_from_header(url_ptr);
    // Try gio first, fall back to xdg-open
    if gtk4::gio::AppInfo::launch_default_for_uri(url, None::<&gtk4::gio::AppLaunchContext>)
        .is_err()
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

/// Check if dark mode is preferred.
pub fn is_dark_mode() -> i64 {
    crate::app::ensure_gtk_init();
    let settings = gtk4::Settings::default().unwrap();
    if settings.is_gtk_application_prefer_dark_theme() {
        return 1;
    }
    // Also check theme name for common dark theme patterns
    if let Some(theme) = settings.gtk_theme_name() {
        let theme = theme.to_lowercase();
        if theme.contains("dark") {
            return 1;
        }
    }
    0
}

/// Set a preference value. value is either a f64 number or a NaN-boxed string.
pub fn preferences_set(key_ptr: *const u8, value: f64) {
    ensure_prefs_loaded();
    let key = str_from_header(key_ptr);

    // Check if value is a NaN-boxed string
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

/// Get a preference value. Returns NaN-boxed string or the numeric value.
pub fn preferences_get(key_ptr: *const u8) -> f64 {
    ensure_prefs_loaded();
    let key = str_from_header(key_ptr);

    PREFS.with(|p| {
        let prefs = p.borrow();
        if let Some(val) = prefs.get(key) {
            // Try to parse as f64 first
            if let Ok(n) = val.parse::<f64>() {
                n
            } else {
                // Return as NaN-boxed string
                let bytes = val.as_bytes();
                let str_ptr = unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64) };
                unsafe { js_nanbox_string(str_ptr as i64) }
            }
        } else {
            f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
        }
    })
}

/// Send a desktop notification.
pub fn notification_send(title_ptr: *const u8, body_ptr: *const u8) {
    let title = str_from_header(title_ptr);
    let body = str_from_header(body_ptr);

    crate::app::GTK_APP.with(|ga| {
        if let Some(app) = ga.borrow().as_ref() {
            let notif = gtk4::gio::Notification::new(title);
            notif.set_body(Some(body));
            app.send_notification(None, &notif);
        } else {
            // Fallback: try notify-send
            let _ = std::process::Command::new("notify-send")
                .arg(title)
                .arg(body)
                .spawn();
        }
    });
}
