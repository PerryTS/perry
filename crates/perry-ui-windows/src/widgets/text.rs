//! Text widget — STATIC control (SS_LEFT) with custom color/font support

use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::System::SystemServices::SS_LEFT;
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

/// Per-widget text color (COLORREF) and background brush
#[cfg(target_os = "windows")]
struct TextStyle {
    color: u32, // COLORREF (0x00BBGGRR)
    bg_brush: HBRUSH,
    font: HFONT,
}

#[cfg(not(target_os = "windows"))]
struct TextStyle {
    color: u32,
}

thread_local! {
    static TEXT_STYLES: RefCell<HashMap<i64, TextStyle>> = RefCell::new(HashMap::new());

    // Map from HWND (as isize) -> widget handle for fast WM_CTLCOLORSTATIC lookup
    static HWND_TO_HANDLE: RefCell<HashMap<isize, i64>> = RefCell::new(HashMap::new());
}

/// Create a Text label. Returns widget handle.
pub fn create(text_ptr: *const u8) -> i64 {
    let text = str_from_header(text_ptr);
    let control_id = alloc_control_id();

    #[cfg(target_os = "windows")]
    {
        let wide = to_wide(text);
        let class_name = to_wide("STATIC");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(wide.as_ptr()),
                WINDOW_STYLE(SS_LEFT.0 | WS_CHILD.0 | WS_VISIBLE.0),
                0,
                0,
                100,
                20,
                Some(super::get_parking_hwnd()),
                Some(HMENU(control_id as *mut _)),
                Some(HINSTANCE::from(hinstance)),
                None,
            )
            .unwrap();

            let handle = register_widget(hwnd, WidgetKind::Text, control_id);

            HWND_TO_HANDLE.with(|m| {
                m.borrow_mut().insert(hwnd.0 as isize, handle);
            });

            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = text;
        register_widget(0, WidgetKind::Text, control_id)
    }
}

/// Set the text string of a Text widget from a raw string pointer.
pub fn set_string(handle: i64, text_ptr: *const u8) {
    let text = str_from_header(text_ptr);
    set_text_str(handle, text);
}

/// Issue #707 — cap visible lines on a STATIC control.
///
/// Win32 STATIC has no `MaxLines` style; truncation is controlled via
/// the SS_ENDELLIPSIS / SS_PATHELLIPSIS / SS_WORDELLIPSIS style bits
/// instead. We approximate the iOS/Android semantics:
///   - `lines == 0` (unlimited) clears any ellipsis style.
///   - `lines == 1` forces single-line + tail-ellipsis (SS_ENDELLIPSIS,
///     which forces a single-line render).
///   - `lines > 1` is best-effort on Win32: STATIC doesn't expose a
///     per-line cap, so we keep ellipsis off and rely on the control
///     bounds clipping the trailing lines. A richer impl would owner-
///     draw the text via DrawText with DT_END_ELLIPSIS + a clipping rect.
pub fn set_number_of_lines(handle: i64, lines: i64) {
    #[cfg(target_os = "windows")]
    {
        const SS_ENDELLIPSIS: u32 = 0x4000;
        const SS_PATHELLIPSIS: u32 = 0x8000;
        const SS_WORDELLIPSIS: u32 = 0xC000;
        const ELLIPSIS_MASK: u32 = SS_ENDELLIPSIS | SS_PATHELLIPSIS | SS_WORDELLIPSIS;

        if let Some(hwnd) = super::get_hwnd(handle) {
            unsafe {
                let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
                let new_style: u32 = if lines == 1 {
                    (style & !ELLIPSIS_MASK) | SS_ENDELLIPSIS
                } else {
                    style & !ELLIPSIS_MASK
                };
                let _ = SetWindowLongPtrW(hwnd, GWL_STYLE, new_style as isize);
                // SetWindowPos with SWP_FRAMECHANGED is needed for style
                // changes to take effect on STATIC controls.
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND(std::ptr::null_mut())),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
                );
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, lines);
    }
}

