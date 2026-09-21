use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSColor, NSEvent, NSSecureTextFieldCell, NSText, NSTextField, NSTextFieldCell, NSView,
};
use objc2_core_foundation::CGRect;
use objc2_foundation::{MainThreadMarker, NSEdgeInsets, NSObjectProtocol};
use std::cell::Cell;

mod button;

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
            let bounds = inset_rect(bounds, self.ivars().get(), unsafe { self.controlView() }.is_some_and(|view| view.isFlipped()));
            unsafe { msg_send![super(self), drawingRectForBounds: bounds] }
        }

        #[unsafe(method(cellSizeForBounds:))]
        fn cell_size_for_bounds(&self, bounds: CGRect) -> objc2_core_foundation::CGSize {
            let insets = self.ivars().get();
            let bounds = inset_rect(bounds, insets, unsafe { self.controlView() }.is_some_and(|view| view.isFlipped()));
            let size: objc2_core_foundation::CGSize =
                unsafe { msg_send![super(self), cellSizeForBounds: bounds] };
            padded_size(size, insets)
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
            let frame = inset_rect(frame, self.ivars().get(), view.isFlipped());
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
            let frame = inset_rect(frame, self.ivars().get(), view.isFlipped());
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
            let bounds = inset_rect(bounds, self.ivars().get(), unsafe { self.controlView() }.is_some_and(|view| view.isFlipped()));
            unsafe { msg_send![super(self), drawingRectForBounds: bounds] }
        }

        #[unsafe(method(cellSizeForBounds:))]
        fn cell_size_for_bounds(&self, bounds: CGRect) -> objc2_core_foundation::CGSize {
            let insets = self.ivars().get();
            let bounds = inset_rect(bounds, insets, unsafe { self.controlView() }.is_some_and(|view| view.isFlipped()));
            let size: objc2_core_foundation::CGSize =
                unsafe { msg_send![super(self), cellSizeForBounds: bounds] };
            padded_size(size, insets)
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
            let frame = inset_rect(frame, self.ivars().get(), view.isFlipped());
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
            let frame = inset_rect(frame, self.ivars().get(), view.isFlipped());
            let _: () = msg_send![super(self), selectWithFrame: frame, inView: view, editor: editor, delegate: delegate, start: start, length: length];
        }

        #[unsafe(method(setPerryInsetsTop:left:bottom:right:))]
        fn set_perry_insets(&self, top: f64, left: f64, bottom: f64, right: f64) {
            self.ivars().set(top, left, bottom, right);
        }
    }
);

fn inset_rect(rect: CGRect, insets: NSEdgeInsets, flipped: bool) -> CGRect {
    // Native text fields and buttons are flipped: their top moves origin.y.
    // Keep bottom-origin coordinates correct for an unflipped control view.
    CGRect::new(
        objc2_core_foundation::CGPoint::new(
            rect.origin.x + insets.left,
            rect.origin.y + if flipped { insets.top } else { insets.bottom },
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

/// Install at label creation, before callers apply attributed text or styles.
/// Keep the factory label's text, font and line-breaking defaults.
pub(crate) fn install_label_cell(field: &NSTextField, mtm: MainThreadMarker) {
    // Restore the text as a plain stringValue, never the factory label's
    // attributedStringValue. An NSTextField holding an attributed string
    // ignores setTextColor: — the string's baked-in color attribute wins — so
    // an attributedStringValue here silently defeats textSetColor (#10856).
    // The font is restored separately below, and labelColor is set explicitly
    // to keep the label's default appearance.
    let text = field.stringValue();
    let font = field.font();
    let original = field.cell().expect("label has a cell");
    install_text_field_cell(field, mtm);
    field.setBezeled(false);
    field.setBordered(false);
    field.setEditable(false);
    field.setSelectable(false);
    field.setDrawsBackground(false);
    field.setFont(font.as_deref());
    field.setStringValue(&text);
    field.setTextColor(Some(&NSColor::labelColor()));
    if let Some(cell) = field.cell() {
        cell.setWraps(original.wraps());
        cell.setScrollable(original.isScrollable());
        cell.setUsesSingleLineMode(original.usesSingleLineMode());
        cell.setLineBreakMode(original.lineBreakMode());
    }
}

/// Apply padding to AppKit widgets with a native content-inset mechanism.
/// Perry's labels and buttons include padding in their native sizing and
/// drawing paths, without replacing the widget or its target/action.
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
                super::max_width::refresh_children(view);
                return;
            }
        }

        if AnyClass::get(c"NSButton").is_some_and(|cls| view.isKindOfClass(cls)) {
            button::set_insets(view, insets);
            return;
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
                        let _: () = msg_send![view, invalidateIntrinsicContentSize];
                        let _: () = msg_send![view, setNeedsDisplay: true];
                        return;
                    }
                }
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
                return;
            }
        }
    }
    #[cfg(debug_assertions)]
    eprintln!(
        "[perry/ui] setPadding is not supported for {}; place the widget in a padded stack",
        view.class()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_foundation::NSString;

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
            false,
        );
        assert_eq!(got.origin.x, 10.0);
        assert_eq!(got.origin.y, 10.0);
        assert_eq!(got.size.width, 0.0);
        assert_eq!(got.size.height, 0.0);
    }

    /// #10856 — a label built by install_label_cell must honor setTextColor:.
    /// The regression baked the factory attributedStringValue (labelColor)
    /// into the field, and an NSTextField with an attributed string ignores
    /// setTextColor:, so textSetColor was a silent no-op. After the fix the
    /// field holds a plain stringValue, so a red textColor takes effect and
    /// the synthesized attributedStringValue carries red at index 0.
    #[test]
    fn install_label_cell_lets_set_text_color_win() {
        // Cargo runs each test off the main thread; these AppKit calls only
        // create and inspect one NSTextField, with no NSApplication or run
        // loop, so an unchecked main-thread marker is sound here.
        let mtm = unsafe { MainThreadMarker::new_unchecked() };
        let field = NSTextField::labelWithString(&NSString::from_str("hi"), mtm);
        install_label_cell(&field, mtm);

        let red = NSColor::colorWithSRGBRed_green_blue_alpha(1.0, 0.0, 0.0, 1.0);
        field.setTextColor(Some(&red));

        let attributed = field.attributedStringValue();
        let key = NSString::from_str("NSColor");
        let color: Option<Retained<NSColor>> = unsafe {
            msg_send![&*attributed, attribute: &*key, atIndex: 0usize, effectiveRange: std::ptr::null_mut::<objc2_foundation::NSRange>()]
        };
        let color = color.expect("label carries a foreground color at index 0");
        let srgb = color
            .colorUsingColorSpace(&objc2_app_kit::NSColorSpace::sRGBColorSpace())
            .expect("foreground color converts to sRGB");
        let (r, g, b) = (
            srgb.redComponent(),
            srgb.greenComponent(),
            srgb.blueComponent(),
        );
        assert!(r > 0.9, "red component {r} — setTextColor: was overridden (#10856)");
        assert!(g < 0.1, "green component {g}");
        assert!(b < 0.1, "blue component {b}");
    }
}
