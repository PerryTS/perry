// Canvas widget — custom drawing via Core Graphics
//
// Stores a command buffer that replays on each drawRect: call.
// Commands: MoveTo, LineTo, Stroke, FillGradient, BeginPath, Clear.

use crate::ffi::js_string_from_bytes;
use crate::ffi::CGContextAddLineToPoint;
use crate::ffi::CGContextBeginPath;
use crate::ffi::CGContextClosePath;
use crate::ffi::CGContextFillPath;
use crate::ffi::CGContextFillRect;
use crate::ffi::CGContextMoveToPoint;
use crate::ffi::CGContextSetLineWidth;
use crate::ffi::CGContextStrokePath;
use crate::ffi::CGContextStrokeRect;
use crate::srgb;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::NSView;
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{MainThreadMarker, NSString};
use perry_ui::canvas_font::{parse_canvas_font, CanvasFont};

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
    fn CGContextClearRect(c: CGContextRef, rect: CGRect);
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
    ClearRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
    SetFont(CanvasFont),
    FillText {
        text: String,
        x: f64,
        y: f64,
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
                | DrawCommand::ClearRect { .. }
                | DrawCommand::FillText { .. }
        )
    })
}

/// Prefixes `commands` with the drawing state earlier batches left set, so a saved batch
/// replays alone and the state lasts as in HTML canvas: a setter until the next one, the
/// current path until the next BeginPath. Records the state `commands` leave into `state`.
fn carry_state(state: &mut Vec<DrawCommand>, commands: Vec<DrawCommand>) -> Vec<DrawCommand> {
    let mut batch = state.clone();
    let is_path = |cmd: &DrawCommand| {
        matches!(
            cmd,
            DrawCommand::MoveTo(..) | DrawCommand::LineTo(..) | DrawCommand::ClosePath
        )
    };
    for cmd in &commands {
        if matches!(cmd, DrawCommand::BeginPath) {
            state.retain(|kept| !is_path(kept));
        } else if is_path(cmd) {
            state.push(cmd.clone());
        } else if matches!(
            cmd,
            DrawCommand::SetFillColor { .. }
                | DrawCommand::SetStrokeColor { .. }
                | DrawCommand::SetLineWidth(_)
                | DrawCommand::SetFont(_)
        ) {
            state.retain(|kept| std::mem::discriminant(kept) != std::mem::discriminant(cmd));
            state.push(cmd.clone());
        }
    }
    batch.extend(commands);
    batch
}

/// Drains the pending commands for one paint. Canvas calls between paints form a single
/// frame, so frame-loop apps don't replay every historical command. The last rendered batch
/// is kept for native repaints that arrive before the app submits new canvas commands.
fn take_frame(key: usize) -> Option<Vec<DrawCommand>> {
    CANVAS_COMMANDS
        .with(|cmds| {
            let mut cmds = cmds.borrow_mut();
            cmds.get_mut(&key).and_then(|pending| {
                if pending.is_empty() || !command_batch_renders(pending) {
                    None
                } else {
                    let commands = CANVAS_STATE.with(|s| {
                        carry_state(
                            s.borrow_mut().entry(key).or_default(),
                            std::mem::take(pending),
                        )
                    });
                    CANVAS_LAST_FRAME.with(|last| {
                        last.borrow_mut().insert(key, commands.clone());
                    });
                    Some(commands)
                }
            })
        })
        .or_else(|| CANVAS_LAST_FRAME.with(|last| last.borrow().get(&key).cloned()))
}