/// Issue #707 — STATIC truncation mode (0=word-wrap, 1=head, 2=middle, 3=tail).
///
/// Win32 STATIC doesn't have a "head" ellipsis mode — `SS_PATHELLIPSIS`
/// drops middle segments of a path-like string, `SS_WORDELLIPSIS`
/// truncates on word boundaries (tail-ish), and `SS_ENDELLIPSIS` is the
/// canonical tail truncation. We map:
///   - 0 → clear ellipsis (matches "no truncation")
///   - 1 (head) → fall back to SS_ENDELLIPSIS (Win32 has no head mode)
///   - 2 (middle) → SS_PATHELLIPSIS
///   - 3 (tail) → SS_ENDELLIPSIS
pub fn set_truncation_mode(handle: i64, mode: i64) {
    #[cfg(target_os = "windows")]
    {
        const SS_ENDELLIPSIS: u32 = 0x4000;
        const SS_PATHELLIPSIS: u32 = 0x8000;
        const SS_WORDELLIPSIS: u32 = 0xC000;
        const ELLIPSIS_MASK: u32 = SS_ENDELLIPSIS | SS_PATHELLIPSIS | SS_WORDELLIPSIS;

        if let Some(hwnd) = super::get_hwnd(handle) {
            unsafe {
                let bit: u32 = match mode {
                    1 => SS_ENDELLIPSIS, // no native head mode
                    2 => SS_PATHELLIPSIS,
                    3 => SS_ENDELLIPSIS,
                    _ => 0,
                };
                let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
                let new_style = (style & !ELLIPSIS_MASK) | bit;
                let _ = SetWindowLongPtrW(hwnd, GWL_STYLE, new_style as isize);
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND(std::ptr::null_mut())),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
                );
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, mode);
    }
}

/// Set horizontal text alignment on a Text widget (issue #3621).
/// Public `alignment` follows the canonical Perry/AppKit scheme:
/// 0=left, 1=right, 2=center, 3=justified, 4=natural. Win32 STATIC
/// controls express horizontal alignment through the SS_LEFT/SS_CENTER/
/// SS_RIGHT style bits (the low 2 bits of the style-type field). Justified
/// and natural have no native STATIC equivalent, so both fall back to left.
pub fn set_text_alignment(handle: i64, alignment: i64) {
    #[cfg(target_os = "windows")]
    {
        // SS_LEFT=0, SS_CENTER=1, SS_RIGHT=2. Mask the low 2 bits.
        const SS_ALIGN_MASK: u32 = 0x3;
        if let Some(hwnd) = super::get_hwnd(handle) {
            unsafe {
                let bits: u32 = match alignment {
                    1 => 2, // right
                    2 => 1, // center
                    _ => 0, // left / justified / natural
                };
                let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
                let new_style = (style & !SS_ALIGN_MASK) | bits;
                let _ = SetWindowLongPtrW(hwnd, GWL_STYLE, new_style as isize);
                // SWP_FRAMECHANGED forces STATIC style changes to take effect.
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND(std::ptr::null_mut())),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
                );
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, alignment);
    }
}

/// Set the text string of a Text widget from a &str (used by state bindings).
pub fn set_text_str(handle: i64, text: &str) {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            let wide = to_wide(text);
            unsafe {
                let _ = SetWindowTextW(hwnd, windows::core::PCWSTR(wide.as_ptr()));
                // Repaint explicitly — our STATICs are styled via
                // WM_CTLCOLORSTATIC with a transparent background, so the
                // WM_SETTEXT default repaint can leave the old glyphs
                // behind (classic timer-driven-counter symptom: the text
                // changes but the screen doesn't).
                let _ = InvalidateRect(Some(hwnd), None, true);
            }
            // New content ⇒ new measured extent. Layout otherwise only
            // runs on WM_SIZE, so a longer string stayed clipped to the
            // old bounds until the user resized the window.
            crate::app::request_layout();
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, text);
    }
}

/// Set the text color (RGBA 0.0-1.0).
pub fn set_color(handle: i64, r: f64, g: f64, b: f64, _a: f64) {
    let cr = ((r * 255.0) as u32) | (((g * 255.0) as u32) << 8) | (((b * 255.0) as u32) << 16);

    #[cfg(target_os = "windows")]
    {
        // Get or create a null brush for transparent background
        let bg_brush = unsafe { GetStockObject(NULL_BRUSH) };
        let bg_brush = HBRUSH(bg_brush.0);

        TEXT_STYLES.with(|styles| {
            let mut styles = styles.borrow_mut();
            let entry = styles.entry(handle).or_insert(TextStyle {
                color: cr,
                bg_brush,
                font: HFONT::default(),
            });
            entry.color = cr;
            entry.bg_brush = bg_brush;
        });

        // Force repaint
        if let Some(hwnd) = super::get_hwnd(handle) {
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, true);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, cr);
    }
}

/// Set the font size of a Text widget.
pub fn set_font_size(handle: i64, size: f64) {
    #[cfg(target_os = "windows")]
    {
        let font = create_font(size as i32, 400); // FW_NORMAL = 400
        apply_font(handle, font);
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, size);
    }
}

/// Set the font weight of a Text widget (size + weight).
pub fn set_font_weight(handle: i64, size: f64, weight: f64) {
    #[cfg(target_os = "windows")]
    {
        // Perry weight: 0.0=ultralight, 0.25=light, 0.4=regular, 0.5=medium,
        // 0.6=semibold, 0.7=bold, 1.0=heavy. Map to Win32 FW_ values.
        let win32_weight = if weight >= 0.9 {
            800
        }
        // heavy/black
        else if weight >= 0.65 {
            700
        }
        // bold
        else if weight >= 0.55 {
            600
        }
        // semi-bold
        else if weight >= 0.45 {
            500
        }
        // medium
        else {
            400
        }; // regular
        let font = create_font(size as i32, win32_weight);
        apply_font(handle, font);
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, size, weight);
    }
}

