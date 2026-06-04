// Canvas widget — custom drawing via GTK4 DrawingArea + Cairo
//
// Stores a command buffer that replays on each draw callback.
// Commands: BeginPath, MoveTo, LineTo, Stroke, FillGradient, Clear.

use gtk4::prelude::*;
use gtk4::DrawingArea;

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicI64, Ordering};

use super::register_widget;

/// Drawing commands stored in command buffer.
#[derive(Clone)]
enum DrawCommand {
    BeginPath,
    MoveTo(f64, f64),
    LineTo(f64, f64),
    Stroke {
        r: f64,
        g: f64,
        b: f64,
        a: f64,
        line_width: f64,
    },
    FillGradient {
        r1: f64,
        g1: f64,
        b1: f64,
        a1: f64,
        r2: f64,
        g2: f64,
        b2: f64,
        a2: f64,
        direction: f64,
    },
    DrawImage {
        image: i64,
        sx: f64,
        sy: f64,
        sw: f64,
        sh: f64,
        dx: f64,
        dy: f64,
        dw: f64,
        dh: f64,
    },
}

fn command_batch_renders(commands: &[DrawCommand]) -> bool {
    commands.iter().any(|cmd| {
        matches!(
            cmd,
            DrawCommand::Stroke { .. }
                | DrawCommand::FillGradient { .. }
                | DrawCommand::DrawImage { .. }
        )
    })
}

