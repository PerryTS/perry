//! Custom layout engine for VStack/HStack positioning.
//!
//! Win32 has no NSStackView equivalent, so we manually position children
//! within container HWNDs based on their kind (VStack/HStack), spacing,
//! insets, and whether children are spacers.

use crate::widgets::{self, WidgetKind};

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::*;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

/// Recursively layout a widget and its children within the given bounds.
pub fn layout_widget(handle: i64, width: i32, height: i32) {
    let info = widgets::get_widget_info(handle);
    if info.is_none() {
        return;
    }
    let info = info.unwrap();

    if info.hidden {
        return;
    }

    match info.kind {
        WidgetKind::VStack | WidgetKind::Form | WidgetKind::Section | WidgetKind::LazyVStack => {
            layout_stack(handle, width, height, true)
        }
        WidgetKind::HStack => layout_stack(handle, width, height, false),
        WidgetKind::ScrollView => layout_scrollview(handle, width, height),
        WidgetKind::ZStack => layout_zstack(handle, width, height),
        WidgetKind::NavStack => layout_navstack(handle, width, height),
        _ => {}
    }
}

/// Layout children of a stack (VStack or HStack) within the given size.
fn layout_stack(handle: i64, width: i32, height: i32, vertical: bool) {
    let info = match widgets::get_widget_info(handle) {
        Some(i) => i,
        None => return,
    };

    let (top, left, bottom, right) = info.insets;
    let spacing = info.spacing;
    let children = info.children.clone();

    let inset_top = top as i32;
    let inset_left = left as i32;
    let inset_bottom = bottom as i32;
    let inset_right = right as i32;
    let spacing_px = spacing as i32;

    let available_main = if vertical {
        height - inset_top - inset_bottom
    } else {
        width - inset_left - inset_right
    };
    let available_cross = if vertical {
        width - inset_left - inset_right
    } else {
        height - inset_top - inset_bottom
    };

    let detaches_hidden = info.detaches_hidden;

    let distribution = info.distribution;

    // Count visible children and spacers; move hidden children off-screen
    let mut visible_children: Vec<i64> = Vec::new();
    let mut spacer_count = 0i32;

    for &child in &children {
        if let Some(ci) = widgets::get_widget_info(child) {
            if ci.hidden {
                // Move hidden children off-screen to prevent overlap artifacts
                #[cfg(target_os = "windows")]
                {
                    if let Some(child_hwnd) = widgets::get_hwnd_safe(child) {
                        unsafe {
                            let _ = MoveWindow(child_hwnd, -10000, -10000, 0, 0, false);
                        }
                    }
                }
                continue;
            }
            visible_children.push(child);
            if matches!(ci.kind, WidgetKind::Spacer) || ci.fills_remaining {
                spacer_count += 1;
            }
        }
    }

    // Distribution=0 (Fill): if no child is a spacer/fills_remaining, make the
    // last visible non-Spacer child fill remaining space (matches macOS behavior
    // where the lowest-hugging-priority view stretches).
    // Use a local tracking vec instead of permanently mutating widgets, so that
    // repeated layout passes with changing visibility don't accumulate stale flags.
    let mut auto_fill_idx: Option<usize> = None;
    if distribution == 0 && spacer_count == 0 && !visible_children.is_empty() {
        // Find last non-Spacer child
        for i in (0..visible_children.len()).rev() {
            if let Some(ci) = widgets::get_widget_info(visible_children[i]) {
                if !matches!(ci.kind, WidgetKind::Spacer) {
                    auto_fill_idx = Some(i);
                    spacer_count = 1;
                    break;
                }
            }
        }
    }

    if visible_children.is_empty() {
        return;
    }

    // Calculate total spacing between visible children
    let total_spacing = if visible_children.len() > 1 {
        spacing_px * (visible_children.len() as i32 - 1)
    } else {
        0
    };

    // Measure fixed-size children
    let mut fixed_total = 0i32;
    let mut child_sizes: Vec<i32> = Vec::new();

    for (idx, &child) in visible_children.iter().enumerate() {
        let ci = match widgets::get_widget_info(child) {
            Some(ci) => ci,
            None => {
                child_sizes.push(0);
                continue;
            }
        };
        if matches!(ci.kind, WidgetKind::Spacer) || ci.fills_remaining || auto_fill_idx == Some(idx)
        {
            child_sizes.push(0); // placeholder, will be computed below
        } else if !vertical && ci.fixed_width.is_some() {
            // In HStack, use fixed_width as the main-axis size
            let fw = ci.fixed_width.unwrap();
            fixed_total += fw;
            child_sizes.push(fw);
        } else if vertical && ci.fixed_height.is_some() {
            // In VStack, use fixed_height as the main-axis size
            let fh = ci.fixed_height.unwrap();
            fixed_total += fh;
            child_sizes.push(fh);
        } else {
            let size = measure_intrinsic(child, &ci.kind, vertical, available_cross);
            fixed_total += size;
            child_sizes.push(size);
        }
    }

    // Distribute remaining space to spacers
    let remaining = (available_main - fixed_total - total_spacing).max(0);
    let spacer_size = if spacer_count > 0 {
        remaining / spacer_count
    } else {
        0
    };

    for (i, &child) in visible_children.iter().enumerate() {
        if auto_fill_idx == Some(i) {
            child_sizes[i] = spacer_size;
        } else if let Some(ci) = widgets::get_widget_info(child) {
            if matches!(ci.kind, WidgetKind::Spacer) || ci.fills_remaining {
                child_sizes[i] = spacer_size;
            }
        }
    }

    // Position children
    let mut pos = if vertical { inset_top } else { inset_left };

    for (i, &child) in visible_children.iter().enumerate() {
        let size = child_sizes[i];

        #[cfg(target_os = "windows")]
        {
            // Use Mutex-based HWND lookup (reentrancy-safe)
            if let Some(child_hwnd) = widgets::get_hwnd_safe(child) {
                let ci_info = widgets::get_widget_info(child);
                // For the cross-axis, respect fixed dimensions when set
                // (e.g., Image with setSize(56,56) shouldn't stretch to parent height)
                let cross = if let Some(ref ci) = ci_info {
                    if vertical {
                        ci.fixed_width.unwrap_or(available_cross)
                    } else {
                        ci.fixed_height.unwrap_or(available_cross)
                    }
                } else {
                    available_cross
                };
                let (x, y, w, h) = if vertical {
                    (inset_left, pos, cross, size)
                } else {
                    (pos, inset_top, size, cross)
                };
                // Win32 COMBOBOX quirk (issue #1061): the height passed to
                // MoveWindow bounds the *drop-down list*, not just the closed
                // edit/selection box. The layout intrinsic for Picker /
                // Combobox is the closed height (~28px); applying it verbatim
                // clips the dropdown to zero, so the control renders but never
                // opens ("static control, click does nothing — works on
                // Linux/AppKit"). picker::create / combobox::create deliberately
                // pass a 200px height at CreateWindowExW for exactly this
                // reason; relayout was then destroying it. Inflate only the
                // combobox's own window height — sibling stacking still uses
                // the closed `size` (`pos += size` below is unchanged), so
                // layout is unaffected and the extra height is purely the
                // dropdown's maximum extent.
                let move_h = if ci_info
                    .as_ref()
                    .is_some_and(|ci| matches!(ci.kind, WidgetKind::Picker | WidgetKind::Combobox))
                {
                    h.max(dpi(28)) + dpi(200)
                } else {
                    h
                };
                // position child
                unsafe {
                    let _ = MoveWindow(child_hwnd, x, y, w, move_h, true);
                }
                // Apply deferred corner radius now that widget has its final size
                widgets::apply_corner_radius(child);
                widgets::apply_shadow(child);
                // Reload bitmap for Image widgets so it matches the layout size
                if let Some(ci) = ci_info {
                    if matches!(ci.kind, WidgetKind::Image) {
                        widgets::image::reload_bitmap_scaled(child, w, h);
                    }
                }
                // Recursively layout container children
                layout_widget(child, w, h);
            }
        }

        pos += size + spacing_px;
    }
}

