use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSEvent, NSSecureTextFieldCell, NSText, NSTextField, NSTextFieldCell, NSView};
use objc2_core_foundation::CGRect;
use objc2_foundation::{MainThreadMarker, NSEdgeInsets, NSObjectProtocol};
use std::cell::Cell;

pub struct PerryInsetCellIvars {
    top: Cell<f64>,
    left: Cell<f64>,
    bottom: Cell<f64>,
    right: Cell<f64>,
}

impl PerryInsetCellIvars {
    fn new() -> Self {
        Self {
            top: Cell::new(0.0),
            left: Cell::new(0.0),
            bottom: Cell::new(0.0),
            right: Cell::new(0.0),
        }
    }

    fn get(&self) -> NSEdgeInsets {
        NSEdgeInsets {
            top: self.top.get(),
            left: self.left.get(),
            bottom: self.bottom.get(),
            right: self.right.get(),
        }
    }

    fn set(&self, top: f64, left: f64, bottom: f64, right: f64) {
        self.top.set(top);
        self.left.set(left);
        self.bottom.set(bottom);
        self.right.set(right);
    }
}

define_class!(
    #[unsafe(super(NSTextFieldCell))]
    #[name = "PerryInsetTextFieldCell"]
    #[ivars = PerryInsetCellIvars]
    pub struct PerryInsetTextFieldCell;

    impl PerryInsetTextFieldCell {
        #[unsafe(method(drawingRectForBounds:))]
        fn drawing_rect_for_bounds(&self, bounds: CGRect) -> CGRect {
            let bounds = inset_rect(bounds, self.ivars().get());
            unsafe { msg_send![super(self), drawingRectForBounds: bounds] }
        }

        #[unsafe(method(cellSizeForBounds:))]
        fn cell_size_for_bounds(&self, bounds: CGRect) -> objc2_core_foundation::CGSize {
            let size: objc2_core_foundation::CGSize =
                unsafe { msg_send![super(self), cellSizeForBounds: bounds] };
            padded_size(size, self.ivars().get())
        }

        #[unsafe(method(editWithFrame:inView:editor:delegate:event:))]
        unsafe fn edit_with_frame(
            &self,
            frame: CGRect,
            view: &NSView,
            editor: &NSText,
            delegate: Option<&AnyObject>,
            event: Option<&NSEvent>,
        ) {
            let frame = inset_rect(frame, self.ivars().get());
            let _: () = msg_send![super(self), editWithFrame: frame, inView: view, editor: editor, delegate: delegate, event: event];
        }

        #[unsafe(method(selectWithFrame:inView:editor:delegate:start:length:))]
        unsafe fn select_with_frame(
            &self,
            frame: CGRect,
            view: &NSView,
            editor: &NSText,
            delegate: Option<&AnyObject>,
            start: isize,
            length: isize,
        ) {
            let frame = inset_rect(frame, self.ivars().get());
            let _: () = msg_send![super(self), selectWithFrame: frame, inView: view, editor: editor, delegate: delegate, start: start, length: length];
        }

        #[unsafe(method(setPerryInsetsTop:left:bottom:right:))]
        fn set_perry_insets(&self, top: f64, left: f64, bottom: f64, right: f64) {
            self.ivars().set(top, left, bottom, right);
        }
    }
);

define_class!(
    #[unsafe(super(NSSecureTextFieldCell))]
    #[name = "PerryInsetSecureTextFieldCell"]
    #[ivars = PerryInsetCellIvars]
    pub struct PerryInsetSecureTextFieldCell;

    impl PerryInsetSecureTextFieldCell {
        #[unsafe(method(drawingRectForBounds:))]
        fn drawing_rect_for_bounds(&self, bounds: CGRect) -> CGRect {
            let bounds = inset_rect(bounds, self.ivars().get());
            unsafe { msg_send![super(self), drawingRectForBounds: bounds] }
        }

        #[unsafe(method(cellSizeForBounds:))]
        fn cell_size_for_bounds(&self, bounds: CGRect) -> objc2_core_foundation::CGSize {
            let size: objc2_core_foundation::CGSize =
                unsafe { msg_send![super(self), cellSizeForBounds: bounds] };
            padded_size(size, self.ivars().get())
        }

        #[unsafe(method(editWithFrame:inView:editor:delegate:event:))]
        unsafe fn edit_with_frame(
            &self,
            frame: CGRect,
            view: &NSView,
            editor: &NSText,
            delegate: Option<&AnyObject>,
            event: Option<&NSEvent>,
        ) {
            let frame = inset_rect(frame, self.ivars().get());
            let _: () = msg_send![super(self), editWithFrame: frame, inView: view, editor: editor, delegate: delegate, event: event];
        }

        #[unsafe(method(selectWithFrame:inView:editor:delegate:start:length:))]
        unsafe fn select_with_frame(
            &self,
            frame: CGRect,
            view: &NSView,
            editor: &NSText,
            delegate: Option<&AnyObject>,
            start: isize,
            length: isize,
        ) {
            let frame = inset_rect(frame, self.ivars().get());
            let _: () = msg_send![super(self), selectWithFrame: frame, inView: view, editor: editor, delegate: delegate, start: start, length: length];
        }

        #[unsafe(method(setPerryInsetsTop:left:bottom:right:))]
        fn set_perry_insets(&self, top: f64, left: f64, bottom: f64, right: f64) {
            self.ivars().set(top, left, bottom, right);
        }
    }
);

