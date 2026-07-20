//! ScrollView widget — custom scrollable container (WS_VSCROLL + manual scroll logic)

use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::{FillRect, HBRUSH};
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Controls::SetScrollInfo;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{register_widget_with_layout, WidgetKind};

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
static SCROLL_CLASS_REGISTERED: std::sync::Once = std::sync::Once::new();

struct ScrollState {
    scroll_offset: i32,
    content_height: i32,
    viewport_height: i32,
    content_child: Option<i64>,
}

thread_local! {
    static SCROLL_STATES: RefCell<HashMap<i64, ScrollState>> = RefCell::new(HashMap::new());
    /// Issue #553 — registered scroll-end callbacks keyed by scroll handle.
    static SCROLL_END_STATES: RefCell<HashMap<i64, ScrollEndEntry>> = RefCell::new(HashMap::new());
}

struct ScrollEndEntry {
    closure: f64,
    threshold_px: i32,
    armed: bool,
}

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

fn check_scroll_end(handle: i64) {
    let (closure_to_fire, _) = SCROLL_END_STATES.with(|s| {
        let mut states = s.borrow_mut();
        let Some(entry) = states.get_mut(&handle) else {
            return (0.0, false);
        };
        let (offset, content_height, viewport_height) = SCROLL_STATES.with(|ss| {
            let states = ss.borrow();
            states
                .get(&handle)
                .map(|st| (st.scroll_offset, st.content_height, st.viewport_height))
                .unwrap_or((0, 0, 0))
        });
        let visible_bottom = offset + viewport_height;
        let in_zone = visible_bottom >= content_height - entry.threshold_px;
        if in_zone && entry.armed {
            entry.armed = false;
            (entry.closure, true)
        } else if !in_zone && !entry.armed {
            entry.armed = true;
            (0.0, false)
        } else {
            (0.0, false)
        }
    });
    if closure_to_fire != 0.0 {
        unsafe {
            let ptr = js_nanbox_get_pointer(closure_to_fire) as *const u8;
            js_closure_call0(ptr);
        }
    }
}

/// Issue #553 — register a scroll-end callback. The check fires from
/// inside the existing WM_VSCROLL / WM_MOUSEWHEEL handlers via
/// `check_scroll_end(handle)` after the scroll offset is updated.
pub fn set_scroll_end_callback(scroll_handle: i64, callback: f64, threshold_px: f64) {
    SCROLL_END_STATES.with(|s| {
        s.borrow_mut().insert(
            scroll_handle,
            ScrollEndEntry {
                closure: callback,
                threshold_px: if threshold_px > 0.0 {
                    threshold_px as i32
                } else {
                    200
                },
                armed: true,
            },
        );
    });
}

