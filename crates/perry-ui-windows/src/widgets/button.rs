//! Button widget — Win32 BUTTON control (BS_PUSHBUTTON)

use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::{
    DrawTextW, FillRect, InvalidateRect, SelectObject, SetBkMode, SetTextColor, DT_CENTER,
    DT_SINGLELINE, DT_VCENTER, HGDIOBJ, TRANSPARENT,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{alloc_control_id, register_widget, WidgetKind};

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
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

thread_local! {
    // Map from widget handle -> callback pointer
    static BUTTON_CALLBACKS: RefCell<HashMap<i64, *const u8>> = RefCell::new(HashMap::new());
    // Map from widget handle -> text COLORREF
    static BUTTON_TEXT_COLORS: RefCell<HashMap<i64, u32>> = RefCell::new(HashMap::new());
    // Map from button HWND -> widget handle (for WM_DRAWITEM lookup)
    #[cfg(target_os = "windows")]
    static BTN_HWND_TO_HANDLE: RefCell<HashMap<isize, i64>> = RefCell::new(HashMap::new());
}

/// Create a Button. Returns widget handle.
pub fn create(label_ptr: *const u8, on_press: f64) -> i64 {
    let label = str_from_header(label_ptr);
    let callback_ptr = unsafe { js_nanbox_get_pointer(on_press) } as *const u8;
    let control_id = alloc_control_id();

    #[cfg(target_os = "windows")]
    {
        let wide = to_wide(label);
        let class_name = to_wide("BUTTON");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            // Use owner-draw for all buttons so we control rendering (no 3D borders)
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(wide.as_ptr()),
                WINDOW_STYLE(BS_OWNERDRAW as u32 | WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0),
                0,
                0,
                100,
                34,
                super::get_parking_hwnd(),
                HMENU(control_id as *mut _),
                HINSTANCE::from(hinstance),
                None,
            )
            .unwrap();

            let handle = register_widget(hwnd, WidgetKind::Button, control_id);
            BTN_HWND_TO_HANDLE.with(|m| m.borrow_mut().insert(hwnd.0 as isize, handle));
            BUTTON_CALLBACKS.with(|cb| {
                cb.borrow_mut().insert(handle, callback_ptr);
            });
            #[cfg(feature = "geisterhand")]
            {
                extern "C" {
                    fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8);
                }
                unsafe {
                    perry_geisterhand_register(handle, 0, 0, on_press, label_ptr);
                }
            }
            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = label;
        let handle = register_widget(0, WidgetKind::Button, control_id);
        BUTTON_CALLBACKS.with(|cb| {
            cb.borrow_mut().insert(handle, callback_ptr);
        });
        #[cfg(feature = "geisterhand")]
        {
            extern "C" {
                fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8);
            }
            unsafe {
                perry_geisterhand_register(handle, 0, 0, on_press, label_ptr);
            }
        }
        handle
    }
}

/// Handle button click (BN_CLICKED).
pub fn handle_click(handle: i64) {
    // Extract the callback pointer first, then drop the borrow before calling it.
    // The closure may create new buttons (borrowing BUTTON_CALLBACKS mutably).
    let ptr = BUTTON_CALLBACKS.with(|cb| {
        let callbacks = cb.borrow();
        callbacks.get(&handle).copied()
    });
    if let Some(ptr) = ptr {
        unsafe { js_closure_call0(ptr) };
    }
}

/// Set whether a Button has a visible border.
/// When bordered=false, switches to owner-draw mode so we fully control
/// rendering (BS_FLAT still shows borders on Windows).
pub fn set_bordered(handle: i64, bordered: bool) {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            unsafe {
                let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
                let new_style = if bordered {
                    (style & !0x0F) | BS_PUSHBUTTON as u32
                } else {
                    // Switch to owner-draw so we fully control rendering (no border)
                    (style & !0x0F) | BS_OWNERDRAW as u32
                };
                SetWindowLongW(hwnd, GWL_STYLE, new_style as i32);
                if !bordered {
                    BTN_HWND_TO_HANDLE.with(|m| m.borrow_mut().insert(hwnd.0 as isize, handle));
                }
                let _ = InvalidateRect(hwnd, None, true);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, bordered);
    }
}

