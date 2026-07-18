//! Multi-window support (Win32)

use std::cell::RefCell;
use std::collections::HashMap;

fn debug_log(msg: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("perry_window_debug.log")
    {
        let _ = writeln!(f, "{}", msg);
    }
}

thread_local! {
    #[cfg(target_os = "windows")]
    static WINDOWS: RefCell<HashMap<i64, windows::Win32::Foundation::HWND>> = RefCell::new(HashMap::new());
    #[cfg(not(target_os = "windows"))]
    static WINDOWS: RefCell<HashMap<i64, isize>> = RefCell::new(HashMap::new());
    static NEXT_WINDOW_ID: RefCell<i64> = RefCell::new(1);
    /// Maps window id → root widget handle so we can re-layout on resize.
    static WINDOW_ROOTS: RefCell<HashMap<i64, i64>> = RefCell::new(HashMap::new());
    /// Reverse map: HWND (as isize) → window id, for the wndproc.
    #[cfg(target_os = "windows")]
    static HWND_TO_WINDOW: RefCell<HashMap<isize, i64>> = RefCell::new(HashMap::new());
}

/// True when `hwnd` is a secondary Perry `Window(...)` top-level. Used by
/// the message pump's dialog-navigation gate (Tab focus movement must also
/// engage while focus sits on the window itself, before any control has
/// been focused).
#[cfg(target_os = "windows")]
pub fn is_perry_window_hwnd(hwnd_val: isize) -> bool {
    HWND_TO_WINDOW.with(|m| m.borrow().contains_key(&hwnd_val))
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

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn window_default_wnd_proc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::Foundation::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    match msg {
        WM_SIZE => {
            let width = (lparam.0 & 0xFFFF) as i32;
            let height = ((lparam.0 >> 16) & 0xFFFF) as i32;
            let window_id = HWND_TO_WINDOW.with(|m| m.borrow().get(&(hwnd.0 as isize)).copied());
            if let Some(wid) = window_id {
                let root = WINDOW_ROOTS.with(|m| m.borrow().get(&wid).copied());
                if let Some(root_handle) = root {
                    if let Some(child_hwnd) = crate::widgets::get_hwnd(root_handle) {
                        let _ = MoveWindow(child_hwnd, 0, 0, width, height, true);
                        crate::layout::layout_widget(root_handle, width, height);
                    }
                }
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let control_id = (wparam.0 & 0xFFFF) as u16;
            let notify_code = ((wparam.0 >> 16) & 0xFFFF) as u16;
            crate::widgets::handle_command(control_id, notify_code, lparam);
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC => {
            use windows::Win32::Graphics::Gdi::HDC;
            let hdc = HDC(wparam.0 as *mut _);
            let child_hwnd = HWND(lparam.0 as *mut _);
            if let Some(result) = crate::widgets::text::handle_ctlcolor(hdc, child_hwnd) {
                return result;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        x if x == 0x0133 /* WM_CTLCOLOREDIT */ => {
            use windows::Win32::Graphics::Gdi::HDC;
            let hdc = HDC(wparam.0 as *mut _);
            let child_hwnd = HWND(lparam.0 as *mut _);
            if let Some(result) = crate::widgets::text::handle_ctlcolor(hdc, child_hwnd) {
                return result;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Create a new window.
pub fn create(title_ptr: *const u8, width: f64, height: f64) -> i64 {
    let title = str_from_header(title_ptr);
    let id = NEXT_WINDOW_ID.with(|id| {
        let mut id = id.borrow_mut();
        let current = *id;
        *id += 1;
        current
    });

    #[cfg(target_os = "windows")]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::*;
        use windows::Win32::Graphics::Gdi::{UpdateWindow, COLOR_WINDOW, HBRUSH};
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::*;

        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let class_name = to_wide("PerryWindow");

            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(window_default_wnd_proc),
                hInstance: hinstance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as *mut _),
                lpszClassName: PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };
            RegisterClassExW(&wc);

            let title_wide = to_wide(title);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(class_name.as_ptr()),
                PCWSTR(title_wide.as_ptr()),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                width as i32,
                height as i32,
                None,
                None,
                Some(HINSTANCE::from(hinstance)),
                None,
            )
            .unwrap();

            // Modern Win11 chrome by default (rounded corners + theme-aware
            // title bar) to match the main app window — issue #4681. Ignored
            // on Windows 10 and earlier.
            crate::dwm::apply_default_window_chrome(hwnd);

            HWND_TO_WINDOW.with(|m| m.borrow_mut().insert(hwnd.0 as isize, id));
            WINDOWS.with(|w| w.borrow_mut().insert(id, hwnd));
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (title, width, height);
        WINDOWS.with(|w| w.borrow_mut().insert(id, 0));
    }

    id
}

/// Set the body (root widget) of a window.
pub fn set_body(window_handle: i64, widget_handle: i64) {
    debug_log(&format!(
        "[perry-window] set_body: window={} widget={}",
        window_handle, widget_handle
    ));
    WINDOW_ROOTS.with(|m| m.borrow_mut().insert(window_handle, widget_handle));

    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::*;
        use windows::Win32::UI::WindowsAndMessaging::*;
        WINDOWS.with(|w| {
            if let Some(parent_hwnd) = w.borrow().get(&window_handle) {
                if let Some(child_hwnd) = crate::widgets::get_hwnd(widget_handle) {
                    unsafe {
                        let _ = SetParent(child_hwnd, Some(*parent_hwnd));
                        let style = GetWindowLongW(child_hwnd, GWL_STYLE) as u32;
                        SetWindowLongW(child_hwnd, GWL_STYLE, (style | WS_CHILD.0) as i32);
                        // Trigger initial layout (mirrors app_set_body)
                        let mut rect = RECT::default();
                        let _ = GetClientRect(*parent_hwnd, &mut rect);
                        debug_log(&format!(
                            "[perry-window] set_body layout: rect={}x{} hwnd={:?}",
                            rect.right, rect.bottom, parent_hwnd
                        ));
                        let _ = MoveWindow(child_hwnd, 0, 0, rect.right, rect.bottom, true);
                        crate::layout::layout_widget(widget_handle, rect.right, rect.bottom);
                    }
                } else {
                    debug_log(&format!(
                        "[perry-window] set_body: no child hwnd for widget {}",
                        widget_handle
                    ));
                }
            } else {
                debug_log(&format!(
                    "[perry-window] set_body: no parent hwnd for window {}",
                    window_handle
                ));
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window_handle, widget_handle);
    }
}

/// Show a window.
pub fn show(window_handle: i64) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::*;
        use windows::Win32::Graphics::Gdi::UpdateWindow;
        use windows::Win32::UI::WindowsAndMessaging::*;
        WINDOWS.with(|w| {
            if let Some(hwnd) = w.borrow().get(&window_handle) {
                unsafe {
                    let _ = ShowWindow(*hwnd, SW_SHOW);
                    // Re-layout in case set_body was called before the window was visible
                    let root = WINDOW_ROOTS.with(|m| m.borrow().get(&window_handle).copied());
                    debug_log(&format!(
                        "[perry-window] show: window={} root={:?}",
                        window_handle, root
                    ));
                    if let Some(root_handle) = root {
                        if let Some(child_hwnd) = crate::widgets::get_hwnd(root_handle) {
                            let mut rect = RECT::default();
                            let _ = GetClientRect(*hwnd, &mut rect);
                            debug_log(&format!(
                                "[perry-window] show layout: rect={}x{}",
                                rect.right, rect.bottom
                            ));
                            let _ = MoveWindow(child_hwnd, 0, 0, rect.right, rect.bottom, true);
                            crate::layout::layout_widget(root_handle, rect.right, rect.bottom);
                        }
                    }
                    let _ = UpdateWindow(*hwnd);
                }
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window_handle;
    }
}

/// Close a window.
pub fn close(window_handle: i64) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::*;
        WINDOWS.with(|w| {
            if let Some(hwnd) = w.borrow().get(&window_handle) {
                unsafe {
                    let _ = DestroyWindow(*hwnd);
                }
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window_handle;
    }
}

thread_local! {
    pub(crate) static FOCUS_LOST_CALLBACKS: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
}

/// Hide a window without destroying it.
pub fn hide(window_handle: i64) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::*;
        WINDOWS.with(|w| {
            if let Some(hwnd) = w.borrow().get(&window_handle) {
                unsafe {
                    let _ = ShowWindow(*hwnd, SW_HIDE);
                }
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window_handle;
    }
}

/// Set window size.
pub fn set_size(window_handle: i64, width: f64, height: f64) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::*;
        WINDOWS.with(|w| {
            if let Some(hwnd) = w.borrow().get(&window_handle) {
                unsafe {
                    let _ = SetWindowPos(
                        *hwnd,
                        None,
                        0,
                        0,
                        width as i32,
                        height as i32,
                        SWP_NOMOVE | SWP_NOZORDER,
                    );
                }
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window_handle, width, height);
    }
}

/// Register a callback for focus loss. Store it and handle in wndproc WM_ACTIVATE.
pub fn on_focus_lost(window_handle: i64, callback: f64) {
    FOCUS_LOST_CALLBACKS.with(|cbs| {
        cbs.borrow_mut().insert(window_handle, callback);
    });
}
