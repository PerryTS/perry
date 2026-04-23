use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send};
use objc2_ui_kit::UIView;

// Raw ObjC runtime FFI for dynamic class registration
extern "C" {
    fn objc_allocateClassPair(superclass: *const std::ffi::c_void, name: *const i8, extra_bytes: usize) -> *mut std::ffi::c_void;
    fn objc_registerClassPair(cls: *mut std::ffi::c_void);
    fn class_addMethod(cls: *mut std::ffi::c_void, sel: *const std::ffi::c_void, imp: *const std::ffi::c_void, types: *const i8) -> bool;
    fn sel_registerName(name: *const i8) -> *const std::ffi::c_void;
    fn objc_getClass(name: *const i8) -> *const std::ffi::c_void;
}

/// intrinsicContentSize override — returns {-1, 10} so UIStackView (VStack) can stretch this view.
/// Without this, UIStackView gives the view zero height since UIView has no intrinsic size.
unsafe extern "C" fn frame_split_intrinsic_content_size(_this: *mut AnyObject, _sel: *const std::ffi::c_void) -> objc2_core_foundation::CGSize {
    // UIViewNoIntrinsicMetric = -1 for width (don't constrain), small height so VStack can stretch
    objc2_core_foundation::CGSize::new(-1.0, 10.0)
}

/// layoutSubviews callback for PerryFrameSplit — positions children using frames, not Auto Layout.
/// Each direct subview is a wrapper UIView. layoutSubviews sets each wrapper's frame,
/// then also sets the wrapper's first child's frame to fill the wrapper (bounds).
unsafe extern "C" fn frame_split_layout_subviews(this: *mut AnyObject, _sel: *const std::ffi::c_void) {
    let bounds: objc2_core_foundation::CGRect = objc2::msg_send![this, bounds];
    let tag: i64 = objc2::msg_send![this, tag];
    let left_width = tag as f64 / 100.0;

    let subviews: *mut AnyObject = objc2::msg_send![this, subviews];
    let count: usize = objc2::msg_send![subviews, count];

    if count >= 1 {
        let wrapper: *mut AnyObject = objc2::msg_send![subviews, objectAtIndex: 0usize];
        let wrapper_frame = objc2_core_foundation::CGRect::new(
            objc2_core_foundation::CGPoint::new(0.0, 0.0),
            objc2_core_foundation::CGSize::new(left_width, bounds.size.height),
        );
        let _: () = objc2::msg_send![wrapper, setFrame: wrapper_frame];
        // Set wrapper's child to fill wrapper bounds
        let wrapper_subs: *mut AnyObject = objc2::msg_send![wrapper, subviews];
        let wrapper_count: usize = objc2::msg_send![wrapper_subs, count];
        if wrapper_count >= 1 {
            let child: *mut AnyObject = objc2::msg_send![wrapper_subs, objectAtIndex: 0usize];
            let child_frame = objc2_core_foundation::CGRect::new(
                objc2_core_foundation::CGPoint::new(0.0, 0.0),
                objc2_core_foundation::CGSize::new(left_width, bounds.size.height),
            );
            let _: () = objc2::msg_send![child, setFrame: child_frame];
        }
    }
    if count >= 2 {
        let wrapper: *mut AnyObject = objc2::msg_send![subviews, objectAtIndex: 1usize];
        let rw = bounds.size.width - left_width;
        let wrapper_frame = objc2_core_foundation::CGRect::new(
            objc2_core_foundation::CGPoint::new(left_width, 0.0),
            objc2_core_foundation::CGSize::new(rw, bounds.size.height),
        );
        let _: () = objc2::msg_send![wrapper, setFrame: wrapper_frame];
        // Set wrapper's child to fill wrapper bounds
        let wrapper_subs: *mut AnyObject = objc2::msg_send![wrapper, subviews];
        let wrapper_count: usize = objc2::msg_send![wrapper_subs, count];
        if wrapper_count >= 1 {
            let child: *mut AnyObject = objc2::msg_send![wrapper_subs, objectAtIndex: 0usize];
            let child_frame = objc2_core_foundation::CGRect::new(
                objc2_core_foundation::CGPoint::new(0.0, 0.0),
                objc2_core_foundation::CGSize::new(rw, bounds.size.height),
            );
            let _: () = objc2::msg_send![child, setFrame: child_frame];
        }
    }
}

