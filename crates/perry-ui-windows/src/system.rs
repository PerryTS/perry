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
                Some(0),
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

/// Send a notification (#5283).
///
/// Shows a real Windows toast/balloon via `Shell_NotifyIconW(NIM_ADD, …)` with
/// `NIF_INFO` — the same notification-area mechanism `tray.rs` uses, but with a
/// transient hidden owner window so a plain console program (no widgets, no
/// event loop) can post one. Previously this popped a modal `MessageBoxW`,
/// which is a blocking dialog the user must dismiss — not a notification — and
/// in practice looked like "nothing happened" next to the macOS banner
/// (`UNUserNotificationCenter`) behavior. The blocking `MessageBox` survives
/// only as a last-resort fallback if the toast can't be created.
pub fn notification_send(title_ptr: *const u8, body_ptr: *const u8) {
    let title = str_from_header(title_ptr);
    let body = str_from_header(body_ptr);

    #[cfg(target_os = "windows")]
    unsafe {
        if win_notify::show_toast(title, body) {
            return;
        }
        // Fallback: the toast could not be created (e.g. window/class
        // registration failed). A blocking MessageBox at least surfaces the
        // message rather than silently dropping it.
        use windows::core::PCWSTR;
        use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};
        let title_wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let body_wide: Vec<u16> = body.encode_utf16().chain(std::iter::once(0)).collect();
        MessageBoxW(
            None,
            PCWSTR(body_wide.as_ptr()),
            PCWSTR(title_wide.as_ptr()),
            MB_OK | MB_ICONINFORMATION,
        );
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (title, body);
    }
}

