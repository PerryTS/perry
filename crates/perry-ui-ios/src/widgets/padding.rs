use objc2::rc::Retained;
use objc2::{define_class, msg_send, sel, ClassType, DefinedClass, MainThreadOnly};
use objc2_core_foundation::{CGRect, CGSize};
use objc2_foundation::{MainThreadMarker, NSObjectProtocol};
use objc2_ui_kit::{NSDirectionalEdgeInsets, UIButton, UIEdgeInsets, UITextField, UIView};
use std::cell::Cell;

pub struct PerryInsetTextFieldIvars {
    top: Cell<f64>,
    left: Cell<f64>,
    bottom: Cell<f64>,
    right: Cell<f64>,
}

define_class!(
    #[unsafe(super(UITextField))]
    #[name = "PerryInsetTextField"]
    #[ivars = PerryInsetTextFieldIvars]
    pub struct PerryInsetTextField;

    impl PerryInsetTextField {
        #[unsafe(method(textRectForBounds:))]
        fn text_rect_for_bounds(&self, bounds: CGRect) -> CGRect {
            let rect: CGRect = unsafe { msg_send![super(self), textRectForBounds: bounds] };
            inset_rect(rect, self.insets())
        }

        #[unsafe(method(editingRectForBounds:))]
        fn editing_rect_for_bounds(&self, bounds: CGRect) -> CGRect {
            let rect: CGRect = unsafe { msg_send![super(self), editingRectForBounds: bounds] };
            inset_rect(rect, self.insets())
        }

        #[unsafe(method(placeholderRectForBounds:))]
        fn placeholder_rect_for_bounds(&self, bounds: CGRect) -> CGRect {
            let rect: CGRect = unsafe { msg_send![super(self), placeholderRectForBounds: bounds] };
            inset_rect(rect, self.insets())
        }

        #[unsafe(method(intrinsicContentSize))]
        fn intrinsic_content_size(&self) -> CGSize {
            let size: CGSize = unsafe { msg_send![super(self), intrinsicContentSize] };
            padded_size(size, self.insets())
        }
    }
);

impl PerryInsetTextField {
    fn insets(&self) -> UIEdgeInsets {
        UIEdgeInsets {
            top: self.ivars().top.get(),
            left: self.ivars().left.get(),
            bottom: self.ivars().bottom.get(),
            right: self.ivars().right.get(),
        }
    }

    fn set_insets(&self, top: f64, left: f64, bottom: f64, right: f64) {
        self.ivars().top.set(top);
        self.ivars().left.set(left);
        self.ivars().bottom.set(bottom);
        self.ivars().right.set(right);
        unsafe {
            let _: () = msg_send![self, setNeedsLayout];
            let _: () = msg_send![self, invalidateIntrinsicContentSize];
        }
    }
}

fn padded_size(size: CGSize, insets: UIEdgeInsets) -> CGSize {
    CGSize::new(
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

fn inset_rect(rect: CGRect, insets: UIEdgeInsets) -> CGRect {
    CGRect::new(
        objc2_core_foundation::CGPoint::new(
            rect.origin.x + insets.left,
            rect.origin.y + insets.top,
        ),
        objc2_core_foundation::CGSize::new(
            (rect.size.width - insets.left - insets.right).max(0.0),
            (rect.size.height - insets.top - insets.bottom).max(0.0),
        ),
    )
}

/// Construct the UITextField subclass used by TextField, SecureField, and
/// ComboBox. Keeping the inset state on the view means changing padding after
/// creation immediately affects display, placeholder, and editing rectangles.
pub(crate) fn new_text_field() -> Retained<UITextField> {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let this = PerryInsetTextField::alloc(mtm).set_ivars(PerryInsetTextFieldIvars {
        top: Cell::new(0.0),
        left: Cell::new(0.0),
        bottom: Cell::new(0.0),
        right: Cell::new(0.0),
    });
    let field: Retained<PerryInsetTextField> = unsafe { msg_send![super(this), init] };
    unsafe { Retained::cast_unchecked(field) }
}

/// Apply padding to the native widget kinds that expose a content-inset API.
/// Leaf views such as UILabel and UIImageView have no such API and remain a
/// documented no-op until Perry gives them a content wrapper.
pub(crate) fn set_edge_insets(view: &UIView, top: f64, left: f64, bottom: f64, right: f64) {
    let insets = UIEdgeInsets {
        top,
        left,
        bottom,
        right,
    };

    unsafe {
        if view.isKindOfClass(PerryInsetTextField::class()) {
            let field = &*(view as *const UIView as *const PerryInsetTextField);
            field.set_insets(top, left, bottom, right);
            return;
        }

        if let Some(cls) = objc2::runtime::AnyClass::get(c"UIStackView") {
            if view.isKindOfClass(cls) {
                let _: () = msg_send![view, setLayoutMarginsRelativeArrangement: true];
                let _: () = msg_send![view, setDirectionalLayoutMargins: insets];
                return;
            }
        }

        // UITextView is a UIScrollView subclass, so it must be tested first:
        // textContainerInset pads the text while contentInset moves scrolling.
        if let Some(cls) = objc2::runtime::AnyClass::get(c"UITextView") {
            if view.isKindOfClass(cls) {
                let _: () = msg_send![view, setTextContainerInset: insets];
                return;
            }
        }

        if let Some(cls) = objc2::runtime::AnyClass::get(c"UIButton") {
            if view.isKindOfClass(cls) {
                let button = &*(view as *const UIView as *const UIButton);
                if button.respondsToSelector(sel!(configuration)) {
                    if let Some(configuration) = button.configuration() {
                        configuration.setContentInsets(NSDirectionalEdgeInsets {
                            top,
                            leading: left,
                            bottom,
                            trailing: right,
                        });
                        button.setConfiguration(Some(&configuration));
                        return;
                    }
                }
                let _: () = msg_send![view, setContentEdgeInsets: insets];
                return;
            }
        }

        if let Some(cls) = objc2::runtime::AnyClass::get(c"UIScrollView") {
            if view.isKindOfClass(cls) {
                let _: () = msg_send![view, setContentInset: insets];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inset_rect_clamps_an_over_inset_axis() {
        let rect = CGRect::new(
            objc2_core_foundation::CGPoint::new(2.0, 3.0),
            objc2_core_foundation::CGSize::new(20.0, 10.0),
        );
        let got = inset_rect(
            rect,
            UIEdgeInsets {
                top: 4.0,
                left: 8.0,
                bottom: 7.0,
                right: 14.0,
            },
        );
        assert_eq!(got.origin.x, 10.0);
        assert_eq!(got.origin.y, 7.0);
        assert_eq!(got.size.width, 0.0);
        assert_eq!(got.size.height, 0.0);
    }
}