thread_local! {
    /// Pending canvas commands, keyed by widget handle
    static CANVAS_COMMANDS: RefCell<HashMap<i64, Vec<DrawCommand>>> = RefCell::new(HashMap::new());
    /// Last rendered command batch, used for native repaints that arrive before new commands.
    static CANVAS_LAST_FRAME: RefCell<HashMap<i64, Vec<DrawCommand>>> = RefCell::new(HashMap::new());
    /// Canvas sizes (width, height), keyed by widget handle
    static CANVAS_SIZES: RefCell<HashMap<i64, (f64, f64)>> = RefCell::new(HashMap::new());
    static CANVAS_IMAGES: RefCell<HashMap<i64, gtk4::gdk_pixbuf::Pixbuf>> = RefCell::new(HashMap::new());
    static IMAGE_CACHE: RefCell<HashMap<String, i64>> = RefCell::new(HashMap::new());
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

/// Create a Canvas widget with given dimensions.
pub fn create(width: f64, height: f64) -> i64 {
    let area = DrawingArea::new();
    area.set_content_width(width as i32);
    area.set_content_height(height as i32);

    // Register early so we have the handle for the command buffer key
    let widget = area.clone().upcast::<gtk4::Widget>();
    let handle = register_widget(widget);

    CANVAS_COMMANDS.with(|cmds| {
        cmds.borrow_mut().insert(handle, Vec::new());
    });
    CANVAS_LAST_FRAME.with(|last| {
        last.borrow_mut().insert(handle, Vec::new());
    });
    CANVAS_SIZES.with(|s| {
        s.borrow_mut().insert(handle, (width, height));
    });

    // Set the draw function — replays the command buffer using Cairo
    area.set_draw_func(move |_area, cr, _w, _h| {
        let (canvas_w, canvas_h) =
            CANVAS_SIZES.with(|s| s.borrow().get(&handle).copied().unwrap_or((0.0, 0.0)));

        let commands = CANVAS_COMMANDS
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

        if let Some(commands) = commands {
            // Track current path points for gradient fill
            let mut path_points: Vec<(f64, f64)> = Vec::new();

            for cmd in commands.iter() {
                match cmd {
                    DrawCommand::BeginPath => {
                        path_points.clear();
                    }
                    DrawCommand::MoveTo(x, y) => {
                        // GTK4/Cairo origin is top-left — same as TypeScript expects.
                        // No Y-flip needed (unlike macOS which is bottom-left).
                        path_points.push((*x, *y));
                    }
                    DrawCommand::LineTo(x, y) => {
                        path_points.push((*x, *y));
                    }
                    DrawCommand::Stroke {
                        r,
                        g,
                        b,
                        a,
                        line_width,
                    } => {
                        if path_points.len() >= 2 {
                            cr.save().ok();
                            cr.set_source_rgba(*r, *g, *b, *a);
                            cr.set_line_width(*line_width);
                            cr.set_line_cap(gtk4::cairo::LineCap::Round);
                            cr.set_line_join(gtk4::cairo::LineJoin::Round);
                            cr.new_path();
                            cr.move_to(path_points[0].0, path_points[0].1);
                            for pt in &path_points[1..] {
                                cr.line_to(pt.0, pt.1);
                            }
                            cr.stroke().ok();
                            cr.restore().ok();
                        }
                    }
                    DrawCommand::FillGradient {
                        r1,
                        g1,
                        b1,
                        a1,
                        r2,
                        g2,
                        b2,
                        a2,
                        direction,
                    } => {
                        if path_points.len() >= 2 {
                            cr.save().ok();

                            // Build closed path for clipping — area under/beside the line
                            cr.new_path();
                            cr.move_to(path_points[0].0, path_points[0].1);
                            for pt in &path_points[1..] {
                                cr.line_to(pt.0, pt.1);
                            }
                            // Close to bottom edge (top-left origin, so canvas_h is bottom)
                            let last_x = path_points[path_points.len() - 1].0;
                            let first_x = path_points[0].0;
                            cr.line_to(last_x, canvas_h);
                            cr.line_to(first_x, canvas_h);
                            cr.close_path();
                            cr.clip();

                            // Draw linear gradient
                            let gradient = if *direction < 0.5 {
                                // Vertical: top to bottom
                                gtk4::cairo::LinearGradient::new(0.0, 0.0, 0.0, canvas_h)
                            } else {
                                // Horizontal: left to right
                                gtk4::cairo::LinearGradient::new(0.0, 0.0, canvas_w, 0.0)
                            };
                            gradient.add_color_stop_rgba(0.0, *r1, *g1, *b1, *a1);
                            gradient.add_color_stop_rgba(1.0, *r2, *g2, *b2, *a2);
                            cr.set_source(&gradient).ok();
                            cr.paint().ok();

                            cr.restore().ok();
                        }
                    }
                    DrawCommand::DrawImage {
                        image,
                        sx,
                        sy,
                        sw,
                        sh,
                        dx,
                        dy,
                        dw,
                        dh,
                    } => {
                        CANVAS_IMAGES.with(|images| {
                            if let Some(pixbuf) = images.borrow().get(image) {
                                let src_w = if *sw > 0.0 {
                                    *sw
                                } else {
                                    pixbuf.width() as f64
                                };
                                let src_h = if *sh > 0.0 {
                                    *sh
                                } else {
                                    pixbuf.height() as f64
                                };
                                let dst_w = if *dw > 0.0 { *dw } else { src_w };
                                let dst_h = if *dh > 0.0 { *dh } else { src_h };
                                if src_w <= 0.0 || src_h <= 0.0 || dst_w <= 0.0 || dst_h <= 0.0 {
                                    return;
                                }
                                let src = pixbuf.new_subpixbuf(
                                    (*sx).max(0.0) as i32,
                                    (*sy).max(0.0) as i32,
                                    src_w as i32,
                                    src_h as i32,
                                );
                                let scaled = src
                                    .scale_simple(
                                        dst_w as i32,
                                        dst_h as i32,
                                        gtk4::gdk_pixbuf::InterpType::Bilinear,
                                    )
                                    .unwrap_or(src);
                                cr.save().ok();
                                // gdk4's `GdkCairoContextExt::set_source_pixbuf` (in scope via
                                // `gtk4::prelude::*`) — a method on the cairo Context, not a free
                                // function under `gtk4::gdk::cairo`.
                                cr.set_source_pixbuf(&scaled, *dx, *dy);
                                cr.paint().ok();
                                cr.restore().ok();
                            }
                        });
                    }
                }
            }
        }
    });

    handle
}

/// Clear all drawing commands.
pub fn clear(handle: i64) {
    CANVAS_COMMANDS.with(|cmds| {
        if let Some(commands) = cmds.borrow_mut().get_mut(&handle) {
            commands.clear();
        }
    });
    CANVAS_LAST_FRAME.with(|last| {
        if let Some(commands) = last.borrow_mut().get_mut(&handle) {
            commands.clear();
        }
    });
    // Trigger redraw
    if let Some(widget) = super::get_widget(handle) {
        if let Some(area) = widget.downcast_ref::<DrawingArea>() {
            area.queue_draw();
        }
    }
}

/// Begin a new path.
pub fn begin_path(handle: i64) {
    CANVAS_COMMANDS.with(|cmds| {
        if let Some(commands) = cmds.borrow_mut().get_mut(&handle) {
            commands.push(DrawCommand::BeginPath);
        }
    });
}

/// Move pen to point.
pub fn move_to(handle: i64, x: f64, y: f64) {
    CANVAS_COMMANDS.with(|cmds| {
        if let Some(commands) = cmds.borrow_mut().get_mut(&handle) {
            commands.push(DrawCommand::MoveTo(x, y));
        }
    });
}

/// Line to point.
pub fn line_to(handle: i64, x: f64, y: f64) {
    CANVAS_COMMANDS.with(|cmds| {
        if let Some(commands) = cmds.borrow_mut().get_mut(&handle) {
            commands.push(DrawCommand::LineTo(x, y));
        }
    });
}

/// Stroke the current path.
pub fn stroke(handle: i64, r: f64, g: f64, b: f64, a: f64, line_width: f64) {
    CANVAS_COMMANDS.with(|cmds| {
        if let Some(commands) = cmds.borrow_mut().get_mut(&handle) {
            commands.push(DrawCommand::Stroke {
                r,
                g,
                b,
                a,
                line_width,
            });
        }
    });
    // Trigger redraw
    if let Some(widget) = super::get_widget(handle) {
        if let Some(area) = widget.downcast_ref::<DrawingArea>() {
            area.queue_draw();
        }
    }
}

/// Fill the current path area with a gradient.
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
    CANVAS_COMMANDS.with(|cmds| {
        if let Some(commands) = cmds.borrow_mut().get_mut(&handle) {
            commands.push(DrawCommand::FillGradient {
                r1,
                g1,
                b1,
                a1,
                r2,
                g2,
                b2,
                a2,
                direction,
            });
        }
    });
    // Trigger redraw
    if let Some(widget) = super::get_widget(handle) {
        if let Some(area) = widget.downcast_ref::<DrawingArea>() {
            area.queue_draw();
        }
    }
}

