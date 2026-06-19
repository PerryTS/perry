//! Rich text editor — Win32 RichEdit control (`MSFTEDIT_CLASS` =
//! `RichEdit50W`). Bold / italic / underline applied to the current
//! selection via `EM_SETCHARFORMAT` + `CHARFORMAT2W`.
//!
//! HTML round-trip is a v1 placeholder: RichEdit's native serialised
//! format is RTF (via `EM_STREAMIN` / `EM_STREAMOUT`), not HTML, so a
//! proper round-trip would need an RTF↔HTML converter. For v1
//! `setHtml` strips tags and inserts the result as plain text;
//! `getHtml` wraps the plain text in `<p>...</p>` with HTML escaping.
//! Tracked as a #478 follow-up.

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

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
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
const EM_SETCHARFORMAT: u32 = 0x0444;
#[cfg(target_os = "windows")]
const SCF_SELECTION: u32 = 0x0001;
#[cfg(target_os = "windows")]
const CFM_BOLD: u32 = 0x0001;
#[cfg(target_os = "windows")]
const CFM_ITALIC: u32 = 0x0002;
#[cfg(target_os = "windows")]
const CFM_UNDERLINE: u32 = 0x0004;
#[cfg(target_os = "windows")]
const CFE_BOLD: u32 = CFM_BOLD;
#[cfg(target_os = "windows")]
const CFE_ITALIC: u32 = CFM_ITALIC;
#[cfg(target_os = "windows")]
const CFE_UNDERLINE: u32 = CFM_UNDERLINE;
#[cfg(target_os = "windows")]
const ES_MULTILINE: u32 = 0x0004;
#[cfg(target_os = "windows")]
const ES_AUTOVSCROLL: u32 = 0x0040;
#[cfg(target_os = "windows")]
const ES_WANTRETURN: u32 = 0x1000;

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
    static CALLBACKS: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
}

pub fn create(width: f64, height: f64, on_change: f64) -> i64 {
    let control_id = alloc_control_id();

    #[cfg(target_os = "windows")]
    {
        ensure_richedit_loaded();
        // MSFTEDIT_CLASS = "RichEdit50W" (the post-1.0 RichEdit class).
        let class_name = to_wide("RichEdit50W");
        let style = WINDOW_STYLE(
            ES_MULTILINE
                | ES_AUTOVSCROLL
                | ES_WANTRETURN
                | WS_CHILD.0
                | WS_VISIBLE.0
                | WS_TABSTOP.0
                | WS_VSCROLL.0
                | WS_BORDER.0,
        );
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(std::ptr::null()),
                style,
                0,
                0,
                width.max(40.0) as i32,
                height.max(40.0) as i32,
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
            CALLBACKS.with(|m| {
                m.borrow_mut().insert(handle, on_change);
            });
            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (width, height, on_change);
        register_widget(0, WidgetKind::RichText, control_id)
    }
}

pub fn set_string(handle: i64, text_ptr: *const u8) {
    let s = str_from_header(text_ptr);
    #[cfg(target_os = "windows")]
    {
        let Some(hwnd) = super::get_hwnd(handle) else {
            return;
        };
        let wide = to_wide(s);
        unsafe {
            SetWindowTextW(hwnd, windows::core::PCWSTR(wide.as_ptr())).ok();
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, s);
    }
}

pub fn get_string(handle: i64) -> f64 {
    let undefined = f64::from_bits(0x7FFC_0000_0000_0001);
    #[cfg(target_os = "windows")]
    {
        let Some(hwnd) = super::get_hwnd(handle) else {
            return undefined;
        };
        unsafe {
            // 64KB is the practical RichEdit working-set ceiling; above
            // that callers should switch to EM_STREAMOUT.
            let mut buf = vec![0u16; 64 * 1024];
            let len = GetWindowTextW(hwnd, &mut buf) as usize;
            let s = String::from_utf16_lossy(&buf[..len]);
            let bytes = s.as_bytes();
            let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
            js_nanbox_string(header as i64)
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
        undefined
    }
}

/// `set_html` v1 — strips tags and inserts as plain text. RichEdit's
/// native serialised format is RTF; HTML round-trip would need an
/// RTF↔HTML converter (tracked as a #478 follow-up).
pub fn set_html(handle: i64, html_ptr: *const u8) -> i64 {
    let html = str_from_header(html_ptr);
    if html.is_empty() {
        return 0;
    }
    let plain = strip_html_tags(html);
    set_string(handle, plain_as_header(&plain));
    if plain.is_empty() {
        0
    } else {
        1
    }
}

fn strip_html_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

/// Build a transient StringHeader pointer from a Rust string. The
/// header is allocated from the runtime's GC arena via
/// `js_string_from_bytes` and lives until the next GC cycle.
fn plain_as_header(s: &str) -> *const u8 {
    let bytes = s.as_bytes();
    unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64) }
}

