//! iOS toast presenter for `showToast(msg)` (Phase 2 v3.3).
//!
//! Renders a HUD-style banner near the top of the key UIWindow for ~2.5s
//! using a UIView overlay with a translucent dark rounded background and a
//! centered UILabel. Multiple toasts queued in quick succession render
//! back-to-back rather than overwriting each other (FIFO).
//!
//! ## Animation
//!
//! Alpha 0→1 over 0.25s via `UIView.animateWithDuration:`, holds for 2s,
//! then fades out 1→0 over 0.25s and removes the view. Uses NSTimer for the
//! hold and cleanup steps — same scheduling pattern as the pump timer in
//! `app::PerryPumpTarget`.
//!
//! ## Wiring
//!
//! `app::register_cross_platform_text_handlers` calls
//! `js_register_show_toast_handler` at app startup, passing
//! `show_toast_handler` here. When user TS calls `showToast("Saved!")` the
//! codegen emits `perry_arkts_show_toast`; the runtime forwards (ptr, len)
//! to this handler on the main thread.

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread};
use objc2_foundation::{NSObject, NSString};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

const TOAST_DURATION_SECS: f64 = 2.5;
const TOAST_FADE_SECS: f64 = 0.25;
const TOAST_WIDTH: f64 = 300.0;
const TOAST_HEIGHT: f64 = 52.0;
const TOAST_TOP_MARGIN: f64 = 60.0;

thread_local! {
    static TOAST_QUEUE: RefCell<VecDeque<String>> = const { RefCell::new(VecDeque::new()) };
    static PRESENTING: Cell<bool> = const { Cell::new(false) };
    static ACTIVE_VIEW: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
}

/// Cross-platform handler entry point registered with
/// `js_register_show_toast_handler` at app startup.
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

fn drain_if_idle() {
    if PRESENTING.with(|p| p.get()) {
        return;
    }
    let next = TOAST_QUEUE.with(|q| q.borrow_mut().pop_front());
    let Some(msg) = next else { return };
    PRESENTING.with(|p| p.set(true));
    present_toast(msg);
}

