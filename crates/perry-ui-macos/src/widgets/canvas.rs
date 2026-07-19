// Canvas widget — custom drawing via Core Graphics
//
// Stores a command buffer that replays on each drawRect: call.
// Commands: MoveTo, LineTo, Stroke, FillGradient, BeginPath, Clear.

use crate::ffi::CGContextClosePath;
use crate::ffi::CGContextFillPath;
use crate::ffi::CGContextStrokePath;
use crate::ffi::CGContextAddLineToPoint;
use crate::ffi::CGContextMoveToPoint;
use crate::ffi::CGContextBeginPath;
use crate::ffi::CGContextStrokeRect;
use crate::ffi::CGContextFillRect;
use crate::ffi::CGContextSetLineWidth;
use crate::ffi::CGContextSetRGBStrokeColor;
use crate::ffi::CGContextSetRGBFillColor;
use crate::ffi::js_string_from_bytes;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadOnly};
use objc2_app_kit::NSView;
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::MainThreadMarker;

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicI64, Ordering};

use super::register_widget;

// Core Graphics C API
type CGContextRef = *mut c_void;
type CGColorSpaceRef = *mut c_void;
type CGGradientRef = *mut c_void;
type CFDataRef = *const c_void;
type CGImageRef = *mut c_void;
type CGImageSourceRef = *mut c_void;
type CGFloat = f64;

extern "C" {
    fn CGContextSaveGState(c: CGContextRef);
    fn CGContextRestoreGState(c: CGContextRef);
    fn CGContextClip(c: CGContextRef);
    fn CGContextSetLineCap(c: CGContextRef, cap: i32);
    fn CGContextSetLineJoin(c: CGContextRef, join: i32);
    fn CGContextDrawImage(c: CGContextRef, rect: CGRect, image: CGImageRef);
    fn CGContextDrawLinearGradient(
        c: CGContextRef,
        gradient: CGGradientRef,
        start_point: CGPoint,
        end_point: CGPoint,
        options: u32,
    );
    fn CGColorSpaceCreateWithName(name: *const c_void) -> CGColorSpaceRef;
    fn CGColorSpaceRelease(space: CGColorSpaceRef);
    fn CGGradientCreateWithColorComponents(
        space: CGColorSpaceRef,
        components: *const CGFloat,
        locations: *const CGFloat,
        count: usize,
    ) -> CGGradientRef;
    fn CGGradientRelease(gradient: CGGradientRef);
    fn CFDataCreate(allocator: *const c_void, bytes: *const u8, length: isize) -> CFDataRef;
    fn CFRelease(obj: *const c_void);
    fn CGImageSourceCreateWithData(data: CFDataRef, options: *const c_void) -> CGImageSourceRef;
    fn CGImageSourceCreateImageAtIndex(
        source: CGImageSourceRef,
        index: usize,
        options: *const c_void,
    ) -> CGImageRef;
    fn CGImageCreateWithImageInRect(image: CGImageRef, rect: CGRect) -> CGImageRef;
    fn CGImageGetWidth(image: CGImageRef) -> usize;
    fn CGImageGetHeight(image: CGImageRef) -> usize;
}

extern "C" {
    static kCGColorSpaceSRGB: *const c_void;
}

// Drawing commands stored in command buffer
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
    SetStrokeColor {
        r: f64,
        g: f64,
        b: f64,
        a: f64,
    },
    SetFillColor {
        r: f64,
        g: f64,
        b: f64,
        a: f64,
    },
    SetLineWidth(f64),
    ClosePath,
    StrokePath,
    FillPath,
    FillRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
    StrokeRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
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

#[derive(Clone, Copy)]
struct CanvasImage {
    cg_image: CGImageRef,
    width: f64,
    height: f64,
}

fn command_batch_renders(commands: &[DrawCommand]) -> bool {
    commands.iter().any(|cmd| {
        matches!(
            cmd,
            DrawCommand::Stroke { .. }
                | DrawCommand::FillGradient { .. }
                | DrawCommand::StrokePath
                | DrawCommand::FillPath
                | DrawCommand::FillRect { .. }
                | DrawCommand::StrokeRect { .. }
        )
    })
}

