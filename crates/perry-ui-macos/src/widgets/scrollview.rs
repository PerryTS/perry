use crate::ffi::class_addMethod;
use crate::ffi::sel_registerName;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, AnyThread, DefinedClass};
use objc2_app_kit::{NSScrollView, NSView};
use objc2_core_foundation::{CGPoint, CGRect};
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSString};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Once;

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

// Raw ObjC runtime FFI for dynamic class registration
extern "C" {
    fn objc_allocateClassPair(
        superclass: *const std::ffi::c_void,
        name: *const i8,
        extra_bytes: usize,
    ) -> *mut std::ffi::c_void;
    fn objc_registerClassPair(cls: *mut std::ffi::c_void);
    fn objc_getClass(name: *const i8) -> *const std::ffi::c_void;
}

extern "C" fn flipped_is_flipped(
    _this: *const std::ffi::c_void,
    _sel: *const std::ffi::c_void,
) -> i8 {
    1 // YES
}

/// Register a flipped NSView subclass so document views scroll from the top.
fn ensure_flipped_class() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let superclass = objc_getClass(c"NSView".as_ptr());
        let cls = objc_allocateClassPair(superclass, c"PerryFlippedView".as_ptr(), 0);
        if cls.is_null() {
            return;
        }
        let sel = sel_registerName(c"isFlipped".as_ptr());
        class_addMethod(
            cls,
            sel,
            flipped_is_flipped as *const std::ffi::c_void,
            c"B@:".as_ptr(),
        );
        objc_registerClassPair(cls);
    });
}

/// Create an NSScrollView with vertical scrollbar. Returns widget handle.
pub fn create() -> i64 {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let scroll = NSScrollView::new(mtm);
    scroll.setHasVerticalScroller(true);
    scroll.setAutohidesScrollers(true);
    scroll.setDrawsBackground(false);
    // Disable autoresizing mask so Auto Layout constraints work when inside ZStack or other
    // constraint-based containers. Without this, the autoresizing mask conflicts with explicit
    // constraints, causing the scroll view to have zero size.
    unsafe {
        let _: () = msg_send![&*scroll, setTranslatesAutoresizingMaskIntoConstraints: false];
    }
    let view: Retained<NSView> = unsafe { Retained::cast_unchecked(scroll) };
    super::register_widget(view)
}

/// Set the document (content) view of a scroll view.
/// Uses a flipped wrapper + Auto Layout for top-origin scrolling.
/// Changes distribution to GravityAreas and sets minimum row heights on children.
pub fn set_child(scroll_handle: i64, child_handle: i64) {
    if let (Some(scroll_view), Some(child)) = (
        super::get_widget(scroll_handle),
        super::get_widget(child_handle),
    ) {
        unsafe {
            let sv: &NSScrollView = &*(Retained::as_ptr(&scroll_view) as *const NSScrollView);

            // Create a flipped wrapper so content starts from top
            ensure_flipped_class();
            let flipped_cls = AnyClass::get(c"PerryFlippedView").unwrap();
            let wrapper: Retained<AnyObject> = msg_send![flipped_cls, new];

            // Auto Layout on both
            let _: () = msg_send![&*child, setTranslatesAutoresizingMaskIntoConstraints: false];
            let _: () = msg_send![&*wrapper, setTranslatesAutoresizingMaskIntoConstraints: false];

            // Add child to wrapper
            let _: () = msg_send![&*wrapper, addSubview: &*child];

            // Pin child edges to wrapper
            let child_top: Retained<AnyObject> = msg_send![&*child, topAnchor];
            let child_lead: Retained<AnyObject> = msg_send![&*child, leadingAnchor];
            let child_trail: Retained<AnyObject> = msg_send![&*child, trailingAnchor];
            let child_bot: Retained<AnyObject> = msg_send![&*child, bottomAnchor];
            let wrap_top: Retained<AnyObject> = msg_send![&*wrapper, topAnchor];
            let wrap_lead: Retained<AnyObject> = msg_send![&*wrapper, leadingAnchor];
            let wrap_trail: Retained<AnyObject> = msg_send![&*wrapper, trailingAnchor];
            let wrap_bot: Retained<AnyObject> = msg_send![&*wrapper, bottomAnchor];
            let c1: Retained<AnyObject> =
                msg_send![&*child_top, constraintEqualToAnchor: &*wrap_top];
            let c2: Retained<AnyObject> =
                msg_send![&*child_lead, constraintEqualToAnchor: &*wrap_lead];
            let c3: Retained<AnyObject> =
                msg_send![&*child_trail, constraintEqualToAnchor: &*wrap_trail];
            let c4: Retained<AnyObject> =
                msg_send![&*child_bot, constraintEqualToAnchor: &*wrap_bot];
            let _: () = msg_send![&*c1, setActive: true];
            let _: () = msg_send![&*c2, setActive: true];
            let _: () = msg_send![&*c3, setActive: true];
            let _: () = msg_send![&*c4, setActive: true];

            // Set wrapper as document view
            let wrapper_view: &NSView = &*(Retained::as_ptr(&wrapper) as *const NSView);
            sv.setDocumentView(Some(wrapper_view));

            // Pin wrapper width to clip view
            let clip_view = sv.contentView();
            let wrap_w: Retained<AnyObject> = msg_send![&*wrapper, widthAnchor];
            let clip_w: Retained<AnyObject> = msg_send![&*clip_view, widthAnchor];
            let c5: Retained<AnyObject> = msg_send![&*wrap_w, constraintEqualToAnchor: &*clip_w];
            let _: () = msg_send![&*c5, setActive: true];

            // If NSStackView, switch to GravityAreas, stretch children to fill width
            let stack_cls = AnyClass::get(c"NSStackView");
            if let Some(cls) = stack_cls {
                if (*child).isKindOfClass(cls) {
                    // GravityAreas: children use intrinsic height
                    let _: () = msg_send![&*child, setDistribution: -1_isize];
                    // Change alignment from Leading to Width so children fill cross-axis
                    // NSLayoutAttribute: Leading=5, Width=7 (fills cross-axis)
                    let _: () = msg_send![&*child, setAlignment: 7_isize];

                    let arranged: Retained<AnyObject> = msg_send![&*child, arrangedSubviews];
                    let n: usize = msg_send![&*arranged, count];
                    for i in 0..n {
                        let subview: *mut AnyObject = msg_send![&*arranged, objectAtIndex: i];
                        if subview.is_null() {
                            continue;
                        }
                        // Set minimum height 24px on each arranged subview
                        let sub_h: Retained<AnyObject> = msg_send![subview, heightAnchor];
                        let hc: Retained<AnyObject> =
                            msg_send![&*sub_h, constraintGreaterThanOrEqualToConstant: 24.0_f64];
                        let _: () = msg_send![&*hc, setActive: true];
                    }
                }
            }
        }
    }
}

