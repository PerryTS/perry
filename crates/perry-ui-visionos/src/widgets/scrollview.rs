use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_ui_kit::{UIScrollView, UIView};
use objc2_foundation::NSObject;
use objc2_core_foundation::CGPoint;
use std::cell::RefCell;
use std::collections::HashMap;

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    static _dispatch_main_q: std::ffi::c_void;
    fn dispatch_async_f(
        queue: *const std::ffi::c_void,
        context: *mut std::ffi::c_void,
        work: unsafe extern "C" fn(*mut std::ffi::c_void),
    );
}

unsafe extern "C" fn refresh_callback_trampoline(context: *mut std::ffi::c_void) {
    let _ = std::panic::catch_unwind(|| {
        let closure_f64 = f64::from_bits(context as u64);
        let closure_ptr = js_nanbox_get_pointer(closure_f64);
        js_closure_call0(closure_ptr as *const u8);
    });
}

thread_local! {
    static REFRESH_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
}

pub struct PerryRefreshTargetIvars {
    callback_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryRefreshTarget"]
    #[ivars = PerryRefreshTargetIvars]
    pub struct PerryRefreshTarget;

    impl PerryRefreshTarget {
        #[unsafe(method(handleRefresh:))]
        fn handle_refresh(&self, _sender: &AnyObject) {
            let key = self.ivars().callback_key.get();
            REFRESH_CALLBACKS.with(|cbs| {
                if let Some(&closure_f64) = cbs.borrow().get(&key) {
                    unsafe {
                        dispatch_async_f(
                            &_dispatch_main_q as *const _ as *const std::ffi::c_void,
                            closure_f64.to_bits() as *mut std::ffi::c_void,
                            refresh_callback_trampoline,
                        );
                    }
                }
            });
        }
    }
);

impl PerryRefreshTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryRefreshTargetIvars {
            callback_key: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

/// Create a UIScrollView. Returns widget handle.
pub fn create() -> i64 {
    unsafe {
        let scroll: Retained<UIScrollView> = msg_send![
            objc2::runtime::AnyClass::get(c"UIScrollView").unwrap(),
            new
        ];
        let _: () = msg_send![&*scroll, setTranslatesAutoresizingMaskIntoConstraints: false];
        // Disable touch delay to avoid iOS 26 crash in
        // UIGestureRecognizer _delayTouchesForEvent:inPhase:
        let _: () = msg_send![&*scroll, setDelaysContentTouches: false];
        // Dismiss keyboard when user scrolls (UIScrollViewKeyboardDismissModeOnDrag = 1)
        let _: () = msg_send![&*scroll, setKeyboardDismissMode: 1i64];
        // Automatically adjust content inset for safe area (status bar, home indicator)
        // UIScrollViewContentInsetAdjustmentAlways = 2
        let _: () = msg_send![&*scroll, setContentInsetAdjustmentBehavior: 2i64];

        let view: Retained<UIView> = Retained::cast_unchecked(scroll);
        let handle = super::register_widget(view);
        #[cfg(feature = "geisterhand")]
        {
            extern "C" { fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8); }
            unsafe { perry_geisterhand_register(handle, 8, 0, 0.0, std::ptr::null()); }
        }
        handle
    }
}

/// Set both x and y scroll offsets. Used by geisterhand for programmatic scrolling.
#[no_mangle]
pub extern "C" fn perry_ui_scroll_set_offset(scroll_handle: i64, x: f64, y: f64) {
    if let Some(scroll_view) = super::get_widget(scroll_handle) {
        unsafe {
            use objc2_core_foundation::CGPoint;
            let point = CGPoint { x, y };
            let _: () = objc2::msg_send![&*scroll_view, setContentOffset: point, animated: false];
        }
    }
}

