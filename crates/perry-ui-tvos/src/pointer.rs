//! Continuous pointer events for perry/ui on iOS (issue #1868).
//!
//! Implementation uses a custom `UIGestureRecognizer` subclass that
//! observes raw `touchesBegan:` / `touchesMoved:` / `touchesEnded:` /
//! `touchesCancelled:` for the primary touch and fires the registered
//! callbacks with a `PointerEvent { x, y, button, pointerType: "touch" }`.
//!
//! The recognizer never transitions to `.recognized`, so it doesn't
//! steal taps from the underlying control — the standard tap / scroll
//! handling continues normally.
//!
//! `button` is always `0` (Left) on touch — multi-button mice on iPad
//! aren't surfaced here in v1; a follow-up can route `UIEvent.buttonMask`
//! when present.

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, NSObject};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_core_foundation::CGPoint;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use crate::widgets::get_widget;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_pointer_event_new(x: f64, y: f64, button: u32, pointer_type: u32) -> f64;
}

const POINTER_TYPE_TOUCH: u32 = 1;

const PHASE_DOWN: u32 = 0;
const PHASE_MOVE: u32 = 1;
const PHASE_UP: u32 = 2;

thread_local! {
    static MOUSE_DOWN_CB: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
    static MOUSE_UP_CB: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
    static MOUSE_MOVE_CB: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
    /// Recognizer-instance address (usize) → widget handle. Used by the
    /// instance methods on `PerryPointerRecognizer` to find which handle
    /// they belong to.
    static RECOG_TO_HANDLE: RefCell<HashMap<usize, i64>> = RefCell::new(HashMap::new());
    /// Widgets that already have a recognizer attached — avoids
    /// double-installing when the user calls multiple `set_on_mouse_*`
    /// for the same widget.
    static INSTALLED: RefCell<std::collections::HashSet<i64>> =
        RefCell::new(std::collections::HashSet::new());
}

pub struct PerryPointerRecognizerIvars {
    key: Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryPointerRecognizer"]
    #[ivars = PerryPointerRecognizerIvars]
    pub struct PerryPointerRecognizer;

    impl PerryPointerRecognizer {
        #[unsafe(method(touchesBegan:withEvent:))]
        fn touches_began(&self, touches: &AnyObject, _event: &AnyObject) {
            dispatch_phase(self, touches, PHASE_DOWN);
        }

        #[unsafe(method(touchesMoved:withEvent:))]
        fn touches_moved(&self, touches: &AnyObject, _event: &AnyObject) {
            dispatch_phase(self, touches, PHASE_MOVE);
        }

        #[unsafe(method(touchesEnded:withEvent:))]
        fn touches_ended(&self, touches: &AnyObject, _event: &AnyObject) {
            dispatch_phase(self, touches, PHASE_UP);
            unsafe {
                // UIGestureRecognizerStateFailed = 5. Setting this lets
                // the system clean up our recognizer for this gesture
                // without it claiming the touch — the underlying
                // control still gets to handle the tap.
                let _: () = msg_send![self, setState: 5i64];
            }
        }

        #[unsafe(method(touchesCancelled:withEvent:))]
        fn touches_cancelled(&self, touches: &AnyObject, _event: &AnyObject) {
            // Treat cancellation as an "up" so apps can finish drags
            // cleanly when a system gesture (e.g. swipe-from-edge)
            // takes over.
            dispatch_phase(self, touches, PHASE_UP);
            unsafe {
                let _: () = msg_send![self, setState: 5i64];
            }
        }
    }
);

impl PerryPointerRecognizer {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryPointerRecognizerIvars { key: Cell::new(0) });
        unsafe { msg_send![super(this), init] }
    }
}

fn dispatch_phase(recog: &PerryPointerRecognizer, touches: &AnyObject, phase: u32) {
    let key = recog.ivars().key.get();
    let handle = RECOG_TO_HANDLE.with(|m| m.borrow().get(&key).copied().unwrap_or(0));
    if handle == 0 {
        return;
    }
    let Some(view) = get_widget(handle) else {
        return;
    };
    // Use anyObject from the touches NSSet → first touch (primary).
    let touch: *mut AnyObject = unsafe { msg_send![touches, anyObject] };
    if touch.is_null() {
        return;
    }
    let local: CGPoint = unsafe { msg_send![touch, locationInView: &*view] };
    let cb_f64 = match phase {
        PHASE_DOWN => MOUSE_DOWN_CB.with(|c| c.borrow().get(&handle).copied()),
        PHASE_MOVE => MOUSE_MOVE_CB.with(|c| c.borrow().get(&handle).copied()),
        PHASE_UP => MOUSE_UP_CB.with(|c| c.borrow().get(&handle).copied()),
        _ => None,
    };
    let Some(cb_f64) = cb_f64 else {
        return;
    };
    unsafe {
        let closure_ptr = js_nanbox_get_pointer(cb_f64);
        if closure_ptr == 0 {
            return;
        }
        let pe = js_pointer_event_new(local.x, local.y, 0, POINTER_TYPE_TOUCH);
        js_closure_call1(closure_ptr as *const u8, pe);
    }
}

fn ensure_recognizer_attached(handle: i64) {
    let already = INSTALLED.with(|s| s.borrow().contains(&handle));
    if already {
        return;
    }
    let Some(view) = get_widget(handle) else {
        return;
    };
    let recog = PerryPointerRecognizer::new();
    let key = Retained::as_ptr(&recog) as usize;
    recog.ivars().key.set(key);
    RECOG_TO_HANDLE.with(|m| {
        m.borrow_mut().insert(key, handle);
    });
    unsafe {
        // `cancelsTouchesInView = NO` is the critical flag — without it
        // our recognizer would swallow taps before the underlying
        // UIControl ever sees them.
        let _: () = msg_send![&*recog, setCancelsTouchesInView: false];
        let _: () = msg_send![&*recog, setDelaysTouchesBegan: false];
        let _: () = msg_send![&*recog, setDelaysTouchesEnded: false];
        let _: () = msg_send![&*view, addGestureRecognizer: &*recog];
    }
    // Leak the recognizer so it stays alive for the widget's lifetime.
    std::mem::forget(recog);
    INSTALLED.with(|s| {
        s.borrow_mut().insert(handle);
    });
}

pub fn set_on_mouse_down(handle: i64, callback: f64) {
    MOUSE_DOWN_CB.with(|c| {
        c.borrow_mut().insert(handle, callback);
    });
    ensure_recognizer_attached(handle);
}

pub fn set_on_mouse_up(handle: i64, callback: f64) {
    MOUSE_UP_CB.with(|c| {
        c.borrow_mut().insert(handle, callback);
    });
    ensure_recognizer_attached(handle);
}

pub fn set_on_mouse_move(handle: i64, callback: f64) {
    MOUSE_MOVE_CB.with(|c| {
        c.borrow_mut().insert(handle, callback);
    });
    ensure_recognizer_attached(handle);
}

/// Quiet the "unused class" lint on the recognizer until callers exist.
#[allow(dead_code)]
fn _ref_class() -> Option<&'static AnyClass> {
    AnyClass::get(c"PerryPointerRecognizer")
}