pub fn get_html(handle: i64) -> f64 {
    let plain = {
        // Re-derive the plain text from get_string and unwrap the
        // NaN-boxed STRING. We can't unwrap easily here, so re-read
        // directly via Win32 APIs (matches the get_string body).
        #[cfg(target_os = "windows")]
        {
            let Some(hwnd) = super::get_hwnd(handle) else {
                return f64::from_bits(0x7FFC_0000_0000_0001);
            };
            let mut buf = vec![0u16; 64 * 1024];
            unsafe {
                let len = GetWindowTextW(hwnd, &mut buf) as usize;
                String::from_utf16_lossy(&buf[..len])
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = handle;
            String::new()
        }
    };
    let escaped = plain
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let html = format!("<p>{}</p>", escaped);
    let bytes = html.as_bytes();
    unsafe {
        let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
        js_nanbox_string(header as i64)
    }
}

#[cfg(target_os = "windows")]
fn toggle_effect(handle: i64, mask: u32, effect: u32) {
    let Some(hwnd) = super::get_hwnd(handle) else {
        return;
    };
    unsafe {
        // First read current format on selection so we can probe whether
        // the effect is already on, then flip it.
        let mut probe = CharFormat2W::default();
        probe.cb_size = std::mem::size_of::<CharFormat2W>() as u32;
        probe.dw_mask = mask;
        // EM_GETCHARFORMAT = 0x043A
        SendMessageW(
            hwnd,
            0x043A,
            Some(WPARAM(SCF_SELECTION as usize)),
            Some(LPARAM(&mut probe as *mut _ as isize)),
        );
        let already_on = probe.dw_effects & effect != 0;
        let mut cf = CharFormat2W::default();
        cf.cb_size = std::mem::size_of::<CharFormat2W>() as u32;
        cf.dw_mask = mask;
        cf.dw_effects = if already_on { 0 } else { effect };
        SendMessageW(
            hwnd,
            EM_SETCHARFORMAT,
            Some(WPARAM(SCF_SELECTION as usize)),
            Some(LPARAM(&cf as *const _ as isize)),
        );
    }
}

pub fn toggle_bold(handle: i64) {
    #[cfg(target_os = "windows")]
    toggle_effect(handle, CFM_BOLD, CFE_BOLD);
    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}

pub fn toggle_italic(handle: i64) {
    #[cfg(target_os = "windows")]
    toggle_effect(handle, CFM_ITALIC, CFE_ITALIC);
    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}

pub fn toggle_underline(handle: i64) {
    #[cfg(target_os = "windows")]
    toggle_effect(handle, CFM_UNDERLINE, CFE_UNDERLINE);
    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}

/// EN_CHANGE arrives via WM_COMMAND notify_code=0x0300 — already routed
/// to textfield::handle_change for plain edits. This handler is the
/// per-Combobox-style RichEdit branch the WM_COMMAND router can call
/// once we wire the kind dispatch in mod.rs.
#[cfg(target_os = "windows")]
pub fn handle_change(handle: i64) {
    let on = CALLBACKS.with(|m| m.borrow().get(&handle).copied().unwrap_or(0.0));
    if on == 0.0 {
        return;
    }
    let Some(hwnd) = super::get_hwnd(handle) else {
        return;
    };
    unsafe {
        let mut buf = vec![0u16; 64 * 1024];
        let len = GetWindowTextW(hwnd, &mut buf) as usize;
        let s = String::from_utf16_lossy(&buf[..len]);
        let bytes = s.as_bytes();
        let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
        let arg = js_nanbox_string(header as i64);
        let closure_ptr = js_nanbox_get_pointer(on) as *const u8;
        js_closure_call1(closure_ptr, arg);
    }
}
