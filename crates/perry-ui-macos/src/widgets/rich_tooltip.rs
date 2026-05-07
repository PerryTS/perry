//! macOS rich tooltip presenter (issue #479).
//!
//! Plain-text tooltips go through `widgets::set_tooltip` →
//! `NSView.setToolTip:`, which AppKit handles natively (with VoiceOver
//! pickup). This module covers the richer case: render an arbitrary
//! widget tree (already-built handle from `WIDGETS`) as the tooltip body
//! and show it in a borderless NSPanel anchored to the host widget after
//! a hover delay.
//!
//! Wiring: `perry_ui_widget_set_rich_tooltip(widget, content, delay_ms)`
//! (defined in lib.rs) calls `set_rich_tooltip` here. We register a
//! per-widget `PerryHoverTooltipTarget` (NSObject subclass) as the owner
//! of an NSTrackingArea on the widget. AppKit dispatches `mouseEntered:`
//! and `mouseExited:` to the target, which schedules / cancels an
//! NSTimer driving the show/dismiss lifecycle.
//!
//! The displayed panel is a borderless NSPanel (NSFloatingWindowLevel)
//! whose contentView contains the user's content widget pulled from
//! `WIDGETS`. Sized to fit the content view's `fittingSize` and
//! positioned 8pt below the host widget — falls back above when there
//! isn't room below the screen edge.

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_app_kit::NSView;
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_foundation::NSObject;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use crate::widgets::get_widget;

#[derive(Clone)]
struct RichTooltipBinding {
    content_handle: i64,
    hover_delay_ms: u32,
    active_panel: Option<Retained<AnyObject>>,
    show_timer: Option<Retained<AnyObject>>,
}

thread_local! {
    static BINDINGS: RefCell<HashMap<i64, RichTooltipBinding>> = RefCell::new(HashMap::new());
}

/// Register a rich tooltip on `widget_handle`. `content_handle` must
/// reference an already-registered widget. `hover_delay_ms` defaults
/// to 500 when 0 is passed.
pub fn set_rich_tooltip(widget_handle: i64, content_handle: i64, hover_delay_ms: u32) {
    let delay = if hover_delay_ms == 0 {
        500
    } else {
        hover_delay_ms
    };
    BINDINGS.with(|b| {
        b.borrow_mut().insert(
            widget_handle,
            RichTooltipBinding {
                content_handle,
                hover_delay_ms: delay,
                active_panel: None,
                show_timer: None,
            },
        );
    });
    let Some(view) = get_widget(widget_handle) else {
        return;
    };
    install_tracking(widget_handle, &view);
}

unsafe fn install_tracking_inner(widget_handle: i64, view: &NSView) {
    let target = PerryHoverTooltipTarget::new(widget_handle);
    let target_obj: &AnyObject = &*target;

    let bounds: CGRect = msg_send![view, bounds];
    let ta_cls = AnyClass::get(c"NSTrackingArea").unwrap();
    let ta_alloc: *mut AnyObject = msg_send![ta_cls, alloc];
    // NSTrackingMouseEnteredAndExited (1) | NSTrackingActiveAlways (0x80) |
    // NSTrackingInVisibleRect (0x200) so the rect tracks size changes.
    let options: u64 = 0x01 | 0x80 | 0x200;
    let ta: *mut AnyObject = msg_send![
        ta_alloc, initWithRect: bounds, options: options, owner: target_obj, userInfo: std::ptr::null::<AnyObject>()
    ];
    let _: () = msg_send![view, addTrackingArea: ta];

    // Keep the target alive — leaks one PerryHoverTooltipTarget per
    // widget that gets a rich tooltip. Acceptable: tooltips are
    // typically attached once at construction and live for the app
    // session.
    std::mem::forget(target);
}

fn install_tracking(widget_handle: i64, view: &NSView) {
    unsafe {
        install_tracking_inner(widget_handle, view);
    }
}

