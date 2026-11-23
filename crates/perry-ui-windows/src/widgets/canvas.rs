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
use perry_ui::canvas_font::{parse_canvas_font, CanvasFont};

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
    ClearRect(f64, f64, f64, f64), // x, y, w, h
    SetFillColor(u8, u8, u8, u8),
    SetFont(CanvasFont),
    FillText(String, f64, f64),
    SetStrokeColor(u8, u8, u8, u8),
    SetLineWidth(f64),
    FillRect(f64, f64, f64, f64),   // x, y, w, h
    StrokeRect(f64, f64, f64, f64), // x, y, w, h
    Arc(f64, f64, f64, f64, f64),   // x, y, r, start, end
    ClosePath,
    FillPath,
    StrokePath,
}

fn command_batch_renders(commands: &[DrawCmd]) -> bool {
    commands.iter().any(|cmd| {
        matches!(
            cmd,
            DrawCmd::Clear
                | DrawCmd::Stroke(..)
                | DrawCmd::FillGradient(..)
                | DrawCmd::DrawImage(..)
                | DrawCmd::ClearRect(..)
                | DrawCmd::FillText(..)
                | DrawCmd::FillRect(..)
                | DrawCmd::StrokeRect(..)
                | DrawCmd::FillPath
                | DrawCmd::StrokePath
        )
    })
}

/// Prefixes `commands` with the drawing state earlier batches left set, so a saved batch
/// replays alone and the state lasts as in HTML canvas: a setter until the next one, the
/// current path until the next BeginPath. Records the state `commands` leave into `state`.
fn carry_state(state: &mut Vec<DrawCmd>, commands: Vec<DrawCmd>) -> Vec<DrawCmd> {
    let mut batch = state.clone();
    let is_path = |cmd: &DrawCmd| {
        matches!(
            cmd,
            DrawCmd::MoveTo(..) | DrawCmd::LineTo(..) | DrawCmd::Arc(..) | DrawCmd::ClosePath
        )
    };
    for cmd in &commands {
        if matches!(cmd, DrawCmd::BeginPath) {
            state.retain(|kept| !is_path(kept));
        } else if is_path(cmd) {
            state.push(cmd.clone());
        } else if matches!(
            cmd,
            DrawCmd::SetFillColor(..)
                | DrawCmd::SetStrokeColor(..)
                | DrawCmd::SetLineWidth(_)
                | DrawCmd::SetFont(_)
        ) {
            state.retain(|kept| std::mem::discriminant(kept) != std::mem::discriminant(cmd));
            state.push(cmd.clone());
        }
    }
    batch.extend(commands);
    batch
}