thread_local! {
    /// Pending canvas commands, keyed by view address.
    static CANVAS_COMMANDS: RefCell<HashMap<usize, Vec<DrawCommand>>> = RefCell::new(HashMap::new());
    /// Last rendered command batch, used for native repaints that arrive before new commands.
    static CANVAS_LAST_FRAME: RefCell<HashMap<usize, Vec<DrawCommand>>> = RefCell::new(HashMap::new());
    /// Drawing state in effect after the last taken batch; see `carry_state`.
    static CANVAS_STATE: RefCell<HashMap<usize, Vec<DrawCommand>>> = RefCell::new(HashMap::new());
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

            let commands = take_frame(key);

            if let Some(commands) = commands {
                    // Track current path points for gradient fill.
                    // #854: an `in_path: bool` companion used to live here
                    // but was only ever written, never read. Removed.
                    let mut path_points: Vec<(f64, f64)> = Vec::new();
                    // Stateful (HTML5-canvas-style) drawing state.
                    let mut cur_stroke = (0.0, 0.0, 0.0, 1.0);
                    let mut cur_fill = (0.0, 0.0, 0.0, 1.0);
                    let mut cur_width = 1.0;
                    let mut cur_font = CanvasFont::default();
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
                                        srgb::set_stroke_color(ctx, *r, *g, *b, *a);
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
                                        let color_space = srgb::color_space();
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
                                        srgb::set_stroke_color(ctx, cur_stroke.0, cur_stroke.1, cur_stroke.2, cur_stroke.3);
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
                                        srgb::set_fill_color(ctx, cur_fill.0, cur_fill.1, cur_fill.2, cur_fill.3);
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
                                    srgb::set_fill_color(ctx, cur_fill.0, cur_fill.1, cur_fill.2, cur_fill.3);
                                    CGContextFillRect(ctx, CGRect::new(CGPoint::new(*x, flipped_y), CGSize::new(*w, *h)));
                                    CGContextRestoreGState(ctx);
                                }
                            }
                            DrawCommand::SetFont(font) => {
                                cur_font = font.clone();
                            }
                            DrawCommand::FillText { text, x, y } => unsafe {
                                draw_text(text, &cur_font, cur_fill, *x, canvas_h - y);
                            },
                            DrawCommand::ClearRect { x, y, w, h } => {
                                let flipped_y = canvas_h - y - h;
                                unsafe {
                                    CGContextClearRect(ctx, CGRect::new(CGPoint::new(*x, flipped_y), CGSize::new(*w, *h)));
                                }
                            }
                            DrawCommand::StrokeRect { x, y, w, h } => {
                                let flipped_y = canvas_h - y - h;
                                unsafe {
                                    CGContextSaveGState(ctx);
                                    srgb::set_stroke_color(ctx, cur_stroke.0, cur_stroke.1, cur_stroke.2, cur_stroke.3);
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
    let ns_view: Retained<NSView> = unsafe { Retained::cast_unchecked(view) };
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
                CANVAS_STATE.with(|s| {
                    carry_state(
                        s.borrow_mut().entry(key).or_default(),
                        std::mem::take(commands),
                    )
                });
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

/// Adds a circular arc as HTML canvas does: a line from the current point to the arc's
/// start, then the arc clockwise on screen from `start` to `end` (radians). The arc is
/// recorded as short chords so fill, stroke and fillGradient share the one point list.
pub fn arc(handle: i64, x: f64, y: f64, r: f64, start: f64, end: f64) {
    if !([x, y, r, start, end].iter().all(|v| v.is_finite()) && r >= 0.0) {
        return;
    }
    let tau = std::f64::consts::TAU;
    let sweep = if end - start >= tau {
        tau
    } else {
        (end - start).rem_euclid(tau)
    };
    // About 2pt per chord, at least 8 per full turn.
    let steps = ((sweep * r / 2.0).ceil() as usize)
        .clamp((8.0 * sweep / tau).ceil().max(1.0) as usize, 4096);
    for i in 0..=steps {
        let a = start + sweep * i as f64 / steps as f64;
        line_to(handle, x + r * a.cos(), y + r * a.sin());
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

/// Clear a rectangle to transparent, keeping what earlier commands drew outside it.
pub fn clear_rect(handle: i64, x: f64, y: f64, w: f64, h: f64) {
    push_cmd(handle, DrawCommand::ClearRect { x, y, w, h });
    redraw(handle);
}

pub fn stroke_rect(handle: i64, x: f64, y: f64, w: f64, h: f64) {
    push_cmd(handle, DrawCommand::StrokeRect { x, y, w, h });
    redraw(handle);
}

/// An invalid font spec keeps the previous font, as HTML canvas does.
pub fn set_font(handle: i64, spec: &str) {
    if let Some(font) = parse_canvas_font(spec) {
        push_cmd(handle, DrawCommand::SetFont(font));
    }
}

pub fn fill_text(handle: i64, text: String, x: f64, y: f64) {
    if !(x.is_finite() && y.is_finite()) {
        return;
    }
    push_cmd(handle, DrawCommand::FillText { text, x, y });
    redraw(handle);
}

/// The first available family, generics mapped to system faces, with the bold
/// and italic traits applied where the family has them.
unsafe fn ns_font(font: &CanvasFont) -> Retained<AnyObject> {
    let cls = AnyClass::get(c"NSFont").unwrap();
    let manager: Retained<AnyObject> =
        msg_send![AnyClass::get(c"NSFontManager").unwrap(), sharedFontManager];
    let size = font.size;
    let system = || -> Retained<AnyObject> { msg_send![cls, systemFontOfSize: size] };
    let base = font
        .families
        .iter()
        .find_map(|family| {
            let name = match family.to_ascii_lowercase().as_str() {
                "sans-serif" | "system-ui" => return Some(system()),
                "serif" => "Times New Roman".to_string(),
                "monospace" => "Menlo".to_string(),
                _ => family.clone(),
            };
            let name = NSString::from_str(&name);
            let by_family: Option<Retained<AnyObject>> = msg_send![
                &*manager,
                fontWithFamily: &*name,
                traits: 0usize,
                weight: 5isize,
                size: size
            ];
            by_family.or_else(|| msg_send![cls, fontWithName: &*name, size: size])
        })
        .unwrap_or_else(system);
    // NSItalicFontMask = 1, NSBoldFontMask = 2.
    let mut styled = base;
    for (wanted, mask) in [(font.bold, 2usize), (font.italic, 1usize)] {
        if wanted {
            styled = msg_send![&*manager, convertFont: &*styled, toHaveTrait: mask];
        }
    }
    styled
}

/// Draws `text` into the current NSGraphicsContext with its alphabetic
/// baseline starting at (x, baseline) in unflipped coordinates.
unsafe fn draw_text(
    text: &str,
    font: &CanvasFont,
    fill: (f64, f64, f64, f64),
    x: f64,
    baseline: f64,
) {
    let attrs: Retained<AnyObject> = msg_send![AnyClass::get(c"NSMutableDictionary").unwrap(), new];
    let ns_font = ns_font(font);
    let color = srgb::ns_color(fill.0, fill.1, fill.2, fill.3);
    let _: () = msg_send![&*attrs, setObject: &*ns_font, forKey: &*NSString::from_str("NSFont")];
    let _: () = msg_send![&*attrs, setObject: &*color, forKey: &*NSString::from_str("NSColor")];
    // Without NSStringDrawingUsesLineFragmentOrigin the rect's origin is the baseline.
    let rect = CGRect::new(CGPoint::new(x, baseline), CGSize::new(0.0, 0.0));
    let _: () = msg_send![
        &*NSString::from_str(text),
        drawWithRect: rect,
        options: 0isize,
        attributes: &*attrs,
        context: std::ptr::null_mut::<AnyObject>()
    ];
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
    let raw = unsafe { crate::widgets::image::str_from_header(path) };
    let resolved = resolve_asset_path(&raw);
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

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_app_kit::NSApplication;
    use perry_runtime as _;

    #[test]
    fn arc_records_a_clockwise_chord_path_from_start_to_end() {
        let mtm =
            MainThreadMarker::new().expect("panic-abort tests run on the process main thread");
        let _app = NSApplication::sharedApplication(mtm);
        let canvas = create(100.0, 100.0);
        arc(canvas, 50.0, 50.0, 10.0, 0.0, std::f64::consts::FRAC_PI_2);
        arc(canvas, 0.0, 0.0, f64::NAN, 0.0, 1.0);
        let key = get_canvas_key(canvas).unwrap();
        let points: Vec<(f64, f64)> = CANVAS_COMMANDS.with(|cmds| {
            cmds.borrow()[&key]
                .iter()
                .map(|c| match c {
                    DrawCommand::LineTo(x, y) => (*x, *y),
                    _ => panic!("arc recorded a command other than LineTo"),
                })
                .collect()
        });
        let close = |(x, y): (f64, f64), (ex, ey): (f64, f64)| {
            (x - ex).abs() < 1e-9 && (y - ey).abs() < 1e-9
        };
        assert!(
            close(points[0], (60.0, 50.0)),
            "starts at angle 0: {:?}",
            points[0]
        );
        assert!(
            close(*points.last().unwrap(), (50.0, 60.0)),
            "ends a quarter turn clockwise on screen (y down)"
        );
        assert!(
            points.len() >= 9,
            "a quarter circle of radius 10 is several chords"
        );
        assert!(points
            .iter()
            .all(|&(x, y)| ((x - 50.0).hypot(y - 50.0) - 10.0).abs() < 1e-9));
    }

    #[test]
    fn clear_rect_records_its_rect_after_earlier_commands() {
        let mtm =
            MainThreadMarker::new().expect("panic-abort tests run on the process main thread");
        let _app = NSApplication::sharedApplication(mtm);
        let canvas = create(100.0, 100.0);
        fill_rect(canvas, 0.0, 0.0, 100.0, 100.0);
        crate::lib_ffi::style_and_canvas::perry_ui_canvas_clear_rect(
            canvas, 10.0, 20.0, 30.0, 40.0,
        );
        let key = get_canvas_key(canvas).unwrap();
        let commands = CANVAS_COMMANDS.with(|cmds| cmds.borrow()[&key].clone());
        assert!(
            matches!(
                commands.as_slice(),
                [DrawCommand::FillRect { .. }, DrawCommand::ClearRect { x, y, w, h }]
                    if (*x, *y, *w, *h) == (10.0, 20.0, 30.0, 40.0)
            ),
            "clearRect must append its rect and keep the earlier fillRect"
        );
        assert!(command_batch_renders(&commands[1..]));

        crate::lib_ffi::style_and_canvas::perry_ui_canvas_clear_rect(
            canvas,
            f64::NAN,
            0.0,
            f64::INFINITY,
            10.0,
        );
        let after = CANVAS_COMMANDS.with(|cmds| cmds.borrow()[&key].len());
        assert_eq!(after, 2, "a non-finite clearRect records nothing");
    }

    fn perry_str(text: &str) -> i64 {
        unsafe { js_string_from_bytes(text.as_ptr(), text.len() as u32) as i64 }
    }

    #[test]
    fn fill_text_records_font_and_text_after_fill_color() {
        use crate::lib_ffi::style_and_canvas as ffi;
        let mtm =
            MainThreadMarker::new().expect("panic-abort tests run on the process main thread");
        let _app = NSApplication::sharedApplication(mtm);
        let canvas = create(100.0, 100.0);
        ffi::perry_ui_canvas_set_fill_color(canvas, 1.0, 0.0, 0.0, 1.0);
        ffi::perry_ui_canvas_set_font(canvas, perry_str("bold 20px Menlo"));
        ffi::perry_ui_canvas_fill_text(canvas, perry_str("Hi"), 5.0, 30.0);
        ffi::perry_ui_canvas_set_font(canvas, perry_str("Menlo"));
        let key = get_canvas_key(canvas).unwrap();
        let commands = CANVAS_COMMANDS.with(|cmds| cmds.borrow()[&key].clone());
        let expected = CanvasFont {
            size: 20.0,
            bold: true,
            italic: false,
            families: vec!["Menlo".into()],
        };
        assert!(
            matches!(
                commands.as_slice(),
                [
                    DrawCommand::SetFillColor { r, .. },
                    DrawCommand::SetFont(font),
                    DrawCommand::FillText { text, x, y },
                ] if *r == 1.0 && *font == expected && text == "Hi" && (*x, *y) == (5.0, 30.0)
            ),
            "setFont and fillText record in call order, and an invalid font records nothing"
        );
        assert!(command_batch_renders(&commands[2..]));
    }

    #[test]
    fn drawing_state_carries_into_the_next_batch() {
        use crate::lib_ffi::style_and_canvas as ffi;
        let mtm =
            MainThreadMarker::new().expect("panic-abort tests run on the process main thread");
        let _app = NSApplication::sharedApplication(mtm);
        let canvas = create(100.0, 100.0);
        ffi::perry_ui_canvas_set_fill_color(canvas, 1.0, 0.0, 0.0, 1.0);
        ffi::perry_ui_canvas_set_font(canvas, perry_str("24px serif"));
        ffi::perry_ui_canvas_fill_text(canvas, perry_str("a"), 0.0, 10.0);
        let key = get_canvas_key(canvas).unwrap();
        assert!(take_frame(key).is_some());
        ffi::perry_ui_canvas_fill_text(canvas, perry_str("b"), 0.0, 20.0);
        let batch = take_frame(key).unwrap();
        assert!(
            matches!(
                batch.as_slice(),
                [
                    DrawCommand::SetFillColor { r, .. },
                    DrawCommand::SetFont(font),
                    DrawCommand::FillText { text, .. },
                ] if *r == 1.0 && font.size == 24.0 && text == "b"
            ),
            "a later batch replays with the fill color and font set before the previous paint"
        );
    }

    #[test]
    fn current_path_carries_until_the_next_begin_path() {
        let mtm =
            MainThreadMarker::new().expect("panic-abort tests run on the process main thread");
        let _app = NSApplication::sharedApplication(mtm);
        let canvas = create(100.0, 100.0);
        let key = get_canvas_key(canvas).unwrap();
        begin_path(canvas);
        move_to(canvas, 0.0, 0.0);
        line_to(canvas, 10.0, 10.0);
        stroke_path(canvas);
        assert!(take_frame(key).is_some());
        set_stroke_color(canvas, 1.0, 0.0, 0.0, 1.0);
        stroke_path(canvas);
        let batch = take_frame(key).unwrap();
        assert!(
            matches!(
                batch.as_slice(),
                [
                    DrawCommand::MoveTo(x0, y0),
                    DrawCommand::LineTo(x1, y1),
                    DrawCommand::SetStrokeColor { .. },
                    DrawCommand::StrokePath,
                ] if (*x0, *y0, *x1, *y1) == (0.0, 0.0, 10.0, 10.0)
            ),
            "stroke() keeps the path, so a later batch strokes it again"
        );
        begin_path(canvas);
        fill_rect(canvas, 0.0, 0.0, 1.0, 1.0);
        assert!(take_frame(key).is_some());
        stroke_path(canvas);
        let batch = take_frame(key).unwrap();
        assert!(
            matches!(
                batch.as_slice(),
                [DrawCommand::SetStrokeColor { .. }, DrawCommand::StrokePath]
            ),
            "beginPath drops the carried path"
        );
    }

    #[test]
    fn invalid_line_width_keeps_the_previous_one() {
        use crate::lib_ffi::style_and_canvas as ffi;
        let mtm =
            MainThreadMarker::new().expect("panic-abort tests run on the process main thread");
        let _app = NSApplication::sharedApplication(mtm);
        let canvas = create(100.0, 100.0);
        for w in [4.0, 0.0, f64::NAN, -1.0, f64::INFINITY] {
            ffi::perry_ui_canvas_set_line_width(canvas, w);
        }
        let key = get_canvas_key(canvas).unwrap();
        let commands = CANVAS_COMMANDS.with(|cmds| cmds.borrow()[&key].clone());
        assert!(matches!(commands.as_slice(), [DrawCommand::SetLineWidth(w)] if *w == 4.0));
    }

    #[test]
    fn non_finite_rects_record_nothing() {
        use crate::lib_ffi::style_and_canvas as ffi;
        let mtm =
            MainThreadMarker::new().expect("panic-abort tests run on the process main thread");
        let _app = NSApplication::sharedApplication(mtm);
        let canvas = create(100.0, 100.0);
        ffi::perry_ui_canvas_fill_rect(canvas, f64::NAN, 0.0, 10.0, 10.0);
        ffi::perry_ui_canvas_stroke_rect(canvas, 0.0, 0.0, f64::INFINITY, 10.0);
        ffi::perry_ui_canvas_fill_rect(canvas, 10.0, 10.0, -5.0, -5.0);
        let key = get_canvas_key(canvas).unwrap();
        let commands = CANVAS_COMMANDS.with(|cmds| cmds.borrow()[&key].clone());
        assert!(
            matches!(commands.as_slice(), [DrawCommand::FillRect { w, h, .. }] if (*w, *h) == (-5.0, -5.0)),
            "only the finite rect records, negative sizes included"
        );
    }

    #[test]
    fn fill_text_inks_above_its_baseline_only() {
        extern "C" {
            fn CGBitmapContextCreate(
                data: *mut c_void,
                width: usize,
                height: usize,
                bits_per_component: usize,
                bytes_per_row: usize,
                space: CGColorSpaceRef,
                bitmap_info: u32,
            ) -> CGContextRef;
            fn CGContextRelease(c: CGContextRef);
        }
        #[repr(C)]
        struct CGContext {
            _private: [u8; 0],
        }
        unsafe impl objc2::encode::RefEncode for CGContext {
            const ENCODING_REF: objc2::encode::Encoding = objc2::encode::Encoding::Pointer(
                &objc2::encode::Encoding::Struct("CGContext", &[]),
            );
        }
        const SIZE: usize = 40;
        const BASELINE: f64 = 10.0;
        let mut pixels = vec![0u8; SIZE * SIZE * 4];
        let font = parse_canvas_font("30px sans-serif").unwrap();
        unsafe {
            // kCGImageAlphaPremultipliedLast
            let ctx = CGBitmapContextCreate(
                pixels.as_mut_ptr().cast(),
                SIZE,
                SIZE,
                8,
                SIZE * 4,
                srgb::color_space(),
                1,
            );
            let ns_ctx: *mut AnyObject = msg_send![
                objc2::class!(NSGraphicsContext),
                graphicsContextWithCGContext: ctx.cast::<CGContext>(),
                flipped: false
            ];
            let _: () = msg_send![objc2::class!(NSGraphicsContext), saveGraphicsState];
            let _: () = msg_send![objc2::class!(NSGraphicsContext), setCurrentContext: ns_ctx];
            draw_text("H", &font, (0.0, 0.0, 0.0, 1.0), 2.0, BASELINE);
            let _: () = msg_send![objc2::class!(NSGraphicsContext), restoreGraphicsState];
            CGContextRelease(ctx);
        }
        // Bitmap rows run top-down; CoreGraphics y runs bottom-up.
        let baseline_row = SIZE - BASELINE as usize;
        let inked = |rows: std::ops::Range<usize>| {
            rows.flat_map(|row| (0..SIZE).map(move |col| (row * SIZE + col) * 4 + 3))
                .any(|alpha| pixels[alpha] > 0)
        };
        assert!(inked(0..baseline_row), "an H draws above its baseline");
        assert!(
            !inked(baseline_row + 1..SIZE),
            "an H draws nothing below its baseline"
        );
    }
}
