// Canvas widget — custom drawing via GTK4 DrawingArea + Cairo
//
// Stores a command buffer that replays on each draw callback.
// Commands: BeginPath, MoveTo, LineTo, Stroke, FillGradient, Clear.

use gtk4::prelude::*;
use gtk4::DrawingArea;

use std::cell::RefCell;
use std::collections::HashMap;

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
}

thread_local! {
    /// Canvas command buffers, keyed by widget handle
    static CANVAS_COMMANDS: RefCell<HashMap<i64, Vec<DrawCommand>>> = RefCell::new(HashMap::new());
    /// Canvas sizes (width, height), keyed by widget handle
    static CANVAS_SIZES: RefCell<HashMap<i64, (f64, f64)>> = RefCell::new(HashMap::new());
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
    CANVAS_SIZES.with(|s| {
        s.borrow_mut().insert(handle, (width, height));
    });

    // Set the draw function — replays the command buffer using Cairo
    area.set_draw_func(move |_area, cr, _w, _h| {
        let (canvas_w, canvas_h) =
            CANVAS_SIZES.with(|s| s.borrow().get(&handle).copied().unwrap_or((0.0, 0.0)));

        CANVAS_COMMANDS.with(|cmds| {
            let cmds = cmds.borrow();
            if let Some(commands) = cmds.get(&handle) {
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
                    }
                }
            }
        });
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
