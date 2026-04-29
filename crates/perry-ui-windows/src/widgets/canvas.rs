//! Canvas widget — custom window class with GDI-based drawing via command buffer
//! Draw commands are accumulated and replayed in WM_PAINT.

use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{register_widget_with_layout, WidgetKind};

/// Drawing commands accumulated and replayed in WM_PAINT.
#[derive(Clone, Debug)]
pub enum DrawCmd {
    BeginPath,
    MoveTo(f64, f64),
    LineTo(f64, f64),
    Stroke(u8, u8, u8, u8, f64), // r, g, b, a, line_width
    FillGradient(u8, u8, u8, u8, u8, u8, u8, u8, f64),
    // r1, g1, b1, a1, r2, g2, b2, a2, direction (0=vertical, 1=horizontal)
    Clear,
}

thread_local! {
    static CANVAS_CMDS: RefCell<HashMap<i64, Vec<DrawCmd>>> = RefCell::new(HashMap::new());
}

#[cfg(target_os = "windows")]
static CANVAS_CLASS_REGISTERED: std::sync::Once = std::sync::Once::new();

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
fn ensure_class_registered() {
    CANVAS_CLASS_REGISTERED.call_once(|| unsafe {
        let hinstance = GetModuleHandleW(None).unwrap();
        let class_name = to_wide("PerryCanvas");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(canvas_wnd_proc),
            hInstance: hinstance.into(),
            hbrBackground: HBRUSH(unsafe { GetStockObject(WHITE_BRUSH) }.0),
            lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassExW(&wc);
    });
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn canvas_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let handle = super::find_handle_by_hwnd(hwnd);
            if handle > 0 {
                paint_canvas(handle, hwnd);
            }
            // We must call BeginPaint/EndPaint even if we drew nothing, to validate the region
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[cfg(target_os = "windows")]
fn paint_canvas(handle: i64, hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);

        CANVAS_CMDS.with(|cmds| {
            let cmds = cmds.borrow();
            if let Some(cmd_list) = cmds.get(&handle) {
                let mut current_pen = HPEN::default();
                let mut path_points: Vec<(i32, i32)> = Vec::new();

                for cmd in cmd_list {
                    match cmd {
                        DrawCmd::Clear => {
                            let mut rect = RECT::default();
                            let _ = GetClientRect(hwnd, &mut rect);
                            let brush = GetStockObject(WHITE_BRUSH);
                            let _ = FillRect(hdc, &rect, HBRUSH(brush.0));
                        }
                        DrawCmd::BeginPath => {
                            path_points.clear();
                        }
                        DrawCmd::MoveTo(x, y) => {
                            MoveToEx(hdc, *x as i32, *y as i32, None);
                            path_points.push((*x as i32, *y as i32));
                        }
                        DrawCmd::LineTo(x, y) => {
                            LineTo(hdc, *x as i32, *y as i32);
                            path_points.push((*x as i32, *y as i32));
                        }
                        DrawCmd::Stroke(r, g, b, _a, width) => {
                            let color =
                                COLORREF((*r as u32) | ((*g as u32) << 8) | ((*b as u32) << 16));
                            let pen = CreatePen(PS_SOLID, *width as i32, color);
                            let old_pen = SelectObject(hdc, pen);

                            // Replay path with the pen
                            let mut first = true;
                            for &(px, py) in &path_points {
                                if first {
                                    MoveToEx(hdc, px, py, None);
                                    first = false;
                                } else {
                                    LineTo(hdc, px, py);
                                }
                            }

                            SelectObject(hdc, old_pen);
                            if !current_pen.is_invalid() {
                                let _ = DeleteObject(current_pen);
                            }
                            current_pen = pen;
                        }
                        DrawCmd::FillGradient(r1, g1, b1, _a1, r2, g2, b2, _a2, direction) => {
                            // Fill entire canvas with gradient
                            let mut rect = RECT::default();
                            let _ = GetClientRect(hwnd, &mut rect);
                            let vertical = *direction < 0.5;

                            let steps = if vertical {
                                (rect.bottom - rect.top).max(1)
                            } else {
                                (rect.right - rect.left).max(1)
                            };

                            for i in 0..steps {
                                let t = i as f64 / steps as f64;
                                let cr = (*r1 as f64 * (1.0 - t) + *r2 as f64 * t) as u32;
                                let cg = (*g1 as f64 * (1.0 - t) + *g2 as f64 * t) as u32;
                                let cb = (*b1 as f64 * (1.0 - t) + *b2 as f64 * t) as u32;
                                let color = COLORREF(cr | (cg << 8) | (cb << 16));
                                let brush = CreateSolidBrush(color);
                                let band = if vertical {
                                    RECT {
                                        left: rect.left,
                                        top: rect.top + i,
                                        right: rect.right,
                                        bottom: rect.top + i + 1,
                                    }
                                } else {
                                    RECT {
                                        left: rect.left + i,
                                        top: rect.top,
                                        right: rect.left + i + 1,
                                        bottom: rect.bottom,
                                    }
                                };
                                let _ = FillRect(hdc, &band, brush);
                                let _ = DeleteObject(brush);
                            }
                        }
                    }
                }

                if !current_pen.is_invalid() {
                    let _ = DeleteObject(current_pen);
                }
            }
        });

        let _ = EndPaint(hwnd, &ps);
    }
}

