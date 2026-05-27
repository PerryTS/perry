//! Canvas widget — custom window class with GDI-based drawing via command buffer
//! Draw commands are accumulated and replayed in WM_PAINT.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicI64, Ordering};

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
    DrawImage(i64, f64, f64, f64, f64, f64, f64, f64, f64),
    // image, sx, sy, sw, sh, dx, dy, dw, dh
    Clear,
}

fn command_batch_renders(commands: &[DrawCmd]) -> bool {
    commands.iter().any(|cmd| {
        matches!(
            cmd,
            DrawCmd::Clear
                | DrawCmd::Stroke(..)
                | DrawCmd::FillGradient(..)
                | DrawCmd::DrawImage(..)
        )
    })
}

thread_local! {
    static CANVAS_CMDS: RefCell<HashMap<i64, Vec<DrawCmd>>> = RefCell::new(HashMap::new());
    static CANVAS_LAST_FRAME: RefCell<HashMap<i64, Vec<DrawCmd>>> = RefCell::new(HashMap::new());
    static CANVAS_IMAGE_PATHS: RefCell<HashMap<i64, String>> = RefCell::new(HashMap::new());
    static CANVAS_IMAGE_CACHE: RefCell<HashMap<String, i64>> = RefCell::new(HashMap::new());
    static CANVAS_IMAGE_SIZES: RefCell<HashMap<i64, (f64, f64)>> = RefCell::new(HashMap::new());
}

static NEXT_IMAGE_HANDLE: AtomicI64 = AtomicI64::new(1);

const JS_TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

extern "C" {
    fn js_object_alloc(class_id: u32, field_count: u32) -> *mut c_void;
    fn js_object_set_field_by_name(obj: *mut c_void, key: *const c_void, value: f64);
    fn js_object_get_field_by_name_f64(obj: *mut c_void, key: *const c_void) -> f64;
    fn js_string_from_bytes(data: *const u8, len: u32) -> *mut c_void;
    fn js_nanbox_pointer(ptr: i64) -> f64;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_promise_resolved(value: f64) -> *mut c_void;
    fn js_promise_rejected(reason: f64) -> *mut c_void;
}

fn js_key(name: &[u8]) -> *mut c_void {
    unsafe { js_string_from_bytes(name.as_ptr(), name.len() as u32) }
}

fn set_image_field(obj: *mut c_void, name: &[u8], value: f64) {
    unsafe { js_object_set_field_by_name(obj, js_key(name), value) }
}

fn resolved_image_promise(handle: i64, width: f64, height: f64) -> i64 {
    unsafe {
        let obj = js_object_alloc(0, 4);
        if obj.is_null() {
            let msg = b"Failed to allocate Canvas image object";
            let reason =
                js_nanbox_string(js_string_from_bytes(msg.as_ptr(), msg.len() as u32) as i64);
            return js_promise_rejected(reason) as i64;
        }
        set_image_field(obj, b"__perryImageHandle", handle as f64);
        set_image_field(obj, b"width", width);
        set_image_field(obj, b"height", height);
        set_image_field(obj, b"ready", f64::from_bits(JS_TAG_TRUE));
        js_promise_resolved(js_nanbox_pointer(obj as i64)) as i64
    }
}

fn rejected_image_promise(message: &str) -> i64 {
    unsafe {
        let reason =
            js_nanbox_string(js_string_from_bytes(message.as_ptr(), message.len() as u32) as i64);
        js_promise_rejected(reason) as i64
    }
}

