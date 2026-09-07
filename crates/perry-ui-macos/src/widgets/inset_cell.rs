//! Text-inset cell for NSTextField.
//!
//! NSTextField has no property for the inset between its bezel and its text.
//! An NSTextFieldCell subclass that shrinks the drawing, title, editing and
//! selection rects is the only way to reach it, so a Perry TextField carries
//! one of these cells and `set_edge_insets` writes the inset onto it.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSEvent, NSText, NSTextFieldCell, NSView};
use objc2_core_foundation::CGRect;
use objc2_foundation::{MainThreadMarker, NSString};
use std::cell::Cell;

/// (top, left, bottom, right), in points.
pub struct InsetCellIvars {
    insets: Cell<(f64, f64, f64, f64)>,
}

define_class!(
    #[unsafe(super(NSTextFieldCell))]
    #[name = "PerryInsetTextFieldCell"]
    #[ivars = InsetCellIvars]
    pub struct PerryInsetTextFieldCell;

    impl PerryInsetTextFieldCell {
        #[unsafe(method(drawingRectForBounds:))]
        fn drawing_rect_for_bounds(&self, bounds: CGRect) -> CGRect {
            let sup: CGRect = unsafe { msg_send![super(self), drawingRectForBounds: bounds] };
            self.inset_rect(sup)
        }

        #[unsafe(method(titleRectForBounds:))]
        fn title_rect_for_bounds(&self, bounds: CGRect) -> CGRect {
            let sup: CGRect = unsafe { msg_send![super(self), titleRectForBounds: bounds] };
            self.inset_rect(sup)
        }

        #[unsafe(method(editWithFrame:inView:editor:delegate:event:))]
        fn edit_with_frame(
            &self,
            rect: CGRect,
            view: &NSView,
            editor: &NSText,
            delegate: *mut AnyObject,
            event: *mut NSEvent,
        ) {
            let r = self.inset_rect(rect);
            unsafe {
                let _: () = msg_send![
                    super(self),
                    editWithFrame: r, inView: view, editor: editor, delegate: delegate, event: event
                ];
            }
        }

        #[unsafe(method(selectWithFrame:inView:editor:delegate:start:length:))]
        fn select_with_frame(
            &self,
            rect: CGRect,
            view: &NSView,
            editor: &NSText,
            delegate: *mut AnyObject,
            start: isize,
            length: isize,
        ) {
            let r = self.inset_rect(rect);
            unsafe {
                let _: () = msg_send![
                    super(self),
                    selectWithFrame: r, inView: view, editor: editor, delegate: delegate, start: start, length: length
                ];
            }
        }
    }
);

impl PerryInsetTextFieldCell {
    /// A cell seeded with the field's placeholder/string, ready to replace the
    /// default cell before the field is configured.
    pub fn new(text: &str, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(InsetCellIvars {
            insets: Cell::new((0.0, 0.0, 0.0, 0.0)),
        });
        let ns = NSString::from_str(text);
        unsafe { msg_send![super(this), initTextCell: &*ns] }
    }

    pub fn set_insets(&self, top: f64, left: f64, bottom: f64, right: f64) {
        self.ivars().insets.set((top, left, bottom, right));
    }

    /// Shrink a rect by the current insets, clamped so it never inverts.
    fn inset_rect(&self, r: CGRect) -> CGRect {
        let (top, left, bottom, right) = self.ivars().insets.get();
        let w = (r.size.width - left - right).max(0.0);
        let h = (r.size.height - top - bottom).max(0.0);
        // NSTextField cells draw in the control's (unflipped) space, so the
        // bottom inset moves the origin up and the top inset only shrinks height.
        CGRect::new(
            objc2_core_foundation::CGPoint::new(r.origin.x + left, r.origin.y + bottom),
            objc2_core_foundation::CGSize::new(w, h),
        )
    }
}
