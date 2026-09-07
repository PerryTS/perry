//! Text-inset field for UITextField (iOS).
//!
//! UITextField has no property for the inset between its border and its text.
//! A subclass that shrinks `textRect`/`editingRect`/`placeholderRect` is the
//! way to reach it, so a Perry TextField / SecureField is one of these and
//! `set_edge_insets` writes the inset onto it. UITextView (TextArea) needs no
//! subclass — it has a first-class `textContainerInset`.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, ClassType, DefinedClass, MainThreadOnly};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::MainThreadMarker;
use objc2_ui_kit::UITextField;
use std::cell::Cell;

/// (top, left, bottom, right), in points.
pub struct InsetFieldIvars {
    insets: Cell<(f64, f64, f64, f64)>,
}

/// Shrink a rect by the insets, clamped so it never inverts. UIKit is
/// origin-top-left, so the top inset moves the origin down.
fn shrink(insets: (f64, f64, f64, f64), r: CGRect) -> CGRect {
    let (top, left, bottom, right) = insets;
    let w = (r.size.width - left - right).max(0.0);
    let h = (r.size.height - top - bottom).max(0.0);
    CGRect::new(
        CGPoint::new(r.origin.x + left, r.origin.y + top),
        CGSize::new(w, h),
    )
}

define_class!(
    #[unsafe(super(UITextField))]
    #[name = "PerryInsetTextField"]
    #[ivars = InsetFieldIvars]
    pub struct PerryInsetTextField;

    impl PerryInsetTextField {
        #[unsafe(method(textRectForBounds:))]
        fn text_rect_for_bounds(&self, bounds: CGRect) -> CGRect {
            let sup: CGRect = unsafe { msg_send![super(self), textRectForBounds: bounds] };
            shrink(self.ivars().insets.get(), sup)
        }

        #[unsafe(method(editingRectForBounds:))]
        fn editing_rect_for_bounds(&self, bounds: CGRect) -> CGRect {
            let sup: CGRect = unsafe { msg_send![super(self), editingRectForBounds: bounds] };
            shrink(self.ivars().insets.get(), sup)
        }

        #[unsafe(method(placeholderRectForBounds:))]
        fn placeholder_rect_for_bounds(&self, bounds: CGRect) -> CGRect {
            let sup: CGRect = unsafe { msg_send![super(self), placeholderRectForBounds: bounds] };
            shrink(self.ivars().insets.get(), sup)
        }
    }
);

impl PerryInsetTextField {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(InsetFieldIvars {
            insets: Cell::new((0.0, 0.0, 0.0, 0.0)),
        });
        unsafe { msg_send![super(this), init] }
    }
}

/// Write the inset onto `view` if it is a Perry inset field, and relayout.
/// Returns whether it matched.
pub fn try_set_field_insets(
    view: *mut AnyObject,
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
) -> bool {
    if view.is_null() {
        return false;
    }
    unsafe {
        let is_ours: bool = msg_send![view, isKindOfClass: PerryInsetTextField::class()];
        if is_ours {
            (*(view as *const PerryInsetTextField))
                .ivars()
                .insets
                .set((top, left, bottom, right));
            let _: () = msg_send![view, setNeedsLayout];
            return true;
        }
    }
    false
}