fn schedule_show(widget_handle: i64) {
    let delay_ms = BINDINGS.with(|b| b.borrow().get(&widget_handle).map(|x| x.hover_delay_ms));
    let Some(delay_ms) = delay_ms else { return };

    cancel_timer(widget_handle);

    unsafe {
        let target = PerryShowTooltipTarget::new(widget_handle);
        let sel = Sel::register(c"fireShowTooltip:");
        let interval = (delay_ms as f64) / 1000.0;
        let timer: Retained<AnyObject> = msg_send![
            objc2::class!(NSTimer),
            scheduledTimerWithTimeInterval: interval,
            target: &*target,
            selector: sel,
            userInfo: std::ptr::null::<AnyObject>(),
            repeats: false
        ];
        BINDINGS.with(|b| {
            if let Some(binding) = b.borrow_mut().get_mut(&widget_handle) {
                binding.show_timer = Some(timer);
            }
        });
        std::mem::forget(target);
    }
}

fn cancel_timer(widget_handle: i64) {
    let timer = BINDINGS.with(|b| {
        b.borrow_mut()
            .get_mut(&widget_handle)
            .and_then(|x| x.show_timer.take())
    });
    if let Some(t) = timer {
        unsafe {
            let _: () = msg_send![&*t, invalidate];
        }
    }
}

fn dismiss_panel(widget_handle: i64) {
    cancel_timer(widget_handle);
    let panel = BINDINGS.with(|b| {
        b.borrow_mut()
            .get_mut(&widget_handle)
            .and_then(|x| x.active_panel.take())
    });
    if let Some(p) = panel {
        unsafe {
            let _: () = msg_send![&*p, orderOut: std::ptr::null::<AnyObject>()];
            let _: () = msg_send![&*p, close];
        }
    }
}

fn present_panel(widget_handle: i64) {
    let (content_handle,) = BINDINGS.with(|b| {
        b.borrow()
            .get(&widget_handle)
            .map(|x| (x.content_handle,))
            .unwrap_or((0,))
    });
    if content_handle == 0 {
        return;
    }
    let Some(host) = get_widget(widget_handle) else {
        return;
    };
    let Some(content) = get_widget(content_handle) else {
        return;
    };

    unsafe {
        // Query content's natural size — fittingSize honours the view's
        // intrinsic contents and active Auto Layout constraints.
        let content_obj: &AnyObject = &*content;
        let mut content_size: CGSize = msg_send![content_obj, fittingSize];
        if content_size.width <= 0.0 {
            content_size.width = 240.0;
        }
        if content_size.height <= 0.0 {
            content_size.height = 80.0;
        }
        let pad = 8.0_f64;
        let panel_w = content_size.width + 2.0 * pad;
        let panel_h = content_size.height + 2.0 * pad;

        // Anchor: convert host's bounds to screen coordinates and place
        // 8pt below. Caller-side: NSView -> NSWindow -> NSScreen.
        let host_obj: &AnyObject = &*host;
        let host_bounds: CGRect = msg_send![host_obj, bounds];
        let in_window: CGRect =
            msg_send![host_obj, convertRect: host_bounds, toView: std::ptr::null::<AnyObject>()];
        let host_window: *mut AnyObject = msg_send![host_obj, window];
        if host_window.is_null() {
            return;
        }
        let win_origin_in_screen: CGRect = msg_send![host_window, convertRectToScreen: in_window];

        let mut panel_x = win_origin_in_screen.origin.x;
        let mut panel_y = win_origin_in_screen.origin.y - panel_h - 4.0;

        // If overflowing below the screen, flip above the widget.
        let main_screen: *mut AnyObject = msg_send![objc2::class!(NSScreen), mainScreen];
        if !main_screen.is_null() {
            let screen_frame: CGRect = msg_send![main_screen, visibleFrame];
            if panel_y < screen_frame.origin.y {
                panel_y = win_origin_in_screen.origin.y + win_origin_in_screen.size.height + 4.0;
            }
            if panel_x + panel_w > screen_frame.origin.x + screen_frame.size.width {
                panel_x = screen_frame.origin.x + screen_frame.size.width - panel_w - 4.0;
            }
            if panel_x < screen_frame.origin.x {
                panel_x = screen_frame.origin.x + 4.0;
            }
        }

        let frame = CGRect {
            origin: CGPoint {
                x: panel_x,
                y: panel_y,
            },
            size: CGSize {
                width: panel_w,
                height: panel_h,
            },
        };

        let panel_cls = AnyClass::get(c"NSPanel").unwrap();
        let alloc: *mut AnyObject = msg_send![panel_cls, alloc];
        let raw_panel: *mut AnyObject = msg_send![
            alloc,
            initWithContentRect: frame,
            styleMask: 0u64,    // borderless
            backing: 2u64,
            defer: false
        ];
        let panel: Retained<AnyObject> = Retained::from_raw(raw_panel).expect("NSPanel init nil");

        let _: () = msg_send![&*panel, setLevel: 3i64]; // NSFloatingWindowLevel
        let _: () = msg_send![&*panel, setOpaque: false];
        let clear: *mut AnyObject = msg_send![AnyClass::get(c"NSColor").unwrap(), clearColor];
        let _: () = msg_send![&*panel, setBackgroundColor: clear];
        let _: () = msg_send![&*panel, setHasShadow: true];
        let _: () = msg_send![&*panel, setIgnoresMouseEvents: false];
        let _: () = msg_send![&*panel, setHidesOnDeactivate: false];

        let panel_content: *mut AnyObject = msg_send![&*panel, contentView];
        let _: () = msg_send![panel_content, setWantsLayer: true];
        let layer: *mut AnyObject = msg_send![panel_content, layer];
        let _: () = msg_send![layer, setCornerRadius: 8.0_f64 as CGFloat];
        let _: () = msg_send![layer, setMasksToBounds: true];
        let bg_color: *mut AnyObject = msg_send![
            AnyClass::get(c"NSColor").unwrap(),
            colorWithRed: 0.10 as CGFloat,
            green: 0.10 as CGFloat,
            blue: 0.10 as CGFloat,
            alpha: 0.92 as CGFloat
        ];
        let cg: *mut AnyObject = msg_send![bg_color, CGColor];
        let _: () = msg_send![layer, setBackgroundColor: cg];

        // Place the user content view inside the panel content with `pad`
        // inset on every side. setFrame applies AppKit autoresize since
        // we don't enforce constraints here.
        let inner_frame = CGRect {
            origin: CGPoint { x: pad, y: pad },
            size: content_size,
        };
        let _: () = msg_send![content_obj, setFrame: inner_frame];
        let _: () = msg_send![panel_content, addSubview: content_obj];

        let _: () = msg_send![&*panel, orderFront: std::ptr::null::<AnyObject>()];

        BINDINGS.with(|b| {
            if let Some(binding) = b.borrow_mut().get_mut(&widget_handle) {
                binding.active_panel = Some(panel);
            }
        });
    }
}

