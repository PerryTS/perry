//! Text-inset cells for NSTextField and NSSecureTextField.
//!
//! Neither control has a property for the inset between its bezel and its text.
//! An NSTextFieldCell / NSSecureTextFieldCell subclass that shrinks the drawing,
//! title, editing and selection rects is the only way to reach it, so a Perry
//! TextField / SecureField carries one of these cells and `set_edge_insets`
//! writes the inset onto it. The secure variant subclasses NSSecureTextFieldCell
//! so the dot-masking field editor is preserved.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, ClassType, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSEvent, NSSecureTextFieldCell, NSText, NSTextFieldCell, NSView};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{MainThreadMarker, NSString};
use std::cell::Cell;

/// (top, left, bottom, right), in points.
pub struct InsetCellIvars {
    insets: Cell<(f64, f64, f64, f64)>,
}

impl InsetCellIvars {
    fn zero() -> Self {
        InsetCellIvars {
            insets: Cell::new((0.0, 0.0, 0.0, 0.0)),
        }
    }
}

/// Shrink a rect by the insets, clamped so it never inverts. NSTextField cells
/// draw in the control's (unflipped) space, so the bottom inset moves the origin
/// up and the top inset only shrinks the height.
fn shrink(insets: (f64, f64, f64, f64), r: CGRect) -> CGRect {
    let (top, left, bottom, right) = insets;
    let w = (r.size.width - left - right).max(0.0);
    let h = (r.size.height - top - bottom).max(0.0);
    CGRect::new(
        CGPoint::new(r.origin.x + left, r.origin.y + bottom),
        CGSize::new(w, h),
    )
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
            shrink(self.ivars().insets.get(), sup)
        }

        #[unsafe(method(titleRectForBounds:))]
        fn title_rect_for_bounds(&self, bounds: CGRect) -> CGRect {
            let sup: CGRect = unsafe { msg_send![super(self), titleRectForBounds: bounds] };
            shrink(self.ivars().insets.get(), sup)
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
            let r = shrink(self.ivars().insets.get(), rect);
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
            let r = shrink(self.ivars().insets.get(), rect);
            unsafe {
                let _: () = msg_send![
                    super(self),
                    selectWithFrame: r, inView: view, editor: editor, delegate: delegate, start: start, length: length
                ];
            }
        }
    }
);

define_class!(
    #[unsafe(super(NSSecureTextFieldCell))]
    #[name = "PerryInsetSecureTextFieldCell"]
    #[ivars = InsetCellIvars]
    pub struct PerryInsetSecureTextFieldCell;

    impl PerryInsetSecureTextFieldCell {
        #[unsafe(method(drawingRectForBounds:))]
        fn drawing_rect_for_bounds(&self, bounds: CGRect) -> CGRect {
            let sup: CGRect = unsafe { msg_send![super(self), drawingRectForBounds: bounds] };
            shrink(self.ivars().insets.get(), sup)
        }

        #[unsafe(method(titleRectForBounds:))]
        fn title_rect_for_bounds(&self, bounds: CGRect) -> CGRect {
            let sup: CGRect = unsafe { msg_send![super(self), titleRectForBounds: bounds] };
            shrink(self.ivars().insets.get(), sup)
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
            let r = shrink(self.ivars().insets.get(), rect);
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
            let r = shrink(self.ivars().insets.get(), rect);
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
    /// A cell seeded with the field's text, ready to replace the default cell
    /// before the field is configured.
    pub fn new(text: &str, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(InsetCellIvars::zero());
        let ns = NSString::from_str(text);
        unsafe { msg_send![super(this), initTextCell: &*ns] }
    }
}

impl PerryInsetSecureTextFieldCell {
    pub fn new(text: &str, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(InsetCellIvars::zero());
        let ns = NSString::from_str(text);
        unsafe { msg_send![super(this), initTextCell: &*ns] }
    }
}

/// Write the inset onto `cell` if it is one of the Perry inset cells. Returns
/// whether it matched, so the caller can tell a Perry field from a foreign one.
pub fn try_set_cell_insets(
    cell: *mut AnyObject,
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
) -> bool {
    if cell.is_null() {
        return false;
    }
    unsafe {
        let is_plain: bool =
            msg_send![cell, isKindOfClass: PerryInsetTextFieldCell::class()];
        if is_plain {
            (*(cell as *const PerryInsetTextFieldCell))
                .ivars()
                .insets
                .set((top, left, bottom, right));
            return true;
        }
        let is_secure: bool =
            msg_send![cell, isKindOfClass: PerryInsetSecureTextFieldCell::class()];
        if is_secure {
            (*(cell as *const PerryInsetSecureTextFieldCell))
                .ivars()
                .insets
                .set((top, left, bottom, right));
            return true;
        }
    }
    false
}
