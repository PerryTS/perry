//! macOS toast presenter for `showToast(msg)` (Phase 2 v3.3).
//!
//! Renders a HUD-style banner near the top of the active window for ~2.5s
//! using a borderless `NSPanel` containing an NSTextField with rounded
//! corners and translucent dark background. Multiple toasts queued in
//! quick succession render back-to-back rather than overwriting each other.
//!
//! ## Wiring
//!
//! `app::register_cross_platform_text_handlers` calls
//! `js_register_show_toast_handler` from the perry-runtime crate at app
//! startup, passing `show_toast_handler` here as the registered fn
//! pointer. When user TS code later calls `showToast("Saved!")`, the
//! codegen emits a call to `perry_arkts_show_toast` (the cross-platform
//! symbol defined in perry-runtime/src/ui_text_registry.rs); the runtime
//! decodes the NaN-boxed string and forwards to this handler on the main
//! thread.
//!
//! The handler enqueues into a per-process `TOAST_QUEUE` and, when no
//! toast is currently animating, kicks off the next one. NSTimer drives
//! the fade-in / hold / fade-out / cleanup steps — same scheduling
//! pattern as `app::install_test_mode_exit_timer` and `perry_ui_app_set_timer`.

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread};
use objc2_app_kit::{NSColor, NSTextField};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

const TOAST_DURATION_SECS: f64 = 2.5;
const TOAST_FADE_SECS: f64 = 0.25;
const TOAST_WIDTH: f64 = 320.0;
const TOAST_HEIGHT: f64 = 56.0;
const TOAST_TOP_MARGIN: f64 = 64.0;

thread_local! {
    /// FIFO of pending toast messages.
    static TOAST_QUEUE: RefCell<VecDeque<String>> = const { RefCell::new(VecDeque::new()) };
    /// `true` while a toast is currently animating.
    static PRESENTING: Cell<bool> = const { Cell::new(false) };
    /// Per-active-toast NSPanel handle. Held for the full 2.5s lifecycle
    /// so the fade-out + cleanup callbacks can re-orderOut + close.
    static ACTIVE_PANEL: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
}

/// Cross-platform handler entry point. Registered with
/// `js_register_show_toast_handler` at app startup. Queues the message
/// and triggers the presenter if idle.
pub extern "C" fn show_toast_handler(msg_ptr: *const u8, msg_len: usize) {
    if msg_ptr.is_null() {
        return;
    }
    let msg = unsafe {
        let bytes = std::slice::from_raw_parts(msg_ptr, msg_len);
        String::from_utf8_lossy(bytes).into_owned()
    };
    TOAST_QUEUE.with(|q| q.borrow_mut().push_back(msg));
    drain_if_idle();
}

/// If no toast is currently active, pop the next queued message and
/// show it. Called both on enqueue and after each toast finishes.
fn drain_if_idle() {
    if PRESENTING.with(|p| p.get()) {
        return;
    }
    let next = TOAST_QUEUE.with(|q| q.borrow_mut().pop_front());
    let Some(msg) = next else { return };
    PRESENTING.with(|p| p.set(true));
    present_toast(msg);
}