/// Set the font family of a Text widget.
pub fn set_font_family(handle: i64, family_ptr: *const u8) {
    let family = str_from_header(family_ptr);

    #[cfg(target_os = "windows")]
    {
        // Map common names to Windows font names
        let win_family = match family {
            "monospace" | "monospaced" | ".AppleSystemUIFontMonospaced" => "Consolas",
            "system" | ".AppleSystemUIFont" => "Segoe UI",
            "serif" => "Times New Roman",
            "sans-serif" => "Segoe UI",
            other => other,
        };

        // Preserve existing font size and weight from the current HFONT
        let (size, weight) = TEXT_STYLES.with(|styles| {
            let styles = styles.borrow();
            if let Some(style) = styles.get(&handle) {
                if !style.font.is_invalid() {
                    let mut lf = LOGFONTW::default();
                    unsafe {
                        GetObjectW(
                            style.font.into(),
                            std::mem::size_of::<LOGFONTW>() as i32,
                            Some(&mut lf as *mut _ as *mut _),
                        );
                    }
                    // Undo DPI scaling to get the logical size back
                    let logical_size = ((-lf.lfHeight) as f64 / crate::app::get_dpi_scale()) as i32;
                    return (logical_size.max(1), lf.lfWeight);
                }
            }
            (14, 400) // default fallback
        });

        let font = create_font_with_family(size, weight, win_family);
        apply_font(handle, font);
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, family);
    }
}