thread_local! {
    static CANVAS_CMDS: RefCell<HashMap<i64, Vec<DrawCmd>>> = RefCell::new(HashMap::new());
    static CANVAS_LAST_FRAME: RefCell<HashMap<i64, Vec<DrawCmd>>> = RefCell::new(HashMap::new());
    /// Drawing state in effect after the last taken batch; see `carry_state`.
    static CANVAS_STATE: RefCell<HashMap<i64, Vec<DrawCmd>>> = RefCell::new(HashMap::new());
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

fn resolved_image_promise(handle: i64, width: f64, height: f64) -> i64 {
    perry_ffi::runtime_layouts::image_result(handle, width, height)
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
                        let commands = CANVAS_STATE.with(|s| {
                            carry_state(
                                s.borrow_mut().entry(handle).or_default(),
                                std::mem::take(pending),
                            )
                        });
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
            let mut cur_fill = (0u8, 0u8, 0u8);
            let mut cur_stroke = (0u8, 0u8, 0u8);
            let mut cur_width = 1.0;
            let mut cur_font = CanvasFont::default();
            // The path the stateful fill()/stroke() render: its MoveTo, LineTo, Arc and ClosePath.
            let mut path: Vec<DrawCmd> = Vec::new();

            for cmd in cmd_list {
                match cmd {
                    DrawCmd::Clear => {
                        let mut rect = RECT::default();
                        let _ = GetClientRect(hwnd, &mut rect);
                        let brush = GetStockObject(WHITE_BRUSH);
                        let _ = FillRect(hdc, &rect, HBRUSH(brush.0));
                    }
                    DrawCmd::ClearRect(x, y, w, h) => {
                        // A negative width or height extends the rectangle the other way;
                        // Win32 needs ordered endpoints.
                        let rect = RECT {
                            left: x.min(x + w) as i32,
                            top: y.min(y + h) as i32,
                            right: x.max(x + w) as i32,
                            bottom: y.max(y + h) as i32,
                        };
                        let brush = GetStockObject(WHITE_BRUSH);
                        let _ = FillRect(hdc, &rect, HBRUSH(brush.0));
                    }
                    DrawCmd::BeginPath => {
                        path_points.clear();
                        path.clear();
                    }
                    DrawCmd::SetFillColor(r, g, b, _a) => {
                        cur_fill = (r, g, b);
                    }
                    DrawCmd::SetFont(font) => {
                        cur_font = font;
                    }
                    DrawCmd::FillText(text, x, y) => {
                        let hfont = create_canvas_font(hdc, &cur_font);
                        let old_font = SelectObject(hdc, hfont.into());
                        let (r, g, b) = cur_fill;
                        SetTextColor(
                            hdc,
                            COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16)),
                        );
                        SetBkMode(hdc, TRANSPARENT);
                        SetTextAlign(hdc, TA_BASELINE | TA_LEFT);
                        let wide: Vec<u16> = text.encode_utf16().collect();
                        let _ = TextOutW(hdc, x as i32, y as i32, &wide);
                        SelectObject(hdc, old_font);
                        let _ = DeleteObject(hfont.into());
                    }
                    // Path commands draw nothing until a stroke or fill renders them.
                    DrawCmd::MoveTo(x, y) => {
                        path_points.push((x as i32, y as i32));
                        path.push(DrawCmd::MoveTo(x, y));
                    }
                    DrawCmd::LineTo(x, y) => {
                        path_points.push((x as i32, y as i32));
                        path.push(DrawCmd::LineTo(x, y));
                    }
                    DrawCmd::Arc(x, y, r, start, end) => {
                        path_points.extend(
                            arc_chords(x, y, r, start, end)
                                .into_iter()
                                .map(|(x, y)| (x as i32, y as i32)),
                        );
                        path.push(DrawCmd::Arc(x, y, r, start, end));
                    }
                    DrawCmd::ClosePath => path.push(DrawCmd::ClosePath),
                    DrawCmd::SetStrokeColor(r, g, b, _a) => {
                        cur_stroke = (r, g, b);
                    }
                    DrawCmd::SetLineWidth(w) => {
                        cur_width = w;
                    }
                    DrawCmd::FillRect(x, y, w, h) => {
                        let rect = RECT {
                            left: x.min(x + w).round() as i32,
                            top: y.min(y + h).round() as i32,
                            right: x.max(x + w).round() as i32,
                            bottom: y.max(y + h).round() as i32,
                        };
                        let brush = CreateSolidBrush(colorref(cur_fill));
                        let _ = FillRect(hdc, &rect, brush);
                        let _ = DeleteObject(brush.into());
                    }
                    DrawCmd::StrokeRect(x, y, w, h) => {
                        let corners = vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
                        stroke_subpaths(hdc, &[(corners, true)], colorref(cur_stroke), cur_width);
                    }
                    DrawCmd::FillPath => {
                        fill_subpaths(hdc, &flatten_path(&path), colorref(cur_fill));
                    }
                    DrawCmd::StrokePath => {
                        stroke_subpaths(hdc, &flatten_path(&path), colorref(cur_stroke), cur_width);
                    }
                    DrawCmd::Stroke(r, g, b, _a, width) => {
                        let color = COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16));
                        let pen = CreatePen(PS_SOLID, width as i32, color);
                        let old_pen = SelectObject(hdc, pen.into());

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
                            let _ = DeleteObject(current_pen.into());
                        }
                        current_pen = pen;
                    }
                    DrawCmd::FillGradient(r1, g1, b1, _a1, r2, g2, b2, _a2, direction) => {
                        if path_points.len() < 2 {
                            continue;
                        }
                        let mut rect = RECT::default();
                        let _ = GetClientRect(hwnd, &mut rect);
                        // Clipped to the area between the path and the bottom edge, as an area chart.
                        let mut area: Vec<POINT> =
                            path_points.iter().map(|&(x, y)| POINT { x, y }).collect();
                        area.push(POINT {
                            x: path_points[path_points.len() - 1].0,
                            y: rect.bottom,
                        });
                        area.push(POINT {
                            x: path_points[0].0,
                            y: rect.bottom,
                        });
                        let clip = CreatePolygonRgn(&area, WINDING);
                        SelectClipRgn(hdc, Some(clip));
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
                            let _ = DeleteObject(brush.into());
                        }
                        SelectClipRgn(hdc, None);
                        let _ = DeleteObject(clip.into());
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
                                            0_isize,
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
                let _ = DeleteObject(current_pen.into());
            }
        }

        let _ = EndPaint(hwnd, &ps);
    }
}

