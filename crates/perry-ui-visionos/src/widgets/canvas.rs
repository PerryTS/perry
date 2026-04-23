// Canvas widget — custom drawing via Core Graphics
//
// Stores a command buffer that replays on each drawRect: call.
// Commands: MoveTo, LineTo, Stroke, FillGradient, BeginPath, Clear.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_ui_kit::UIView;
use objc2_core_foundation::{CGPoint, CGSize, CGRect};
use objc2_foundation::{NSObject, MainThreadMarker};

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;

use super::register_widget;

// Core Graphics C API
type CGContextRef = *mut c_void;
type CGColorSpaceRef = *mut c_void;
type CGGradientRef = *mut c_void;
type CGFloat = f64;

extern "C" {
    fn UIGraphicsGetCurrentContext() -> CGContextRef;
    fn CGContextSaveGState(c: CGContextRef);
    fn CGContextRestoreGState(c: CGContextRef);
    fn CGContextBeginPath(c: CGContextRef);
    fn CGContextMoveToPoint(c: CGContextRef, x: CGFloat, y: CGFloat);
    fn CGContextAddLineToPoint(c: CGContextRef, x: CGFloat, y: CGFloat);
    fn CGContextStrokePath(c: CGContextRef);
    fn CGContextClosePath(c: CGContextRef);
    fn CGContextClip(c: CGContextRef);
    fn CGContextSetLineWidth(c: CGContextRef, width: CGFloat);
    fn CGContextSetLineCap(c: CGContextRef, cap: i32);
    fn CGContextSetLineJoin(c: CGContextRef, join: i32);
    fn CGContextSetRGBStrokeColor(c: CGContextRef, r: CGFloat, g: CGFloat, b: CGFloat, a: CGFloat);
    fn CGContextDrawLinearGradient(
        c: CGContextRef, gradient: CGGradientRef,
        start_point: CGPoint, end_point: CGPoint, options: u32,
    );
    fn CGColorSpaceCreateDeviceRGB() -> CGColorSpaceRef;
    fn CGColorSpaceRelease(space: CGColorSpaceRef);
    fn CGGradientCreateWithColorComponents(
        space: CGColorSpaceRef, components: *const CGFloat,
        locations: *const CGFloat, count: usize,
    ) -> CGGradientRef;
    fn CGGradientRelease(gradient: CGGradientRef);
}

// Drawing commands stored in command buffer
#[derive(Clone)]
enum DrawCommand {
    BeginPath,
    MoveTo(f64, f64),
    LineTo(f64, f64),
    Stroke { r: f64, g: f64, b: f64, a: f64, line_width: f64 },
    FillGradient {
        r1: f64, g1: f64, b1: f64, a1: f64,
        r2: f64, g2: f64, b2: f64, a2: f64,
        direction: f64,
    },
}

thread_local! {
    /// Canvas command buffers, keyed by view address
    static CANVAS_COMMANDS: RefCell<HashMap<usize, Vec<DrawCommand>>> = RefCell::new(HashMap::new());
    /// Canvas sizes (width, height), keyed by view address
    static CANVAS_SIZES: RefCell<HashMap<usize, (f64, f64)>> = RefCell::new(HashMap::new());
}

