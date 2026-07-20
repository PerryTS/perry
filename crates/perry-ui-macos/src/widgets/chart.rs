//! macOS Chart widget (issue #474, v1).
//!
//! Custom `NSView` subclass that draws line / bar / pie charts via
//! CoreGraphics in `drawRect:`. Apple's `Charts` framework is iOS 16+
//! / macOS 13+ only — drawing manually keeps the widget portable to
//! older targets and keeps the runtime small.
//!
//! API shape (TS):
//!   Chart(kind, width, height): Widget        // 0=line, 1=bar, 2=pie
//!   chartAddDataPoint(widget, label, value): void
//!   chartClearData(widget): void
//!   chartSetTitle(widget, title): void
//!   chartReload(widget): void                 // setNeedsDisplay
//!
//! Out of scope v1 (per #474 scope notes): multi-series line, grouped/
//! stacked bars, donut, area, axis labels, legend, hover/tap tooltips,
//! animated transitions, color theming. The base widget plumbing lands
//! first; richer modes follow as #474 follow-ups once the surface is
//! used in TS apps.

use crate::ffi::CGContextAddLineToPoint;
use crate::ffi::CGContextBeginPath;
use crate::ffi::CGContextClosePath;
use crate::ffi::CGContextFillPath;
use crate::ffi::CGContextFillRect;
use crate::ffi::CGContextMoveToPoint;
use crate::ffi::CGContextSetLineWidth;
use crate::ffi::CGContextSetRGBFillColor;
use crate::ffi::CGContextSetRGBStrokeColor;
use crate::ffi::CGContextStrokePath;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{define_class, AnyThread, DefinedClass, MainThreadOnly};
use objc2_app_kit::NSView;
use objc2_core_foundation::CGFloat;
use objc2_foundation::MainThreadMarker;
use std::cell::{Cell, RefCell};

#[repr(i64)]
#[derive(Copy, Clone)]
pub enum ChartKind {
    Line = 0,
    Bar = 1,
    Pie = 2,
}

struct ChartEntry {
    handle: i64,
    kind: ChartKind,
    title: String,
    data: Vec<(String, f64)>,
}

thread_local! {
    static CHARTS: RefCell<Vec<ChartEntry>> = const { RefCell::new(Vec::new()) };
}

fn find_idx(handle: i64) -> Option<usize> {
    CHARTS.with(|c| c.borrow().iter().position(|e| e.handle == handle))
}

fn str_from_header(ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let header = ptr as *const crate::string_header::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len)).to_string()
    }
}

// ===========================================================================
// PerryChartView — custom NSView with drawRect: override.
// ===========================================================================

pub struct PerryChartViewIvars {
    pub handle: Cell<i64>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[name = "PerryChartView"]
    #[ivars = PerryChartViewIvars]
    pub struct PerryChartView;

    impl PerryChartView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, dirty: objc2_core_foundation::CGRect) {
            let _ = dirty;
            let handle = self.ivars().handle.get();
            let entry = CHARTS.with(|c| {
                c.borrow().iter().find(|e| e.handle == handle).map(|e| {
                    (e.kind as i64, e.title.clone(), e.data.clone())
                })
            });
            let Some((kind, title, data)) = entry else { return };
            unsafe {
                let bounds: objc2_core_foundation::CGRect = msg_send![self, bounds];
                draw_chart(kind, &title, &data, bounds);
            }
        }
    }
);

impl PerryChartView {
    fn new(handle: i64, frame: objc2_core_foundation::CGRect) -> Retained<Self> {
        let mtm = MainThreadMarker::new().expect("perry/ui chart must be on main thread");
        let this = Self::alloc(mtm).set_ivars(PerryChartViewIvars {
            handle: Cell::new(handle),
        });
        unsafe {
            let init: Retained<Self> = msg_send![super(this), initWithFrame: frame];
            init
        }
    }
}

// ===========================================================================
// Drawing routines — CoreGraphics on the current NSGraphicsContext.
// ===========================================================================

unsafe fn current_cg_context() -> *mut std::ffi::c_void {
    let ctx_cls = AnyClass::get(c"NSGraphicsContext").unwrap();
    let cur: *mut AnyObject = msg_send![ctx_cls, currentContext];
    if cur.is_null() {
        return std::ptr::null_mut();
    }
    msg_send![cur, CGContext]
}

extern "C" {
    fn CGContextAddArc(
        c: *mut std::ffi::c_void,
        x: CGFloat,
        y: CGFloat,
        radius: CGFloat,
        start_angle: CGFloat,
        end_angle: CGFloat,
        clockwise: i32,
    );
}

const PADDING: CGFloat = 24.0;
const PALETTE: &[(CGFloat, CGFloat, CGFloat)] = &[
    (0.20, 0.55, 0.92),
    (0.94, 0.38, 0.40),
    (0.32, 0.78, 0.42),
    (0.95, 0.69, 0.20),
    (0.62, 0.42, 0.86),
    (0.18, 0.74, 0.78),
    (0.86, 0.40, 0.62),
    (0.55, 0.55, 0.55),
];