fn image_handle_from_arg(image: i64) -> i64 {
    if image <= 0 {
        return image;
    }
    let key = js_key(b"__perryImageHandle");
    let value = unsafe { js_object_get_field_by_name_f64(image as *mut c_void, key) };
    if value.is_finite() && value > 0.0 {
        value as i64
    } else {
        image
    }
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

        let cmd_list = CANVAS_CMDS
            .with(|cmds| {
                let mut cmds = cmds.borrow_mut();
                cmds.get_mut(&handle).and_then(|pending| {
                    if pending.is_empty() || !command_batch_renders(pending) {
                        None
                    } else {
                        let commands = std::mem::take(pending);
                        CANVAS_LAST_FRAME.with(|last| {
                            last.borrow_mut().insert(handle, commands.clone());
                        });
                        Some(commands)
                    }
                })
            })
            .or_else(|| CANVAS_LAST_FRAME.with(|last| last.borrow().get(&handle).cloned()));

        if let Some(cmd_list) = cmd_list {
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
                        MoveToEx(hdc, x as i32, y as i32, None);
                        path_points.push((x as i32, y as i32));
                    }
                    DrawCmd::LineTo(x, y) => {
                        LineTo(hdc, x as i32, y as i32);
                        path_points.push((x as i32, y as i32));
                    }
                    DrawCmd::Stroke(r, g, b, _a, width) => {
                        let color = COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16));
                        let pen = CreatePen(PS_SOLID, width as i32, color);
                        let old_pen = SelectObject(hdc, pen);

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
                        let mut rect = RECT::default();
                        let _ = GetClientRect(hwnd, &mut rect);
                        let vertical = direction < 0.5;

                        let steps = if vertical {
                            (rect.bottom - rect.top).max(1)
                        } else {
                            (rect.right - rect.left).max(1)
                        };

                        for i in 0..steps {
                            let t = i as f64 / steps as f64;
                            let cr = (r1 as f64 * (1.0 - t) + r2 as f64 * t) as u32;
                            let cg = (g1 as f64 * (1.0 - t) + g2 as f64 * t) as u32;
                            let cb = (b1 as f64 * (1.0 - t) + b2 as f64 * t) as u32;
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
                    DrawCmd::DrawImage(image, sx, sy, sw, sh, dx, dy, dw, dh) => {
                        use windows::Win32::Graphics::GdiPlus::*;

                        let path = CANVAS_IMAGE_PATHS.with(|m| m.borrow().get(&image).cloned());
                        let Some(path) = path else {
                            continue;
                        };
                        let mut token: usize = 0;
                        let input = GdiplusStartupInput {
                            GdiplusVersion: 1,
                            ..Default::default()
                        };
                        if GdiplusStartup(&mut token, &input, std::ptr::null_mut()).0 == 0 {
                            let mut gp_image: *mut GpImage = std::ptr::null_mut();
                            let wide_path = to_wide(&path);
                            let _ = GdipLoadImageFromFile(
                                windows::core::PCWSTR(wide_path.as_ptr()),
                                &mut gp_image,
                            );
                            if !gp_image.is_null() {
                                let mut graphics: *mut GpGraphics = std::ptr::null_mut();
                                GdipCreateFromHDC(hdc, &mut graphics);
                                if !graphics.is_null() {
                                    let mut iw = 0u32;
                                    let mut ih = 0u32;
                                    let _ = GdipGetImageWidth(gp_image, &mut iw);
                                    let _ = GdipGetImageHeight(gp_image, &mut ih);
                                    let src_w = if sw > 0.0 { sw } else { iw as f64 };
                                    let src_h = if sh > 0.0 { sh } else { ih as f64 };
                                    let dst_w = if dw > 0.0 { dw } else { src_w };
                                    let dst_h = if dh > 0.0 { dh } else { src_h };
                                    if src_w > 0.0 && src_h > 0.0 && dst_w > 0.0 && dst_h > 0.0 {
                                        let _ = GdipSetInterpolationMode(
                                            graphics,
                                            InterpolationMode(7),
                                        );
                                        let _ = GdipDrawImageRectRectI(
                                            graphics,
                                            gp_image,
                                            dx as i32,
                                            dy as i32,
                                            dst_w as i32,
                                            dst_h as i32,
                                            sx as i32,
                                            sy as i32,
                                            src_w as i32,
                                            src_h as i32,
                                            Unit(2),
                                            std::ptr::null_mut(),
                                            None,
                                            std::ptr::null_mut(),
                                        );
                                    }
                                    GdipDeleteGraphics(graphics);
                                }
                                GdipDisposeImage(gp_image);
                            }
                            GdiplusShutdown(token);
                        }
                    }
                }
            }

            if !current_pen.is_invalid() {
                let _ = DeleteObject(current_pen);
            }
        }

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
            CANVAS_LAST_FRAME.with(|last| {
                last.borrow_mut().insert(handle, Vec::new());
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
        CANVAS_LAST_FRAME.with(|last| {
            last.borrow_mut().insert(handle, Vec::new());
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
    CANVAS_LAST_FRAME.with(|last| {
        if let Some(list) = last.borrow_mut().get_mut(&handle) {
            list.clear();
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

#[cfg(target_os = "windows")]
fn resolve_asset_path(path: &str) -> String {
    if std::path::Path::new(path).is_absolute() {
        return path.to_string();
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let candidate = exe_dir.join(path);
            if candidate.exists() {
                return candidate.to_string_lossy().to_string();
            }
        }
    }
    path.to_string()
}

pub fn load_image(path_ptr: *const u8) -> i64 {
    #[cfg(target_os = "windows")]
    {
        let path = crate::widgets::image::str_from_header(path_ptr);
        let resolved = resolve_asset_path(path);
        if let Some(handle) = CANVAS_IMAGE_CACHE.with(|c| c.borrow().get(&resolved).copied()) {
            let (width, height) = CANVAS_IMAGE_SIZES
                .with(|sizes| sizes.borrow().get(&handle).copied())
                .unwrap_or((0.0, 0.0));
            return resolved_image_promise(handle, width, height);
        }
        let handle = NEXT_IMAGE_HANDLE.fetch_add(1, Ordering::Relaxed);
        CANVAS_IMAGE_PATHS.with(|m| m.borrow_mut().insert(handle, resolved.clone()));
        CANVAS_IMAGE_SIZES.with(|m| m.borrow_mut().insert(handle, (0.0, 0.0)));
        CANVAS_IMAGE_CACHE.with(|c| c.borrow_mut().insert(resolved, handle));
        resolved_image_promise(handle, 0.0, 0.0)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path_ptr;
        rejected_image_promise("Canvas image loading is only available on Windows builds")
    }
}

pub fn draw_image(
    handle: i64,
    image: i64,
    sx: f64,
    sy: f64,
    sw: f64,
    sh: f64,
    dx: f64,
    dy: f64,
    dw: f64,
    dh: f64,
) {
    let image = image_handle_from_arg(image);
    push_cmd(
        handle,
        DrawCmd::DrawImage(image, sx, sy, sw, sh, dx, dy, dw, dh),
    );
    invalidate(handle);
}
