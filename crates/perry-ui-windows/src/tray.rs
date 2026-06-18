//! System tray icon (issue #490) — Win32 `Shell_NotifyIconW` notification-area
//! icon plus a popup `HMENU` driven by `TrackPopupMenu`.
//!
//! Mirrors `crates/perry-ui-macos/src/tray.rs`. Handle-based dispatch: 1-based
//! indices into a thread-local `Vec<TrayEntry>`. The shell sends mouse events
//! back to us via a per-tray callback message (`WM_USER + 200`). With the
//! legacy v0 semantics (no `NIM_SETVERSION` call), `wParam` carries the
//! tray's `uID` and `lParam` carries the actual mouse event ID
//! (`WM_LBUTTONUP` etc.). The app `WndProc` (see `app.rs`) forwards that
//! message into `handle_callback_message`, which dispatches left-click → JS
//! callback, right-click → `TrackPopupMenu` on the attached menu's `HMENU`.

use std::cell::RefCell;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{HWND, POINT};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, GetCursorPos, GetSystemMetrics, LoadIconW, LoadImageW, SetForegroundWindow,
    TrackPopupMenu, HICON, IDI_APPLICATION, IMAGE_ICON, LR_DEFAULTCOLOR, LR_LOADFROMFILE,
    SM_CXSMICON, SM_CYSMICON, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    WM_LBUTTONUP, WM_RBUTTONUP,
};

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

/// Per-tray callback message. Multiple trays multiplex through the same
/// message ID — we disambiguate by the `uID` we registered (legacy v0
/// semantics: `wParam` low word = uID).
#[cfg(target_os = "windows")]
pub const WM_PERRY_TRAY: u32 = windows::Win32::UI::WindowsAndMessaging::WM_USER + 200;

/// Extract a &str from a *const StringHeader pointer. Mirrors menu.rs.
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

struct TrayEntry {
    /// Notification-area icon ID (the `uID` field of `NOTIFYICONDATAW`).
    /// Unique within the process; we hand them out monotonically starting at 1.
    uid: u32,
    /// HWND that receives the per-tray callback message — always the app's
    /// main HWND so WM_PERRY_TRAY routes back into our WndProc.
    #[cfg(target_os = "windows")]
    hwnd: HWND,
    #[cfg(not(target_os = "windows"))]
    hwnd: isize,
    /// Currently-installed icon handle (we own it and `DestroyIcon` on
    /// replacement / destroy). May be null if the user passed an empty
    /// path AND the system default `IDI_APPLICATION` lookup failed.
    #[cfg(target_os = "windows")]
    hicon: HICON,
    #[cfg(not(target_os = "windows"))]
    hicon: isize,
    /// Attached menu handle (via `trayAttachMenu`) — looked up in `menu.rs`'s
    /// MENUS storage at click time. Stored as the menu *handle*, not the
    /// HMENU, so re-using `menu.rs`'s lookup keeps a single source of truth.
    menu_handle: i64,
    /// JS click callback as a raw closure pointer (NaN-unbox already applied).
    callback_ptr: *const u8,
    /// Has Shell_NotifyIcon(NIM_ADD) succeeded? Set false after NIM_DELETE so
    /// repeat destroys are no-ops.
    alive: bool,
}

thread_local! {
    static TRAYS: RefCell<Vec<TrayEntry>> = RefCell::new(Vec::new());
    /// Monotonic uID for `NOTIFYICONDATAW.uID`. Starts at 1 — `0` is fine on
    /// Windows but reserving it matches our handle convention.
    static NEXT_TRAY_UID: std::cell::Cell<u32> = std::cell::Cell::new(1);
}

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Load an icon from a file path. Empty path → fall back to `IDI_APPLICATION`.
/// Returns a freshly-owned `HICON` (caller is responsible for `DestroyIcon`).
#[cfg(target_os = "windows")]
fn load_tray_icon(path: &str) -> HICON {
    unsafe {
        let cx = GetSystemMetrics(SM_CXSMICON);
        let cy = GetSystemMetrics(SM_CYSMICON);

        if !path.is_empty() {
            let wide = to_wide(path);
            // LoadImageW with LR_LOADFROMFILE handles both .ico and .png on
            // modern Windows (Vista+ icon format support is broad enough that
            // a 32x32 PNG works in practice — same trick Electron uses).
            if let Ok(handle) = LoadImageW(
                None,
                windows::core::PCWSTR(wide.as_ptr()),
                IMAGE_ICON,
                cx,
                cy,
                LR_LOADFROMFILE | LR_DEFAULTCOLOR,
            ) {
                if !handle.is_invalid() {
                    return HICON(handle.0);
                }
            }
        }

        // Fallback — system default. LoadIconW(NULL, IDI_APPLICATION) returns
        // a shared icon that we should NOT DestroyIcon, but the SDK is
        // tolerant of the call (it's a no-op on shared icons), and tracking
        // ownership separately complicates `set_icon` for negligible gain.
        LoadIconW(None, IDI_APPLICATION).unwrap_or(HICON(std::ptr::null_mut()))
    }
}

