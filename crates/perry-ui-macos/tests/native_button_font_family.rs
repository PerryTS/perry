// AppKit UI objects must be created and inspected on the main thread.
#[cfg(target_os = "macos")]
fn main() {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{NSApplication, NSButton, NSFont};
    use objc2_foundation::{MainThreadMarker, NSRange, NSString};
    use perry_ui_macos::{perry_ui_button_set_font_family, widgets};

    if std::env::args().any(|arg| arg == "--list") {
        println!("native_button_font_family: test");
        return;
    }

    let mtm = MainThreadMarker::new().expect("native button test runs on the main thread");
    let _app = NSApplication::sharedApplication(mtm);
    let label = perry_runtime::string::js_string_from_bytes(b"Copy".as_ptr(), 4);
    let handle = widgets::button::create(label.cast(), 0.0);
    let view = widgets::get_widget(handle).expect("button is registered");
    let button = unsafe { &*(Retained::as_ptr(&view) as *const NSButton) };
    button.setFont(Some(&NSFont::boldSystemFontOfSize(17.0)));
    widgets::button::set_text_color(handle, 0.25, 0.5, 0.75, 1.0);

    let family = perry_runtime::string::js_string_from_bytes(b"Menlo".as_ptr(), 5);
    perry_ui_button_set_font_family(handle, family as i64);

    let font = button.font().expect("button font");
    assert_eq!(font.familyName().expect("font family").to_string(), "Menlo");
    assert_eq!(font.pointSize(), 17.0);
    assert!(
        font.fontName().to_string().contains("Bold"),
        "family conversion must preserve the button's weight"
    );

    unsafe {
        let title: *mut AnyObject = objc2::msg_send![button, attributedTitle];
        let font_key = NSString::from_str("NSFont");
        let color_key = NSString::from_str("NSColor");
        let font_in_title: *mut NSFont = objc2::msg_send![
            title,
            attribute: &*font_key,
            atIndex: 0usize,
            effectiveRange: std::ptr::null_mut::<NSRange>()
        ];
        let color_in_title: *mut AnyObject = objc2::msg_send![
            title,
            attribute: &*color_key,
            atIndex: 0usize,
            effectiveRange: std::ptr::null_mut::<NSRange>()
        ];
        assert!(!font_in_title.is_null(), "attributed title keeps font");
        assert!(
            !color_in_title.is_null(),
            "attributed title keeps text color"
        );
        assert_eq!(
            (*font_in_title)
                .familyName()
                .expect("attributed font family")
                .to_string(),
            "Menlo"
        );
    }

    widgets::button::set_bordered(handle, false);
    assert_eq!(
        button.font().expect("borderless button font").pointSize(),
        17.0
    );
    let text = widgets::text::create(label.cast());
    perry_ui_button_set_font_family(text, family as i64);
    println!("PASS native button font family");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