#[cfg(target_os = "windows")]
fn colorref((r, g, b): (u8, u8, u8)) -> COLORREF {
    COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))
}

#[cfg(target_os = "windows")]
fn gdi_point((x, y): (f64, f64)) -> POINT {
    POINT {
        x: x.round() as i32,
        y: y.round() as i32,
    }
}

/// Fills the subpaths as one nonzero-winding region, without an outline.
#[cfg(target_os = "windows")]
unsafe fn fill_subpaths(hdc: HDC, subpaths: &[(Vec<(f64, f64)>, bool)], color: COLORREF) {
    let polygons: Vec<&Vec<(f64, f64)>> = subpaths
        .iter()
        .map(|(points, _)| points)
        .filter(|points| points.len() >= 2)
        .collect();
    if polygons.is_empty() {
        return;
    }
    let points: Vec<POINT> = polygons
        .iter()
        .flat_map(|points| points.iter().copied().map(gdi_point))
        .collect();
    let counts: Vec<i32> = polygons.iter().map(|points| points.len() as i32).collect();
    let brush = CreateSolidBrush(color);
    let old_brush = SelectObject(hdc, brush.into());
    let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
    SetPolyFillMode(hdc, WINDING);
    let _ = PolyPolygon(hdc, points.as_ptr(), &counts);
    SelectObject(hdc, old_pen);
    SelectObject(hdc, old_brush);
    let _ = DeleteObject(brush.into());
}

#[cfg(target_os = "windows")]
unsafe fn stroke_subpaths(
    hdc: HDC,
    subpaths: &[(Vec<(f64, f64)>, bool)],
    color: COLORREF,
    width: f64,
) {
    let pen = CreatePen(PS_SOLID, width.round() as i32, color);
    let old_pen = SelectObject(hdc, pen.into());
    for (points, closed) in subpaths {
        if points.len() < 2 {
            continue;
        }
        let mut line: Vec<POINT> = points.iter().copied().map(gdi_point).collect();
        if *closed {
            line.push(line[0]);
        }
        let _ = Polyline(hdc, &line);
    }
    SelectObject(hdc, old_pen);
    let _ = DeleteObject(pen.into());
}

/// The end angle HTML canvas `arc` reaches from `start` going clockwise: a span of 2π or
/// more is the whole circle, any other span is taken modulo 2π.
#[cfg(any(target_os = "windows", test))]
fn clockwise_end(start: f64, end: f64) -> f64 {
    let tau = std::f64::consts::TAU;
    if end - start >= tau {
        start + tau
    } else {
        start + (end - start).rem_euclid(tau)
    }
}

/// The points of an HTML canvas `arc`, as chords of about two units, at least 8 per full turn,
/// for the point list the colored stroke() and fillGradient read.
#[cfg(any(target_os = "windows", test))]
fn arc_chords(x: f64, y: f64, r: f64, start: f64, end: f64) -> Vec<(f64, f64)> {
    let sweep = clockwise_end(start, end) - start;
    let min_steps = (8.0 * sweep / std::f64::consts::TAU).ceil().max(1.0) as usize;
    let steps = ((sweep * r / 2.0).ceil() as usize).clamp(min_steps, 1024);
    (0..=steps)
        .map(move |i| {
            let angle = start + sweep * i as f64 / steps as f64;
            (x + r * angle.cos(), y + r * angle.sin())
        })
        .collect()
}