/// Set whether a Text widget is selectable.
pub fn set_selectable(handle: i64, _selectable: bool) {
    // Win32 STATIC controls are not selectable by default.
    // To make text selectable, we'd need to use an EDIT control with ES_READONLY.
    // For now, this is a no-op — selectable text can be implemented later by
    // swapping the STATIC with an ES_READONLY EDIT control.
    let _ = handle;
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

/// Handle WM_CTLCOLORSTATIC — set text color and background for styled text widgets.
#[cfg(target_os = "windows")]
pub fn handle_ctlcolor(hdc: HDC, child_hwnd: HWND) -> Option<LRESULT> {
    let handle = HWND_TO_HANDLE.with(|m| m.borrow().get(&(child_hwnd.0 as isize)).copied());

    let handle = handle?;

    // Find the nearest ancestor brush for background
    let ancestor_brush = find_ancestor_brush(child_hwnd);

    let null_brush = LRESULT(unsafe { GetStockObject(NULL_BRUSH) }.0 as isize);

    // With WS_CLIPCHILDREN on parent VStack/HStack, the parent doesn't paint
    // under child controls. Return the ancestor brush so the Text control fills
    // its own background with the correct color.
    let bg_brush = ancestor_brush
        .map(|b| LRESULT(b.0 as isize))
        .unwrap_or(null_brush);

    TEXT_STYLES.with(|styles| {
        let styles = styles.borrow();
        if let Some(style) = styles.get(&handle) {
            unsafe {
                SetTextColor(hdc, COLORREF(style.color));
                SetBkMode(hdc, TRANSPARENT);
            }
            if !style.font.is_invalid() {
                unsafe {
                    SelectObject(hdc, style.font.into());
                }
            }
            Some(bg_brush)
        } else {
            if ancestor_brush.is_some() {
                unsafe {
                    SetBkMode(hdc, TRANSPARENT);
                }
                Some(bg_brush)
            } else {
                None
            }
        }
    })
}

#[cfg(target_os = "windows")]
fn create_font(size: i32, weight: i32) -> HFONT {
    create_font_with_family(size, weight, "Segoe UI")
}

#[cfg(target_os = "windows")]
/// Public variant for use by button.rs icon font setup.
pub fn create_font_with_family_pub(size: i32, weight: i32, family: &str) -> HFONT {
    create_font_with_family(size, weight, family)
}

fn create_font_with_family(size: i32, weight: i32, family: &str) -> HFONT {
    let family_wide = to_wide(family);
    // Scale font size by DPI factor (96 DPI = 1.0x, 144 DPI = 1.5x)
    let scaled_size = (size as f64 * crate::app::get_dpi_scale()) as i32;
    unsafe {
        CreateFontW(
            -scaled_size, // nHeight (negative = character height, DPI-scaled)
            0,            // nWidth (0 = default)
            0,            // nEscapement
            0,            // nOrientation
            weight,       // fnWeight
            0,            // fdwItalic
            0,            // fdwUnderline
            0,            // fdwStrikeOut
            windows::Win32::Graphics::Gdi::FONT_CHARSET(0), // fdwCharSet (DEFAULT_CHARSET)
            windows::Win32::Graphics::Gdi::FONT_OUTPUT_PRECISION(0), // fdwOutputPrecision
            windows::Win32::Graphics::Gdi::FONT_CLIP_PRECISION(0), // fdwClipPrecision
            windows::Win32::Graphics::Gdi::FONT_QUALITY(0), // fdwQuality
            0,            // fdwPitchAndFamily
            windows::core::PCWSTR(family_wide.as_ptr()),
        )
    }
}

#[cfg(target_os = "windows")]
fn apply_font(handle: i64, font: HFONT) {
    TEXT_STYLES.with(|styles| {
        let mut styles = styles.borrow_mut();
        let entry = styles.entry(handle).or_insert(TextStyle {
            color: 0,
            bg_brush: HBRUSH::default(),
            font: HFONT::default(),
        });
        // Clean up old font
        if !entry.font.is_invalid() {
            unsafe {
                let _ = DeleteObject(entry.font.into());
            }
        }
        entry.font = font;
    });

    if let Some(hwnd) = super::get_hwnd(handle) {
        unsafe {
            SendMessageW(
                hwnd,
                WM_SETFONT,
                Some(WPARAM(font.0 as usize)),
                Some(LPARAM(1)),
            );
        }
        // Font metrics feed measure_text_height — a size/weight/family
        // change alters the intrinsic extent, so reflow (single choke
        // point for set_font_size / set_font_weight / set_font_family).
        crate::app::request_layout();
    }
}

/// Stored text-decoration values per widget (issue #185 Phase B closure).
/// 0=none, 1=underline, 2=strikethrough.
static DECORATION_VALUES: std::sync::Mutex<Vec<(i64, i64)>> = std::sync::Mutex::new(Vec::new());

/// Set text decoration on a Text widget (issue #185 Phase B / #210
/// closure). Values: 0=none, 1=underline, 2=strikethrough.
///
/// Reads the widget's current HFONT via `GetObjectW`, mutates the
/// `lfUnderline` / `lfStrikeOut` LOGFONT flags, recreates via
/// `CreateFontIndirectW`, and re-emits via `WM_SETFONT` — same shape as
/// `apply_font`'s lifecycle (DeleteObject on the old HFONT before
/// assigning the new one is handled by `apply_font` itself). The
/// `DECORATION_VALUES` store still tracks last-set values so resize +
/// font-change cascades can re-apply the decoration without losing it.
pub fn set_decoration(handle: i64, decoration: i64) {
    if let Ok(mut decorations) = DECORATION_VALUES.lock() {
        if let Some(slot) = decorations.iter_mut().find(|e| e.0 == handle) {
            slot.1 = decoration;
        } else {
            decorations.push((handle, decoration));
        }
    }

    #[cfg(target_os = "windows")]
    {
        apply_decoration(handle, decoration);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = decoration;
    }
}

/// Read the current LOGFONT, set underline/strikeout per `decoration`,
/// recreate the HFONT, and re-emit via `apply_font`. Falls back to a
/// fresh "Segoe UI" 14/400 font if no HFONT exists yet (so calling
/// `set_decoration` before any other font setter still works).
#[cfg(target_os = "windows")]
fn apply_decoration(handle: i64, decoration: i64) {
    let existing = TEXT_STYLES.with(|styles| {
        let styles = styles.borrow();
        styles
            .get(&handle)
            .map(|s| s.font)
            .filter(|f| !f.is_invalid())
    });

    let mut lf = LOGFONTW::default();
    if let Some(font) = existing {
        unsafe {
            GetObjectW(
                font.into(),
                std::mem::size_of::<LOGFONTW>() as i32,
                Some(&mut lf as *mut _ as *mut _),
            );
        }
    } else {
        // No font set yet — start from a Segoe UI 14/400 baseline.
        let scaled = (14.0 * crate::app::get_dpi_scale()) as i32;
        lf.lfHeight = -scaled;
        lf.lfWeight = 400;
        let family = to_wide("Segoe UI");
        let n = family.len().min(lf.lfFaceName.len());
        lf.lfFaceName[..n].copy_from_slice(&family[..n]);
    }

    lf.lfUnderline = if decoration == 1 { 1 } else { 0 };
    lf.lfStrikeOut = if decoration == 2 { 1 } else { 0 };

    let new_font = unsafe { CreateFontIndirectW(&lf) };
    if !new_font.is_invalid() {
        apply_font(handle, new_font);
    }
}

/// Read the stored decoration value (introspection hook).
pub fn get_decoration(handle: i64) -> Option<i64> {
    DECORATION_VALUES
        .lock()
        .ok()
        .and_then(|d| d.iter().find(|e| e.0 == handle).map(|e| e.1))
}
