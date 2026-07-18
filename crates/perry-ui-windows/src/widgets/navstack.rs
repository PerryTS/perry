//! NavStack widget — container with push/pop page navigation via show/hide children

use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::HBRUSH;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{register_widget_with_layout, WidgetKind};

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
static NAVSTACK_CLASS_REGISTERED: std::sync::Once = std::sync::Once::new();

#[cfg(target_os = "windows")]
fn ensure_class_registered() {
    NAVSTACK_CLASS_REGISTERED.call_once(|| unsafe {
        let hinstance = GetModuleHandleW(None).unwrap();
        let class_name = to_wide("PerryNavStack");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(navstack_wnd_proc),
            hInstance: hinstance.into(),
            hbrBackground: HBRUSH(std::ptr::null_mut()),
            lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassExW(&wc);
    });
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn navstack_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

struct NavPage {
    title: String,
    body_handle: i64,
}

struct NavStackState {
    pages: Vec<NavPage>,
}

thread_local! {
    static NAV_STACKS: RefCell<HashMap<i64, NavStackState>> = RefCell::new(HashMap::new());
}

/// Create a NavStack with an initial page. Returns widget handle.
/// title_ptr = title for the initial page, body_handle = widget handle of the initial page body.
pub fn create(title_ptr: *const u8, body_handle: i64) -> i64 {
    let title = str_from_header(title_ptr).to_string();

    #[cfg(target_os = "windows")]
    {
        ensure_class_registered();
        let class_name = to_wide("PerryNavStack");
        let window_text = to_wide("");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                // WS_EX_CONTROLPARENT: dialog manager recurses into
                // containers for Tab navigation (see vstack.rs).
                WINDOW_EX_STYLE(WS_EX_CONTROLPARENT.0),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(window_text.as_ptr()),
                WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN,
                0,
                0,
                100,
                100,
                Some(super::get_parking_hwnd()),
                None,
                Some(HINSTANCE::from(hinstance)),
                None,
            )
            .unwrap();

            let handle =
                register_widget_with_layout(hwnd, WidgetKind::NavStack, 0.0, (0.0, 0.0, 0.0, 0.0));

            // Add the initial body as a child
            super::add_child(handle, body_handle);

            NAV_STACKS.with(|stacks| {
                stacks.borrow_mut().insert(
                    handle,
                    NavStackState {
                        pages: vec![NavPage { title, body_handle }],
                    },
                );
            });

            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let handle =
            register_widget_with_layout(0, WidgetKind::NavStack, 0.0, (0.0, 0.0, 0.0, 0.0));

        super::add_child(handle, body_handle);

        NAV_STACKS.with(|stacks| {
            stacks.borrow_mut().insert(
                handle,
                NavStackState {
                    pages: vec![NavPage { title, body_handle }],
                },
            );
        });

        handle
    }
}

/// Push a new page onto the NavStack. Hides the current page and shows the new one.
pub fn push(handle: i64, title_ptr: *const u8, body_handle: i64) {
    let title = str_from_header(title_ptr).to_string();

    // Hide the current top page
    NAV_STACKS.with(|stacks| {
        let mut stacks = stacks.borrow_mut();
        if let Some(state) = stacks.get_mut(&handle) {
            if let Some(current) = state.pages.last() {
                super::set_hidden(current.body_handle, true);
            }
            state.pages.push(NavPage { title, body_handle });
        }
    });

    // Add new body as child and show it
    super::add_child(handle, body_handle);

    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(body_handle) {
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOW);
            }
        }
    }
}

/// Pop the top page from the NavStack. Hides the current page and shows the previous one.
pub fn pop(handle: i64) {
    NAV_STACKS.with(|stacks| {
        let mut stacks = stacks.borrow_mut();
        if let Some(state) = stacks.get_mut(&handle) {
            if state.pages.len() <= 1 {
                return; // Cannot pop the last page
            }

            // Hide and remove the current top page
            if let Some(popped) = state.pages.pop() {
                super::set_hidden(popped.body_handle, true);
            }

            // Show the new top page
            if let Some(current) = state.pages.last() {
                super::set_hidden(current.body_handle, false);
            }
        }
    });
}