/// Register the PerryFrameSplit UIView subclass (once).
fn register_frame_split_class() {
    unsafe {
        let superclass = objc_getClass(c"UIView".as_ptr());
        let cls = objc_allocateClassPair(superclass, c"PerryFrameSplit".as_ptr(), 0);
        if cls.is_null() {
            return; // already registered
        }
        let sel = sel_registerName(c"layoutSubviews".as_ptr());
        class_addMethod(
            cls,
            sel,
            frame_split_layout_subviews as *const std::ffi::c_void,
            c"v@:".as_ptr(),
        );
        // Override intrinsicContentSize to return {-1, 10} so UIStackView can stretch this view
        let size_sel = sel_registerName(c"intrinsicContentSize".as_ptr());
        class_addMethod(
            cls,
            size_sel,
            frame_split_intrinsic_content_size as *const std::ffi::c_void,
            c"{CGSize=dd}@:".as_ptr(),
        );
        objc_registerClassPair(cls);
    }
}

/// Create a frame-based horizontal split container.
/// Children are positioned by `layoutSubviews` using frames — NO Auto Layout on children.
/// This avoids the constraint conflicts that cause black screen with embedded UIViews.
pub fn create_frame_split(left_width: f64) -> i64 {
    register_frame_split_class();
    unsafe {
        let cls = objc2::runtime::AnyClass::get(c"PerryFrameSplit").unwrap();
        let view: Retained<UIView> = objc2::msg_send![cls, new];
        // Container itself uses Auto Layout (so VStack can size it)
        let _: () = objc2::msg_send![&*view, setTranslatesAutoresizingMaskIntoConstraints: false];
        // Store left_width * 100 in tag
        let tag = (left_width * 100.0) as i64;
        let _: () = objc2::msg_send![&*view, setTag: tag];
        super::register_widget(view)
    }
}

/// Add a child to a frame-based split.
/// Wraps the child in a plain UIView. Both wrapper and child use frame-based layout.
/// The parent's layoutSubviews explicitly sets frames for wrappers AND their children.
pub fn frame_split_add_child(parent: &UIView, child: &UIView) {
    unsafe {
        // Create a wrapper UIView — uses frame-based layout (the default: translatesAutoresizing=true)
        let wrapper: Retained<UIView> = objc2::msg_send![
            objc2::runtime::AnyClass::get(c"UIView").unwrap(),
            new
        ];
        // Child must use frame-based layout too (layoutSubviews sets its frame explicitly)
        let _: () = objc2::msg_send![child, setTranslatesAutoresizingMaskIntoConstraints: true];
        // Clip to wrapper bounds so content doesn't overflow
        let _: () = objc2::msg_send![&*wrapper, setClipsToBounds: true];

        let _: () = objc2::msg_send![&*wrapper, addSubview: child];
        parent.addSubview(&wrapper);
    }
}

/// Create a plain UIView that lays out exactly two children side by side
/// using Auto Layout constraints (not UIStackView).
///
/// The first child added gets a fixed width (left_width) pinned to the left.
/// The second child fills the remaining space on the right.
/// This avoids UIStackView layout conflicts with embedded native views.
pub fn create(left_width: f64) -> i64 {
    unsafe {
        let view: Retained<UIView> = msg_send![
            objc2::runtime::AnyClass::get(c"UIView").unwrap(),
            new
        ];
        let _: () = msg_send![&*view, setTranslatesAutoresizingMaskIntoConstraints: false];

        // Store left_width in the view's tag (multiplied by 100 to preserve one decimal)
        let tag = (left_width * 100.0) as i64;
        let _: () = msg_send![&*view, setTag: tag];

        super::register_widget(view)
    }
}