#[cfg(target_os = "windows")]
fn ensure_class_registered() {
    SCROLL_CLASS_REGISTERED.call_once(|| unsafe {
        let hinstance = GetModuleHandleW(None).unwrap();
        let class_name = to_wide("PerryScrollView");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(scroll_wnd_proc),
            hInstance: HINSTANCE::from(hinstance),
            hbrBackground: HBRUSH(std::ptr::null_mut()),
            lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassExW(&wc);
    });
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn scroll_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_VSCROLL => {
            let handle = super::find_handle_by_hwnd(hwnd);
            if handle > 0 {
                handle_vscroll(handle, hwnd, wparam);
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let handle = super::find_handle_by_hwnd(hwnd);
            if handle > 0 {
                let delta = ((wparam.0 >> 16) as i16) as i32;
                let scroll_amount = -(delta / 120) * 40; // 40px per notch
                scroll_by(handle, hwnd, scroll_amount);
            }
            LRESULT(0)
        }
        WM_COMMAND | WM_CTLCOLORSTATIC | WM_CTLCOLORBTN | WM_CONTEXTMENU | WM_DRAWITEM => {
            if let Ok(parent) = GetParent(hwnd) {
                return SendMessageW(parent, msg, Some(wparam), Some(lparam));
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_ERASEBKGND | WM_PAINT => {
            let color =
                super::get_hwnd_bg_color(hwnd).or_else(|| super::find_ancestor_hwnd_bg_color(hwnd));
            if let Some(color) = color {
                let brush = windows::Win32::Graphics::Gdi::CreateSolidBrush(
                    windows::Win32::Foundation::COLORREF(color),
                );
                if msg == WM_ERASEBKGND {
                    let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut _);
                    let mut rect = RECT::default();
                    let _ = GetClientRect(hwnd, &mut rect);
                    let _ = FillRect(hdc, &rect, brush);
                    let _ = windows::Win32::Graphics::Gdi::DeleteObject(brush.into());
                    return LRESULT(1);
                } else {
                    let mut ps = windows::Win32::Graphics::Gdi::PAINTSTRUCT::default();
                    let hdc = windows::Win32::Graphics::Gdi::BeginPaint(hwnd, &mut ps);
                    let mut rect = RECT::default();
                    let _ = GetClientRect(hwnd, &mut rect);
                    let _ = FillRect(hdc, &rect, brush);
                    let _ = windows::Win32::Graphics::Gdi::DeleteObject(brush.into());
                    windows::Win32::Graphics::Gdi::EndPaint(hwnd, &ps);
                    return LRESULT(0);
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Walk the HWND parent chain to find the nearest ancestor with a background brush.
#[cfg(target_os = "windows")]
fn find_ancestor_brush(mut hwnd: HWND) -> Option<HBRUSH> {
    for _ in 0..10 {
        if let Ok(parent) = unsafe { GetParent(hwnd) } {
            if parent.0.is_null() {
                break;
            }
            let parent_handle = super::find_handle_by_hwnd(parent);
            if parent_handle > 0 {
                if let Some(brush) = super::get_bg_brush(parent_handle) {
                    return Some(brush);
                }
            }
            hwnd = parent;
        } else {
            break;
        }
    }
    None
}

/// Create a ScrollView. Returns widget handle.
pub fn create() -> i64 {
    #[cfg(target_os = "windows")]
    {
        ensure_class_registered();
        let class_name = to_wide("PerryScrollView");
        let window_text = to_wide("");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(window_text.as_ptr()),
                WS_CHILD | WS_VISIBLE | WS_VSCROLL | WS_CLIPCHILDREN,
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

            let handle = register_widget_with_layout(
                hwnd,
                WidgetKind::ScrollView,
                0.0,
                (0.0, 0.0, 0.0, 0.0),
            );
            SCROLL_STATES.with(|states| {
                states.borrow_mut().insert(
                    handle,
                    ScrollState {
                        scroll_offset: 0,
                        content_height: 0,
                        viewport_height: 0,
                        content_child: None,
                    },
                );
            });
            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let handle =
            register_widget_with_layout(0, WidgetKind::ScrollView, 0.0, (0.0, 0.0, 0.0, 0.0));
        SCROLL_STATES.with(|states| {
            states.borrow_mut().insert(
                handle,
                ScrollState {
                    scroll_offset: 0,
                    content_height: 0,
                    viewport_height: 0,
                    content_child: None,
                },
            );
        });
        handle
    }
}

/// Set the content child of a ScrollView.
pub fn set_child(scroll_handle: i64, child_handle: i64) {
    super::add_child(scroll_handle, child_handle);

    SCROLL_STATES.with(|states| {
        let mut states = states.borrow_mut();
        if let Some(state) = states.get_mut(&scroll_handle) {
            state.content_child = Some(child_handle);
        }
    });
}

/// Update scroll info after layout (called from layout engine).
pub fn update_scroll_info(handle: i64, viewport_height: i32, content_height: i32) {
    SCROLL_STATES.with(|states| {
        let mut states = states.borrow_mut();
        if let Some(state) = states.get_mut(&handle) {
            state.viewport_height = viewport_height;
            state.content_height = content_height;
        }
    });

    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            unsafe {
                let si = SCROLLINFO {
                    cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
                    fMask: SIF_RANGE | SIF_PAGE,
                    nMin: 0,
                    nMax: content_height - 1,
                    nPage: viewport_height as u32,
                    nPos: 0,
                    nTrackPos: 0,
                };
                SetScrollInfo(hwnd, SB_VERT, &si, true);
            }
        }
    }
}

/// Scroll a ScrollView to make a child visible.
pub fn scroll_to(scroll_handle: i64, child_handle: i64) {
    #[cfg(target_os = "windows")]
    {
        if let (Some(scroll_hwnd), Some(child_hwnd)) = (
            super::get_hwnd(scroll_handle),
            super::get_hwnd(child_handle),
        ) {
            unsafe {
                let mut child_rect = RECT::default();
                let mut scroll_rect = RECT::default();
                let _ = GetWindowRect(child_hwnd, &mut child_rect);
                let _ = GetWindowRect(scroll_hwnd, &mut scroll_rect);

                // Convert child position relative to scrollview
                let child_top = child_rect.top - scroll_rect.top;
                let child_bottom = child_rect.bottom - scroll_rect.top;

                let viewport_height = SCROLL_STATES.with(|states| {
                    states
                        .borrow()
                        .get(&scroll_handle)
                        .map(|s| s.viewport_height)
                        .unwrap_or(0)
                });

                let current_offset = get_offset(scroll_handle) as i32;

                if child_top < 0 {
                    // Child is above viewport — scroll up
                    set_offset(scroll_handle, (current_offset + child_top) as f64);
                } else if child_bottom > viewport_height {
                    // Child is below viewport — scroll down
                    set_offset(
                        scroll_handle,
                        (current_offset + child_bottom - viewport_height) as f64,
                    );
                }
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (scroll_handle, child_handle);
    }
}

/// Get the vertical scroll offset.
pub fn get_offset(scroll_handle: i64) -> f64 {
    SCROLL_STATES.with(|states| {
        states
            .borrow()
            .get(&scroll_handle)
            .map(|s| s.scroll_offset as f64)
            .unwrap_or(0.0)
    })
}

/// Geisterhand automation hook — same registered shape as the macOS
/// `perry_ui_scroll_set_offset` (handle, x, y). The Windows ScrollView is
/// vertical-only, so `x` is accepted for cross-platform harness parity
/// and ignored.
#[cfg(all(feature = "geisterhand", target_os = "windows"))]
#[no_mangle]
pub extern "C" fn perry_ui_scroll_set_offset(handle: i64, _x: f64, y: f64) {
    set_offset(handle, y);
}

/// Set the vertical scroll offset.
pub fn set_offset(scroll_handle: i64, offset: f64) {
    let new_offset = offset as i32;

    SCROLL_STATES.with(|states| {
        let mut states = states.borrow_mut();
        if let Some(state) = states.get_mut(&scroll_handle) {
            let max_offset = (state.content_height - state.viewport_height).max(0);
            state.scroll_offset = new_offset.clamp(0, max_offset);
        }
    });

    apply_scroll_offset(scroll_handle);
}

fn apply_scroll_offset(handle: i64) {
    let (offset, content_child) = SCROLL_STATES.with(|states| {
        let states = states.borrow();
        if let Some(state) = states.get(&handle) {
            (state.scroll_offset, state.content_child)
        } else {
            (0, None)
        }
    });

    #[cfg(target_os = "windows")]
    {
        if let Some(child) = content_child {
            if let Some(child_hwnd) = super::get_hwnd(child) {
                unsafe {
                    let _ = SetWindowPos(
                        child_hwnd,
                        None,
                        0,
                        -offset,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER,
                    );
                }
            }
        }

        // Update scrollbar position
        if let Some(hwnd) = super::get_hwnd(handle) {
            unsafe {
                let si = SCROLLINFO {
                    cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
                    fMask: SIF_POS,
                    nPos: offset,
                    ..Default::default()
                };
                SetScrollInfo(hwnd, SB_VERT, &si, true);
            }
        }
    }

    let _ = (offset, content_child);
}

#[cfg(target_os = "windows")]
fn handle_vscroll(handle: i64, hwnd: HWND, wparam: WPARAM) {
    let action = (wparam.0 & 0xFFFF) as u16;
    let current = SCROLL_STATES.with(|states| {
        states
            .borrow()
            .get(&handle)
            .map(|s| (s.scroll_offset, s.content_height, s.viewport_height))
    });

    if let Some((current_offset, content_height, viewport_height)) = current {
        let max_offset = (content_height - viewport_height).max(0);
        let line_size = 20;
        let page_size = viewport_height;

        let action = SCROLLBAR_COMMAND(action as i32);
        let new_offset = match action {
            SB_LINEUP => (current_offset - line_size).max(0),
            SB_LINEDOWN => (current_offset + line_size).min(max_offset),
            SB_PAGEUP => (current_offset - page_size).max(0),
            SB_PAGEDOWN => (current_offset + page_size).min(max_offset),
            SB_THUMBTRACK | SB_THUMBPOSITION => unsafe {
                let mut si = SCROLLINFO {
                    cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
                    fMask: SIF_TRACKPOS,
                    ..Default::default()
                };
                let _ = GetScrollInfo(hwnd, SB_VERT, &mut si);
                si.nTrackPos.clamp(0, max_offset)
            },
            SB_TOP => 0,
            SB_BOTTOM => max_offset,
            _ => current_offset,
        };

        set_offset(handle, new_offset as f64);
        check_scroll_end(handle);
    }
}

#[cfg(target_os = "windows")]
fn scroll_by(handle: i64, _hwnd: HWND, delta: i32) {
    let current = get_offset(handle) as i32;
    set_offset(handle, (current + delta) as f64);
    check_scroll_end(handle);
}
