//! BottomNavigation widget — Win32 implementation as a horizontal
//! row of BUTTON child controls inside an HSTACK-style container.
//!
//! macOS uses NSStackView with NSButton items styled as a tab bar; the
//! Win32 equivalent is an owner-drawn STATIC parent hosting one BUTTON
//! per item. Selection is tracked in a registry; clicking an item
//! fires `on_select(index)` and updates the visible "selected" style by
//! re-applying the bordered/unbordered state to each button.
//!
//! Badge support is text-suffix based (`Label (3)`) — Windows BUTTON
//! controls don't have a native badge view, and a custom owner-drawn
//! pass to render a colored circle would land in a follow-up.

use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{alloc_control_id, register_widget, WidgetKind};

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

#[derive(Clone)]
struct NavItem {
    icon: String,
    label: String,
    badge: String,
    /// Per-item BUTTON HWND.
    #[cfg(target_os = "windows")]
    btn_hwnd: HWND,
    #[cfg(not(target_os = "windows"))]
    btn_hwnd: isize,
    btn_control_id: u16,
}

struct NavEntry {
    handle: i64,
    items: Vec<NavItem>,
    selected: i64,
    on_select: f64,
    /// Issue #706 — packed COLORREF (0x00BBGGRR) for the active tab.
    /// Windows standard BUTTON controls do NOT honor `WM_CTLCOLORBTN`
    /// text color (the visual style draws them with system colors), so
    /// the field is wired through and persisted on the entry, ready
    /// for a follow-up owner-drawn rewrite that respects it. Stored
    /// state still flows correctly through codegen and back, and
    /// matches the macOS/iOS/Android impl shape.
    selected_tint: Option<u32>,
    /// Issue #706 — packed COLORREF for inactive tabs.
    unselected_tint: Option<u32>,
}

thread_local! {
    static NAVS: RefCell<HashMap<i64, NavEntry>> = RefCell::new(HashMap::new());
    /// Map per-item button control_id → (parent_handle, item_index) for
    /// `handle_command` to route back to the right callback.
    static ITEM_LOOKUP: RefCell<HashMap<u16, (i64, i64)>> = RefCell::new(HashMap::new());
}

fn str_from_header(ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len)).to_string()
    }
}

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn create(on_select: f64) -> i64 {
    let control_id = alloc_control_id();

    #[cfg(target_os = "windows")]
    {
        let class_name = to_wide("STATIC");
        let window_text = to_wide("");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(window_text.as_ptr()),
                WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0),
                0,
                0,
                400,
                56,
                Some(super::get_parking_hwnd()),
                Some(HMENU(control_id as *mut _)),
                Some(HINSTANCE::from(hinstance)),
                None,
            )
            .unwrap();

            let handle = register_widget(hwnd, WidgetKind::HStack, control_id);
            NAVS.with(|n| {
                n.borrow_mut().insert(
                    handle,
                    NavEntry {
                        handle,
                        items: Vec::new(),
                        selected: -1,
                        on_select,
                        selected_tint: None,
                        unselected_tint: None,
                    },
                );
            });
            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = on_select;
        let handle = register_widget(0, WidgetKind::HStack, control_id);
        NAVS.with(|n| {
            n.borrow_mut().insert(
                handle,
                NavEntry {
                    handle,
                    items: Vec::new(),
                    selected: -1,
                    on_select,
                    selected_tint: None,
                    unselected_tint: None,
                },
            );
        });
        handle
    }
}

pub fn add_item(handle: i64, icon_ptr: *const u8, label_ptr: *const u8) {
    let icon = str_from_header(icon_ptr);
    let label = str_from_header(label_ptr);

    #[cfg(target_os = "windows")]
    {
        let parent_hwnd = match super::get_hwnd(handle) {
            Some(h) => h,
            None => return,
        };
        let item_id = alloc_control_id();
        unsafe {
            let class_name = to_wide("BUTTON");
            let display = if icon.is_empty() {
                label.clone()
            } else {
                format!("{} {}", icon, label)
            };
            let display_wide = to_wide(&display);
            let hinstance = GetModuleHandleW(None).unwrap();
            let btn = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(display_wide.as_ptr()),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                0,
                0,
                80,
                48,
                Some(parent_hwnd),
                Some(HMENU(item_id as *mut _)),
                Some(HINSTANCE::from(hinstance)),
                None,
            );
            let Ok(btn) = btn else { return };

            NAVS.with(|n| {
                let mut navs = n.borrow_mut();
                if let Some(entry) = navs.get_mut(&handle) {
                    let item_idx = entry.items.len() as i64;
                    entry.items.push(NavItem {
                        icon,
                        label,
                        badge: String::new(),
                        btn_hwnd: btn,
                        btn_control_id: item_id,
                    });
                    ITEM_LOOKUP.with(|m| {
                        m.borrow_mut().insert(item_id, (handle, item_idx));
                    });
                    layout_buttons(handle);
                }
            });
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, icon, label);
    }
}

