//! Per-instance button padding, following the backend's drag/drop subclass
//! pattern. No cell or view is reconstructed: AppKit's factory state survives.
use super::{inset_rect, padded_size};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
use objc2::{msg_send, sel};
use objc2_app_kit::{NSButton, NSButtonCell, NSView};
use objc2_core_foundation::{CGRect, CGSize};
use objc2_foundation::{NSEdgeInsets, NSValue};
use std::ffi::CString;

static INSETS_KEY: u8 = 0;
const CLASS_PREFIX: &[u8] = b"PerryButtonPadding_";

fn padding_class(view: &NSView) -> Option<&'static AnyClass> {
    let mut cls = Some(view.class());
    while let Some(current) = cls {
        if current.name().to_bytes().starts_with(CLASS_PREFIX) {
            return Some(current);
        }
        cls = current.superclass();
    }
    None
}

pub(super) fn set_insets(view: &NSView, insets: NSEdgeInsets) {
    unsafe {
        if padding_class(view).is_none() {
            let original = view.class();
            let name = CString::new(format!(
                "PerryButtonPadding_{}",
                original.name().to_string_lossy()
            ))
            .unwrap();
            let subclass = AnyClass::get(&name).unwrap_or_else(|| {
                let mut builder =
                    ClassBuilder::new(&name, original).expect("padding subclass name");
                builder.add_method(
                    sel!(intrinsicContentSize),
                    intrinsic_size as unsafe extern "C-unwind" fn(*mut NSButton, Sel) -> CGSize,
                );
                builder.add_method(
                    sel!(wantsUpdateLayer),
                    wants_update_layer as unsafe extern "C-unwind" fn(*mut NSButton, Sel) -> Bool,
                );
                builder.add_method(
                    sel!(drawRect:),
                    draw_rect as unsafe extern "C-unwind" fn(*mut NSButton, Sel, CGRect),
                );
                builder.register()
            });
            // Main-thread-only NSView access; the subclass adds no ivars and
            // all method signatures match NSButton. It also composes with
            // drag/drop's dynamic subclasses, before or after this call.
            assert_eq!(original.instance_size(), subclass.instance_size());
            let previous = AnyObject::set_class(view, subclass);
            assert_eq!(previous, original);
        }
        // The association is owned by the native view, so it is released with
        // the view and cannot leak or be reused for a different widget address.
        let value = NSValue::new(insets);
        objc2::ffi::objc_setAssociatedObject(
            view as *const NSView as *mut AnyObject,
            (&INSETS_KEY as *const u8).cast(),
            Retained::as_ptr(&value) as *mut AnyObject,
            objc2::ffi::OBJC_ASSOCIATION_RETAIN_NONATOMIC,
        );
        view.invalidateIntrinsicContentSize();
        view.setNeedsDisplay(true);
    }
}

unsafe fn insets(button: &NSButton) -> NSEdgeInsets {
    let ptr = objc2::ffi::objc_getAssociatedObject(
        button as *const NSButton as *const AnyObject,
        (&INSETS_KEY as *const u8).cast(),
    );
    if ptr.is_null() {
        return NSEdgeInsets {
            top: 0.0,
            left: 0.0,
            bottom: 0.0,
            right: 0.0,
        };
    }
    // This private association key stores only NSValue<NSEdgeInsets>.
    (&*(ptr as *const NSValue)).get()
}

fn superclass(button: &NSButton) -> &'static AnyClass {
    padding_class(button)
        .expect("padding method owner")
        .superclass()
        .unwrap()
}

unsafe extern "C-unwind" fn intrinsic_size(button: *mut NSButton, _: Sel) -> CGSize {
    let button = &*button;
    let size = msg_send![super(button, superclass(button)), intrinsicContentSize];
    padded_size(size, insets(button))
}

unsafe extern "C-unwind" fn wants_update_layer(button: *mut NSButton, _: Sel) -> Bool {
    let button = &*button;
    let i = insets(button);
    if i.top != 0.0 || i.left != 0.0 || i.bottom != 0.0 || i.right != 0.0 {
        Bool::NO
    } else {
        msg_send![super(button, superclass(button)), wantsUpdateLayer]
    }
}

unsafe extern "C-unwind" fn draw_rect(button: *mut NSButton, _: Sel, dirty: CGRect) {
    let button = &*button;
    let i = insets(button);
    if i.top == 0.0 && i.left == 0.0 && i.bottom == 0.0 && i.right == 0.0 {
        let _: () = msg_send![super(button, superclass(button)), drawRect: dirty];
        return;
    }
    if let Some(cell) = button.cell() {
        let cell = &*(Retained::as_ptr(&cell) as *const NSButtonCell);
        let bounds = button.bounds();
        if button.isBordered() {
            cell.drawBezelWithFrame_inView(bounds, button);
        }
        let content = inset_rect(bounds, i, button.isFlipped());
        cell.drawInteriorWithFrame_inView(content, button);
    }
}