pub fn load_image(path_ptr: *const u8) -> i64 {
    crate::app::ensure_gtk_init();
    let raw = crate::widgets::image::str_from_header(path_ptr);
    let path = raw.split('\0').next().unwrap_or(raw);
    let resolved = crate::ffi::layout::resolve_asset_path(path);
    let key = resolved.to_string_lossy().to_string();
    if let Some(handle) = IMAGE_CACHE.with(|c| c.borrow().get(&key).copied()) {
        if let Some((width, height)) = CANVAS_IMAGES.with(|images| {
            images
                .borrow()
                .get(&handle)
                .map(|pixbuf| (pixbuf.width() as f64, pixbuf.height() as f64))
        }) {
            return resolved_image_promise(handle, width, height);
        }
        return rejected_image_promise("Cached Canvas image handle was missing");
    }
    let pixbuf = match gtk4::gdk_pixbuf::Pixbuf::from_file(&resolved) {
        Ok(p) => p,
        Err(_) => return rejected_image_promise(&format!("Failed to load image: {}", key)),
    };
    let width = pixbuf.width() as f64;
    let height = pixbuf.height() as f64;
    let handle = NEXT_IMAGE_HANDLE.fetch_add(1, Ordering::Relaxed);
    CANVAS_IMAGES.with(|images| images.borrow_mut().insert(handle, pixbuf));
    IMAGE_CACHE.with(|cache| cache.borrow_mut().insert(key, handle));
    resolved_image_promise(handle, width, height)
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
    CANVAS_COMMANDS.with(|cmds| {
        if let Some(commands) = cmds.borrow_mut().get_mut(&handle) {
            commands.push(DrawCommand::DrawImage {
                image,
                sx,
                sy,
                sw,
                sh,
                dx,
                dy,
                dw,
                dh,
            });
        }
    });
    if let Some(widget) = super::get_widget(handle) {
        if let Some(area) = widget.downcast_ref::<DrawingArea>() {
            area.queue_draw();
        }
    }
}