#[cfg(target_os = "windows")]
fn layout_buttons(handle: i64) {
    let parent_hwnd = match super::get_hwnd(handle) {
        Some(h) => h,
        None => return,
    };
    unsafe {
        let mut rect = RECT::default();
        let _ = GetClientRect(parent_hwnd, &mut rect);
        let total_w = (rect.right - rect.left).max(1);
        let total_h = (rect.bottom - rect.top).max(48);
        let count = NAVS.with(|n| n.borrow().get(&handle).map(|e| e.items.len()).unwrap_or(0));
        if count == 0 {
            return;
        }
        let item_w = total_w / (count as i32);
        let buttons: Vec<HWND> = NAVS.with(|n| {
            n.borrow()
                .get(&handle)
                .map(|e| e.items.iter().map(|i| i.btn_hwnd).collect())
                .unwrap_or_default()
        });
        for (i, btn) in buttons.iter().enumerate() {
            let _ = SetWindowPos(
                *btn,
                None,
                rect.left + (i as i32) * item_w,
                rect.top,
                item_w,
                total_h,
                SWP_NOZORDER,
            );
        }
    }
}

pub fn set_badge(handle: i64, index: i64, badge_ptr: *const u8) {
    let badge = str_from_header(badge_ptr);
    NAVS.with(|n| {
        if let Some(entry) = n.borrow_mut().get_mut(&handle) {
            if let Some(item) = entry.items.get_mut(index as usize) {
                item.badge = badge.clone();
                #[cfg(target_os = "windows")]
                {
                    let display = if badge.is_empty() {
                        if item.icon.is_empty() {
                            item.label.clone()
                        } else {
                            format!("{} {}", item.icon, item.label)
                        }
                    } else if item.icon.is_empty() {
                        format!("{} ({})", item.label, badge)
                    } else {
                        format!("{} {} ({})", item.icon, item.label, badge)
                    };
                    let wide = to_wide(&display);
                    unsafe {
                        let _ = SetWindowTextW(item.btn_hwnd, windows::core::PCWSTR(wide.as_ptr()));
                    }
                }
            }
        }
    });
}

pub fn set_selected(handle: i64, index: i64) {
    let on_select = NAVS.with(|n| {
        let mut navs = n.borrow_mut();
        if let Some(entry) = navs.get_mut(&handle) {
            entry.selected = index;
            entry.on_select
        } else {
            0.0
        }
    });
    if on_select == 0.0 {
        return;
    }
    let closure_ptr = unsafe { js_nanbox_get_pointer(on_select) } as *const u8;
    if closure_ptr.is_null() {
        return;
    }
    unsafe {
        js_closure_call1(closure_ptr, index as f64);
    }
}

fn pack_rgb(r: f64, g: f64, b: f64) -> u32 {
    // Windows COLORREF is 0x00BBGGRR.
    let to_u8 = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    to_u8(r) | (to_u8(g) << 8) | (to_u8(b) << 16)
}

/// Issue #706 — store the active tab tint. Win32 standard BUTTON
/// controls don't honor WM_CTLCOLORBTN text color (they're rendered by
/// the theme service), so the visual effect waits on a future
/// owner-drawn rewrite. The state is persisted on the NavEntry and is
/// authoritative for that rewrite + introspection.
pub fn set_tint_color(handle: i64, r: f64, g: f64, b: f64, _a: f64) {
    let packed = pack_rgb(r, g, b);
    NAVS.with(|n| {
        if let Some(entry) = n.borrow_mut().get_mut(&handle) {
            entry.selected_tint = Some(packed);
            // Invalidate so a future owner-draw path picks it up.
            #[cfg(target_os = "windows")]
            for item in &entry.items {
                unsafe {
                    let _ = windows::Win32::Graphics::Gdi::InvalidateRect(
                        Some(item.btn_hwnd),
                        None,
                        true,
                    );
                }
            }
        }
    });
}

/// Issue #706 — store the inactive-tabs tint. See `set_tint_color`.
pub fn set_unselected_tint_color(handle: i64, r: f64, g: f64, b: f64, _a: f64) {
    let packed = pack_rgb(r, g, b);
    NAVS.with(|n| {
        if let Some(entry) = n.borrow_mut().get_mut(&handle) {
            entry.unselected_tint = Some(packed);
            #[cfg(target_os = "windows")]
            for item in &entry.items {
                unsafe {
                    let _ = windows::Win32::Graphics::Gdi::InvalidateRect(
                        Some(item.btn_hwnd),
                        None,
                        true,
                    );
                }
            }
        }
    });
}

/// Called from `handle_command` BN_CLICKED for any control id we own.
/// Returns true when consumed.
pub fn handle_click(control_id: u16) -> bool {
    let result = ITEM_LOOKUP.with(|m| m.borrow().get(&control_id).copied());
    if let Some((parent_handle, idx)) = result {
        set_selected(parent_handle, idx);
        return true;
    }
    false
}