fn inset_rect(rect: CGRect, insets: NSEdgeInsets) -> CGRect {
    // AppKit's unflipped cell coordinates grow upward, so bottom moves origin.y.
    CGRect::new(
        objc2_core_foundation::CGPoint::new(
            rect.origin.x + insets.left,
            rect.origin.y + insets.bottom,
        ),
        objc2_core_foundation::CGSize::new(
            (rect.size.width - insets.left - insets.right).max(0.0),
            (rect.size.height - insets.top - insets.bottom).max(0.0),
        ),
    )
}

fn padded_size(
    size: objc2_core_foundation::CGSize,
    insets: NSEdgeInsets,
) -> objc2_core_foundation::CGSize {
    objc2_core_foundation::CGSize::new(
        if size.width < 0.0 {
            size.width
        } else {
            size.width + insets.left + insets.right
        },
        if size.height < 0.0 {
            size.height
        } else {
            size.height + insets.top + insets.bottom
        },
    )
}

pub(crate) fn install_text_field_cell(field: &NSTextField, mtm: MainThreadMarker) {
    let this = PerryInsetTextFieldCell::alloc(mtm).set_ivars(PerryInsetCellIvars::new());
    let cell: Retained<PerryInsetTextFieldCell> = unsafe { msg_send![super(this), init] };
    unsafe {
        let _: () = msg_send![field, setCell: &*cell];
    }
}

pub(crate) fn install_secure_text_field_cell(field: &NSTextField, mtm: MainThreadMarker) {
    let this = PerryInsetSecureTextFieldCell::alloc(mtm).set_ivars(PerryInsetCellIvars::new());
    let cell: Retained<PerryInsetSecureTextFieldCell> = unsafe { msg_send![super(this), init] };
    unsafe {
        let _: () = msg_send![field, setCell: &*cell];
    }
}

/// Apply padding to AppKit widgets with a native content-inset mechanism.
/// NSButton, NSTextField labels, and NSImageView do not expose one and remain
/// explicit no-ops until Perry gives those leaf widgets content wrappers.
pub(crate) fn set_edge_insets(view: &NSView, top: f64, left: f64, bottom: f64, right: f64) {
    let insets = NSEdgeInsets {
        top,
        left,
        bottom,
        right,
    };
    unsafe {
        if let Some(cls) = AnyClass::get(c"NSStackView") {
            if view.isKindOfClass(cls) {
                let _: () = msg_send![view, setEdgeInsets: insets];
                return;
            }
        }

        if let Some(cls) = AnyClass::get(c"NSTextField") {
            if view.isKindOfClass(cls) {
                let cell: *mut AnyObject = msg_send![view, cell];
                if !cell.is_null() {
                    let selector =
                        objc2::runtime::Sel::register(c"setPerryInsetsTop:left:bottom:right:");
                    let responds: bool = msg_send![cell, respondsToSelector: selector];
                    if responds {
                        let _: () = msg_send![cell, setPerryInsetsTop: top, left: left, bottom: bottom, right: right];
                        let _: () = msg_send![view, setNeedsDisplay: true];
                    }
                }
                return;
            }
        }

        if let Some(cls) = AnyClass::get(c"NSTextView") {
            if view.isKindOfClass(cls) {
                let symmetric =
                    objc2_core_foundation::CGSize::new((left + right) / 2.0, (top + bottom) / 2.0);
                let _: () = msg_send![view, setTextContainerInset: symmetric];
                return;
            }
        }

        if let Some(cls) = AnyClass::get(c"NSScrollView") {
            if view.isKindOfClass(cls) {
                // TextArea is registered as its enclosing NSScrollView, and
                // NSScrollView's four-sided contentInsets preserve asymmetry.
                let _: () = msg_send![view, setContentInsets: insets];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inset_rect_uses_appkit_bottom_origin_and_clamps() {
        let rect = CGRect::new(
            objc2_core_foundation::CGPoint::new(2.0, 3.0),
            objc2_core_foundation::CGSize::new(20.0, 10.0),
        );
        let got = inset_rect(
            rect,
            NSEdgeInsets {
                top: 4.0,
                left: 8.0,
                bottom: 7.0,
                right: 14.0,
            },
        );
        assert_eq!(got.origin.x, 10.0);
        assert_eq!(got.origin.y, 10.0);
        assert_eq!(got.size.width, 0.0);
        assert_eq!(got.size.height, 0.0);
    }
}