/// Add a child to a split view container. The first child becomes the left panel
/// (fixed width from tag), the second becomes the right panel (fills remaining).
pub fn add_child(parent: &UIView, child: &UIView, child_index: usize) {
    unsafe {
        let _: () = msg_send![child, setTranslatesAutoresizingMaskIntoConstraints: false];
        parent.addSubview(child);

        // Get stored left_width from tag
        let tag: i64 = msg_send![parent, tag];
        let left_width = tag as f64 / 100.0;

        let child_top: Retained<AnyObject> = msg_send![child, topAnchor];
        let child_bottom: Retained<AnyObject> = msg_send![child, bottomAnchor];
        let parent_top: Retained<AnyObject> = msg_send![parent, topAnchor];
        let parent_bottom: Retained<AnyObject> = msg_send![parent, bottomAnchor];

        // Pin top and bottom to parent
        let tc: Retained<AnyObject> = msg_send![&*child_top, constraintEqualToAnchor: &*parent_top];
        let bc: Retained<AnyObject> = msg_send![&*child_bottom, constraintEqualToAnchor: &*parent_bottom];
        let _: () = msg_send![&*tc, setActive: true];
        let _: () = msg_send![&*bc, setActive: true];

        if child_index == 0 {
            // Left panel: pin leading to parent, fixed width
            let child_leading: Retained<AnyObject> = msg_send![child, leadingAnchor];
            let parent_leading: Retained<AnyObject> = msg_send![parent, leadingAnchor];
            let lc: Retained<AnyObject> = msg_send![&*child_leading, constraintEqualToAnchor: &*parent_leading];
            let _: () = msg_send![&*lc, setActive: true];

            let child_width: Retained<AnyObject> = msg_send![child, widthAnchor];
            let wc: Retained<AnyObject> = msg_send![&*child_width, constraintEqualToConstant: left_width];
            let _: () = msg_send![&*wc, setActive: true];
        } else {
            // Right panel: pin trailing to parent, leading to previous sibling's trailing
            let child_leading: Retained<AnyObject> = msg_send![child, leadingAnchor];
            let child_trailing: Retained<AnyObject> = msg_send![child, trailingAnchor];
            let parent_trailing: Retained<AnyObject> = msg_send![parent, trailingAnchor];

            // Find the first subview (left panel) to pin against
            let subviews: Retained<AnyObject> = msg_send![parent, subviews];
            let first_child: *const AnyObject = msg_send![&*subviews, firstObject];
            if !first_child.is_null() {
                let left_trailing: Retained<AnyObject> = msg_send![first_child, trailingAnchor];
                let lc: Retained<AnyObject> = msg_send![&*child_leading, constraintEqualToAnchor: &*left_trailing];
                let _: () = msg_send![&*lc, setActive: true];
            }

            let rc: Retained<AnyObject> = msg_send![&*child_trailing, constraintEqualToAnchor: &*parent_trailing];
            let _: () = msg_send![&*rc, setActive: true];
        }
    }
}

/// Create a vertical layout container (plain UIView, NOT UIStackView).
/// Lays out 3 children: top (intrinsic height), middle (fills), bottom (intrinsic height).
/// UIStackView distribution=Fill doesn't stretch nested UIStackViews on iOS,
/// so this manual approach is needed for reliable fill behavior.
pub fn create_vbox() -> i64 {
    unsafe {
        let view: Retained<UIView> = msg_send![
            objc2::runtime::AnyClass::get(c"UIView").unwrap(),
            new
        ];
        let _: () = msg_send![&*view, setTranslatesAutoresizingMaskIntoConstraints: false];
        super::register_widget(view)
    }
}