fn layout_scrollview(handle: i64, width: i32, height: i32) {
    let info = match widgets::get_widget_info(handle) {
        Some(i) => i,
        None => return,
    };

    // ScrollView has at most one content child
    if let Some(&child) = info.children.first() {
        #[cfg(target_os = "windows")]
        {
            if let Some(child_hwnd) = widgets::get_hwnd_safe(child) {
                // Content gets full width, but its own natural height
                let child_info = widgets::get_widget_info(child);
                let content_height = if let Some(ci) = &child_info {
                    measure_intrinsic(child, &ci.kind, true, width).max(height)
                } else {
                    height
                };
                unsafe {
                    let _ = MoveWindow(child_hwnd, 0, 0, width, content_height, true);
                }
                widgets::apply_corner_radius(child);
                widgets::apply_shadow(child);
                layout_widget(child, width, content_height);

                // Update scroll info
                crate::widgets::scrollview::update_scroll_info(handle, height, content_height);
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = (child, width, height);
        }
    }
}

/// Layout a ZStack — all children fill the container.
fn layout_zstack(handle: i64, width: i32, height: i32) {
    let info = match widgets::get_widget_info(handle) {
        Some(i) => i,
        None => return,
    };

    for &child in &info.children {
        if let Some(ci) = widgets::get_widget_info(child) {
            if ci.hidden {
                continue;
            }
            #[cfg(target_os = "windows")]
            {
                if let Some(child_hwnd) = widgets::get_hwnd_safe(child) {
                    unsafe {
                        let _ = MoveWindow(child_hwnd, 0, 0, width, height, true);
                    }
                    widgets::apply_corner_radius(child);
                    widgets::apply_shadow(child);
                    layout_widget(child, width, height);
                }
            }
        }
    }
}

/// Layout a NavStack — only the top page fills the container.
fn layout_navstack(handle: i64, width: i32, height: i32) {
    let info = match widgets::get_widget_info(handle) {
        Some(i) => i,
        None => return,
    };

    for &child in &info.children {
        if let Some(ci) = widgets::get_widget_info(child) {
            if ci.hidden {
                continue;
            }
            #[cfg(target_os = "windows")]
            {
                if let Some(child_hwnd) = widgets::get_hwnd_safe(child) {
                    unsafe {
                        let _ = MoveWindow(child_hwnd, 0, 0, width, height, true);
                    }
                    widgets::apply_corner_radius(child);
                    widgets::apply_shadow(child);
                    layout_widget(child, width, height);
                }
            }
        }
    }
}

/// Scale a logical (96-DPI) pixel constant by the current DPI factor.
///
/// The intrinsic control sizes below are authored at 96 DPI. Fonts, the
/// window size, stack spacing, and explicit setSize dimensions are all
/// scaled at their entry points (app.rs / widget_create.rs /
/// widget_layout_extras.rs) — but these layout-engine constants never
/// were, so on a 125–200% display every fixed-height control (TextField,
/// Toggle, Picker, …) clipped its own DPI-scaled font (the #5884 family
/// of bugs). Measured text extents are already physical pixels and must
/// NOT pass through this.
fn dpi(px: i32) -> i32 {
    scale_by(px, crate::app::get_dpi_scale())
}

/// Pure scaling core, unit-testable without the process-global DPI state.
/// A scale of exactly 1.0 (or an unset/degenerate reading ≤ 1.0 — Windows
/// never scales below 100%) returns the constant unchanged.
fn scale_by(px: i32, scale: f64) -> i32 {
    if scale <= 1.0 {
        return px;
    }
    (px as f64 * scale).round() as i32
}

/// Measure the intrinsic size of a widget along the main axis.
fn measure_intrinsic(handle: i64, kind: &WidgetKind, vertical: bool, cross_size: i32) -> i32 {
    // Check fixed dimensions first — they override intrinsic measurement
    if let Some(info) = widgets::get_widget_info(handle) {
        if vertical {
            if let Some(fh) = info.fixed_height {
                return fh;
            }
        } else {
            if let Some(fw) = info.fixed_width {
                return fw;
            }
        }
    }
    match kind {
        WidgetKind::Text => {
            #[cfg(target_os = "windows")]
            {
                if let Some(hwnd) = widgets::get_hwnd_safe(handle) {
                    return measure_text_height(hwnd, cross_size, vertical);
                }
            }
            if vertical {
                dpi(20)
            } else {
                dpi(100)
            }
        }
        WidgetKind::Button => {
            #[cfg(target_os = "windows")]
            {
                if let Some(hwnd) = widgets::get_hwnd_safe(handle) {
                    let size = measure_text_height(hwnd, cross_size, vertical);
                    // Add padding: 8px vertical, 16px horizontal (logical)
                    return if vertical {
                        size + dpi(16)
                    } else {
                        size + dpi(32)
                    };
                }
            }
            if vertical {
                dpi(34)
            } else {
                dpi(100)
            }
        }
        WidgetKind::TextField => {
            if vertical {
                dpi(30)
            } else {
                dpi(200)
            }
        }
        WidgetKind::Toggle => {
            if vertical {
                dpi(24)
            } else {
                dpi(100)
            }
        }
        WidgetKind::Slider => {
            if vertical {
                dpi(24)
            } else {
                dpi(200)
            }
        }
        WidgetKind::Divider => {
            if vertical {
                dpi(2)
            } else {
                dpi(2)
            }
        }
        WidgetKind::Spacer => 0, // handled separately
        WidgetKind::VStack | WidgetKind::HStack => {
            measure_stack_intrinsic(handle, kind, vertical, cross_size)
        }
        WidgetKind::ScrollView | WidgetKind::LazyVStack => {
            // ScrollView/LazyVStack takes all available space
            if vertical {
                dpi(200)
            } else {
                dpi(200)
            }
        }
        WidgetKind::SecureField => {
            if vertical {
                dpi(24)
            } else {
                dpi(200)
            }
        }
        WidgetKind::ProgressView => {
            if vertical {
                dpi(20)
            } else {
                dpi(200)
            }
        }
        WidgetKind::Form | WidgetKind::Section => {
            measure_stack_intrinsic(handle, &WidgetKind::VStack, vertical, cross_size)
        }
        WidgetKind::ZStack | WidgetKind::NavStack => {
            // ZStack/NavStack takes all available space
            if vertical {
                dpi(200)
            } else {
                dpi(200)
            }
        }
        WidgetKind::Picker => {
            if vertical {
                dpi(28)
            } else {
                dpi(200)
            }
        }
        WidgetKind::Canvas => {
            if vertical {
                dpi(200)
            } else {
                dpi(200)
            }
        }
        WidgetKind::Image => {
            if vertical {
                dpi(24)
            } else {
                dpi(24)
            }
        }
        WidgetKind::Calendar => {
            if vertical {
                dpi(240)
            } else {
                dpi(280)
            }
        }
        WidgetKind::DatePicker => {
            // Compact date field — a single row, narrow like a textfield.
            if vertical {
                dpi(28)
            } else {
                dpi(160)
            }
        }
        WidgetKind::Combobox => {
            if vertical {
                dpi(28)
            } else {
                dpi(200)
            }
        }
        WidgetKind::TreeView => {
            if vertical {
                dpi(200)
            } else {
                dpi(240)
            }
        }
        WidgetKind::RichText => {
            if vertical {
                dpi(200)
            } else {
                dpi(240)
            }
        }
        WidgetKind::Chart => {
            if vertical {
                dpi(200)
            } else {
                dpi(240)
            }
        }
        WidgetKind::TextArea => {
            if vertical {
                dpi(120)
            } else {
                dpi(280)
            }
        }
    }
}

fn measure_stack_intrinsic(handle: i64, kind: &WidgetKind, vertical: bool, cross_size: i32) -> i32 {
    let info = match widgets::get_widget_info(handle) {
        Some(i) => i,
        None => return 0,
    };

    let is_same_direction = (vertical && matches!(kind, WidgetKind::VStack))
        || (!vertical && matches!(kind, WidgetKind::HStack));

    let spacing = info.spacing as i32;
    let (top, left, bottom, right) = info.insets;
    let inset_main = if vertical {
        top as i32 + bottom as i32
    } else {
        left as i32 + right as i32
    };
    let inset_cross = if vertical {
        left as i32 + right as i32
    } else {
        top as i32 + bottom as i32
    };
    let inner_cross = (cross_size - inset_cross).max(0);

    let children = &info.children;
    let mut total = inset_main;
    let mut visible_count = 0;

    for &child in children {
        if let Some(ci) = widgets::get_widget_info(child) {
            if ci.hidden {
                continue;
            }
            if matches!(ci.kind, WidgetKind::Spacer) {
                visible_count += 1;
                continue;
            }
            if is_same_direction {
                total += measure_intrinsic(child, &ci.kind, vertical, inner_cross);
                visible_count += 1;
            } else {
                let size = measure_intrinsic(child, &ci.kind, vertical, inner_cross);
                total = total.max(size + inset_main);
                visible_count += 1;
            }
        }
    }

    if is_same_direction && visible_count > 1 {
        total += spacing * (visible_count - 1);
    }

    total
}

#[cfg(target_os = "windows")]
fn measure_text_height(hwnd: HWND, width: i32, vertical: bool) -> i32 {
    unsafe {
        let hdc = GetDC(Some(hwnd));
        if hdc.is_invalid() {
            return if vertical { dpi(20) } else { dpi(100) };
        }

        let text_len = GetWindowTextLengthW(hwnd);
        if text_len == 0 {
            let _ = ReleaseDC(Some(hwnd), hdc);
            return if vertical { dpi(20) } else { dpi(100) };
        }

        let mut buf = vec![0u16; (text_len + 1) as usize];
        GetWindowTextW(hwnd, &mut buf);

        // Send WM_GETFONT to get the current font
        let hfont =
            HFONT(SendMessageW(hwnd, WM_GETFONT, Some(WPARAM(0)), Some(LPARAM(0))).0 as *mut _);
        let old_font = if !hfont.is_invalid() {
            SelectObject(hdc, hfont.into())
        } else {
            HGDIOBJ::default()
        };

        if vertical {
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: 0,
            };
            DrawTextW(
                hdc,
                &mut buf[..text_len as usize],
                &mut rect,
                DT_CALCRECT | DT_WORDBREAK | DT_LEFT,
            );

            if !old_font.is_invalid() {
                SelectObject(hdc, old_font);
            }
            let _ = ReleaseDC(Some(hwnd), hdc);

            (rect.bottom - rect.top).max(dpi(16))
        } else {
            let mut size = SIZE::default();
            GetTextExtentPoint32W(hdc, &buf[..text_len as usize], &mut size);

            if !old_font.is_invalid() {
                SelectObject(hdc, old_font);
            }
            let _ = ReleaseDC(Some(hwnd), hdc);

            size.cx.max(dpi(20))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::scale_by;

    #[test]
    fn identity_at_100_percent() {
        assert_eq!(scale_by(30, 1.0), 30);
        assert_eq!(scale_by(200, 1.0), 200);
    }

    #[test]
    fn degenerate_scale_is_identity() {
        // An unset/zeroed DPI reading must never shrink controls.
        assert_eq!(scale_by(30, 0.0), 30);
        assert_eq!(scale_by(30, 0.5), 30);
    }

    #[test]
    fn scales_and_rounds_at_common_factors() {
        assert_eq!(scale_by(30, 1.25), 38); // 37.5 rounds up
        assert_eq!(scale_by(30, 1.5), 45);
        assert_eq!(scale_by(30, 2.0), 60);
        assert_eq!(scale_by(2, 1.5), 3); // Divider stays visible
        assert_eq!(scale_by(28, 1.75), 49);
    }
}

/// Force-invalidate all widgets with a background brush so WM_PAINT fires.
pub fn force_paint_backgrounds(handle: i64) {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = widgets::get_hwnd_safe(handle) {
            if widgets::get_bg_brush(handle).is_some() {
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, true);
                    let _ = UpdateWindow(hwnd);
                }
            }
        }
        if let Some(info) = widgets::get_widget_info(handle) {
            if !info.hidden {
                for &child in &info.children {
                    force_paint_backgrounds(child);
                }
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}
