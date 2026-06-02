//! iOS rich tooltip presenter (issue #479).
//!
//! Scope: iOS has no hover, so we use long-press as the trigger
//! gesture. The macOS impl in `perry-ui-macos` uses NSTrackingArea
//! mouseEntered/mouseExited; here we attach a
//! UILongPressGestureRecognizer to the trigger widget and present the
//! content widget tree in a transient overlay UIView attached to the
//! key window. Anchored above/below the trigger using the trigger's
//! frame in window coordinates (`convertRect:toView: nil`), and
//! dismissed on .ended/.cancelled or tap-outside.
//!
//! Wiring: `perry_ui_widget_set_rich_tooltip(widget, content, ms)` in
//! lib.rs calls `set_rich_tooltip` here. `ms` is interpreted as the
//! UILongPressGestureRecognizer `minimumPressDuration` (ms → seconds);
//! 0 → 0.5s default.

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, AnyThread, DefinedClass};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::NSObject;
use objc2_ui_kit::UIView;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use crate::widgets::get_widget;

struct RichTooltipBinding {
    content_handle: i64,
    min_press_secs: f64,
    /// Active overlay container view, if presented.
    active_overlay: Option<Retained<UIView>>,
    /// Dismiss-on-tap-outside gesture recognizer attached to the
    /// overlay's backdrop. Held so we can remove it on dismiss.
    dismiss_recognizer: Option<Retained<AnyObject>>,
}

thread_local! {
    static BINDINGS: RefCell<HashMap<i64, RichTooltipBinding>> = RefCell::new(HashMap::new());
}

/// Register a rich tooltip on `widget_handle`. `content_handle` is the
/// already-built content widget; `hover_delay_ms` becomes the long-press
/// minimum-press-duration. Defaults to 500ms when 0 is passed.
pub fn set_rich_tooltip(widget_handle: i64, content_handle: i64, hover_delay_ms: u32) {
    let min_press_secs = if hover_delay_ms == 0 {
        0.5
    } else {
        (hover_delay_ms as f64) / 1000.0
    };
    BINDINGS.with(|b| {
        b.borrow_mut().insert(
            widget_handle,
            RichTooltipBinding {
                content_handle,
                min_press_secs,
                active_overlay: None,
                dismiss_recognizer: None,
            },
        );
    });
    let Some(view) = get_widget(widget_handle) else {
        return;
    };
    unsafe {
        install_long_press(widget_handle, &view, min_press_secs);
    }
}

unsafe fn install_long_press(widget_handle: i64, view: &UIView, min_press_secs: f64) {
    let target = PerryRichTooltipTarget::new(widget_handle);
    let target_obj: &AnyObject = &*target;
    let gr_cls = AnyClass::get(c"UILongPressGestureRecognizer").unwrap();
    let alloc: *mut AnyObject = msg_send![gr_cls, alloc];
    let sel = Sel::register(c"handleLongPress:");
    let recognizer: *mut AnyObject = msg_send![alloc, initWithTarget: target_obj, action: sel];
    let _: () = msg_send![recognizer, setMinimumPressDuration: min_press_secs];
    let _: () = msg_send![view, addGestureRecognizer: recognizer];

    // Leak the target — long-press lives as long as the trigger widget,
    // which is the whole-session lifetime in practice. Matches the
    // macOS leak strategy for `PerryHoverTooltipTarget`.
    std::mem::forget(target);
}