/// Set the title text of a Button.
pub fn set_title(handle: i64, title_ptr: *const u8) {
    let title = str_from_header(title_ptr);

    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            let wide = to_wide(title);
            unsafe {
                let _ = SetWindowTextW(hwnd, windows::core::PCWSTR(wide.as_ptr()));
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, title);
    }
}

/// Set button image by SF Symbol name. On Windows, maps known names to Unicode/text fallbacks.
pub fn set_image(handle: i64, name_ptr: *const u8) {
    let name = str_from_header(name_ptr);
    // Use non-emoji Unicode glyphs that respect SetTextColor on Windows.
    // Emoji glyphs (U+1Fxxx) use color fonts and IGNORE SetTextColor.
    let fallback = match name {
        // Activity bar & common UI icons — use Segoe UI Symbol / basic Unicode
        "doc.on.doc" | "doc.on.doc.fill" => "\u{25A1}\u{25A0}", // □■ (files)
        "magnifyingglass" => "\u{2315}",                        // ⌕ (search)
        "arrow.triangle.branch" => "\u{2387}",                  // ⎇ (git branch)
        "arrow.triangle.2.circlepath" => "\u{21BB}",            // ↻ (sync)
        "sparkles" => "\u{2606}",                               // ☆ (AI)
        "terminal" => ">_",                                     // terminal prompt
        "ladybug" | "ladybug.fill" => "\u{25C8}",               // ◈ (debug)
        "puzzlepiece.extension" | "puzzlepiece.extension.fill" => "\u{29C9}", // ⧉ (extensions)
        "gearshape" | "gearshape.fill" | "gear" => "\u{2699}",  // ⚙
        "gearshape.2" => "\u{2699}",                            // ⚙
        "folder" | "folder.fill" => "\u{25B7}",                 // ▷ (folder)
        "doc.text" | "doc.text.fill" | "doc.plaintext" => "\u{25A1}", // □ (doc)
        "doc" => "\u{25A1}",                                    // □
        "doc.badge.plus" => "+\u{25A1}",                        // new file
        "folder.badge.plus" => "+\u{25B7}",                     // new folder
        "xmark" => "\u{2715}",                                  // ✕
        "circle.fill" => "\u{25CF}",                            // ●
        "chevron.right" => "\u{203A}",                          // ›
        "chevron.down" => "\u{2304}",                           // ⌄
        "chevron.left.forwardslash.chevron.right" => "</>",     // code
        "sidebar.left" | "sidebar.leading" => "\u{2261}",       // ≡
        "plus" => "+",
        "ellipsis" => "\u{22EF}", // ⋯
        // File type icons
        "swift" => "TS",            // TypeScript (maps from swift)
        "curlybraces" => "{}",      // JSON
        "paintbrush" => "\u{2338}", // ⌸ (CSS)
        // Debug icons
        "play.fill" => "\u{25B6}",                          // ▶
        "pause.fill" => "\u{2016}",                         // ‖ (pause)
        "stop.fill" => "\u{25A0}",                          // ■
        "arrow.right" => "\u{2192}",                        // → (step over)
        "arrow.down.right" => "\u{2198}",                   // ↘ (step into)
        "arrow.up.left" => "\u{2196}",                      // ↖ (step out)
        "arrow.up.left.and.arrow.down.right" => "\u{2922}", // ⤢ (maximize)
        "arrow.down.right.and.arrow.up.left" => "\u{2925}", // ⤥ (collapse)
        _ => name,
    };

    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            // Replace the button text with the icon fallback.
            let wide = to_wide(fallback);
            unsafe {
                let _ = SetWindowTextW(hwnd, windows::core::PCWSTR(wide.as_ptr()));
            }
            // Set font to "Segoe UI Symbol" so Unicode glyphs render at the correct
            // size. The default "Segoe UI" doesn't contain these symbols and Win32
            // falls back to a tiny glyph from another font.
            let font =
                crate::widgets::text::create_font_with_family_pub(20, 400, "Segoe UI Symbol");
            unsafe {
                SendMessageW(hwnd, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, fallback);
    }
}

