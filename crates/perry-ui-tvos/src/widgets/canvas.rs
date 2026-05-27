// Canvas widget — custom drawing via Core Graphics
//
// Stores a command buffer that replays on each drawRect: call.
// Commands: MoveTo, LineTo, Stroke, FillGradient, BeginPath, Clear.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{MainThreadMarker, NSObject};
use objc2_ui_kit::UIView;

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
    fn CGContextDrawImage(c: CGContextRef, rect: CGRect, image: CGImageRef);
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
    fn CGContextDrawLinearGradient(
        c: CGContextRef,
        gradient: CGGradientRef,
        start_point: CGPoint,
        end_point: CGPoint,
        options: u32,
    );
    fn CGColorSpaceCreateDeviceRGB() -> CGColorSpaceRef;
    fn CGColorSpaceRelease(space: CGColorSpaceRef);
    fn CGGradientCreateWithColorComponents(
        space: CGColorSpaceRef,
        components: *const CGFloat,
        locations: *const CGFloat,
        count: usize,
    ) -> CGGradientRef;
    fn CGGradientRelease(gradient: CGGradientRef);
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
            DrawCommand::Stroke { .. } | DrawCommand::FillGradient { .. }
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
                                        if src_w <= 0.0 || src_h <= 0.0 || dst_w <= 0.0 || dst_h <= 0.0 {
                                            return;
                                        }
                                        unsafe {
                                            CGContextSaveGState(ctx);
                                            let draw_image = if *sw > 0.0
                                                || *sh > 0.0
                                                || *sx != 0.0
                                                || *sy != 0.0
                                            {
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
                                                        CGPoint::new(*dx, *dy),
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
            let width_anchor: Retained<AnyObject> = msg_send![&*view, widthAnchor];
            let constraint: Retained<AnyObject> = msg_send![
                &*width_anchor, constraintEqualToConstant: width
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

    // Cast to UIView for registration
    let ui_view: Retained<UIView> = unsafe { Retained::cast_unchecked(view) };
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
        CANVAS_LAST_FRAME.with(|last| {
            if let Some(commands) = last.borrow_mut().get_mut(&key) {
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
                let _: () = msg_send![&*view, setNeedsDisplay];
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
                let _: () = msg_send![&*view, setNeedsDisplay];
            }
        }
    }
}

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
            let _: () = msg_send![&*view, setNeedsDisplay];
        }
    }
}

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let len = *(ptr as *const u32) as usize;
        let data = ptr.add(4);
        let slice = std::slice::from_raw_parts(data, len);
        std::str::from_utf8_unchecked(slice)
    }
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
    let raw = str_from_header(path);
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