thread_local! {
    /// Pending canvas commands, keyed by view address.
    static CANVAS_COMMANDS: RefCell<HashMap<usize, Vec<DrawCommand>>> = RefCell::new(HashMap::new());
    /// Last rendered command batch, used for native repaints that arrive before new commands.
    static CANVAS_LAST_FRAME: RefCell<HashMap<usize, Vec<DrawCommand>>> = RefCell::new(HashMap::new());
    /// Canvas sizes (width, height), keyed by view address
    static CANVAS_SIZES: RefCell<HashMap<usize, (f64, f64)>> = RefCell::new(HashMap::new());
    static CANVAS_IMAGES: RefCell<HashMap<i64, CanvasImage>> = RefCell::new(HashMap::new());
    static IMAGE_CACHE: RefCell<HashMap<String, i64>> = RefCell::new(HashMap::new());
}

static NEXT_IMAGE_HANDLE: AtomicI64 = AtomicI64::new(1);

const JS_TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

extern "C" {
    fn js_object_alloc(class_id: u32, field_count: u32) -> *mut c_void;
    fn js_object_set_field_by_name(obj: *mut c_void, key: *const c_void, value: f64);
    fn js_object_get_field_by_name_f64(obj: *mut c_void, key: *const c_void) -> f64;
    fn js_nanbox_pointer(ptr: i64) -> f64;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_promise_resolved(value: f64) -> *mut c_void;
    fn js_promise_rejected(reason: f64) -> *mut c_void;
}