fn present_toast(msg: String) {
    unsafe {
        // Find the key UIWindow to anchor the toast overlay.
        let app_cls = AnyClass::get(c"UIApplication").expect("UIApplication");
        let app: *mut AnyObject = msg_send![app_cls, sharedApplication];
        let key_window: *mut AnyObject = msg_send![app, keyWindow];
        if key_window.is_null() {
            PRESENTING.with(|p| p.set(false));
            drain_if_idle();
            return;
        }

        let win_bounds: objc2_core_foundation::CGRect = msg_send![key_window, bounds];
        let toast_x = (win_bounds.size.width - TOAST_WIDTH) / 2.0;
        let toast_y = TOAST_TOP_MARGIN;

        // Container view: rounded, translucent dark background.
        let view_cls = AnyClass::get(c"UIView").expect("UIView");
        let toast_view_raw: *mut AnyObject = msg_send![view_cls, alloc];
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
        let toast_view_raw: *mut AnyObject = msg_send![toast_view_raw, initWithFrame: frame];
        let toast_view: Retained<AnyObject> =
            Retained::retain(toast_view_raw).expect("toast view alloc");

        // Style: rounded + semi-transparent dark background.
        let layer: *mut AnyObject = msg_send![&*toast_view, layer];
        let _: () = msg_send![layer, setCornerRadius: 12.0_f64];
        let _: () = msg_send![layer, setMasksToBounds: true];

        let ui_color_cls = AnyClass::get(c"UIColor").expect("UIColor");
        let bg_color: Retained<AnyObject> = msg_send![
            ui_color_cls,
            colorWithRed: 0.0_f64,
            green: 0.0_f64,
            blue: 0.0_f64,
            alpha: 0.78_f64
        ];
        let _: () = msg_send![&*toast_view, setBackgroundColor: &*bg_color];
        let _: () = msg_send![&*toast_view, setAlpha: 0.0_f64];
        let _: () = msg_send![&*toast_view, setUserInteractionEnabled: false];

        // UILabel: centered white text.
        let label_cls = AnyClass::get(c"UILabel").expect("UILabel");
        let label_raw: *mut AnyObject = msg_send![label_cls, new];
        let ns_msg = NSString::from_str(&msg);
        let _: () = msg_send![label_raw, setText: &*ns_msg];
        let white: Retained<AnyObject> = msg_send![ui_color_cls, whiteColor];
        let _: () = msg_send![label_raw, setTextColor: &*white];
        let _: () = msg_send![label_raw, setTextAlignment: 1i64]; // NSTextAlignmentCenter
        let _: () = msg_send![label_raw, setNumberOfLines: 2i64];
        let _: () = msg_send![label_raw, setLineBreakMode: 0u64]; // NSLineBreakByWordWrapping

        let ui_font_cls = AnyClass::get(c"UIFont").expect("UIFont");
        let font: Retained<AnyObject> = msg_send![
            ui_font_cls,
            systemFontOfSize: 14.0_f64
        ];
        let _: () = msg_send![label_raw, setFont: &*font];
        let _: () = msg_send![label_raw, setTranslatesAutoresizingMaskIntoConstraints: false];

        let _: () = msg_send![&*toast_view, addSubview: label_raw];

        // Auto Layout constraints: center label inside toast view with 12pt padding.
        let label_cx: *mut AnyObject = msg_send![label_raw, centerXAnchor];
        let view_cx: *mut AnyObject = msg_send![&*toast_view, centerXAnchor];
        let cx: *mut AnyObject = msg_send![label_cx, constraintEqualToAnchor: view_cx];
        let _: () = msg_send![cx, setActive: true];

        let label_cy: *mut AnyObject = msg_send![label_raw, centerYAnchor];
        let view_cy: *mut AnyObject = msg_send![&*toast_view, centerYAnchor];
        let cy: *mut AnyObject = msg_send![label_cy, constraintEqualToAnchor: view_cy];
        let _: () = msg_send![cy, setActive: true];

        let label_lead: *mut AnyObject = msg_send![label_raw, leadingAnchor];
        let view_lead: *mut AnyObject = msg_send![&*toast_view, leadingAnchor];
        let lc: *mut AnyObject = msg_send![
            label_lead, constraintGreaterThanOrEqualToAnchor: view_lead, constant: 12.0_f64
        ];
        let _: () = msg_send![lc, setActive: true];

        let label_trail: *mut AnyObject = msg_send![label_raw, trailingAnchor];
        let view_trail: *mut AnyObject = msg_send![&*toast_view, trailingAnchor];
        let tc: *mut AnyObject = msg_send![
            label_trail, constraintLessThanOrEqualToAnchor: view_trail, constant: -12.0_f64
        ];
        let _: () = msg_send![tc, setActive: true];

        // Add to key window (above all content), then fade in.
        let _: () = msg_send![key_window, addSubview: &*toast_view];
        let view_ptr = Retained::as_ptr(&toast_view) as usize;
        ACTIVE_VIEW.with(|av| *av.borrow_mut() = Some(toast_view.clone()));
        let _ = Retained::into_raw(toast_view);

        // Fade in.
        let animation_block = block2::RcBlock::new(move || {
            let vp = view_ptr as *mut AnyObject;
            if !vp.is_null() {
                let _: () = msg_send![vp, setAlpha: 1.0_f64];
            }
        });
        let _: () = msg_send![
            AnyClass::get(c"UIView").unwrap(),
            animateWithDuration: TOAST_FADE_SECS,
            animations: &*animation_block
        ];

        // Schedule fade-out.
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

// ===========================================================================
// NSTimer target classes — same define_class! pattern as PerryPumpTarget
// ===========================================================================

pub struct PerryToastFadeOutTargetIvars;

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryTVOSToastFadeOutTarget"]
    #[ivars = PerryToastFadeOutTargetIvars]
    pub struct PerryToastFadeOutTarget;

    impl PerryToastFadeOutTarget {
        #[unsafe(method(toastFadeOut:))]
        fn toast_fade_out(&self, _sender: &AnyObject) {
            let view_ptr = ACTIVE_VIEW.with(|av| {
                av.borrow().as_ref().map(|v| Retained::as_ptr(v) as usize)
            });
            if let Some(ptr) = view_ptr {
                unsafe {
                    let vp = ptr as *mut AnyObject;
                    let animation_block = block2::RcBlock::new(move || {
                        let _: () = msg_send![vp, setAlpha: 0.0_f64];
                    });
                    let _: () = msg_send![
                        AnyClass::get(c"UIView").unwrap(),
                        animateWithDuration: TOAST_FADE_SECS,
                        animations: &*animation_block
                    ];
                }
            }
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
    #[name = "PerryTVOSToastCleanupTarget"]
    #[ivars = PerryToastCleanupTargetIvars]
    pub struct PerryToastCleanupTarget;

    impl PerryToastCleanupTarget {
        #[unsafe(method(toastCleanup:))]
        fn toast_cleanup(&self, _sender: &AnyObject) {
            let view_opt = ACTIVE_VIEW.with(|av| av.borrow_mut().take());
            if let Some(view) = view_opt {
                unsafe {
                    let _: () = msg_send![&*view, removeFromSuperview];
                }
                drop(view);
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