/// Scroll so that the given child widget is visible.
pub fn scroll_to(scroll_handle: i64, child_handle: i64) {
    if let (Some(scroll_view), Some(child)) = (
        super::get_widget(scroll_handle),
        super::get_widget(child_handle),
    ) {
        unsafe {
            let _sv: &NSScrollView = &*(Retained::as_ptr(&scroll_view) as *const NSScrollView);
            let child_frame: CGRect = msg_send![&*child, frame];
            let _: () = msg_send![&*child, scrollRectToVisible: child_frame];
        }
    }
}

/// Get the vertical scroll offset (contentView.bounds.origin.y).
pub fn get_offset(scroll_handle: i64) -> f64 {
    if let Some(scroll_view) = super::get_widget(scroll_handle) {
        unsafe {
            let sv: &NSScrollView = &*(Retained::as_ptr(&scroll_view) as *const NSScrollView);
            let content_view = sv.contentView();
            let bounds: CGRect = msg_send![&*content_view, bounds];
            bounds.origin.y
        }
    } else {
        0.0
    }
}

/// Set the vertical scroll offset.
pub fn set_offset(scroll_handle: i64, offset: f64) {
    if let Some(scroll_view) = super::get_widget(scroll_handle) {
        unsafe {
            let sv: &NSScrollView = &*(Retained::as_ptr(&scroll_view) as *const NSScrollView);
            let content_view = sv.contentView();
            let point = CGPoint::new(0.0, offset);
            let _: () = msg_send![&*content_view, setBoundsOrigin: point];
        }
    }
}

// =============================================================================
// Issue #553 — onScrollEnd hook (infinite-scroll callback)
// =============================================================================

struct ScrollEndState {
    closure: f64,
    threshold_px: f64,
    armed: bool,
}

thread_local! {
    static SCROLL_END_STATES: RefCell<HashMap<i64, ScrollEndState>> = RefCell::new(HashMap::new());
    static SCROLL_END_OBSERVER_TO_HANDLE: RefCell<HashMap<usize, i64>> = RefCell::new(HashMap::new());
}

pub struct PerryScrollEndObserverIvars {
    key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryScrollEndObserver"]
    #[ivars = PerryScrollEndObserverIvars]
    pub struct PerryScrollEndObserver;