// ===========================================================================
// NSObject targets — owner of the NSTrackingArea + NSTimer firing host.
// Same `define_class!` pattern as `widgets::toast::PerryToastFadeOutTarget`.
// ===========================================================================

pub struct PerryHoverTooltipIvars {
    widget_handle: Cell<i64>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryHoverTooltipTarget"]
    #[ivars = PerryHoverTooltipIvars]
    pub struct PerryHoverTooltipTarget;

    impl PerryHoverTooltipTarget {
        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &AnyObject) {
            schedule_show(self.ivars().widget_handle.get());
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &AnyObject) {
            dismiss_panel(self.ivars().widget_handle.get());
        }
    }
);

impl PerryHoverTooltipTarget {
    fn new(widget_handle: i64) -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryHoverTooltipIvars {
            widget_handle: Cell::new(widget_handle),
        });
        unsafe { msg_send![super(this), init] }
    }
}

pub struct PerryShowTooltipIvars {
    widget_handle: Cell<i64>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryShowTooltipTarget"]
    #[ivars = PerryShowTooltipIvars]
    pub struct PerryShowTooltipTarget;

    impl PerryShowTooltipTarget {
        #[unsafe(method(fireShowTooltip:))]
        fn fire_show(&self, _sender: &AnyObject) {
            present_panel(self.ivars().widget_handle.get());
        }
    }
);

impl PerryShowTooltipTarget {
    fn new(widget_handle: i64) -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryShowTooltipIvars {
            widget_handle: Cell::new(widget_handle),
        });
        unsafe { msg_send![super(this), init] }
    }
}