// Custom UIView subclass for canvas drawing
pub struct PerryCanvasViewIvars {
    view_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(UIView))]
    #[name = "PerryCanvasView"]
    #[ivars = PerryCanvasViewIvars]
    pub struct PerryCanvasView;

    impl PerryCanvasView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty_rect: CGRect) {
            let key = self.ivars().view_key.get();

            // Get the current graphics context (iOS)
            let ctx: CGContextRef = unsafe { UIGraphicsGetCurrentContext() };
            if ctx.is_null() { return; }

            // Get canvas size for gradient direction
            let (canvas_w, canvas_h) = CANVAS_SIZES.with(|s| {
                s.borrow().get(&key).copied().unwrap_or((0.0, 0.0))
            });

            // Replay command buffer
            CANVAS_COMMANDS.with(|cmds| {
                let cmds = cmds.borrow();
                if let Some(commands) = cmds.get(&key) {
                    // Track current path points for gradient fill
                    let mut path_points: Vec<(f64, f64)> = Vec::new();
                    let mut in_path = false;

                    for cmd in commands.iter() {
                        match cmd {
                            DrawCommand::BeginPath => {
                                path_points.clear();
                                in_path = true;
                            }
                            DrawCommand::MoveTo(x, y) => {
                                // No Y-flipping on iOS (origin is top-left)
                                path_points.push((*x, *y));
                            }
                            DrawCommand::LineTo(x, y) => {
                                path_points.push((*x, *y));
                            }
                            DrawCommand::Stroke { r, g, b, a, line_width } => {
                                if path_points.len() >= 2 {
                                    unsafe {
                                        CGContextSaveGState(ctx);
                                        CGContextSetRGBStrokeColor(ctx, *r, *g, *b, *a);
                                        CGContextSetLineWidth(ctx, *line_width);
                                        CGContextSetLineCap(ctx, 1); // kCGLineCapRound
                                        CGContextSetLineJoin(ctx, 1); // kCGLineJoinRound
                                        CGContextBeginPath(ctx);
                                        CGContextMoveToPoint(ctx, path_points[0].0, path_points[0].1);
                                        for pt in &path_points[1..] {
                                            CGContextAddLineToPoint(ctx, pt.0, pt.1);
                                        }
                                        CGContextStrokePath(ctx);
                                        CGContextRestoreGState(ctx);
                                    }
                                }
                                in_path = false;
                            }
                            DrawCommand::FillGradient { r1, g1, b1, a1, r2, g2, b2, a2, direction } => {
                                if path_points.len() >= 2 {
                                    unsafe {
                                        CGContextSaveGState(ctx);

                                        // Build closed path for clipping
                                        CGContextBeginPath(ctx);
                                        CGContextMoveToPoint(ctx, path_points[0].0, path_points[0].1);
                                        for pt in &path_points[1..] {
                                            CGContextAddLineToPoint(ctx, pt.0, pt.1);
                                        }
                                        // Close to bottom (iOS: larger Y = lower on screen)
                                        let last_x = path_points[path_points.len() - 1].0;
                                        let first_x = path_points[0].0;
                                        CGContextAddLineToPoint(ctx, last_x, canvas_h); // bottom-right
                                        CGContextAddLineToPoint(ctx, first_x, canvas_h); // bottom-left
                                        CGContextClosePath(ctx);
                                        CGContextClip(ctx);

                                        // Draw gradient
                                        let color_space = CGColorSpaceCreateDeviceRGB();
                                        let components: [CGFloat; 8] = [
                                            *r1, *g1, *b1, *a1,
                                            *r2, *g2, *b2, *a2,
                                        ];
                                        let locations: [CGFloat; 2] = [0.0, 1.0];
                                        let gradient = CGGradientCreateWithColorComponents(
                                            color_space,
                                            components.as_ptr(),
                                            locations.as_ptr(),
                                            2,
                                        );

                                        let (start, end) = if *direction < 0.5 {
                                            // Vertical: top to bottom (iOS: 0,0 is top-left)
                                            (CGPoint::new(0.0, 0.0), CGPoint::new(0.0, canvas_h))
                                        } else {
                                            // Horizontal: left to right
                                            (CGPoint::new(0.0, 0.0), CGPoint::new(canvas_w, 0.0))
                                        };

                                        CGContextDrawLinearGradient(ctx, gradient, start, end, 0);
                                        CGGradientRelease(gradient);
                                        CGColorSpaceRelease(color_space);
                                        CGContextRestoreGState(ctx);
                                    }
                                }
                                in_path = false;
                            }
                        }
                    }
                }
            });
        }
    }
);

impl PerryCanvasView {
    fn new(width: f64, height: f64, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(PerryCanvasViewIvars {
            view_key: std::cell::Cell::new(0),
        });
        let view: Retained<Self> = unsafe { msg_send![super(this), init] };

        let frame = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(width, height));
        unsafe {
            let _: () = msg_send![&*view, setFrame: frame];
        }

        // Set opaque=false so background is transparent
        unsafe {
            let _: () = msg_send![&*view, setOpaque: false];
        }

