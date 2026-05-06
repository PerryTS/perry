//! Chart widget — custom owner-draw window class `PerryChart` rendering
//! line / bar / pie via GDI primitives in WM_PAINT (issue #474 / Windows
//! parity work).
//!
//! Mirrors the macOS / GTK4 visual conventions (24px padding,
//! 8-color palette, top-aligned title) so the three impls stay close
//! to pixel-equivalent. GDI primitives used: `Polyline` for line
//! charts, `Rectangle` (or `FillRect`) for bar fills, `Pie` for the
//! built-in pie wedge primitive.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Once;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{alloc_control_id, register_widget, WidgetKind};

#[repr(i64)]
#[derive(Copy, Clone)]
pub enum ChartKind {
    Line = 0,
    Bar = 1,
    Pie = 2,
}

struct ChartEntry {
    kind: ChartKind,
    title: String,
    data: Vec<(String, f64)>,
}

thread_local! {
    static CHARTS: RefCell<HashMap<i64, ChartEntry>> = RefCell::new(HashMap::new());
}

static CHART_CLASS_REGISTERED: Once = Once::new();

const PADDING: i32 = 24;
const PALETTE_RGB: &[(u8, u8, u8)] = &[
    (51, 140, 235),
    (240, 97, 102),
    (82, 199, 107),
    (242, 176, 51),
    (158, 107, 219),
    (46, 189, 199),
    (219, 102, 158),
    (140, 140, 140),
];

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

#[cfg(target_os = "windows")]
fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))
}

#[cfg(target_os = "windows")]
fn ensure_class_registered() {
    CHART_CLASS_REGISTERED.call_once(|| unsafe {
        let hinstance = GetModuleHandleW(None).unwrap();
        let class_name = to_wide("PerryChart");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(chart_wnd_proc),
            hInstance: hinstance.into(),
            hbrBackground: HBRUSH(GetStockObject(WHITE_BRUSH).0),
            lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassExW(&wc);
    });
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn chart_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let handle = super::find_handle_by_hwnd(hwnd);
            if handle > 0 {
                paint_chart(handle, hwnd);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[cfg(target_os = "windows")]
fn paint_chart(handle: i64, hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        // White background.
        let _ = FillRect(hdc, &client, HBRUSH(GetStockObject(WHITE_BRUSH).0));

        let snapshot = CHARTS.with(|c| {
            c.borrow()
                .get(&handle)
                .map(|e| (e.kind as i64, e.title.clone(), e.data.clone()))
        });
        if let Some((kind, title, data)) = snapshot {
            let title_band = if title.is_empty() { 0 } else { 22 };
            let plot = RECT {
                left: client.left + PADDING,
                top: client.top + PADDING + title_band,
                right: client.right - PADDING,
                bottom: client.bottom - PADDING,
            };
            if !title.is_empty() {
                draw_title(hdc, &client, &title);
            }
            if !data.is_empty() && plot.right > plot.left && plot.bottom > plot.top {
                match kind {
                    0 => draw_line(hdc, &plot, &data),
                    1 => draw_bars(hdc, &plot, &data),
                    2 => draw_pie(hdc, &plot, &data),
                    _ => {}
                }
            }
        }

        let _ = EndPaint(hwnd, &ps);
    }
}

#[cfg(target_os = "windows")]
unsafe fn draw_title(hdc: HDC, client: &RECT, title: &str) {
    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, rgb(25, 25, 25));
    let wide = to_wide(title);
    let mut rect = RECT {
        left: client.left,
        top: client.top + 4,
        right: client.right,
        bottom: client.top + 22,
    };
    DrawTextW(
        hdc,
        &wide[..wide.len() - 1],
        &mut rect,
        DT_CENTER | DT_SINGLELINE | DT_VCENTER,
    );
}