/// Build and present a single toast window. Animates alpha 0→1 over
/// `TOAST_FADE_SECS`, holds for the visible interval, schedules a
/// fade-out timer, then closes the window and triggers the next queue
/// drain.
fn present_toast(msg: String) {
    let Some(mtm) = MainThreadMarker::new() else {
        // Not on main thread — drop. A future v3.4 follow-up could
        // marshal via dispatch_async, but every Perry UI call is
        // expected to be on the main thread already.
        PRESENTING.with(|p| p.set(false));
        return;
    };

    unsafe {
        // Find the key window so we anchor the toast to its frame.
        // Falls back to mainWindow if no key window. If the app has no
        // window at all (agent-style), drop the toast silently.
        let app_cls = AnyClass::get(c"NSApplication").unwrap();
        let app: *mut AnyObject = msg_send![app_cls, sharedApplication];
        let mut anchor_window: *mut AnyObject = msg_send![app, keyWindow];
        if anchor_window.is_null() {
            anchor_window = msg_send![app, mainWindow];
        }
        if anchor_window.is_null() {
            PRESENTING.with(|p| p.set(false));
            drain_if_idle();
            return;
        }

        let win_frame: objc2_core_foundation::CGRect = msg_send![anchor_window, frame];
        let toast_x = win_frame.origin.x + (win_frame.size.width - TOAST_WIDTH) / 2.0;
        let toast_y = win_frame.origin.y + win_frame.size.height - TOAST_TOP_MARGIN - TOAST_HEIGHT;

        // Borderless NSPanel — non-activating overlay above the window.
        let panel_cls = AnyClass::get(c"NSPanel").unwrap();
        let panel: Retained<AnyObject> = {
            let alloc: *mut AnyObject = msg_send![panel_cls, alloc];
            let frame = objc2_core_foundation::CGRect {
                origin: objc2_core_foundation::CGPoint {
                    x: toast_x,
                    y: toast_y,
                },
                size: objc2_core_foundation::CGSize {
                    width: TOAST_WIDTH,
                    height: TOAST_HEIGHT,
                },
            };
            let initialized: *mut AnyObject = msg_send![
                alloc,
                initWithContentRect: frame,
                styleMask: 0u64,        // NSWindowStyleMaskBorderless
                backing: 2u64,          // NSBackingStoreBuffered
                defer: false
            ];
            Retained::from_raw(initialized).expect("NSPanel init returned nil")
        };

        let _: () = msg_send![&*panel, setLevel: 3i64]; // NSFloatingWindowLevel
        let _: () = msg_send![&*panel, setOpaque: false];
        let clear_bg: Retained<NSColor> = msg_send![AnyClass::get(c"NSColor").unwrap(), clearColor];
        let _: () = msg_send![&*panel, setBackgroundColor: &*clear_bg];
        let _: () = msg_send![&*panel, setHasShadow: true];
        let _: () = msg_send![&*panel, setIgnoresMouseEvents: true];
        let _: () = msg_send![&*panel, setHidesOnDeactivate: false];
        let _: () = msg_send![&*panel, setAlphaValue: 0.0_f64];

        // Inner content view — rounded translucent dark background +
        // centered NSTextField.
        let content_view: *mut AnyObject = msg_send![&*panel, contentView];
        let _: () = msg_send![content_view, setWantsLayer: true];
        let layer: *mut AnyObject = msg_send![content_view, layer];
        let _: () = msg_send![layer, setCornerRadius: 12.0_f64];
        let _: () = msg_send![layer, setMasksToBounds: true];

        let bg: Retained<NSColor> = msg_send![
            AnyClass::get(c"NSColor").unwrap(),
            colorWithRed: 0.0 as CGFloat,
            green: 0.0 as CGFloat,
            blue: 0.0 as CGFloat,
            alpha: 0.78 as CGFloat
        ];
        let cg: *mut AnyObject = msg_send![&*bg, CGColor];
        let _: () = msg_send![layer, setBackgroundColor: cg];

        let ns_msg = NSString::from_str(&msg);
        let label: Retained<NSTextField> = NSTextField::labelWithString(&ns_msg, mtm);
        let _: () = msg_send![&*label, setTranslatesAutoresizingMaskIntoConstraints: false];

        let white: Retained<NSColor> = msg_send![AnyClass::get(c"NSColor").unwrap(), whiteColor];
        let _: () = msg_send![&*label, setTextColor: &*white];
        let _: () = msg_send![&*label, setAlignment: 1i64]; // NSTextAlignmentCenter
        let font_cls = AnyClass::get(c"NSFont").unwrap();
        let font: Retained<AnyObject> = msg_send![
            font_cls,
            systemFontOfSize: 14.0 as CGFloat,
            weight: 0.0 as CGFloat
        ];
        let _: () = msg_send![&*label, setFont: &*font];
        let _: () = msg_send![&*label, setMaximumNumberOfLines: 2i64];
        let cell: *mut AnyObject = msg_send![&*label, cell];
        if !cell.is_null() {
            let _: () = msg_send![cell, setLineBreakMode: 0u64];
            let _: () = msg_send![cell, setTruncatesLastVisibleLine: true];
        }

        let label_obj: &AnyObject = &*label;
        let _: () = msg_send![content_view, addSubview: label_obj];
        let _: () = msg_send![label_obj, setUsesSingleLineMode: false];
        constrain_center(content_view, label_obj);

        // Show without activating + fade in.
        let _: () = msg_send![&*panel, orderFront: std::ptr::null::<AnyObject>()];
        animate_alpha(&panel, 1.0, TOAST_FADE_SECS);

        // Stash so the fade-out / cleanup callbacks can reach it.
        ACTIVE_PANEL.with(|p| *p.borrow_mut() = Some(panel.clone()));
        // Let the panel's own retain hold it alive across the timers
        // — we'll release on cleanup. Hand off the Retained.
        let _ = Retained::into_raw(panel);

        // Fade-out timer — fires (TOAST_DURATION_SECS - TOAST_FADE_SECS)
        // seconds in. The Step-1 target then sets up Step-2 (cleanup).
        let visible_secs = TOAST_DURATION_SECS - TOAST_FADE_SECS;
        let target = PerryToastFadeOutTarget::new();
        let sel = Sel::register(c"toastFadeOut:");
        let _: Retained<AnyObject> = msg_send![
            objc2::class!(NSTimer),
            scheduledTimerWithTimeInterval: visible_secs,
            target: &*target,
            selector: sel,
            userInfo: std::ptr::null::<AnyObject>(),
            repeats: false
        ];
        std::mem::forget(target);
    }
}

