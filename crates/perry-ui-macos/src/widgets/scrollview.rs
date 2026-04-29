use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_app_kit::{NSScrollView, NSView};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{MainThreadMarker, NSObjectProtocol};
use std::sync::Once;

// Raw ObjC runtime FFI for dynamic class registration
extern "C" {
    fn objc_allocateClassPair(
        superclass: *const std::ffi::c_void,
        name: *const i8,
        extra_bytes: usize,
    ) -> *mut std::ffi::c_void;
    fn objc_registerClassPair(cls: *mut std::ffi::c_void);
    fn class_addMethod(
        cls: *mut std::ffi::c_void,
        sel: *const std::ffi::c_void,
        imp: *const std::ffi::c_void,
        types: *const i8,
    ) -> bool;
    fn sel_registerName(name: *const i8) -> *const std::ffi::c_void;
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