/// Create a tray icon. Returns 1-based handle, or 0 on failure
/// (no main app HWND yet — `trayCreate` must be called after `appCreate`).
pub fn create(icon_path_ptr: *const u8) -> i64 {
    let path = str_from_header(icon_path_ptr);

    #[cfg(target_os = "windows")]
    {
        let hwnd = match crate::app::get_main_hwnd() {
            Some(h) => h,
            None => {
                eprintln!(
                    "[perry] warning: trayCreate called before appCreate — \
                     no main HWND to receive tray callbacks. Returning 0."
                );
                return 0;
            }
        };

        let uid = NEXT_TRAY_UID.with(|c| {
            let v = c.get();
            c.set(v + 1);
            v
        });

        let hicon = load_tray_icon(path);

        unsafe {
            let mut data = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: uid,
                uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
                uCallbackMessage: WM_PERRY_TRAY,
                hIcon: hicon,
                ..Default::default()
            };
            // Tooltip starts empty — `trayTooltip` updates it later.
            data.szTip[0] = 0;

            let ok = Shell_NotifyIconW(NIM_ADD, &data).as_bool();
            if !ok {
                eprintln!("[perry] warning: Shell_NotifyIconW(NIM_ADD) failed");
                if !hicon.is_invalid() {
                    let _ = DestroyIcon(hicon);
                }
                return 0;
            }

            TRAYS.with(|t| {
                let mut trays = t.borrow_mut();
                trays.push(TrayEntry {
                    uid,
                    hwnd,
                    hicon,
                    menu_handle: 0,
                    callback_ptr: std::ptr::null(),
                    alive: true,
                });
                trays.len() as i64
            })
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        TRAYS.with(|t| {
            let mut trays = t.borrow_mut();
            trays.push(TrayEntry {
                uid: 0,
                hwnd: 0,
                hicon: 0,
                menu_handle: 0,
                callback_ptr: std::ptr::null(),
                alive: false,
            });
            trays.len() as i64
        })
    }
}

pub fn set_icon(handle: i64, icon_path_ptr: *const u8) {
    let path = str_from_header(icon_path_ptr);

    #[cfg(target_os = "windows")]
    {
        TRAYS.with(|t| {
            let mut trays = t.borrow_mut();
            let idx = (handle - 1) as usize;
            let entry = match trays.get_mut(idx) {
                Some(e) if e.alive => e,
                _ => return,
            };

            let new_icon = load_tray_icon(path);
            let old_icon = entry.hicon;

            unsafe {
                let data = NOTIFYICONDATAW {
                    cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                    hWnd: entry.hwnd,
                    uID: entry.uid,
                    uFlags: NIF_ICON,
                    hIcon: new_icon,
                    ..Default::default()
                };
                let _ = Shell_NotifyIconW(NIM_MODIFY, &data);

                if !old_icon.is_invalid() {
                    let _ = DestroyIcon(old_icon);
                }
            }

            entry.hicon = new_icon;
        });
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, path);
    }
}

pub fn set_tooltip(handle: i64, tooltip_ptr: *const u8) {
    let tooltip = str_from_header(tooltip_ptr);

    #[cfg(target_os = "windows")]
    {
        TRAYS.with(|t| {
            let trays = t.borrow();
            let idx = (handle - 1) as usize;
            let entry = match trays.get(idx) {
                Some(e) if e.alive => e,
                _ => return,
            };

            unsafe {
                let mut data = NOTIFYICONDATAW {
                    cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                    hWnd: entry.hwnd,
                    uID: entry.uid,
                    uFlags: NIF_TIP,
                    ..Default::default()
                };
                // Truncate to 127 UTF-16 code units (szTip is [u16; 128] with
                // a NUL terminator). Long tooltips silently truncate — same
                // policy as Explorer.
                let wide: Vec<u16> = tooltip.encode_utf16().take(127).collect();
                for (i, &c) in wide.iter().enumerate() {
                    data.szTip[i] = c;
                }
                data.szTip[wide.len()] = 0;

                let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
            }
        });
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, tooltip);
    }
}