/// Add Auto Layout constraints to center `child` inside `parent` with
/// 12pt horizontal padding.
unsafe fn constrain_center(parent: *mut AnyObject, child: &AnyObject) {
    let parent_obj: &AnyObject = &*parent;

    let child_x: *mut AnyObject = msg_send![child, centerXAnchor];
    let parent_x: *mut AnyObject = msg_send![parent_obj, centerXAnchor];
    let cx: *mut AnyObject = msg_send![child_x, constraintEqualToAnchor: parent_x];
    let _: () = msg_send![cx, setActive: true];

    let child_y: *mut AnyObject = msg_send![child, centerYAnchor];
    let parent_y: *mut AnyObject = msg_send![parent_obj, centerYAnchor];
    let cy: *mut AnyObject = msg_send![child_y, constraintEqualToAnchor: parent_y];
    let _: () = msg_send![cy, setActive: true];

    let child_lead: *mut AnyObject = msg_send![child, leadingAnchor];
    let parent_lead: *mut AnyObject = msg_send![parent_obj, leadingAnchor];
    let lc: *mut AnyObject = msg_send![
        child_lead,
        constraintGreaterThanOrEqualToAnchor: parent_lead,
        constant: 12.0 as CGFloat
    ];
    let _: () = msg_send![lc, setActive: true];

    let child_trail: *mut AnyObject = msg_send![child, trailingAnchor];
    let parent_trail: *mut AnyObject = msg_send![parent_obj, trailingAnchor];
    let tc: *mut AnyObject = msg_send![
        child_trail,
        constraintLessThanOrEqualToAnchor: parent_trail,
        constant: -12.0 as CGFloat
    ];
    let _: () = msg_send![tc, setActive: true];
}

/// Animate `alphaValue` of `panel` to `target` over `duration` seconds
/// via `NSAnimationContext` + `setAnimator`.
unsafe fn animate_alpha(panel: &AnyObject, target: f64, duration: f64) {
    let ctx_cls = AnyClass::get(c"NSAnimationContext").unwrap();
    let _: () = msg_send![ctx_cls, beginGrouping];
    let current: *mut AnyObject = msg_send![ctx_cls, currentContext];
    let _: () = msg_send![current, setDuration: duration as CGFloat];
    let animator: *mut AnyObject = msg_send![panel, animator];
    let _: () = msg_send![animator, setAlphaValue: target];
    let _: () = msg_send![ctx_cls, endGrouping];
}

// ===========================================================================
// NSTimer target classes for fade-out and cleanup. Same `define_class!` +
// `scheduledTimerWithTimeInterval:target:selector:` pattern as
// `app::PerryTestExitTarget` and `app::PerryTimerTarget`.
// ===========================================================================

pub struct PerryToastFadeOutTargetIvars;

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryToastFadeOutTarget"]
    #[ivars = PerryToastFadeOutTargetIvars]
    pub struct PerryToastFadeOutTarget;

    impl PerryToastFadeOutTarget {
        #[unsafe(method(toastFadeOut:))]
        fn toast_fade_out(&self, _sender: &AnyObject) {
            ACTIVE_PANEL.with(|p| {
                if let Some(panel) = p.borrow().as_ref() {
                    unsafe {
                        animate_alpha(panel, 0.0, TOAST_FADE_SECS);
                    }
                }
            });
            // Schedule cleanup after fade-out completes.
            unsafe {
                let cleanup = PerryToastCleanupTarget::new();
                let sel = Sel::register(c"toastCleanup:");
                let _: Retained<AnyObject> = msg_send![
                    objc2::class!(NSTimer),
                    scheduledTimerWithTimeInterval: TOAST_FADE_SECS,
                    target: &*cleanup,
                    selector: sel,
                    userInfo: std::ptr::null::<AnyObject>(),
                    repeats: false
                ];
                std::mem::forget(cleanup);
            }
        }
    }
);

impl PerryToastFadeOutTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryToastFadeOutTargetIvars);
        unsafe { msg_send![super(this), init] }
    }
}

pub struct PerryToastCleanupTargetIvars;

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryToastCleanupTarget"]
    #[ivars = PerryToastCleanupTargetIvars]
    pub struct PerryToastCleanupTarget;

    impl PerryToastCleanupTarget {
        #[unsafe(method(toastCleanup:))]
        fn toast_cleanup(&self, _sender: &AnyObject) {
            // Take ownership back from the std::mem::forget at present_toast,
            // close the panel, drop, advance the queue.
            let panel_opt = ACTIVE_PANEL.with(|p| p.borrow_mut().take());
            if let Some(panel) = panel_opt {
                unsafe {
                    let _: () = msg_send![&*panel, orderOut: std::ptr::null::<AnyObject>()];
                    let _: () = msg_send![&*panel, close];
                }
                drop(panel);
            }
            PRESENTING.with(|p| p.set(false));
            drain_if_idle();
        }
    }
);

impl PerryToastCleanupTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryToastCleanupTargetIvars);
        unsafe { msg_send![super(this), init] }
    }
}