#[cfg(target_os = "windows")]
unsafe fn draw_line(hdc: HDC, plot: &RECT, data: &[(String, f64)]) {
    let max = data.iter().map(|(_, v)| *v).fold(f64::NEG_INFINITY, f64::max);
    let min = data.iter().map(|(_, v)| *v).fold(f64::INFINITY, f64::min);
    let range = if (max - min).abs() < 1e-9 { 1.0 } else { max - min };
    let n = data.len();
    let plot_w = (plot.right - plot.left) as f64;
    let plot_h = (plot.bottom - plot.top) as f64;
    let dx = if n > 1 { plot_w / (n as f64 - 1.0) } else { 0.0 };

    // Baseline along the bottom of the plot rect.
    let axis_pen = CreatePen(PS_SOLID, 1, rgb(178, 178, 178));
    let old = SelectObject(hdc, axis_pen);
    MoveToEx(hdc, plot.left, plot.bottom, None);
    let _ = LineTo(hdc, plot.right, plot.bottom);
    SelectObject(hdc, old);
    let _ = DeleteObject(axis_pen);

    // Series.
    let (r, g, b) = PALETTE_RGB[0];
    let pen = CreatePen(PS_SOLID, 2, rgb(r, g, b));
    let old = SelectObject(hdc, pen);
    for (i, (_, v)) in data.iter().enumerate() {
        let px = plot.left + (i as f64 * dx) as i32;
        // Win32 GDI origin is top-left like Cairo; flip y so big values
        // render higher.
        let py = plot.bottom - ((v - min) / range * plot_h) as i32;
        if i == 0 {
            MoveToEx(hdc, px, py, None);
        } else {
            let _ = LineTo(hdc, px, py);
        }
    }
    SelectObject(hdc, old);
    let _ = DeleteObject(pen);
}

#[cfg(target_os = "windows")]
unsafe fn draw_bars(hdc: HDC, plot: &RECT, data: &[(String, f64)]) {
    let max = data.iter().map(|(_, v)| *v).fold(0.0_f64, f64::max);
    if max <= 0.0 {
        return;
    }
    let n = data.len();
    let plot_w = (plot.right - plot.left) as f64;
    let plot_h = (plot.bottom - plot.top) as f64;
    let bar_w = plot_w / (n as f64) * 0.7;
    let gap = plot_w / (n as f64) * 0.3;
    for (i, (_, v)) in data.iter().enumerate() {
        let bh = (v / max) * plot_h;
        let bx = plot.left + ((i as f64) * (bar_w + gap) + gap / 2.0) as i32;
        let by = plot.bottom - bh as i32;
        let bw_i = bar_w as i32;
        let (r, g, b) = PALETTE_RGB[i % PALETTE_RGB.len()];
        let brush = CreateSolidBrush(rgb(r, g, b));
        let bar_rect = RECT {
            left: bx,
            top: by,
            right: bx + bw_i,
            bottom: plot.bottom,
        };
        let _ = FillRect(hdc, &bar_rect, brush);
        let _ = DeleteObject(brush);
    }
}