/// Set the text color of a button. Switches to owner-draw mode.
pub fn set_text_color(handle: i64, r: f64, g: f64, b: f64, _a: f64) {
    let r_byte = (r * 255.0).round().min(255.0).max(0.0) as u32;
    let g_byte = (g * 255.0).round().min(255.0).max(0.0) as u32;
    let b_byte = (b * 255.0).round().min(255.0).max(0.0) as u32;
    let color = r_byte | (g_byte << 8) | (b_byte << 16);

    BUTTON_TEXT_COLORS.with(|c| c.borrow_mut().insert(handle, color));

    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            BTN_HWND_TO_HANDLE.with(|m| m.borrow_mut().insert(hwnd.0 as isize, handle));
            unsafe {
                // Switch to owner-draw so we control text rendering
                let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
                let new_style = (style & !0x0F) | BS_OWNERDRAW as u32;
                SetWindowLongW(hwnd, GWL_STYLE, new_style as i32);
                let _ = InvalidateRect(hwnd, None, true);
            }
        }
    }
}

/// Handle WM_DRAWITEM for owner-draw buttons. Returns true if handled.
#[cfg(target_os = "windows")]
pub fn handle_draw_item(lparam: LPARAM) -> bool {
    let dis = unsafe { &*(lparam.0 as *const windows::Win32::UI::Controls::DRAWITEMSTRUCT) };
    let btn_hwnd_val = dis.hwndItem.0 as isize;

    let handle = BTN_HWND_TO_HANDLE.with(|m| m.borrow().get(&btn_hwnd_val).copied());
    let handle = match handle {
        Some(h) => h,
        None => return false,
    };

    let text_color = BUTTON_TEXT_COLORS.with(|c| c.borrow().get(&handle).copied());
    // Default: dark charcoal text for buttons without explicit color
    let text_color = text_color.unwrap_or(0x00333333);

    unsafe {
        let hdc = dis.hDC;
        let rect = dis.rcItem;

        // Fill background with own color or transparent parent color
        let bg_color = super::get_hwnd_bg_color(dis.hwndItem)
            .or_else(|| super::find_ancestor_hwnd_bg_color(dis.hwndItem));
        let has_own_bg = super::get_hwnd_bg_color(dis.hwndItem).is_some();

        if let Some(color) = bg_color {
            let brush = windows::Win32::Graphics::Gdi::CreateSolidBrush(COLORREF(color));
            if has_own_bg {
                // Button has its own bg color — draw rounded rect
                let rgn = windows::Win32::Graphics::Gdi::CreateRoundRectRgn(
                    rect.left,
                    rect.top,
                    rect.right + 1,
                    rect.bottom + 1,
                    8,
                    8,
                );
                windows::Win32::Graphics::Gdi::FillRgn(hdc, rgn, brush);
                let _ = windows::Win32::Graphics::Gdi::DeleteObject(rgn);
            } else {
                FillRect(hdc, &rect, brush);
            }
            let _ = windows::Win32::Graphics::Gdi::DeleteObject(brush);
        }

        // Draw centered text
        SetTextColor(hdc, COLORREF(text_color));
        SetBkMode(hdc, TRANSPARENT);

        let hfont = windows::Win32::Graphics::Gdi::HFONT(
            SendMessageW(dis.hwndItem, WM_GETFONT, WPARAM(0), LPARAM(0)).0 as *mut _,
        );
        let old_font = if !hfont.is_invalid() {
            SelectObject(hdc, hfont)
        } else {
            HGDIOBJ::default()
        };

        let text_len = GetWindowTextLengthW(dis.hwndItem);
        if text_len > 0 {
            let mut buf = vec![0u16; (text_len + 1) as usize];
            GetWindowTextW(dis.hwndItem, &mut buf);
            let mut text_rect = rect;
            DrawTextW(
                hdc,
                &mut buf[..text_len as usize],
                &mut text_rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
        }

        if !old_font.is_invalid() {
            SelectObject(hdc, old_font);
        }
    }
    true
}