/// The path's subpaths as point lists, each with whether it is closed, since GDI draws
/// polylines rather than canvas paths. As on HTML canvas, a LineTo with no current point starts
/// a subpath, an arc runs clockwise (y down) and joins the current point to its start, and
/// closing a subpath starts the next one at its first point.
#[cfg(any(target_os = "windows", test))]
fn flatten_path(path: &[DrawCmd]) -> Vec<(Vec<(f64, f64)>, bool)> {
    fn extend(subpaths: &mut Vec<(Vec<(f64, f64)>, bool)>, points: Vec<(f64, f64)>) {
        match subpaths.last_mut() {
            Some((current, _)) => current.extend(points),
            None => subpaths.push((points, false)),
        }
    }
    let mut subpaths: Vec<(Vec<(f64, f64)>, bool)> = Vec::new();
    for cmd in path {
        match *cmd {
            DrawCmd::MoveTo(x, y) => subpaths.push((vec![(x, y)], false)),
            DrawCmd::LineTo(x, y) => extend(&mut subpaths, vec![(x, y)]),
            DrawCmd::Arc(x, y, r, start, end) => {
                extend(&mut subpaths, arc_chords(x, y, r, start, end))
            }
            DrawCmd::ClosePath => {
                if let Some((points, closed)) = subpaths.last_mut() {
                    *closed = true;
                    let first = points[0];
                    subpaths.push((vec![first], false));
                }
            }
            _ => {}
        }
    }
    subpaths
}

/// The first family GDI has, generics mapped to Windows faces, else Segoe UI.
#[cfg(target_os = "windows")]
unsafe fn create_canvas_font(hdc: HDC, font: &CanvasFont) -> HFONT {
    let weight = (if font.bold { FW_BOLD } else { FW_NORMAL }).0 as i32;
    let create = |face: &str| {
        let wide = to_wide(face);
        CreateFontW(
            -(font.size.round() as i32), // negative: character height, the CSS font size
            0,
            0,
            0,
            weight,
            font.italic as u32,
            0,
            0,
            FONT_CHARSET(0),
            FONT_OUTPUT_PRECISION(0),
            FONT_CLIP_PRECISION(0),
            FONT_QUALITY(0),
            0,
            windows::core::PCWSTR(wide.as_ptr()),
        )
    };
    for family in &font.families {
        let (face, generic) = match family.to_ascii_lowercase().as_str() {
            "sans-serif" | "system-ui" => ("Segoe UI", true),
            "serif" => ("Times New Roman", true),
            "monospace" => ("Consolas", true),
            _ => (family.as_str(), false),
        };
        let hfont = create(face);
        if generic {
            return hfont;
        }
        // GDI substitutes a missing face; the selected face names the one it used.
        let old = SelectObject(hdc, hfont.into());
        let mut name = [0u16; 32];
        GetTextFaceW(hdc, Some(&mut name));
        SelectObject(hdc, old);
        let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        if String::from_utf16_lossy(&name[..len]).eq_ignore_ascii_case(face) {
            return hfont;
        }
        let _ = DeleteObject(hfont.into());
    }
    create("Segoe UI")
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
                Some(super::get_parking_hwnd()),
                None,
                Some(HINSTANCE::from(hinstance)),
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
                let _ = InvalidateRect(Some(hwnd), None, false);
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
            CANVAS_STATE.with(|s| {
                carry_state(
                    s.borrow_mut().entry(handle).or_default(),
                    std::mem::take(list),
                )
            });
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

/// Clear a rectangle to the background, keeping what earlier commands drew outside it.
pub fn clear_rect(handle: i64, x: f64, y: f64, w: f64, h: f64) {
    push_cmd(handle, DrawCmd::ClearRect(x, y, w, h));
    invalidate(handle);
}

pub fn set_fill_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    push_cmd(
        handle,
        DrawCmd::SetFillColor(
            (r * 255.0) as u8,
            (g * 255.0) as u8,
            (b * 255.0) as u8,
            (a * 255.0) as u8,
        ),
    );
}

/// An invalid font spec keeps the previous font, as HTML canvas does.
pub fn set_font(handle: i64, spec_ptr: *const u8) {
    let spec = unsafe { crate::widgets::image::str_from_header(spec_ptr) };
    if let Some(font) = parse_canvas_font(&spec) {
        push_cmd(handle, DrawCmd::SetFont(font));
    }
}

pub fn fill_text(handle: i64, text_ptr: *const u8, x: f64, y: f64) {
    if !(x.is_finite() && y.is_finite()) {
        return;
    }
    let text = unsafe { crate::widgets::image::str_from_header(text_ptr) };
    push_cmd(handle, DrawCmd::FillText(text, x, y));
    invalidate(handle);
}

