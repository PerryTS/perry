use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ClassBuilder, Sel};
use objc2::{msg_send, sel, ClassType};
use objc2_app_kit::{NSButton, NSView};
use objc2_foundation::{MainThreadMarker, NSString};
use perry_ui_macos::{drag_drop, widgets};
use std::sync::atomic::{AtomicUsize, Ordering};

static CLICKS: AtomicUsize = AtomicUsize::new(0);

extern "C-unwind" fn clicked(_: *mut AnyObject, _: Sel, _: *mut AnyObject) {
    CLICKS.fetch_add(1, Ordering::Relaxed);
}

pub(super) fn check(mtm: MainThreadMarker) {
    unsafe {
        // An observable native mouseDown endpoint, without opening a window
        // or depending on a hardware event loop. The drop-only path must
        // forward here, through either ordering of the two behaviors.
        let mut builder = ClassBuilder::new(c"PaddingTestButton", NSButton::class()).unwrap();
        builder.add_method(
            sel!(mouseDown:),
            clicked as extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject),
        );
        let class = builder.register();
        for padding_first in [true, false] {
            let button = NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Drop"),
                None,
                None,
                mtm,
            );
            AnyObject::set_class(&button, class);
            let view: Retained<NSView> = Retained::cast_unchecked(button);
            let handle = widgets::register_widget(view.clone());
            let before = view.intrinsicContentSize();
            if padding_first {
                widgets::set_edge_insets(handle, 3.0, 5.0, 7.0, 11.0);
            }
            drag_drop::perry_ui_widget_on_drop(handle, 0.0);
            if !padding_first {
                widgets::set_edge_insets(handle, 3.0, 5.0, 7.0, 11.0);
            }
            let padded = view.intrinsicContentSize();
            assert_eq!(padded.width, before.width + 16.0);
            assert_eq!(padded.height, before.height + 10.0);
            let count = CLICKS.load(Ordering::Relaxed);
            let _: () = msg_send![&*view, mouseDown: std::ptr::null::<AnyObject>()];
            assert_eq!(
                CLICKS.load(Ordering::Relaxed),
                count + 1,
                "padding_first={padding_first}"
            );

            let class_before = view.class();
            drag_drop::perry_ui_widget_on_drop(handle, 0.0);
            widgets::set_edge_insets(handle, 3.0, 5.0, 7.0, 11.0);
            assert_eq!(
                view.class(),
                class_before,
                "repeated setters must not nest subclasses"
            );
            let _: () = msg_send![&*view, mouseDown: std::ptr::null::<AnyObject>()];
            assert_eq!(CLICKS.load(Ordering::Relaxed), count + 2);
        }
    }
}