fn present_overlay(widget_handle: i64) {
    let content_handle = BINDINGS.with(|b| {
        b.borrow()
            .get(&widget_handle)
            .map(|x| x.content_handle)
            .unwrap_or(0)
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
        // Find the key window — UIApplication.sharedApplication.keyWindow
        // is deprecated on iOS 13+ but works for the simple case; iterate
        // connectedScenes for the modern path.
        let key_window = find_key_window();
        if key_window.is_null() {
            return;
        }

        // Compute trigger's frame in window coordinates.
        let host_obj: &AnyObject = &*host;
        let host_bounds: CGRect = msg_send![host_obj, bounds];
        let host_in_window: CGRect =
            msg_send![host_obj, convertRect: host_bounds, toView: std::ptr::null::<AnyObject>()];

        // Sensible content size — UIView doesn't have AppKit's
        // `fittingSize`; use systemLayoutSizeFittingSize: with a
        // compressed-target size to honour intrinsicContentSize +
        // active constraints. Fall back to 240×80 if the result is 0.
        let target_size = CGSize {
            width: 1.0,
            height: 1.0,
        };
        let content_obj: &AnyObject = &*content;
        let mut content_size: CGSize =
            msg_send![content_obj, systemLayoutSizeFittingSize: target_size];
        if content_size.width <= 1.0 {
            content_size.width = 240.0;
        }
        if content_size.height <= 1.0 {
            content_size.height = 80.0;
        }
        let pad = 8.0_f64;
        let panel_w = content_size.width + 2.0 * pad;
        let panel_h = content_size.height + 2.0 * pad;

        // Anchor below the trigger; flip above if it overflows the
        // window. The full-screen overlay receives tap-outside dismiss.
        let window_bounds: CGRect = msg_send![key_window, bounds];

        let mut panel_x = host_in_window.origin.x;
        let mut panel_y = host_in_window.origin.y + host_in_window.size.height + 4.0;
        if panel_y + panel_h > window_bounds.origin.y + window_bounds.size.height {
            // Overflow below → place above the trigger instead.
            panel_y = host_in_window.origin.y - panel_h - 4.0;
        }
        if panel_x + panel_w > window_bounds.origin.x + window_bounds.size.width {
            panel_x = window_bounds.origin.x + window_bounds.size.width - panel_w - 4.0;
        }
        if panel_x < window_bounds.origin.x {
            panel_x = window_bounds.origin.x + 4.0;
        }

        // Backdrop fills the window — captures tap-outside dismiss.
        let view_cls = AnyClass::get(c"UIView").unwrap();
        let backdrop_alloc: *mut AnyObject = msg_send![view_cls, alloc];
        let backdrop_raw: *mut AnyObject = msg_send![backdrop_alloc, initWithFrame: window_bounds];
        let backdrop: Retained<UIView> =
            Retained::from_raw(backdrop_raw as *mut UIView).expect("backdrop init nil");
        // Transparent — only there to absorb the tap-outside.
        let clear: *mut AnyObject = msg_send![AnyClass::get(c"UIColor").unwrap(), clearColor];
        let _: () = msg_send![&*backdrop, setBackgroundColor: clear];

        // Tooltip card.
        let card_frame = CGRect {
            origin: CGPoint {
                x: panel_x,
                y: panel_y,
            },
            size: CGSize {
                width: panel_w,
                height: panel_h,
            },
        };
        let card_alloc: *mut AnyObject = msg_send![view_cls, alloc];
        let card_raw: *mut AnyObject = msg_send![card_alloc, initWithFrame: card_frame];
        let card: Retained<UIView> =
            Retained::from_raw(card_raw as *mut UIView).expect("card init nil");

        // Dark translucent background, 8pt corner radius, soft shadow.
        let bg: Retained<AnyObject> = msg_send![
            AnyClass::get(c"UIColor").unwrap(),
            colorWithRed: 0.10_f64,
            green: 0.10_f64,
            blue: 0.10_f64,
            alpha: 0.92_f64
        ];
        let _: () = msg_send![&*card, setBackgroundColor: &*bg];
        let card_layer: *mut AnyObject = msg_send![&*card, layer];
        if !card_layer.is_null() {
            let _: () = msg_send![card_layer, setCornerRadius: 8.0_f64];
            let _: () = msg_send![card_layer, setMasksToBounds: false];
            let shadow_color: *mut AnyObject =
                msg_send![AnyClass::get(c"UIColor").unwrap(), blackColor];
            let cg_shadow: *mut AnyObject = msg_send![shadow_color, CGColor];
            let _: () = msg_send![card_layer, setShadowColor: cg_shadow];
            let _: () = msg_send![card_layer, setShadowOpacity: 0.3_f64 as f32];
            let _: () = msg_send![card_layer, setShadowRadius: 6.0_f64];
            let shadow_off = CGSize {
                width: 0.0,
                height: 2.0,
            };
            let _: () = msg_send![card_layer, setShadowOffset: shadow_off];
        }

        // Place the user's content inside the card with `pad` inset on
        // every side. AppKit-style autoresize masks — we don't enforce
        // constraints here because the content view may already have
        // its own.
        let inner_frame = CGRect {
            origin: CGPoint { x: pad, y: pad },
            size: content_size,
        };
        let _: () = msg_send![content_obj, setFrame: inner_frame];
        let _: () = msg_send![content_obj, setTranslatesAutoresizingMaskIntoConstraints: true];
        let _: () = msg_send![&*card, addSubview: content_obj];

        let _: () = msg_send![&*backdrop, addSubview: &*card];
        let _: () = msg_send![key_window, addSubview: &*backdrop];

        // Tap-outside-to-dismiss: a UITapGestureRecognizer on the
        // backdrop that ignores taps falling on the card.
        let dismiss_target = PerryRichTooltipTarget::new(widget_handle);
        let gr_cls = AnyClass::get(c"UITapGestureRecognizer").unwrap();
        let alloc: *mut AnyObject = msg_send![gr_cls, alloc];
        let sel = Sel::register(c"handleDismissTap:");
        let dismiss_obj: &AnyObject = &*dismiss_target;
        let recognizer: *mut AnyObject = msg_send![alloc, initWithTarget: dismiss_obj, action: sel];
        let _: () = msg_send![&*backdrop, addGestureRecognizer: recognizer];
        let recognizer_retained: Retained<AnyObject> =
            Retained::retain(recognizer).expect("tap recognizer retain");
        std::mem::forget(dismiss_target);

        BINDINGS.with(|b| {
            if let Some(binding) = b.borrow_mut().get_mut(&widget_handle) {
                binding.active_overlay = Some(backdrop);
                binding.dismiss_recognizer = Some(recognizer_retained);
            }
        });
    }
}

fn dismiss_overlay(widget_handle: i64) {
    let (overlay, _gr) = BINDINGS.with(|b| {
        b.borrow_mut()
            .get_mut(&widget_handle)
            .map(|x| (x.active_overlay.take(), x.dismiss_recognizer.take()))
            .unwrap_or((None, None))
    });
    if let Some(overlay) = overlay {
        unsafe {
            let _: () = msg_send![&*overlay, removeFromSuperview];
        }
    }
}

/// Locate the key UIWindow. Tries the iOS 13+ scene-based path first
/// (UIApplication.sharedApplication.connectedScenes → keyWindow), then
/// falls back to the deprecated UIApplication.keyWindow.
unsafe fn find_key_window() -> *mut AnyObject {
    let app_cls = AnyClass::get(c"UIApplication").unwrap();
    let app: *mut AnyObject = msg_send![app_cls, sharedApplication];
    if app.is_null() {
        return std::ptr::null_mut();
    }

    // Scene-based lookup: pick the first UIWindowScene in foreground
    // active, then its keyWindow.
    let scenes: *mut AnyObject = msg_send![app, connectedScenes];
    if !scenes.is_null() {
        let enumerator: *mut AnyObject = msg_send![scenes, objectEnumerator];
        if !enumerator.is_null() {
            loop {
                let scene: *mut AnyObject = msg_send![enumerator, nextObject];
                if scene.is_null() {
                    break;
                }
                let activation_state: i64 = msg_send![scene, activationState];
                // UISceneActivationStateForegroundActive = 0
                if activation_state != 0 {
                    continue;
                }
                let scene_cls = AnyClass::get(c"UIWindowScene");
                if let Some(cls) = scene_cls {
                    let is_window_scene: bool = msg_send![scene, isKindOfClass: cls];
                    if !is_window_scene {
                        continue;
                    }
                }
                let windows: *mut AnyObject = msg_send![scene, windows];
                if windows.is_null() {
                    continue;
                }
                let count: i64 = msg_send![windows, count];
                for i in 0..count {
                    let window: *mut AnyObject = msg_send![windows, objectAtIndex: i as u64];
                    if window.is_null() {
                        continue;
                    }
                    let is_key: bool = msg_send![window, isKeyWindow];
                    if is_key {
                        return window;
                    }
                }
                // No keyWindow in this scene? Fall back to its first.
                if count > 0 {
                    return msg_send![windows, objectAtIndex: 0u64];
                }
            }
        }
    }

    // Deprecated fallback — works on pre-iOS-13 contexts.
    let windows: *mut AnyObject = msg_send![app, windows];
    if !windows.is_null() {
        let count: i64 = msg_send![windows, count];
        if count > 0 {
            return msg_send![windows, objectAtIndex: 0u64];
        }
    }
    std::ptr::null_mut()
}

// ===========================================================================
// NSObject target — owner of the long-press gesture recognizer and the
// tap-outside-to-dismiss recognizer.
// ===========================================================================

pub struct PerryRichTooltipIvars {
    widget_handle: Cell<i64>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryRichTooltipTargetIOS"]
    #[ivars = PerryRichTooltipIvars]
    pub struct PerryRichTooltipTarget;

    impl PerryRichTooltipTarget {
        #[unsafe(method(handleLongPress:))]
        fn handle_long_press(&self, gr: &AnyObject) {
            unsafe {
                // UIGestureRecognizerState.began = 1, .changed = 2,
                // .ended = 3, .cancelled = 4. Present on .began;
                // dismiss on .ended/.cancelled.
                let state: i64 = msg_send![gr, state];
                let h = self.ivars().widget_handle.get();
                match state {
                    1 => present_overlay(h),
                    3 | 4 => dismiss_overlay(h),
                    _ => {}
                }
            }
        }

        #[unsafe(method(handleDismissTap:))]
        fn handle_dismiss_tap(&self, _gr: &AnyObject) {
            dismiss_overlay(self.ivars().widget_handle.get());
        }
    }
);

impl PerryRichTooltipTarget {
    fn new(widget_handle: i64) -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryRichTooltipIvars {
            widget_handle: Cell::new(widget_handle),
        });
        unsafe { msg_send![super(this), init] }
    }
}