/// Win32 toast-notification helper for [`notification_send`] (#5283).
///
/// A console program that only imports `notificationSend` has no app window and
/// runs no message loop, so we stand up a throwaway hidden window to own the
/// notification-area icon, post the balloon, pump messages just long enough for
/// the shell to hand it to the Action Center, then tear everything down.
#[cfg(target_os = "windows")]
mod win_notify {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::SystemInformation::GetTickCount64;
    use windows::Win32::System::Threading::Sleep;
    use windows::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIIF_INFO, NIM_ADD, NIM_DELETE,
        NOTIFYICONDATAW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetWindowLongPtrW,
        LoadIconW, PeekMessageW, RegisterClassW, SetWindowLongPtrW, TranslateMessage,
        CW_USEDEFAULT, GWLP_USERDATA, HICON, IDI_APPLICATION, MSG, PM_REMOVE, WINDOW_EX_STYLE,
        WM_USER, WNDCLASSW, WS_OVERLAPPED,
    };

    /// Per-window callback message for the transient notification icon. Offset
    /// from `WM_USER` so it can't collide with the standard window messages;
    /// distinct from `tray.rs`'s `WM_USER + 200` for clarity (different window).
    const WM_PERRY_NOTIF: u32 = WM_USER + 201;

    /// Balloon lifecycle, stored per-window in `GWLP_USERDATA` (written by
    /// `wnd_proc`, polled by the pump loop): 0 = pending, 1 = shown (now queued
    /// into the Action Center and surviving icon removal), 2 = dismissed/timed-
    /// out. Keeping the state on the window — rather than in a process-global —
    /// means two concurrent `show_toast` calls (e.g. one from a `perry/thread`
    /// worker) each own a separate window and can't stomp each other's state.
    /// `GWLP_USERDATA` defaults to 0 (pending) on a fresh window.
    const STATE_SHOWN: isize = 1;
    const STATE_DISMISSED: isize = 2;

    /// Notification-area balloon events. `windows` 0.62 doesn't surface the
    /// `NIN_BALLOON*` constants under our enabled features, so we spell them
    /// out (`WM_USER + 2..=5`, per `<shellapi.h>`).
    const NIN_BALLOONSHOW: u32 = WM_USER + 2;
    const NIN_BALLOONHIDE: u32 = WM_USER + 3;
    const NIN_BALLOONTIMEOUT: u32 = WM_USER + 4;
    const NIN_BALLOONUSERCLICK: u32 = WM_USER + 5;

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Copy `s` (truncated to fit, leaving room for the NUL) into a fixed-size
    /// UTF-16 `NOTIFYICONDATAW` field (`szInfoTitle` is `[u16; 64]`, `szInfo`
    /// is `[u16; 256]`). Over-long text silently truncates, matching Explorer.
    fn write_field(field: &mut [u16], s: &str) {
        let max = field.len().saturating_sub(1);
        let wide: Vec<u16> = s.encode_utf16().take(max).collect();
        field[..wide.len()].copy_from_slice(&wide);
        field[wide.len()] = 0;
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_PERRY_NOTIF {
            // Legacy v0 semantics (we never call NIM_SETVERSION): the low word
            // of lParam carries the notification event. Mirrors tray.rs.
            let event = (lparam.0 & 0xFFFF) as u32;
            let new_state = match event {
                NIN_BALLOONSHOW => STATE_SHOWN,
                NIN_BALLOONTIMEOUT | NIN_BALLOONHIDE | NIN_BALLOONUSERCLICK => STATE_DISMISSED,
                _ => return LRESULT(0),
            };
            // Monotonic: never let a stray late event downgrade dismissed back
            // to shown. The window proc runs on the same thread that created
            // the window, so this read-modify-write needs no synchronization.
            if new_state > GetWindowLongPtrW(hwnd, GWLP_USERDATA) {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, new_state);
            }
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    /// Post a single toast. Returns `false` if the OS plumbing could not be set
    /// up (caller then falls back to a `MessageBox`).
    pub unsafe fn show_toast(title: &str, body: &str) -> bool {
        let Ok(hmodule) = GetModuleHandleW(None) else {
            return false;
        };
        let hinstance = HINSTANCE::from(hmodule);

        // Register the owner-window class. Idempotent across calls: a repeat
        // RegisterClassW for the same name fails with ERROR_CLASS_ALREADY_EXISTS,
        // which we ignore — the class stays usable for CreateWindowExW.
        let class_name = to_wide("PerryNotificationWindow");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance,
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);

        // A hidden top-level window owns the icon. A message-only
        // (HWND_MESSAGE) window can't host a notification-area icon, so we use
        // a normal overlapped window and simply never ShowWindow it.
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(class_name.as_ptr()),
            WS_OVERLAPPED,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            0,
            0,
            None,
            None,
            Some(hinstance),
            None,
        ) else {
            return false;
        };
        if hwnd.is_invalid() {
            return false;
        }
        // GWLP_USERDATA defaults to 0 (STATE_PENDING) on a fresh window.

        // IDI_APPLICATION is a shared system icon — we intentionally never
        // DestroyIcon it (a documented no-op on shared icons anyway).
        let hicon: HICON = LoadIconW(None, IDI_APPLICATION).unwrap_or(HICON(std::ptr::null_mut()));

        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_INFO,
            uCallbackMessage: WM_PERRY_NOTIF,
            hIcon: hicon,
            dwInfoFlags: NIIF_INFO,
            ..Default::default()
        };
        write_field(&mut data.szInfoTitle, title);
        write_field(&mut data.szInfo, body);

        if !Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
            let _ = DestroyWindow(hwnd);
            return false;
        }

        // Pump messages until the shell reports the balloon shown (it is then
        // queued into the Action Center and survives icon removal) or
        // dismissed, capped so a console program never hangs when the user has
        // notifications disabled and no event ever arrives.
        let deadline = GetTickCount64() + 4000;
        loop {
            if GetWindowLongPtrW(hwnd, GWLP_USERDATA) == STATE_DISMISSED {
                break;
            }
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if GetWindowLongPtrW(hwnd, GWLP_USERDATA) >= STATE_SHOWN {
                // Shown — give the shell a brief moment to finish handing the
                // toast to the Action Center before we tear the icon down.
                Sleep(250);
                break;
            }
            if GetTickCount64() >= deadline {
                break;
            }
            Sleep(30);
        }

        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
        let _ = DestroyWindow(hwnd);
        true
    }
}