#[cfg(target_os = "windows")]
unsafe fn draw_pie(hdc: HDC, plot: &RECT, data: &[(String, f64)]) {
    let total: f64 = data.iter().map(|(_, v)| *v).sum();
    if total <= 0.0 {
        return;
    }
    let plot_w = (plot.right - plot.left) as i32;
    let plot_h = (plot.bottom - plot.top) as i32;
    let cx = plot.left + plot_w / 2;
    let cy = plot.top + plot_h / 2;
    let radius = (plot_w.min(plot_h) / 2 - 4).max(8);
    let bb_left = cx - radius;
    let bb_top = cy - radius;
    let bb_right = cx + radius;
    let bb_bottom = cy + radius;

    // GDI Pie() takes (left, top, right, bottom, x_radial1, y_radial1,
    // x_radial2, y_radial2). Angles are computed by extending unit
    // vectors from centre out to the bounding-box edge.
    let mut start_angle = -std::f64::consts::FRAC_PI_2;
    for (i, (_, v)) in data.iter().enumerate() {
        let frac = v / total;
        let end_angle = start_angle + frac * std::f64::consts::TAU;

        let r = radius as f64 + 16.0;
        let x1 = cx + (r * start_angle.cos()) as i32;
        let y1 = cy + (r * start_angle.sin()) as i32;
        let x2 = cx + (r * end_angle.cos()) as i32;
        let y2 = cy + (r * end_angle.sin()) as i32;

        let (cr, cg, cb) = PALETTE_RGB[i % PALETTE_RGB.len()];
        let brush = CreateSolidBrush(rgb(cr, cg, cb));
        let pen = CreatePen(PS_SOLID, 1, rgb(255, 255, 255));
        let old_brush = SelectObject(hdc, brush);
        let old_pen = SelectObject(hdc, pen);
        // GDI Pie() with same start and end isn't valid; skip empty
        // wedges produced by a 0-frac entry.
        if (end_angle - start_angle).abs() > 1e-6 {
            let _ = Pie(hdc, bb_left, bb_top, bb_right, bb_bottom, x1, y1, x2, y2);
        }
        SelectObject(hdc, old_brush);
        SelectObject(hdc, old_pen);
        let _ = DeleteObject(brush);
        let _ = DeleteObject(pen);
        start_angle = end_angle;
    }
}

pub fn create(kind: i64, width: f64, height: f64) -> i64 {
    let kind_enum = match kind {
        0 => ChartKind::Line,
        1 => ChartKind::Bar,
        2 => ChartKind::Pie,
        _ => ChartKind::Line,
    };
    let control_id = alloc_control_id();

    #[cfg(target_os = "windows")]
    {
        ensure_class_registered();
        let class_name = to_wide("PerryChart");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(std::ptr::null()),
                WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0),
                0,
                0,
                width.max(40.0) as i32,
                height.max(40.0) as i32,
                super::get_parking_hwnd(),
                HMENU(control_id as *mut _),
                HINSTANCE::from(hinstance),
                None,
            );
            let Ok(hwnd) = hwnd else {
                return register_widget(HWND(std::ptr::null_mut()), WidgetKind::Chart, control_id);
            };
            let handle = register_widget(hwnd, WidgetKind::Chart, control_id);
            CHARTS.with(|c| {
                c.borrow_mut().insert(
                    handle,
                    ChartEntry {
                        kind: kind_enum,
                        title: String::new(),
                        data: Vec::new(),
                    },
                );
            });
            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (width, height);
        let handle = register_widget(0, WidgetKind::Chart, control_id);
        CHARTS.with(|c| {
            c.borrow_mut().insert(
                handle,
                ChartEntry {
                    kind: kind_enum,
                    title: String::new(),
                    data: Vec::new(),
                },
            );
        });
        handle
    }
}

pub fn add_data_point(handle: i64, label_ptr: *const u8, value: f64) {
    let label = str_from_header(label_ptr);
    CHARTS.with(|c| {
        if let Some(entry) = c.borrow_mut().get_mut(&handle) {
            entry.data.push((label, value));
        }
    });
    request_redraw(handle);
}

pub fn clear_data(handle: i64) {
    CHARTS.with(|c| {
        if let Some(entry) = c.borrow_mut().get_mut(&handle) {
            entry.data.clear();
        }
    });
    request_redraw(handle);
}

pub fn set_title(handle: i64, title_ptr: *const u8) {
    let title = str_from_header(title_ptr);
    CHARTS.with(|c| {
        if let Some(entry) = c.borrow_mut().get_mut(&handle) {
            entry.title = title;
        }
    });
    request_redraw(handle);
}

pub fn reload(handle: i64) {
    request_redraw(handle);
}

fn request_redraw(handle: i64) {
    #[cfg(target_os = "windows")]
    {
        let Some(hwnd) = super::get_hwnd(handle) else { return };
        unsafe {
            let _ = InvalidateRect(hwnd, None, true);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}
