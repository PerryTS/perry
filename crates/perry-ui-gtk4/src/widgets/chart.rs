//! GTK4 Chart widget — line / bar / pie via Cairo on a `GtkDrawingArea`
//! (issue #474 / Linux parity work).
//!
//! Mirrors the macOS impl's API (`Chart(kind, w, h)` +
//! `chartAddDataPoint` + `chartClearData` + `chartSetTitle` +
//! `chartReload`) and visual conventions (24pt padding, 8-color
//! palette, top-aligned title). Drawing happens in `set_draw_func`
//! over a Cairo context — same primitive set as the macOS
//! CGContext path so the two impls stay close to pixel-equivalent.

use gtk4::cairo::Context;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

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

const PADDING: f64 = 24.0;
const PALETTE: &[(f64, f64, f64)] = &[
    (0.20, 0.55, 0.92),
    (0.94, 0.38, 0.40),
    (0.32, 0.78, 0.42),
    (0.95, 0.69, 0.20),
    (0.62, 0.42, 0.86),
    (0.18, 0.74, 0.78),
    (0.86, 0.40, 0.62),
    (0.55, 0.55, 0.55),
];

pub fn create(kind: i64, width: f64, height: f64) -> i64 {
    crate::app::ensure_gtk_init();
    let kind_enum = match kind {
        0 => ChartKind::Line,
        1 => ChartKind::Bar,
        2 => ChartKind::Pie,
        _ => ChartKind::Line,
    };
    let area = gtk4::DrawingArea::new();
    area.set_content_width(width.max(40.0) as i32);
    area.set_content_height(height.max(40.0) as i32);

    let handle = super::register_widget(area.clone().upcast());
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

    area.set_draw_func(move |_area, cr, w, h| {
        let snapshot = CHARTS.with(|c| {
            c.borrow()
                .get(&handle)
                .map(|e| (e.kind as i64, e.title.clone(), e.data.clone()))
        });
        let Some((kind, title, data)) = snapshot else { return };
        draw_chart(cr, kind, &title, &data, w as f64, h as f64);
    });

    handle
}

fn draw_chart(cr: &Context, kind: i64, title: &str, data: &[(String, f64)], w: f64, h: f64) {
    // Clear background to white-ish so the chart sits on a clean panel.
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.rectangle(0.0, 0.0, w, h);
    cr.fill().ok();

    let title_band_h = if title.is_empty() { 0.0 } else { 22.0 };
    let plot_x = PADDING;
    let plot_y = PADDING; // GTK4 origin is top-left; plot grows downward.
    let plot_w = (w - 2.0 * PADDING).max(1.0);
    let plot_h = (h - 2.0 * PADDING - title_band_h).max(1.0);

    if !title.is_empty() {
        draw_title_centered(cr, title, w, 4.0, 14.0);
    }

    if data.is_empty() {
        return;
    }
    match kind {
        0 => draw_line(cr, plot_x, plot_y, plot_w, plot_h, data),
        1 => draw_bars(cr, plot_x, plot_y, plot_w, plot_h, data),
        2 => draw_pie(cr, plot_x, plot_y, plot_w, plot_h, data),
        _ => {}
    }
}

fn draw_line(cr: &Context, x: f64, y: f64, w: f64, h: f64, data: &[(String, f64)]) {
    let max = data.iter().map(|(_, v)| *v).fold(f64::NEG_INFINITY, f64::max);
    let min = data.iter().map(|(_, v)| *v).fold(f64::INFINITY, f64::min);
    let range = if (max - min).abs() < 1e-9 {
        1.0
    } else {
        max - min
    };
    let n = data.len();
    let dx = if n > 1 { w / (n as f64 - 1.0) } else { 0.0 };

    // Baseline axis along the top of the plot rect (since y grows down,
    // top is the visual "bottom" of the data).
    cr.set_source_rgb(0.7, 0.7, 0.7);
    cr.set_line_width(1.0);
    cr.move_to(x, y + h);
    cr.line_to(x + w, y + h);
    cr.stroke().ok();

    let (r, g, b) = PALETTE[0];
    cr.set_source_rgb(r, g, b);
    cr.set_line_width(2.0);
    for (i, (_, v)) in data.iter().enumerate() {
        let px = x + (i as f64) * dx;
        // y grows downward — bigger v → smaller y. Map (v - min)/range to
        // the plot height with origin at the bottom of the rect.
        let py = y + h - (v - min) / range * h;
        if i == 0 {
            cr.move_to(px, py);
        } else {
            cr.line_to(px, py);
        }
    }
    cr.stroke().ok();
}

fn draw_bars(cr: &Context, x: f64, y: f64, w: f64, h: f64, data: &[(String, f64)]) {
    let max = data.iter().map(|(_, v)| *v).fold(0.0_f64, f64::max);
    if max <= 0.0 {
        return;
    }
    let n = data.len();
    let bar_w = w / (n as f64) * 0.7;
    let gap = w / (n as f64) * 0.3;
    for (i, (_, v)) in data.iter().enumerate() {
        let bh = (v / max) * h;
        let bx = x + (i as f64) * (bar_w + gap) + gap / 2.0;
        // Bars grow up from the bottom of the plot rect.
        let by = y + h - bh;
        let (r, g, b) = PALETTE[i % PALETTE.len()];
        cr.set_source_rgb(r, g, b);
        cr.rectangle(bx, by, bar_w, bh);
        cr.fill().ok();
    }
}

fn draw_pie(cr: &Context, x: f64, y: f64, w: f64, h: f64, data: &[(String, f64)]) {
    let total: f64 = data.iter().map(|(_, v)| *v).sum();
    if total <= 0.0 {
        return;
    }
    let cx = x + w / 2.0;
    let cy = y + h / 2.0;
    let radius = w.min(h) / 2.0 - 4.0;

    // Cairo's angle convention is clockwise from 3 o'clock; we want
    // 12 o'clock start, clockwise. So start = -π/2 and accumulate.
    let mut start = -std::f64::consts::FRAC_PI_2;
    for (i, (_, v)) in data.iter().enumerate() {
        let frac = v / total;
        let end = start + frac * std::f64::consts::TAU;
        let (r, g, b) = PALETTE[i % PALETTE.len()];
        cr.set_source_rgb(r, g, b);
        cr.move_to(cx, cy);
        cr.arc(cx, cy, radius, start, end);
        cr.close_path();
        cr.fill().ok();
        start = end;
    }
}

fn draw_title_centered(cr: &Context, title: &str, total_w: f64, top_y: f64, font_size: f64) {
    cr.set_source_rgb(0.10, 0.10, 0.10);
    cr.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    cr.set_font_size(font_size);
    let extents = cr.text_extents(title).ok();
    let x = if let Some(ref ext) = extents {
        (total_w - ext.width()) / 2.0
    } else {
        PADDING
    };
    cr.move_to(x, top_y + font_size);
    cr.show_text(title).ok();
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
    if let Some(widget) = super::get_widget(handle) {
        if let Some(area) = widget.downcast_ref::<gtk4::DrawingArea>() {
            area.queue_draw();
        }
    }
}