pub fn attach_menu(tray_handle: i64, menu_handle: i64) {
    TRAYS.with(|t| {
        let mut trays = t.borrow_mut();
        let idx = (tray_handle - 1) as usize;
        if let Some(entry) = trays.get_mut(idx) {
            entry.menu_handle = menu_handle;
        }
    });
}

pub fn on_click(tray_handle: i64, callback: f64) {
    let callback_ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;
    TRAYS.with(|t| {
        let mut trays = t.borrow_mut();
        let idx = (tray_handle - 1) as usize;
        if let Some(entry) = trays.get_mut(idx) {
            entry.callback_ptr = callback_ptr;
        }
    });
}

pub fn destroy(handle: i64) {
    #[cfg(target_os = "windows")]
    {
        TRAYS.with(|t| {
            let mut trays = t.borrow_mut();
            let idx = (handle - 1) as usize;
            let entry = match trays.get_mut(idx) {
                Some(e) if e.alive => e,
                _ => return,
            };

            unsafe {
                let data = NOTIFYICONDATAW {
                    cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                    hWnd: entry.hwnd,
                    uID: entry.uid,
                    ..Default::default()
                };
                let _ = Shell_NotifyIconW(NIM_DELETE, &data);

                if !entry.hicon.is_invalid() {
                    let _ = DestroyIcon(entry.hicon);
                    entry.hicon = HICON(std::ptr::null_mut());
                }
            }

            entry.alive = false;
            // Slot is kept (matches macOS / menu / widget convention) so
            // existing handle indices stay stable.
        });
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}

/// Dispatched from `app::wnd_proc` when `msg == WM_PERRY_TRAY`. The low word
/// of `wparam` is the tray's `uID`; the low word of `lparam` carries the
/// mouse event (`WM_LBUTTONUP` / `WM_RBUTTONUP` / etc.).
///
/// On left-click → fires the JS callback registered via `on_click`.
/// On right-click → pops the attached menu via `TrackPopupMenu` and routes
/// the selected command through `menu::dispatch_menu_item` (same path
/// `WM_CONTEXTMENU` already uses for widget menus).
#[cfg(target_os = "windows")]
pub fn handle_callback_message(wparam: usize, lparam: isize) {
    // Legacy v0 semantics (we never call NIM_SETVERSION, so the shell sends
    // us the pre-XP layout): wParam = uID, lParam = mouse-message ID. The
    // mask-with-0xFFFF on `lparam` is defensive against high bits — the
    // mouse-message values themselves all fit in 16 bits.
    let uid = wparam as u32;
    let mouse_event = (lparam & 0xFFFF) as u32;

    let (callback_ptr, menu_handle, hwnd) = TRAYS.with(|t| {
        let trays = t.borrow();
        for entry in trays.iter() {
            if entry.alive && entry.uid == uid {
                return (entry.callback_ptr, entry.menu_handle, entry.hwnd);
            }
        }
        (std::ptr::null(), 0i64, HWND(std::ptr::null_mut()))
    });

    match mouse_event {
        WM_LBUTTONUP => {
            if !callback_ptr.is_null() {
                unsafe {
                    js_closure_call0(callback_ptr);
                }
            }
        }
        WM_RBUTTONUP => {
            if menu_handle != 0 {
                if let Some(hmenu) = crate::menu::get_hmenu(menu_handle) {
                    unsafe {
                        let mut pt = POINT::default();
                        let _ = GetCursorPos(&mut pt);
                        // MSDN: must SetForegroundWindow before TrackPopupMenu
                        // or the menu won't dismiss properly when the user
                        // clicks elsewhere. The PostMessage(WM_NULL, ...)
                        // dance after is the documented workaround for the
                        // same dismissal bug.
                        let _ = SetForegroundWindow(hwnd);
                        let result = TrackPopupMenu(
                            hmenu,
                            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_LEFTALIGN | TPM_BOTTOMALIGN,
                            pt.x,
                            pt.y,
                            Some(0),
                            hwnd,
                            None,
                        );
                        if result.as_bool() {
                            let selected_id = result.0 as u16;
                            crate::menu::dispatch_menu_item(selected_id);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

#[cfg(not(target_os = "windows"))]
pub fn handle_callback_message(_wparam: usize, _lparam: isize) {}