        // Set a fixed size via Auto Layout constraints
        unsafe {
            let _: () = msg_send![&*view, setTranslatesAutoresizingMaskIntoConstraints: false];
            let width_anchor: Retained<AnyObject> = msg_send![
                &*view, widthAnchor
            ];
            let constraint: Retained<AnyObject> = msg_send![
                &*width_anchor, constraintEqualToConstant: width
            ];
            let _: () = msg_send![&*constraint, setActive: true];

            let height_anchor: Retained<AnyObject> = msg_send![
                &*view, heightAnchor
            ];
            let h_constraint: Retained<AnyObject> = msg_send![
                &*height_anchor, constraintEqualToConstant: height
            ];
            let _: () = msg_send![&*h_constraint, setActive: true];
        }

        view
    }
}

/// Create a Canvas widget with given dimensions.
pub fn create(width: f64, height: f64) -> i64 {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on main thread");
    let view = PerryCanvasView::new(width, height, mtm);
    let key = Retained::as_ptr(&view) as usize;
    view.ivars().view_key.set(key);

    CANVAS_COMMANDS.with(|cmds| {
        cmds.borrow_mut().insert(key, Vec::new());
    });
    CANVAS_SIZES.with(|s| {
        s.borrow_mut().insert(key, (width, height));
    });

    // Cast to UIView for registration
    let ui_view: Retained<UIView> = unsafe {
        Retained::cast_unchecked(view)
    };
    register_widget(ui_view)
}

fn get_canvas_key(handle: i64) -> Option<usize> {
    super::get_widget(handle).map(|view| Retained::as_ptr(&view) as usize)
}

/// Clear all drawing commands.
pub fn clear(handle: i64) {
    if let Some(key) = get_canvas_key(handle) {
        CANVAS_COMMANDS.with(|cmds| {
            if let Some(commands) = cmds.borrow_mut().get_mut(&key) {
                commands.clear();
            }
        });
        // Trigger redraw (UIView: setNeedsDisplay with no argument)
        if let Some(view) = super::get_widget(handle) {
            unsafe {
                let _: () = msg_send![&*view, setNeedsDisplay];
            }
        }
    }
}

/// Begin a new path.
pub fn begin_path(handle: i64) {
    if let Some(key) = get_canvas_key(handle) {
        CANVAS_COMMANDS.with(|cmds| {
            if let Some(commands) = cmds.borrow_mut().get_mut(&key) {
                commands.push(DrawCommand::BeginPath);
            }
        });
    }
}

/// Move pen to point.
pub fn move_to(handle: i64, x: f64, y: f64) {
    if let Some(key) = get_canvas_key(handle) {
        CANVAS_COMMANDS.with(|cmds| {
            if let Some(commands) = cmds.borrow_mut().get_mut(&key) {
                commands.push(DrawCommand::MoveTo(x, y));
            }
        });
    }
}

/// Line to point.
pub fn line_to(handle: i64, x: f64, y: f64) {
    if let Some(key) = get_canvas_key(handle) {
        CANVAS_COMMANDS.with(|cmds| {
            if let Some(commands) = cmds.borrow_mut().get_mut(&key) {
                commands.push(DrawCommand::LineTo(x, y));
            }
        });
    }
}

/// Stroke the current path.
pub fn stroke(handle: i64, r: f64, g: f64, b: f64, a: f64, line_width: f64) {
    if let Some(key) = get_canvas_key(handle) {
        CANVAS_COMMANDS.with(|cmds| {
            if let Some(commands) = cmds.borrow_mut().get_mut(&key) {
                commands.push(DrawCommand::Stroke { r, g, b, a, line_width });
            }
        });
        // Trigger redraw
        if let Some(view) = super::get_widget(handle) {
            unsafe {
                let _: () = msg_send![&*view, setNeedsDisplay];
            }
        }
    }
}

/// Fill the current path area with a gradient.
pub fn fill_gradient(
    handle: i64, r1: f64, g1: f64, b1: f64, a1: f64,
    r2: f64, g2: f64, b2: f64, a2: f64, direction: f64,
) {
    if let Some(key) = get_canvas_key(handle) {
        CANVAS_COMMANDS.with(|cmds| {
            if let Some(commands) = cmds.borrow_mut().get_mut(&key) {
                commands.push(DrawCommand::FillGradient {
                    r1, g1, b1, a1, r2, g2, b2, a2, direction,
                });
            }
        });
        // Trigger redraw
        if let Some(view) = super::get_widget(handle) {
            unsafe {
                let _: () = msg_send![&*view, setNeedsDisplay];
            }
        }
    }
}