fn js_key(name: &[u8]) -> *mut c_void {
    unsafe { js_string_from_bytes(name.as_ptr(), name.len() as u32) as *mut c_void }
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

// Custom NSView subclass for canvas drawing
pub struct PerryCanvasViewIvars {
    view_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[name = "PerryCanvasView"]
    #[ivars = PerryCanvasViewIvars]
    pub struct PerryCanvasView;

    impl PerryCanvasView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty_rect: CGRect) {
            let key = self.ivars().view_key.get();

            // Get the current graphics context
            let ctx: CGContextRef = unsafe {
                let ns_ctx: *mut AnyObject = msg_send![
                    objc2::class!(NSGraphicsContext),
                    currentContext
                ];
                if ns_ctx.is_null() { return; }
                msg_send![ns_ctx, CGContext]
            };
            if ctx.is_null() { return; }

            // Get canvas size for coordinate flipping
            let (canvas_w, canvas_h) = CANVAS_SIZES.with(|s| {
                s.borrow().get(&key).copied().unwrap_or((0.0, 0.0))
            });

            // Drain pending commands for this paint. Canvas calls between paints
            // form a single frame, so frame-loop apps don't replay every historical
            // command. Keep only the last rendered batch for native repaint events
            // that arrive before the app submits new canvas commands.
            let commands = CANVAS_COMMANDS
                .with(|cmds| {
                    let mut cmds = cmds.borrow_mut();
                    cmds.get_mut(&key).and_then(|pending| {
                        if pending.is_empty() || !command_batch_renders(pending) {
                            None
                        } else {
                            let commands = std::mem::take(pending);
                            CANVAS_LAST_FRAME.with(|last| {
                                last.borrow_mut().insert(key, commands.clone());
                            });
                            Some(commands)
                        }
                    })
                })
                .or_else(|| CANVAS_LAST_FRAME.with(|last| last.borrow().get(&key).cloned()));

            if let Some(commands) = commands {
                    // Track current path points for gradient fill.
                    // #854: an `in_path: bool` companion used to live here
                    // but was only ever written, never read. Removed.
                    let mut path_points: Vec<(f64, f64)> = Vec::new();
                    // Stateful (HTML5-canvas-style) drawing state.
                    let mut cur_stroke = (0.0, 0.0, 0.0, 1.0);
                    let mut cur_fill = (0.0, 0.0, 0.0, 1.0);
                    let mut cur_width = 1.0;
                    let mut path_closed = false;

                    for cmd in commands.iter() {
                        match cmd {
                            DrawCommand::BeginPath => {
                                path_points.clear();
                                path_closed = false;
                            }
                            DrawCommand::MoveTo(x, y) => {
                                // Flip Y coordinate (macOS origin is bottom-left)
                                let flipped_y = canvas_h - y;
                                path_points.push((*x, flipped_y));
                            }
                            DrawCommand::LineTo(x, y) => {
                                let flipped_y = canvas_h - y;
                                path_points.push((*x, flipped_y));
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
                            }
                            DrawCommand::FillGradient { r1, g1, b1, a1, r2, g2, b2, a2, direction } => {
                                if path_points.len() >= 2 {
                                    unsafe {
                                        CGContextSaveGState(ctx);

                                        // Build closed path for clipping
                                        // Add bottom edge to close the area under the line
                                        CGContextBeginPath(ctx);
                                        CGContextMoveToPoint(ctx, path_points[0].0, path_points[0].1);
                                        for pt in &path_points[1..] {
                                            CGContextAddLineToPoint(ctx, pt.0, pt.1);
                                        }
                                        // Close to bottom
                                        let last_x = path_points[path_points.len() - 1].0;
                                        let first_x = path_points[0].0;
                                        CGContextAddLineToPoint(ctx, last_x, 0.0); // bottom-right
                                        CGContextAddLineToPoint(ctx, first_x, 0.0); // bottom-left
                                        CGContextClosePath(ctx);
                                        CGContextClip(ctx);

                                        // Draw gradient
                                        let color_space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
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
                                            // Vertical: top to bottom
                                            (CGPoint::new(0.0, canvas_h), CGPoint::new(0.0, 0.0))
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
                            }
                            DrawCommand::SetStrokeColor { r, g, b, a } => {
                                cur_stroke = (*r, *g, *b, *a);
                            }
                            DrawCommand::SetFillColor { r, g, b, a } => {
                                cur_fill = (*r, *g, *b, *a);
                            }
                            DrawCommand::SetLineWidth(w) => {
                                cur_width = *w;
                            }
                            DrawCommand::ClosePath => {
                                path_closed = true;
                            }
                            DrawCommand::StrokePath => {
                                if path_points.len() >= 2 {
                                    unsafe {
                                        CGContextSaveGState(ctx);
                                        CGContextSetRGBStrokeColor(ctx, cur_stroke.0, cur_stroke.1, cur_stroke.2, cur_stroke.3);
                                        CGContextSetLineWidth(ctx, cur_width);
                                        CGContextSetLineCap(ctx, 1);
                                        CGContextSetLineJoin(ctx, 1);
                                        CGContextBeginPath(ctx);
                                        CGContextMoveToPoint(ctx, path_points[0].0, path_points[0].1);
                                        for pt in &path_points[1..] {
                                            CGContextAddLineToPoint(ctx, pt.0, pt.1);
                                        }
                                        if path_closed {
                                            CGContextClosePath(ctx);
                                        }
                                        CGContextStrokePath(ctx);
                                        CGContextRestoreGState(ctx);
                                    }
                                }
                            }
                            DrawCommand::FillPath => {
                                if path_points.len() >= 2 {
                                    unsafe {
                                        CGContextSaveGState(ctx);
                                        CGContextSetRGBFillColor(ctx, cur_fill.0, cur_fill.1, cur_fill.2, cur_fill.3);
                                        CGContextBeginPath(ctx);
                                        CGContextMoveToPoint(ctx, path_points[0].0, path_points[0].1);
                                        for pt in &path_points[1..] {
                                            CGContextAddLineToPoint(ctx, pt.0, pt.1);
                                        }
                                        CGContextClosePath(ctx);
                                        CGContextFillPath(ctx);
                                        CGContextRestoreGState(ctx);
                                    }
                                }
                            }
                            DrawCommand::FillRect { x, y, w, h } => {
                                let flipped_y = canvas_h - y - h;
                                unsafe {
                                    CGContextSaveGState(ctx);
                                    CGContextSetRGBFillColor(ctx, cur_fill.0, cur_fill.1, cur_fill.2, cur_fill.3);
                                    CGContextFillRect(ctx, CGRect::new(CGPoint::new(*x, flipped_y), CGSize::new(*w, *h)));
                                    CGContextRestoreGState(ctx);
                                }
                            }
                            DrawCommand::StrokeRect { x, y, w, h } => {
                                let flipped_y = canvas_h - y - h;
                                unsafe {
                                    CGContextSaveGState(ctx);
                                    CGContextSetRGBStrokeColor(ctx, cur_stroke.0, cur_stroke.1, cur_stroke.2, cur_stroke.3);
                                    CGContextSetLineWidth(ctx, cur_width);
                                    CGContextStrokeRect(ctx, CGRect::new(CGPoint::new(*x, flipped_y), CGSize::new(*w, *h)));
                                    CGContextRestoreGState(ctx);
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
                                    if let Some(img) = images.borrow().get(image).copied() {
                                        let src_w = if *sw > 0.0 { *sw } else { img.width };
                                        let src_h = if *sh > 0.0 { *sh } else { img.height };
                                        let dst_w = if *dw > 0.0 { *dw } else { src_w };
                                        let dst_h = if *dh > 0.0 { *dh } else { src_h };
                                        if dst_w <= 0.0 || dst_h <= 0.0 {
                                            return;
                                        }
                                        let flipped_y = canvas_h - dy - dst_h;
                                        unsafe {
                                            CGContextSaveGState(ctx);
                                            let draw_image = if *sw > 0.0 || *sh > 0.0 || *sx != 0.0 || *sy != 0.0 {
                                                CGImageCreateWithImageInRect(
                                                    img.cg_image,
                                                    CGRect::new(
                                                        CGPoint::new(*sx, *sy),
                                                        CGSize::new(src_w, src_h),
                                                    ),
                                                )
                                            } else {
                                                img.cg_image
                                            };
                                            if !draw_image.is_null() {
                                                CGContextDrawImage(
                                                    ctx,
                                                    CGRect::new(
                                                        CGPoint::new(*dx, flipped_y),
                                                        CGSize::new(dst_w, dst_h),
                                                    ),
                                                    draw_image,
                                                );
                                                if draw_image != img.cg_image {
                                                    CFRelease(draw_image as *const c_void);
                                                }
                                            }
                                            CGContextRestoreGState(ctx);
                                        }
                                    }
                                });
                            }
                        }
                    }
                }
            }

        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            // Return false — we handle flipping manually in drawRect
            false
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

        // Set a fixed size via Auto Layout constraints
        unsafe {
            let _: () = msg_send![&*view, setTranslatesAutoresizingMaskIntoConstraints: false];
            let width_constraint: Retained<AnyObject> = msg_send![&*view, widthAnchor];
            let constraint: Retained<AnyObject> = msg_send![
                &*width_constraint, constraintEqualToConstant: width
            ];
            let _: () = msg_send![&*constraint, setActive: true];

            let height_anchor: Retained<AnyObject> = msg_send![&*view, heightAnchor];
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
    CANVAS_LAST_FRAME.with(|last| {
        last.borrow_mut().insert(key, Vec::new());
    });
    CANVAS_SIZES.with(|s| {
        s.borrow_mut().insert(key, (width, height));
    });

    // Cast to NSView for registration
    let ns_view: Retained<NSView> = unsafe { Retained::cast(view) };
    register_widget(ns_view)
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
        CANVAS_LAST_FRAME.with(|last| {
            if let Some(commands) = last.borrow_mut().get_mut(&key) {
                commands.clear();
            }
        });
        // Trigger redraw
        if let Some(view) = super::get_widget(handle) {
            unsafe {
                let _: () = msg_send![&*view, setNeedsDisplay: true];
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
        if let Some(view) = super::get_widget(handle) {
            unsafe {
                let _: () = msg_send![&*view, setNeedsDisplay: true];
            }
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
    if let Some(key) = get_canvas_key(handle) {
        CANVAS_COMMANDS.with(|cmds| {
            if let Some(commands) = cmds.borrow_mut().get_mut(&key) {
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
        if let Some(view) = super::get_widget(handle) {
            unsafe {
                let _: () = msg_send![&*view, setNeedsDisplay: true];
            }
        }
    }
}

// ── Stateful (HTML5-canvas-style) API ────────────────────────────────
// Setters record state; stroke_path/fill render the accumulated path.

fn push_cmd(handle: i64, cmd: DrawCommand) {
    if let Some(key) = get_canvas_key(handle) {
        CANVAS_COMMANDS.with(|cmds| {
            if let Some(commands) = cmds.borrow_mut().get_mut(&key) {
                commands.push(cmd);
            }
        });
    }
}

fn redraw(handle: i64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let _: () = msg_send![&*view, setNeedsDisplay: true];
        }
    }
}

pub fn set_stroke_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    push_cmd(handle, DrawCommand::SetStrokeColor { r, g, b, a });
}

pub fn set_fill_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    push_cmd(handle, DrawCommand::SetFillColor { r, g, b, a });
}

pub fn set_line_width(handle: i64, w: f64) {
    push_cmd(handle, DrawCommand::SetLineWidth(w));
}

pub fn close_path(handle: i64) {
    push_cmd(handle, DrawCommand::ClosePath);
}

pub fn stroke_path(handle: i64) {
    push_cmd(handle, DrawCommand::StrokePath);
    redraw(handle);
}

pub fn fill(handle: i64) {
    push_cmd(handle, DrawCommand::FillPath);
    redraw(handle);
}

pub fn fill_rect(handle: i64, x: f64, y: f64, w: f64, h: f64) {
    push_cmd(handle, DrawCommand::FillRect { x, y, w, h });
    redraw(handle);
}

pub fn stroke_rect(handle: i64, x: f64, y: f64, w: f64, h: f64) {
    push_cmd(handle, DrawCommand::StrokeRect { x, y, w, h });
    redraw(handle);
}

fn resolve_asset_path(path: &str) -> String {
    if std::path::Path::new(path).is_absolute() {
        return path.to_string();
    }
    if let Some(found) = (|| {
        let bundle_class = objc2::runtime::AnyClass::get(c"NSBundle")?;
        let bundle: *mut objc2::runtime::AnyObject = unsafe { msg_send![bundle_class, mainBundle] };
        if bundle.is_null() {
            return None;
        }
        let res_path: Option<Retained<objc2_foundation::NSString>> =
            unsafe { msg_send![bundle, resourcePath] };
        let rp = res_path?;
        let candidate = std::path::PathBuf::from(rp.to_string()).join(path);
        candidate
            .exists()
            .then(|| candidate.to_string_lossy().to_string())
    })() {
        return found;
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

pub fn load_image(path: *const u8) -> i64 {
    let raw = crate::widgets::image::str_from_header(path);
    let resolved = resolve_asset_path(raw);
    if let Some(handle) = IMAGE_CACHE.with(|c| c.borrow().get(&resolved).copied()) {
        if let Some((width, height)) = CANVAS_IMAGES.with(|images| {
            images
                .borrow()
                .get(&handle)
                .map(|asset| (asset.width, asset.height))
        }) {
            return resolved_image_promise(handle, width, height);
        }
        return rejected_image_promise("Cached Canvas image handle was missing");
    }
    let bytes = match std::fs::read(&resolved) {
        Ok(bytes) if !bytes.is_empty() => bytes,
        _ => return rejected_image_promise(&format!("Failed to load image: {resolved}")),
    };
    unsafe {
        let data = CFDataCreate(std::ptr::null(), bytes.as_ptr(), bytes.len() as isize);
        if data.is_null() {
            return rejected_image_promise(&format!("Failed to allocate image data: {resolved}"));
        }
        let source = CGImageSourceCreateWithData(data, std::ptr::null());
        CFRelease(data);
        if source.is_null() {
            return rejected_image_promise(&format!("Failed to decode image: {resolved}"));
        }
        let image = CGImageSourceCreateImageAtIndex(source, 0, std::ptr::null());
        CFRelease(source);
        if image.is_null() {
            return rejected_image_promise(&format!("Failed to decode image: {resolved}"));
        }
        let handle = NEXT_IMAGE_HANDLE.fetch_add(1, Ordering::Relaxed);
        let asset = CanvasImage {
            cg_image: image,
            width: CGImageGetWidth(image) as f64,
            height: CGImageGetHeight(image) as f64,
        };
        CANVAS_IMAGES.with(|images| images.borrow_mut().insert(handle, asset));
        let width = asset.width;
        let height = asset.height;
        IMAGE_CACHE.with(|cache| cache.borrow_mut().insert(resolved, handle));
        resolved_image_promise(handle, width, height)
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
        },
    );
    redraw(handle);
}
