//! BloomView — a native render-surface host widget (issue #2395 / #5519).
//!
//! Reserves an `NSView` in the Perry UI view tree for an external GPU renderer
//! (e.g. the Bloom engine) to draw into. Perry UI only owns the view and exposes
//! its pointer via `bloomViewGetNativeHandle`; user TypeScript hands that pointer
//! to the renderer, which builds its (Metal) surface on it. Mirrors the Windows
//! implementation (`perry-ui-windows`), with the HWND replaced by the raw
//! `NSView*`.
//!
//! The view is a `PerryBloomView` subclass that accepts first-responder status
//! so the window routes keyboard/mouse here once focused — the attached engine
//! overrides the event selectors to consume them (mirrors Bloom subclassing the
//! Windows host wndproc).

use objc2::rc::Retained;
use objc2::{define_class, msg_send, MainThreadOnly};
use objc2_app_kit::NSView;
use objc2_foundation::MainThreadMarker;

pub struct PerryBloomViewIvars;

define_class!(
    #[unsafe(super(NSView))]
    #[name = "PerryBloomView"]
    #[ivars = PerryBloomViewIvars]
    pub struct PerryBloomView;

    impl PerryBloomView {
        // Accept first-responder status so a focused BloomView receives
        // keyDown:/mouse events (a stock NSView returns NO and never would).
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }
    }
);

/// Create a BloomView host sized `width` × `height` points. Returns the widget
/// handle. The size is pinned via Auto Layout so the renderer's surface comes
/// up non-zero even before the layout engine positions the view (#5519 — a
/// zero-frame view yields a 0×0 surface and nothing renders).
pub fn create(width: f64, height: f64) -> i64 {
    // Public C ABI entry — don't panic across the FFI boundary if called off
    // the main thread; return an invalid (0) handle instead.
    let Some(mtm) = MainThreadMarker::new() else {
        return 0;
    };
    let view: Retained<PerryBloomView> = {
        let this = PerryBloomView::alloc(mtm).set_ivars(PerryBloomViewIvars);
        unsafe { msg_send![super(this), init] }
    };
    // Auto Layout drives the size (set_width/set_height below); opt out of the
    // autoresizing-mask→constraints translation so those constraints take hold.
    unsafe {
        let _: () = msg_send![&view, setTranslatesAutoresizingMaskIntoConstraints: false];
    }
    let ns: Retained<NSView> = unsafe { Retained::cast_unchecked(view) };
    let handle = super::register_widget(ns);
    if width.is_finite() && width >= 1.0 {
        super::set_width(handle, width);
    }
    if height.is_finite() && height >= 1.0 {
        super::set_height(handle, height);
    }
    handle
}

/// Return the raw `NSView*` for a BloomView handle as an integer, for handing
/// to an external GPU renderer. Returns 0 if the handle is unknown. The
/// registry retains the view; the returned pointer is non-owning.
pub fn get_native_handle(handle: i64) -> i64 {
    match super::get_widget(handle) {
        Some(view) => Retained::as_ptr(&view) as i64,
        None => 0,
    }
}