unsafe fn draw_chart(
    kind: i64,
    title: &str,
    data: &[(String, f64)],
    bounds: objc2_core_foundation::CGRect,
) {
    let ctx = current_cg_context();
    if ctx.is_null() {
        return;
    }

    // Background — NSView is transparent by default; explicit white-ish
    // panel keeps the chart legible against any container.
    CGContextSetRGBFillColor(ctx, 1.0, 1.0, 1.0, 1.0);
    CGContextFillRect(ctx, bounds);

    let plot_top = bounds.size.height - PADDING - if title.is_empty() { 0.0 } else { 18.0 };
    let plot = objc2_core_foundation::CGRect {
        origin: objc2_core_foundation::CGPoint::new(PADDING, PADDING),
        size: objc2_core_foundation::CGSize::new(
            bounds.size.width - 2.0 * PADDING,
            (plot_top - PADDING).max(1.0),
        ),
    };

    if !title.is_empty() {
        draw_text_centered(
            title,
            objc2_core_foundation::CGRect {
                origin: objc2_core_foundation::CGPoint::new(
                    bounds.origin.x,
                    bounds.size.height - 22.0,
                ),
                size: objc2_core_foundation::CGSize::new(bounds.size.width, 18.0),
            },
            14.0,
        );
    }

    if data.is_empty() {
        return;
    }

    match kind {
        0 => draw_line(ctx, plot, data),
        1 => draw_bars(ctx, plot, data),
        2 => draw_pie(ctx, plot, data),
        _ => {}
    }
}

unsafe fn draw_line(
    ctx: *mut std::ffi::c_void,
    plot: objc2_core_foundation::CGRect,
    data: &[(String, f64)],
) {
    let max = data
        .iter()
        .map(|(_, v)| *v)
        .fold(f64::NEG_INFINITY, f64::max);
    let min = data.iter().map(|(_, v)| *v).fold(f64::INFINITY, f64::min);
    let range = if (max - min).abs() < 1e-9 {
        1.0
    } else {
        max - min
    };

    let n = data.len();
    let dx = if n > 1 {
        plot.size.width / (n as f64 - 1.0)
    } else {
        0.0
    };

    // Axis — bottom + left.
    CGContextSetRGBStrokeColor(ctx, 0.7, 0.7, 0.7, 1.0);
    CGContextSetLineWidth(ctx, 1.0);
    CGContextBeginPath(ctx);
    CGContextMoveToPoint(ctx, plot.origin.x, plot.origin.y);
    CGContextAddLineToPoint(ctx, plot.origin.x + plot.size.width, plot.origin.y);
    CGContextStrokePath(ctx);

    // Series.
    let (r, g, b) = PALETTE[0];
    CGContextSetRGBStrokeColor(ctx, r, g, b, 1.0);
    CGContextSetLineWidth(ctx, 2.0);
    CGContextBeginPath(ctx);
    for (i, (_, v)) in data.iter().enumerate() {
        let x = plot.origin.x + (i as f64) * dx;
        let y = plot.origin.y + (v - min) / range * plot.size.height;
        if i == 0 {
            CGContextMoveToPoint(ctx, x, y);
        } else {
            CGContextAddLineToPoint(ctx, x, y);
        }
    }
    CGContextStrokePath(ctx);
}

unsafe fn draw_bars(
    ctx: *mut std::ffi::c_void,
    plot: objc2_core_foundation::CGRect,
    data: &[(String, f64)],
) {
    let max = data.iter().map(|(_, v)| *v).fold(0.0_f64, f64::max);
    if max <= 0.0 {
        return;
    }
    let n = data.len();
    let bar_w = plot.size.width / (n as f64) * 0.7;
    let gap = plot.size.width / (n as f64) * 0.3;

    for (i, (_, v)) in data.iter().enumerate() {
        let h = (v / max) * plot.size.height;
        let x = plot.origin.x + (i as f64) * (bar_w + gap) + gap / 2.0;
        let y = plot.origin.y;
        let (r, g, b) = PALETTE[i % PALETTE.len()];
        CGContextSetRGBFillColor(ctx, r, g, b, 1.0);
        CGContextFillRect(
            ctx,
            objc2_core_foundation::CGRect {
                origin: objc2_core_foundation::CGPoint::new(x, y),
                size: objc2_core_foundation::CGSize::new(bar_w, h),
            },
        );
    }
}

