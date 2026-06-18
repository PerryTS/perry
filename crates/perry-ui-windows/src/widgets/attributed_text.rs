//! Issue #710 — `AttributedText` on Windows via a read-only RichEdit
//! control. Each `append` selects the end-of-text, inserts the new
//! substring, then re-selects that range and applies CHARFORMAT2W
//! attributes (bold/italic/underline/color/size). Distinct from
//! `widgets::rich_text` (#478) which is a styled editor with toolbar;
//! this is a static display surface that mirrors the macOS/iOS
//! NSAttributedString builder.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, LoadLibraryW};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{alloc_control_id, register_widget, WidgetKind};

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
const EM_SETSEL: u32 = 0x00B1;
#[cfg(target_os = "windows")]
const EM_REPLACESEL: u32 = 0x00C2;
#[cfg(target_os = "windows")]
const EM_SETCHARFORMAT: u32 = 0x0444;
#[cfg(target_os = "windows")]
const EM_EXSETSEL: u32 = 0x0437;
#[cfg(target_os = "windows")]
const EM_GETTEXTLENGTH: u32 = 0x000E;
#[cfg(target_os = "windows")]
const SCF_SELECTION: u32 = 0x0001;
#[cfg(target_os = "windows")]
const CFM_BOLD: u32 = 0x0001;
#[cfg(target_os = "windows")]
const CFM_ITALIC: u32 = 0x0002;
#[cfg(target_os = "windows")]
const CFM_UNDERLINE: u32 = 0x0004;
#[cfg(target_os = "windows")]
const CFM_SIZE: u32 = 0x80000000;
#[cfg(target_os = "windows")]
const CFM_COLOR: u32 = 0x40000000;
#[cfg(target_os = "windows")]
const CFE_BOLD: u32 = CFM_BOLD;
#[cfg(target_os = "windows")]
const CFE_ITALIC: u32 = CFM_ITALIC;
#[cfg(target_os = "windows")]
const CFE_UNDERLINE: u32 = CFM_UNDERLINE;
#[cfg(target_os = "windows")]
const ES_MULTILINE: u32 = 0x0004;
#[cfg(target_os = "windows")]
const ES_READONLY: u32 = 0x0800;

#[cfg(target_os = "windows")]
#[repr(C)]
#[derive(Default)]
struct CharFormat2W {
    cb_size: u32,
    dw_mask: u32,
    dw_effects: u32,
    y_height: i32,
    y_offset: i32,
    cr_text_color: u32,
    b_char_set: u8,
    b_pitch_and_family: u8,
    sz_face_name: [u16; 32],
    w_weight: u16,
    s_spacing: i16,
    cr_back_color: u32,
    lcid: u32,
    dw_reserved: u32,
    s_style: i16,
    w_kerning: u16,
    b_underline_type: u8,
    b_animation: u8,
    b_rev_author: u8,
    b_underline_color: u8,
}

#[cfg(target_os = "windows")]
#[repr(C)]
struct CharRange {
    min: i32,
    max: i32,
}

#[cfg(target_os = "windows")]
fn ensure_richedit_loaded() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let dll = to_wide("Msftedit.dll");
        unsafe {
            let _ = LoadLibraryW(windows::core::PCWSTR(dll.as_ptr()));
        }
    });
}

thread_local! {
    /// Per-handle running character count — used as the "start" of the
    /// next run for EM_EXSETSEL.
    static LENGTHS: RefCell<HashMap<i64, i32>> = RefCell::new(HashMap::new());
}

pub fn create() -> i64 {
    let control_id = alloc_control_id();

    #[cfg(target_os = "windows")]
    {
        ensure_richedit_loaded();
        let class_name = to_wide("RichEdit50W");
        let style =
            WINDOW_STYLE(ES_MULTILINE | ES_READONLY | WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0);
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(std::ptr::null()),
                style,
                0,
                0,
                200,
                40,
                Some(super::get_parking_hwnd()),
                Some(HMENU(control_id as *mut _)),
                Some(HINSTANCE::from(hinstance)),
                None,
            );
            let Ok(hwnd) = hwnd else {
                return register_widget(
                    HWND(std::ptr::null_mut()),
                    WidgetKind::RichText,
                    control_id,
                );
            };
            let handle = register_widget(hwnd, WidgetKind::RichText, control_id);
            LENGTHS.with(|l| {
                l.borrow_mut().insert(handle, 0);
            });
            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let handle = register_widget(0, WidgetKind::RichText, control_id);
        LENGTHS.with(|l| {
            l.borrow_mut().insert(handle, 0);
        });
        handle
    }
}

