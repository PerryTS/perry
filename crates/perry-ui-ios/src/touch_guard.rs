//! Keep views in the window while a touch is in progress.
//!
//! UIKit removes some of its own gesture recognizers when certain views
//! (text fields and text views, edit menus, selection interactions) leave the
//! window. If one of those recognizers is tracking the current touch, its
//! delayed-event dispatcher is gone, and on the next touch phase
//! `-[UIGestureRecognizer _delayTouchesForEvent:inPhase:]` inserts nil into an
//! NSMutableArray and the app aborts ("object cannot be nil").
//!
//! perry/ui apps rebuild screens with `widgetClearChildren`, often from async
//! callbacks (a fetch finishing) that land while a finger is down. So while
//! any touch is active, removed views are hidden and left in their superview
//! (no longer arranged, so the layout is the same as if they were gone), and
//! are detached once the last touch ends.

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
use objc2::{msg_send, sel};
use objc2_ui_kit::UIView;
use std::cell::{Cell, RefCell};

thread_local! {
    static TOUCH_ACTIVE: Cell<bool> = const { Cell::new(false) };
    /// Views waiting to be detached, with the hidden state to restore.
    static PENDING: RefCell<Vec<(Retained<UIView>, bool)>> = const { RefCell::new(Vec::new()) };
    static FLUSH_QUEUED: Cell<bool> = const { Cell::new(false) };
}

extern "C" {
    static _dispatch_main_q: std::ffi::c_void;
    fn dispatch_async_f(
        q: *const std::ffi::c_void,
        ctx: *mut std::ffi::c_void,
        f: unsafe extern "C" fn(*mut std::ffi::c_void),
    );
}

pub fn touch_active() -> bool {
    TOUCH_ACTIVE.with(|t| t.get())
}

/// Take `view` out of its superview now, or — during a touch — hide it and
/// detach it after the touch ends. The caller has already removed it from a
/// stack's arranged subviews.
pub fn detach(view: &Retained<UIView>) {
    if !touch_active() {
        unsafe { view.removeFromSuperview() };
        return;
    }
    let ptr = Retained::as_ptr(view);
    let already = PENDING.with(|p| p.borrow().iter().any(|(v, _)| Retained::as_ptr(v) == ptr));
    if already {
        return;
    }
    let hidden: bool = unsafe {
        let h: Bool = msg_send![&**view, isHidden];
        h.as_bool()
    };
    unsafe {
        let _: () = msg_send![&**view, setHidden: Bool::YES];
    }
    PENDING.with(|p| p.borrow_mut().push((view.clone(), hidden)));
}

/// `view` is being added somewhere: cancel a pending detach and restore its
/// hidden state.
pub fn reattach(view: &Retained<UIView>) {
    let ptr = Retained::as_ptr(view);
    let restored = PENDING.with(|p| {
        let mut p = p.borrow_mut();
        let i = p.iter().position(|(v, _)| Retained::as_ptr(v) == ptr)?;
        Some(p.remove(i).1)
    });
    if let Some(hidden) = restored {
        unsafe {
            let _: () = msg_send![&**view, setHidden: Bool::new(hidden)];
        }
    }
}

/// `widgetSetHidden` on a view that is waiting to be detached: remember the
/// new state instead of showing it now. Returns true when handled.
pub fn set_hidden_pending(view: &Retained<UIView>, hidden: bool) -> bool {
    let ptr = Retained::as_ptr(view);
    PENDING.with(|p| {
        let mut p = p.borrow_mut();
        match p.iter_mut().find(|(v, _)| Retained::as_ptr(v) == ptr) {
            Some(entry) => {
                entry.1 = hidden;
                true
            }
            None => false,
        }
    })
}

unsafe extern "C" fn flush(_ctx: *mut std::ffi::c_void) {
    FLUSH_QUEUED.with(|f| f.set(false));
    if touch_active() {
        return;
    }
    let views = PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()));
    for (v, hidden) in views {
        v.removeFromSuperview();
        let _: () = msg_send![&*v, setHidden: Bool::new(hidden)];
    }
}

fn queue_flush() {
    let pending = PENDING.with(|p| !p.borrow().is_empty());
    if !pending || FLUSH_QUEUED.with(|f| f.replace(true)) {
        return;
    }
    unsafe { dispatch_async_f(&_dispatch_main_q as *const _, std::ptr::null_mut(), flush) };
}

/// Any touch in `event` still down (phase not ended / cancelled)?
unsafe fn touches_down(event: *mut AnyObject) -> bool {
    let kind: isize = msg_send![event, type];
    if kind != 0 {
        // Not a touches event: leave the state as it is.
        return touch_active();
    }
    let set: *mut AnyObject = msg_send![event, allTouches];
    if set.is_null() {
        return false;
    }
    let all: *mut AnyObject = msg_send![set, allObjects];
    let n: usize = msg_send![all, count];
    for i in 0..n {
        let t: *mut AnyObject = msg_send![all, objectAtIndex: i];
        let phase: isize = msg_send![t, phase];
        // UITouchPhaseEnded = 3, UITouchPhaseCancelled = 4
        if phase != 3 && phase != 4 {
            return true;
        }
    }
    false
}

extern "C" fn window_send_event(this: &AnyObject, _cmd: Sel, event: *mut AnyObject) {
    unsafe {
        let kind: isize = msg_send![event, type];
        if kind == 0 {
            TOUCH_ACTIVE.with(|t| t.set(true));
        }
        let _: () = msg_send![super(this, AnyClass::get(c"UIWindow").unwrap()), sendEvent: event];
        if kind == 0 {
            let down = touches_down(event);
            TOUCH_ACTIVE.with(|t| t.set(down));
            if !down {
                queue_flush();
            }
        }
    }
}

/// `PerryWindow`: UIWindow that tracks whether a touch is in progress.
pub fn window_class() -> &'static AnyClass {
    if let Some(c) = AnyClass::get(c"PerryWindow") {
        return c;
    }
    let sup = AnyClass::get(c"UIWindow").unwrap();
    let mut b = ClassBuilder::new(c"PerryWindow", sup).unwrap();
    unsafe {
        b.add_method(sel!(sendEvent:), window_send_event as extern "C" fn(_, _, _));
    }
    b.register()
}