unsafe fn draw_pie(
    ctx: *mut std::ffi::c_void,
    plot: objc2_core_foundation::CGRect,
    data: &[(String, f64)],
) {
    let total: f64 = data.iter().map(|(_, v)| *v).sum();
    if total <= 0.0 {
        return;
    }
    let cx = plot.origin.x + plot.size.width / 2.0;
    let cy = plot.origin.y + plot.size.height / 2.0;
    let radius = plot.size.width.min(plot.size.height) / 2.0 - 4.0;

    let mut start = -std::f64::consts::FRAC_PI_2; // 12 o'clock
    for (i, (_, v)) in data.iter().enumerate() {
        let frac = v / total;
        let end = start + frac * std::f64::consts::TAU;
        let (r, g, b) = PALETTE[i % PALETTE.len()];
        CGContextSetRGBFillColor(ctx, r, g, b, 1.0);
        CGContextBeginPath(ctx);
        CGContextMoveToPoint(ctx, cx, cy);
        CGContextAddArc(ctx, cx, cy, radius, start, end, 0);
        CGContextClosePath(ctx);
        CGContextFillPath(ctx);
        start = end;
    }
}

unsafe fn draw_text_centered(text: &str, rect: objc2_core_foundation::CGRect, size: f64) {
    use objc2_foundation::NSString;
    let ns_text = NSString::from_str(text);

    let font_cls = AnyClass::get(c"NSFont").unwrap();
    let font: *mut AnyObject = msg_send![font_cls, systemFontOfSize: size as CGFloat];

    let para_cls = AnyClass::get(c"NSMutableParagraphStyle").unwrap();
    let para: *mut AnyObject = msg_send![para_cls, new];
    let _: () = msg_send![para, setAlignment: 1i64]; // NSTextAlignmentCenter

    let dict_cls = AnyClass::get(c"NSMutableDictionary").unwrap();
    let attrs: *mut AnyObject = msg_send![dict_cls, new];
    let font_key = NSString::from_str("NSFont");
    let para_key = NSString::from_str("NSParagraphStyle");
    let color_key = NSString::from_str("NSColor");
    let _: () = msg_send![attrs, setObject: font, forKey: &*font_key];
    let _: () = msg_send![attrs, setObject: para, forKey: &*para_key];

    let color_cls = AnyClass::get(c"NSColor").unwrap();
    let color: *mut AnyObject = msg_send![color_cls, colorWithCalibratedRed: 0.10 as CGFloat, green: 0.10 as CGFloat, blue: 0.10 as CGFloat, alpha: 1.0 as CGFloat];
    let _: () = msg_send![attrs, setObject: color, forKey: &*color_key];

    let _: () = msg_send![&*ns_text, drawInRect: rect, withAttributes: attrs];
}

// ===========================================================================
// Public API.
// ===========================================================================

pub fn create(kind: i64, width: f64, height: f64) -> i64 {
    let kind_enum = match kind {
        0 => ChartKind::Line,
        1 => ChartKind::Bar,
        2 => ChartKind::Pie,
        _ => ChartKind::Line,
    };
    let frame = objc2_core_foundation::CGRect::new(
        objc2_core_foundation::CGPoint::new(0.0, 0.0),
        objc2_core_foundation::CGSize::new(width.max(40.0), height.max(40.0)),
    );
    let handle = CHARTS.with(|c| c.borrow().len() as i64 + 1);
    let view = PerryChartView::new(handle, frame);

    let cast: Retained<NSView> = unsafe { Retained::cast_unchecked(view) };
    let real_handle = super::register_widget(cast);
    // The handle Cell on the view was preset to a guessed value; align
    // it to the real handle returned by `register_widget`.
    if real_handle != handle {
        if let Some(view) = super::get_widget(real_handle) {
            unsafe {
                let typed = Retained::as_ptr(&view) as *const PerryChartView;
                (*typed).ivars().handle.set(real_handle);
            }
        }
    }
    CHARTS.with(|c| {
        c.borrow_mut().push(ChartEntry {
            handle: real_handle,
            kind: kind_enum,
            title: String::new(),
            data: Vec::new(),
        });
    });
    real_handle
}

pub fn add_data_point(handle: i64, label_ptr: *const u8, value: f64) {
    let label = str_from_header(label_ptr);
    if let Some(idx) = find_idx(handle) {
        CHARTS.with(|c| {
            if let Some(entry) = c.borrow_mut().get_mut(idx) {
                entry.data.push((label, value));
            }
        });
    }
    request_redraw(handle);
}

pub fn clear_data(handle: i64) {
    if let Some(idx) = find_idx(handle) {
        CHARTS.with(|c| {
            if let Some(entry) = c.borrow_mut().get_mut(idx) {
                entry.data.clear();
            }
        });
    }
    request_redraw(handle);
}

pub fn set_title(handle: i64, title_ptr: *const u8) {
    let title = str_from_header(title_ptr);
    if let Some(idx) = find_idx(handle) {
        CHARTS.with(|c| {
            if let Some(entry) = c.borrow_mut().get_mut(idx) {
                entry.title = title;
            }
        });
    }
    request_redraw(handle);
}

pub fn reload(handle: i64) {
    request_redraw(handle);
}

fn request_redraw(handle: i64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let _: () = msg_send![&*view, setNeedsDisplay: true];
        }
    }
}