    impl PerryScrollEndObserver {
        #[unsafe(method(boundsDidChange:))]
        fn bounds_did_change(&self, _notification: &AnyObject) {
            let key = self.ivars().key.get();
            let handle = SCROLL_END_OBSERVER_TO_HANDLE.with(|m| {
                m.borrow().get(&key).copied().unwrap_or(0)
            });
            if handle == 0 {
                return;
            }
            check_scroll_end(handle);
        }
    }
);

impl PerryScrollEndObserver {
    fn new(key: usize) -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryScrollEndObserverIvars {
            key: std::cell::Cell::new(key),
        });
        unsafe { msg_send![super(this), init] }
    }
}

fn check_scroll_end(handle: i64) {
    let Some(scroll_view) = super::get_widget(handle) else {
        return;
    };
    let (closure, in_zone) = unsafe {
        let sv: &NSScrollView = &*(Retained::as_ptr(&scroll_view) as *const NSScrollView);
        let content_view = sv.contentView();
        let bounds: CGRect = msg_send![&*content_view, bounds];
        let doc: Retained<AnyObject> = msg_send![sv, documentView];
        if Retained::as_ptr(&doc).is_null() {
            return;
        }
        let doc_frame: CGRect = msg_send![&*doc, frame];
        let visible_bottom = bounds.origin.y + bounds.size.height;
        let (closure, threshold_px) = SCROLL_END_STATES.with(|s| {
            s.borrow()
                .get(&handle)
                .map(|st| (st.closure, st.threshold_px))
                .unwrap_or((0.0, 0.0))
        });
        let in_zone = visible_bottom >= doc_frame.size.height - threshold_px;
        (closure, in_zone)
    };
    if closure == 0.0 {
        return;
    }
    let should_fire = SCROLL_END_STATES.with(|s| {
        let mut states = s.borrow_mut();
        let Some(state) = states.get_mut(&handle) else {
            return false;
        };
        if in_zone && state.armed {
            state.armed = false;
            true
        } else if !in_zone && !state.armed {
            state.armed = true;
            false
        } else {
            false
        }
    });
    if should_fire {
        unsafe {
            let ptr = js_nanbox_get_pointer(closure) as *const u8;
            js_closure_call0(ptr);
        }
    }
}

/// Fire `callback` once when the user scrolls within `threshold_px` of the
/// content's bottom edge. Re-arms when the user scrolls back up past the
/// threshold so the callback can fire repeatedly across pagination loads.
pub fn set_scroll_end_callback(scroll_handle: i64, callback: f64, threshold_px: f64) {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let Some(scroll_view) = super::get_widget(scroll_handle) else {
        return;
    };
    SCROLL_END_STATES.with(|s| {
        s.borrow_mut().insert(
            scroll_handle,
            ScrollEndState {
                closure: callback,
                threshold_px: if threshold_px > 0.0 {
                    threshold_px
                } else {
                    200.0
                },
                armed: true,
            },
        );
    });
    unsafe {
        let sv: &NSScrollView = &*(Retained::as_ptr(&scroll_view) as *const NSScrollView);
        let clip: Retained<AnyObject> = Retained::cast_unchecked(sv.contentView());
        let _: () = msg_send![&*clip, setPostsBoundsChangedNotifications: true];

        let observer = PerryScrollEndObserver::new(0);
        let observer_addr = Retained::as_ptr(&observer) as usize;
        observer.ivars().key.set(observer_addr);
        SCROLL_END_OBSERVER_TO_HANDLE.with(|m| {
            m.borrow_mut().insert(observer_addr, scroll_handle);
        });

        let nc_cls = AnyClass::get(c"NSNotificationCenter").unwrap();
        let nc: Retained<AnyObject> = msg_send![nc_cls, defaultCenter];
        let name = NSString::from_str("NSViewBoundsDidChangeNotification");
        let sel = Sel::register(c"boundsDidChange:");
        let _: () = msg_send![
            &*nc,
            addObserver: &*observer,
            selector: sel,
            name: &*name,
            object: &*clip,
        ];
        std::mem::forget(observer);
    }
}

/// Set both x and y scroll offsets. Used by geisterhand for programmatic scrolling.
#[no_mangle]
pub extern "C" fn perry_ui_scroll_set_offset(scroll_handle: i64, x: f64, y: f64) {
    if let Some(scroll_view) = super::get_widget(scroll_handle) {
        unsafe {
            let sv: &NSScrollView = &*(Retained::as_ptr(&scroll_view) as *const NSScrollView);
            let content_view = sv.contentView();
            let point = CGPoint::new(x, y);
            let _: () = msg_send![&*content_view, setBoundsOrigin: point];
        }
    }
}