/// Create a Canvas with given width and height. Returns widget handle.
pub fn create(width: f64, height: f64) -> i64 {
    #[cfg(target_os = "windows")]
    {
        ensure_class_registered();
        let class_name = to_wide("PerryCanvas");
        let window_text = to_wide("");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(window_text.as_ptr()),
                WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN,
                0,
                0,
                width as i32,
                height as i32,
                super::get_parking_hwnd(),
                None,
                HINSTANCE::from(hinstance),
                None,
            )
            .unwrap();

            let handle =
                register_widget_with_layout(hwnd, WidgetKind::Canvas, 0.0, (0.0, 0.0, 0.0, 0.0));
            CANVAS_CMDS.with(|cmds| {
                cmds.borrow_mut().insert(handle, Vec::new());
            });
            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (width, height);
        let handle = register_widget_with_layout(0, WidgetKind::Canvas, 0.0, (0.0, 0.0, 0.0, 0.0));
        CANVAS_CMDS.with(|cmds| {
            cmds.borrow_mut().insert(handle, Vec::new());
        });
        handle
    }
}

fn push_cmd(handle: i64, cmd: DrawCmd) {
    CANVAS_CMDS.with(|cmds| {
        let mut cmds = cmds.borrow_mut();
        if let Some(list) = cmds.get_mut(&handle) {
            list.push(cmd);
        }
    });
}

fn invalidate(handle: i64) {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}

/// Clear all drawing commands and repaint.
pub fn clear(handle: i64) {
    CANVAS_CMDS.with(|cmds| {
        let mut cmds = cmds.borrow_mut();
        if let Some(list) = cmds.get_mut(&handle) {
            list.clear();
            list.push(DrawCmd::Clear);
        }
    });
    invalidate(handle);
}

/// Begin a new path (resets current path points).
pub fn begin_path(handle: i64) {
    push_cmd(handle, DrawCmd::BeginPath);
}

/// Move the current point to (x, y).
pub fn move_to(handle: i64, x: f64, y: f64) {
    push_cmd(handle, DrawCmd::MoveTo(x, y));
}

/// Draw a line from the current point to (x, y).
pub fn line_to(handle: i64, x: f64, y: f64) {
    push_cmd(handle, DrawCmd::LineTo(x, y));
}

/// Stroke the current path with the given color and line width, then repaint.
pub fn stroke(handle: i64, r: f64, g: f64, b: f64, a: f64, line_width: f64) {
    push_cmd(
        handle,
        DrawCmd::Stroke(
            (r * 255.0) as u8,
            (g * 255.0) as u8,
            (b * 255.0) as u8,
            (a * 255.0) as u8,
            line_width,
        ),
    );
    invalidate(handle);
}

/// Fill the canvas with a gradient. direction: 0=vertical, 1=horizontal.
pub fn fill_gradient(
    handle: i64,
    r1: f64,
    g1: f64,
    b1: f64,
    a1: f64,
    r2: f64,
    g2: f64,
    b2: f64,
    a2: f64,
    direction: f64,
) {
    push_cmd(
        handle,
        DrawCmd::FillGradient(
            (r1 * 255.0) as u8,
            (g1 * 255.0) as u8,
            (b1 * 255.0) as u8,
            (a1 * 255.0) as u8,
            (r2 * 255.0) as u8,
            (g2 * 255.0) as u8,
            (b2 * 255.0) as u8,
            (a2 * 255.0) as u8,
            direction,
        ),
    );
    invalidate(handle);
}
