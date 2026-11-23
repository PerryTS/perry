// FFI: Canvas (immediate-mode draw + stateful 2D-context stubs).
use crate::widgets;

// =============================================================================
// Canvas
// =============================================================================

/// Clear a canvas.
#[no_mangle]
pub extern "C" fn perry_ui_canvas_clear(handle: i64) {
    widgets::canvas::clear(handle);
}

/// Begin a new path on a canvas.
#[no_mangle]
pub extern "C" fn perry_ui_canvas_begin_path(handle: i64) {
    widgets::canvas::begin_path(handle);
}

/// Move the path cursor.
#[no_mangle]
pub extern "C" fn perry_ui_canvas_move_to(handle: i64, x: f64, y: f64) {
    widgets::canvas::move_to(handle, x, y);
}

/// Draw a line to a point.
#[no_mangle]
pub extern "C" fn perry_ui_canvas_line_to(handle: i64, x: f64, y: f64) {
    widgets::canvas::line_to(handle, x, y);
}

/// Stroke the current path.
#[no_mangle]
pub extern "C" fn perry_ui_canvas_stroke(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
    line_width: f64,
) {
    widgets::canvas::stroke(handle, r, g, b, a, line_width);
}

/// Fill the current path with a gradient.
#[no_mangle]
pub extern "C" fn perry_ui_canvas_fill_gradient(
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
    widgets::canvas::fill_gradient(handle, r1, g1, b1, a1, r2, g2, b2, a2, direction);
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_set_fill_color(h: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::canvas::set_fill_color(h, r, g, b, a);
}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_set_stroke_color(h: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::canvas::set_stroke_color(h, r, g, b, a);
}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_set_line_width(h: i64, w: f64) {
    // HTML canvas keeps the previous width when the new one is not finite and positive.
    if w.is_finite() && w > 0.0 {
        widgets::canvas::set_line_width(h, w);
    }
}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_fill_rect(h: i64, x: f64, y: f64, w: f64, ht: f64) {
    // HTML canvas ignores a fillRect with any non-finite argument.
    if [x, y, w, ht].iter().all(|v| v.is_finite()) {
        widgets::canvas::fill_rect(h, x, y, w, ht);
    }
}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_stroke_rect(h: i64, x: f64, y: f64, w: f64, ht: f64) {
    // HTML canvas ignores a strokeRect with any non-finite argument.
    if [x, y, w, ht].iter().all(|v| v.is_finite()) {
        widgets::canvas::stroke_rect(h, x, y, w, ht);
    }
}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_clear_rect(h: i64, x: f64, y: f64, w: f64, ht: f64) {
    // HTML canvas ignores a clearRect with any non-finite argument.
    if !(x.is_finite() && y.is_finite() && w.is_finite() && ht.is_finite()) {
        return;
    }
    widgets::canvas::clear_rect(h, x, y, w, ht);
}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_arc(h: i64, x: f64, y: f64, r: f64, sa: f64, ea: f64) {
    widgets::canvas::arc(h, x, y, r, sa, ea);
}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_close_path(h: i64) {
    widgets::canvas::close_path(h);
}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_fill(h: i64) {
    widgets::canvas::fill(h);
}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_stroke_path(h: i64) {
    widgets::canvas::stroke_path(h);
}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_fill_text(h: i64, ptr: i64, x: f64, y: f64) {
    widgets::canvas::fill_text(h, ptr as *const u8, x, y);
}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_set_font(h: i64, ptr: i64) {
    widgets::canvas::set_font(h, ptr as *const u8);
}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_draw_image(
    h: i64,
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
    widgets::canvas::draw_image(h, image, sx, sy, sw, sh, dx, dy, dw, dh);
}