/// Set the content child of a UIScrollView.
pub fn set_child(scroll_handle: i64, child_handle: i64) {
    if let (Some(scroll_view), Some(child)) = (super::get_widget(scroll_handle), super::get_widget(child_handle)) {
        unsafe {
            // Add child as subview of the scroll view
            scroll_view.addSubview(&child);

            // Pin child to scroll view's content layout guide
            let content_guide: *const objc2::runtime::AnyObject = msg_send![&*scroll_view, contentLayoutGuide];

            let child_leading: *const objc2::runtime::AnyObject = msg_send![&*child, leadingAnchor];
            let child_trailing: *const objc2::runtime::AnyObject = msg_send![&*child, trailingAnchor];
            let child_top: *const objc2::runtime::AnyObject = msg_send![&*child, topAnchor];
            let child_bottom: *const objc2::runtime::AnyObject = msg_send![&*child, bottomAnchor];

            let guide_leading: *const objc2::runtime::AnyObject = msg_send![content_guide, leadingAnchor];
            let guide_trailing: *const objc2::runtime::AnyObject = msg_send![content_guide, trailingAnchor];
            let guide_top: *const objc2::runtime::AnyObject = msg_send![content_guide, topAnchor];
            let guide_bottom: *const objc2::runtime::AnyObject = msg_send![content_guide, bottomAnchor];

            let c1: Retained<objc2::runtime::AnyObject> = msg_send![child_leading, constraintEqualToAnchor: guide_leading];
            let c2: Retained<objc2::runtime::AnyObject> = msg_send![child_trailing, constraintEqualToAnchor: guide_trailing];
            let c3: Retained<objc2::runtime::AnyObject> = msg_send![child_top, constraintEqualToAnchor: guide_top];
            let c4: Retained<objc2::runtime::AnyObject> = msg_send![child_bottom, constraintEqualToAnchor: guide_bottom];

            let _: () = msg_send![&*c1, setActive: true];
            let _: () = msg_send![&*c2, setActive: true];
            let _: () = msg_send![&*c3, setActive: true];
            let _: () = msg_send![&*c4, setActive: true];

            // Match width to scroll view's frame layout guide
            let frame_guide: *const objc2::runtime::AnyObject = msg_send![&*scroll_view, frameLayoutGuide];
            let child_width: *const objc2::runtime::AnyObject = msg_send![&*child, widthAnchor];
            let frame_width: *const objc2::runtime::AnyObject = msg_send![frame_guide, widthAnchor];
            let cw: Retained<objc2::runtime::AnyObject> = msg_send![child_width, constraintEqualToAnchor: frame_width];
            let _: () = msg_send![&*cw, setActive: true];
        }
    }
}

/// Scroll so that the given child widget is visible.
pub fn scroll_to(_scroll_handle: i64, child_handle: i64) {
    if let Some(child) = super::get_widget(child_handle) {
        unsafe {
            let frame: objc2_core_foundation::CGRect = msg_send![&*child, frame];
            // Find the scroll view parent
            let superview: *const objc2::runtime::AnyObject = msg_send![&*child, superview];
            if !superview.is_null() {
                let _: () = msg_send![superview, scrollRectToVisible: frame, animated: true];
            }
        }
    }
}

/// Get the vertical scroll offset (contentOffset.y).
pub fn get_offset(scroll_handle: i64) -> f64 {
    if let Some(scroll_view) = super::get_widget(scroll_handle) {
        unsafe {
            let offset: CGPoint = msg_send![&*scroll_view, contentOffset];
            offset.y
        }
    } else {
        0.0
    }
}

/// Set the vertical scroll offset.
pub fn set_offset(scroll_handle: i64, offset: f64) {
    if let Some(scroll_view) = super::get_widget(scroll_handle) {
        unsafe {
            let point = CGPoint::new(0.0, offset);
            let _: () = msg_send![&*scroll_view, setContentOffset: point, animated: true];
        }
    }
}

/// Attach a UIRefreshControl to a UIScrollView. The callback is invoked when the user pulls to refresh.
pub fn set_refresh_control(scroll_handle: i64, callback: f64) {
    if let Some(scroll_view) = super::get_widget(scroll_handle) {
        unsafe {
            let target = PerryRefreshTarget::new();
            let target_addr = Retained::as_ptr(&target) as usize;
            target.ivars().callback_key.set(target_addr);
            REFRESH_CALLBACKS.with(|cbs| {
                cbs.borrow_mut().insert(target_addr, callback);
            });

            let sel = Sel::register(c"handleRefresh:");
            let rc_cls = objc2::runtime::AnyClass::get(c"UIRefreshControl").unwrap();
            let rc: *mut AnyObject = msg_send![rc_cls, new];
            let _: () = msg_send![rc, addTarget: &*target, action: sel, forControlEvents: 1u64 << 12]; // UIControlEventValueChanged
            let _: () = msg_send![&*scroll_view, setRefreshControl: rc];

            std::mem::forget(target); // keep alive
        }
    }
}

/// End the refresh animation on a UIScrollView's UIRefreshControl.
pub fn end_refreshing(scroll_handle: i64) {
    if let Some(scroll_view) = super::get_widget(scroll_handle) {
        unsafe {
            let rc: *const AnyObject = msg_send![&*scroll_view, refreshControl];
            if !rc.is_null() {
                let _: () = msg_send![rc, endRefreshing];
            }
        }
    }
}