pub fn append(
    handle: i64,
    text_ptr: *const u8,
    bold: i64,
    italic: i64,
    underline: i64,
    font_size: f64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    let text = str_from_header(text_ptr);
    if text.is_empty() {
        return;
    }

    #[cfg(target_os = "windows")]
    {
        let Some(hwnd) = super::get_hwnd(handle) else {
            return;
        };
        let start = LENGTHS.with(|l| l.borrow().get(&handle).copied().unwrap_or(0));
        let wide = to_wide(text);
        let added = (wide.len() - 1) as i32; // exclude trailing NUL

        unsafe {
            // Move caret to end-of-text and insert the new substring.
            let end_total: isize =
                SendMessageW(hwnd, EM_GETTEXTLENGTH, Some(WPARAM(0)), Some(LPARAM(0))).0;
            SendMessageW(
                hwnd,
                EM_SETSEL,
                Some(WPARAM(end_total as usize)),
                Some(LPARAM(end_total)),
            );
            SendMessageW(
                hwnd,
                EM_REPLACESEL,
                Some(WPARAM(0)),
                Some(LPARAM(wide.as_ptr() as isize)),
            );

            // Re-select the just-appended range and apply the attributes.
            let range = CharRange {
                min: start,
                max: start + added,
            };
            SendMessageW(
                hwnd,
                EM_EXSETSEL,
                Some(WPARAM(0)),
                Some(LPARAM(&range as *const _ as isize)),
            );

            let mut cf = CharFormat2W::default();
            cf.cb_size = std::mem::size_of::<CharFormat2W>() as u32;

            if bold != 0 {
                cf.dw_mask |= CFM_BOLD;
                cf.dw_effects |= CFE_BOLD;
            }
            if italic != 0 {
                cf.dw_mask |= CFM_ITALIC;
                cf.dw_effects |= CFE_ITALIC;
            }
            if underline != 0 {
                cf.dw_mask |= CFM_UNDERLINE;
                cf.dw_effects |= CFE_UNDERLINE;
            }
            if font_size > 0.0 {
                cf.dw_mask |= CFM_SIZE;
                // y_height is in twips: 20 twips per point.
                cf.y_height = (font_size * 20.0).round() as i32;
            }
            if a > 0.0 {
                cf.dw_mask |= CFM_COLOR;
                cf.cr_text_color = pack_colorref(r, g, b);
            }

            if cf.dw_mask != 0 {
                SendMessageW(
                    hwnd,
                    EM_SETCHARFORMAT,
                    Some(WPARAM(SCF_SELECTION as usize)),
                    Some(LPARAM(&cf as *const _ as isize)),
                );
            }

            // Move caret back to end so subsequent appends don't fight.
            let new_end = start + added;
            SendMessageW(
                hwnd,
                EM_SETSEL,
                Some(WPARAM(new_end as usize)),
                Some(LPARAM(new_end as isize)),
            );
        }

        LENGTHS.with(|l| {
            if let Some(v) = l.borrow_mut().get_mut(&handle) {
                *v = start + added;
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, text, bold, italic, underline, font_size, r, g, b, a);
    }
}

pub fn clear(handle: i64) {
    #[cfg(target_os = "windows")]
    {
        let Some(hwnd) = super::get_hwnd(handle) else {
            return;
        };
        unsafe {
            // Select-all + replace-with-empty.
            SendMessageW(hwnd, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
            let empty: [u16; 1] = [0];
            SendMessageW(
                hwnd,
                EM_REPLACESEL,
                Some(WPARAM(0)),
                Some(LPARAM(empty.as_ptr() as isize)),
            );
        }
        LENGTHS.with(|l| {
            if let Some(v) = l.borrow_mut().get_mut(&handle) {
                *v = 0;
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}

#[cfg(target_os = "windows")]
fn pack_colorref(r: f64, g: f64, b: f64) -> u32 {
    let to_u8 = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    to_u8(r) | (to_u8(g) << 8) | (to_u8(b) << 16)
}