/// Add a child to a vbox at a specific slot: 0=top, 1=middle, 2=bottom.
/// - top: pinned to parent top, leading, trailing; intrinsic height
/// - middle: pinned between top.bottom and bottom.top, leading, trailing (fills)
/// - bottom: pinned to parent bottom, leading, trailing; intrinsic height
pub fn vbox_add_child(parent: &UIView, child: &UIView, slot: usize) {
    unsafe {
        let _: () = msg_send![child, setTranslatesAutoresizingMaskIntoConstraints: false];
        parent.addSubview(child);

        let child_leading: Retained<AnyObject> = msg_send![child, leadingAnchor];
        let child_trailing: Retained<AnyObject> = msg_send![child, trailingAnchor];
        let parent_leading: Retained<AnyObject> = msg_send![parent, leadingAnchor];
        let parent_trailing: Retained<AnyObject> = msg_send![parent, trailingAnchor];

        // Pin leading and trailing to parent (full width)
        let lc: Retained<AnyObject> = msg_send![&*child_leading, constraintEqualToAnchor: &*parent_leading];
        let rc: Retained<AnyObject> = msg_send![&*child_trailing, constraintEqualToAnchor: &*parent_trailing];
        let _: () = msg_send![&*lc, setActive: true];
        let _: () = msg_send![&*rc, setActive: true];

        if slot == 0 {
            // Top: pin top to parent top
            let child_top: Retained<AnyObject> = msg_send![child, topAnchor];
            let parent_top: Retained<AnyObject> = msg_send![parent, topAnchor];
            let c: Retained<AnyObject> = msg_send![&*child_top, constraintEqualToAnchor: &*parent_top];
            let _: () = msg_send![&*c, setActive: true];
        } else if slot == 2 {
            // Bottom: pin bottom to parent bottom
            let child_bottom: Retained<AnyObject> = msg_send![child, bottomAnchor];
            let parent_bottom: Retained<AnyObject> = msg_send![parent, bottomAnchor];
            let c: Retained<AnyObject> = msg_send![&*child_bottom, constraintEqualToAnchor: &*parent_bottom];
            let _: () = msg_send![&*c, setActive: true];
        } else {
            // Middle (slot 1): pin top to first subview's bottom, bottom to third subview's top
            // At this point, subviews should be [top, middle]. Bottom will be added next.
            // We'll pin the middle's top to the first subview's bottom here,
            // and bottom to the third subview's top when slot=2 is added.
            let subviews: Retained<AnyObject> = msg_send![parent, subviews];
            let count: usize = msg_send![&*subviews, count];
            if count >= 2 {
                // First subview is the "top" slot
                let top_view: *const AnyObject = msg_send![&*subviews, objectAtIndex: 0usize];
                if !top_view.is_null() {
                    let top_bottom: Retained<AnyObject> = msg_send![top_view, bottomAnchor];
                    let child_top: Retained<AnyObject> = msg_send![child, topAnchor];
                    let c: Retained<AnyObject> = msg_send![&*child_top, constraintEqualToAnchor: &*top_bottom];
                    let _: () = msg_send![&*c, setActive: true];
                }
            }
        }
    }
}

/// Finalize the vbox by pinning middle.bottom to bottom.top.
/// Must be called after all 3 children (top=0, middle=1, bottom=2) are added.
pub fn vbox_finalize(parent: &UIView) {
    unsafe {
        let subviews: Retained<AnyObject> = msg_send![parent, subviews];
        let count: usize = msg_send![&*subviews, count];
        if count >= 3 {
            let middle: *const AnyObject = msg_send![&*subviews, objectAtIndex: 1usize];
            let bottom: *const AnyObject = msg_send![&*subviews, objectAtIndex: 2usize];
            if !middle.is_null() && !bottom.is_null() {
                let mid_bottom: Retained<AnyObject> = msg_send![middle, bottomAnchor];
                let bot_top: Retained<AnyObject> = msg_send![bottom, topAnchor];
                let c: Retained<AnyObject> = msg_send![&*mid_bottom, constraintEqualToAnchor: &*bot_top];
                let _: () = msg_send![&*c, setActive: true];
            }
        }
    }
}
