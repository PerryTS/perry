use objc2::msg_send;
use objc2::rc::Retained;
use objc2::MainThreadOnly;
use objc2_app_kit::{NSBox, NSStackView, NSView};
use objc2_foundation::{MainThreadMarker, NSString};

/// Create a form container (vertical NSStackView with padding). Returns widget handle.
pub fn form_create() -> i64 {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    unsafe {
        let stack: Retained<NSStackView> = msg_send![
            NSStackView::alloc(mtm), initWithFrame: objc2_core_foundation::CGRect::ZERO
        ];
        let _: () = msg_send![&*stack, setOrientation: 1i64]; // vertical
        let _: () = msg_send![&*stack, setSpacing: 16.0f64];
        let _: () = msg_send![&*stack, setEdgeInsets: objc2_foundation::NSEdgeInsets {
            top: 16.0, left: 16.0, bottom: 16.0, right: 16.0
        }];

        let view: Retained<NSView> = Retained::cast_unchecked(stack);
        super::register_widget(view)
    }
}

/// Extract a &str from a *const StringHeader pointer.
fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const crate::string_header::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

/// Create a form section (NSBox with optional title and inner vertical NSStackView).
/// Returns widget handle.
pub fn section_create(title_ptr: *const u8) -> i64 {
    let title = str_from_header(title_ptr);
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    unsafe {
        // Create a box with a title for visual grouping
        let nsbox: Retained<NSBox> = msg_send![
            NSBox::alloc(mtm), initWithFrame: objc2_core_foundation::CGRect::new(
                objc2_core_foundation::CGPoint::new(0.0, 0.0),
                objc2_core_foundation::CGSize::new(300.0, 100.0),
            )
        ];

        if !title.is_empty() {
            let ns_title = NSString::from_str(title);
            let _: () = msg_send![&*nsbox, setTitle: &*ns_title];
        } else {
            let _: () = msg_send![&*nsbox, setTitlePosition: 0i64]; // NoTitle
        }

        // Create inner stack view for content
        let stack: Retained<NSStackView> = msg_send![
            NSStackView::alloc(mtm), initWithFrame: objc2_core_foundation::CGRect::ZERO
        ];
        let _: () = msg_send![&*stack, setOrientation: 1i64]; // vertical
        let _: () = msg_send![&*stack, setSpacing: 8.0f64];

        let _: () = msg_send![&*nsbox, setContentView: &*stack];

        let view: Retained<NSView> = Retained::cast_unchecked(nsbox);
        super::register_widget(view)
    }
}
