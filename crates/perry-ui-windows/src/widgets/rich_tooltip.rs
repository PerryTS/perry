//! Rich Tooltip (issue #479) — Win32 popup HWND hosting an arbitrary
//! widget tree, shown on hover after a configurable delay.
//!
//! macOS uses NSPopover; the Win32 equivalent is a popup-style HWND
//! parented to the trigger widget's top-level window. We subclass
//! the trigger HWND to install `TrackMouseEvent` so WM_MOUSEHOVER /
//! WM_MOUSELEAVE arrive correctly, then re-parent the content
//! widget into the popup on first show.

use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use windows::core::PCWSTR;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::{COLOR_WINDOW, HBRUSH};
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    TrackMouseEvent, TME_HOVER, TME_LEAVE, TRACKMOUSEEVENT,
};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy)]
struct Tooltip {
    content_handle: i64,
    delay_ms: u32,
    #[cfg(target_os = "windows")]
    popup: Option<HWND>,
}

thread_local! {
    static TOOLTIPS: RefCell<HashMap<i64, Tooltip>> = RefCell::new(HashMap::new());
    #[cfg(target_os = "windows")]
    static SUBCLASSED: RefCell<std::collections::HashSet<isize>> =
        RefCell::new(std::collections::HashSet::new());
}

#[cfg(target_os = "windows")]
const TIP_SUBCLASS_ID: usize = 0x72_74_69_70; // 'r','t','i','p'

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn set_rich_tooltip(handle: i64, content_handle: i64, hover_delay_ms: f64) {
    let delay = hover_delay_ms.max(0.0) as u32;
    TOOLTIPS.with(|t| {
        t.borrow_mut().insert(
            handle,
            Tooltip {
                content_handle,
                delay_ms: delay,
                #[cfg(target_os = "windows")]
                popup: None,
            },
        );
    });

    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            ensure_subclass(hwnd);
        }
    }
}

#[cfg(target_os = "windows")]
fn ensure_subclass(hwnd: HWND) {
    use windows::Win32::UI::Shell::SetWindowSubclass;
    let key = hwnd.0 as isize;
    let installed = SUBCLASSED.with(|s| s.borrow().contains(&key));
    if !installed {
        unsafe {
            let _ = SetWindowSubclass(hwnd, Some(tooltip_subclass_proc), TIP_SUBCLASS_ID, 0);
        }
        SUBCLASSED.with(|s| {
            s.borrow_mut().insert(key);
        });
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn tooltip_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _refdata: usize,
) -> LRESULT {
    use windows::Win32::UI::Shell::DefSubclassProc;

    match msg {
        WM_MOUSEMOVE => {
            // Arm hover tracking on first move within the trigger.
            let handle = super::find_handle_by_hwnd(hwnd);
            if handle > 0 {
                let delay =
                    TOOLTIPS.with(|t| t.borrow().get(&handle).map(|tip| tip.delay_ms).unwrap_or(0));
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_HOVER | TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: delay.max(150),
                };
                let _ = TrackMouseEvent(&mut tme);
            }
        }
        // WM_MOUSEHOVER = 0x02A1
        x if x == 0x02A1 => {
            let handle = super::find_handle_by_hwnd(hwnd);
            if handle > 0 {
                show_popup(handle);
            }
        }
        // WM_MOUSELEAVE = 0x02A3
        x if x == 0x02A3 => {
            let handle = super::find_handle_by_hwnd(hwnd);
            if handle > 0 {
                hide_popup(handle);
            }
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

#[cfg(target_os = "windows")]
fn show_popup(trigger_handle: i64) {
    let (content_handle, existing_popup) = TOOLTIPS.with(|t| {
        t.borrow()
            .get(&trigger_handle)
            .map(|tip| (tip.content_handle, tip.popup))
            .unwrap_or((0, None))
    });
    if content_handle <= 0 {
        return;
    }
    // If already showing, just bring to front.
    if let Some(popup) = existing_popup {
        unsafe {
            let _ = ShowWindow(popup, SW_SHOW);
        }
        return;
    }

    let trigger_hwnd = match super::get_hwnd(trigger_handle) {
        Some(h) => h,
        None => return,
    };
    let content_hwnd = match super::get_hwnd(content_handle) {
        Some(h) => h,
        None => return,
    };

    unsafe {
        // Compute popup position — under the trigger, aligned to its left edge.
        let mut trigger_rect = RECT::default();
        let _ = GetWindowRect(trigger_hwnd, &mut trigger_rect);
        let popup_x = trigger_rect.left;
        let popup_y = trigger_rect.bottom + 4;

        // Get content size.
        let mut content_rect = RECT::default();
        let _ = GetWindowRect(content_hwnd, &mut content_rect);
        let popup_w = (content_rect.right - content_rect.left).max(160);
        let popup_h = (content_rect.bottom - content_rect.top).max(60);

        let hinstance = GetModuleHandleW(None).unwrap();
        unsafe extern "system" fn tip_popup_wnd_proc(
            hwnd: HWND,
            msg: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> LRESULT {
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }

        let class_name = to_wide("PerryRichTooltip");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(tip_popup_wnd_proc),
            hInstance: hinstance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as *mut _),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let title = to_wide("");
        let popup = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_POPUP | WS_BORDER,
            popup_x,
            popup_y,
            popup_w + 16,
            popup_h + 16,
            None,
            None,
            HINSTANCE::from(hinstance),
            None,
        );
        let popup = match popup {
            Ok(h) => h,
            Err(_) => return,
        };

        // Re-parent the content widget into the popup.
        let _ = SetParent(content_hwnd, popup);
        let _ = SetWindowPos(content_hwnd, None, 8, 8, popup_w, popup_h, SWP_NOACTIVATE);

        let _ = ShowWindow(popup, SW_SHOWNOACTIVATE);

        TOOLTIPS.with(|t| {
            if let Some(tip) = t.borrow_mut().get_mut(&trigger_handle) {
                tip.popup = Some(popup);
            }
        });
    }
}

#[cfg(target_os = "windows")]
fn hide_popup(trigger_handle: i64) {
    TOOLTIPS.with(|t| {
        if let Some(tip) = t.borrow_mut().get_mut(&trigger_handle) {
            if let Some(popup) = tip.popup.take() {
                unsafe {
                    // Detach the content widget back to the parking HWND so
                    // subsequent shows can re-parent cleanly.
                    let content_hwnd = super::get_hwnd(tip.content_handle);
                    if let Some(content) = content_hwnd {
                        let _ = SetParent(content, super::get_parking_hwnd());
                    }
                    let _ = DestroyWindow(popup);
                }
            }
        }
    });
}