pub fn set_stroke_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    push_cmd(
        handle,
        DrawCmd::SetStrokeColor(
            (r * 255.0) as u8,
            (g * 255.0) as u8,
            (b * 255.0) as u8,
            (a * 255.0) as u8,
        ),
    );
}

pub fn set_line_width(handle: i64, w: f64) {
    push_cmd(handle, DrawCmd::SetLineWidth(w));
}

pub fn fill_rect(handle: i64, x: f64, y: f64, w: f64, h: f64) {
    push_cmd(handle, DrawCmd::FillRect(x, y, w, h));
    invalidate(handle);
}

pub fn stroke_rect(handle: i64, x: f64, y: f64, w: f64, h: f64) {
    push_cmd(handle, DrawCmd::StrokeRect(x, y, w, h));
    invalidate(handle);
}

/// A non-finite argument or a negative radius records nothing; HTML canvas ignores the first
/// and throws on the second.
pub fn arc(handle: i64, x: f64, y: f64, r: f64, start: f64, end: f64) {
    if [x, y, r, start, end].iter().all(|v| v.is_finite()) && r >= 0.0 {
        push_cmd(handle, DrawCmd::Arc(x, y, r, start, end));
    }
}

pub fn close_path(handle: i64) {
    push_cmd(handle, DrawCmd::ClosePath);
}

pub fn fill(handle: i64) {
    push_cmd(handle, DrawCmd::FillPath);
    invalidate(handle);
}

pub fn stroke_path(handle: i64) {
    push_cmd(handle, DrawCmd::StrokePath);
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
        let path = unsafe { crate::widgets::image::str_from_header(path_ptr) };
        let resolved = resolve_asset_path(&path);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, PI, TAU};

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
    }

    #[test]
    fn arcs_sweep_clockwise_as_html_canvas_does() {
        assert_eq!(clockwise_end(0.0, FRAC_PI_2), FRAC_PI_2);
        assert!((clockwise_end(FRAC_PI_2, 0.0) - TAU).abs() < 1e-9);
        assert!((clockwise_end(0.0, -FRAC_PI_2) - 3.0 * FRAC_PI_2).abs() < 1e-9);
        assert_eq!(clockwise_end(0.0, TAU), TAU);
        assert_eq!(clockwise_end(PI, 4.0 * PI), PI + TAU);
    }

    #[test]
    fn flattened_path_joins_arcs_and_restarts_after_close() {
        let subpaths = flatten_path(&[
            DrawCmd::MoveTo(0.0, 0.0),
            DrawCmd::LineTo(10.0, 0.0),
            DrawCmd::Arc(10.0, 10.0, 10.0, -FRAC_PI_2, 0.0),
            DrawCmd::ClosePath,
            DrawCmd::LineTo(0.0, 30.0),
        ]);
        assert_eq!(subpaths.len(), 2);
        let (first, closed) = &subpaths[0];
        assert!(*closed);
        assert!(close(first[0], (0.0, 0.0)));
        assert!(close(first[1], (10.0, 0.0)));
        assert!(
            close(first[2], (10.0, 0.0)),
            "the arc starts at its start angle"
        );
        assert!(close(*first.last().unwrap(), (20.0, 10.0)));
        assert!(first.iter().all(|&(x, _)| x >= 0.0 && x <= 20.0 + 1e-9));
        assert_eq!(subpaths[1], (vec![(0.0, 0.0), (0.0, 30.0)], false));
    }

    #[test]
    fn a_small_full_circle_keeps_its_area() {
        let subpaths = flatten_path(&[DrawCmd::Arc(0.0, 0.0, 0.6, 0.0, TAU)]);
        assert_eq!(subpaths.len(), 1);
        assert!(subpaths[0].0.len() >= 9, "at least 8 chords per full turn");
    }

    #[test]
    fn line_to_without_a_current_point_starts_a_subpath() {
        let subpaths = flatten_path(&[DrawCmd::LineTo(5.0, 5.0), DrawCmd::LineTo(9.0, 5.0)]);
        assert_eq!(subpaths, vec![(vec![(5.0, 5.0), (9.0, 5.0)], false)]);
    }
}
