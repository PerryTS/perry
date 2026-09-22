// AppKit widgets require the main thread, so this uses the same harness-free
// process entry point as native_text_color.
#[cfg(target_os = "macos")]
fn main() {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2_app_kit::NSTextField;
    use objc2_foundation::MainThreadMarker;
    use perry_ui_macos::{
        perry_ui_text_set_letter_spacing, perry_ui_text_set_line_height, widgets,
    };

    if std::env::args().any(|arg| arg == "--list") {
        println!("native_text_spacing: test");
        return;
    }
    let _mtm = MainThreadMarker::new().expect("spacing test runs on the main thread");
    let text = "Hello";
    let string = perry_runtime::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    let handle = widgets::text::create(string.cast());
    let view = widgets::get_widget(handle).expect("Text widget is registered");
    let field = unsafe { &*(Retained::as_ptr(&view) as *const NSTextField) };

    perry_ui_text_set_letter_spacing(handle, 2.5);
    perry_ui_text_set_line_height(handle, 1.4);
    assert_eq!(number_attribute(field, "NSKern"), Some(2.5));
    assert!((line_height(field).unwrap() - 1.4).abs() < 0.001);

    widgets::text::set_text_str(handle, "Changed");
    assert_eq!(number_attribute(field, "NSKern"), Some(2.5));
    assert!((line_height(field).unwrap() - 1.4).abs() < 0.001);

    widgets::text::set_color(handle, 1.0, 0.0, 0.0, 1.0);
    let color = attribute(field, "NSColor").expect("attributed label has a color");
    let red: f64 = unsafe { msg_send![&*color, redComponent] };
    assert!(red > 0.9, "color setter still changes an attributed label");

    widgets::text::set_font_size(handle, 21.0);
    let font = attribute(field, "NSFont").expect("attributed label has a font");
    let size: f64 = unsafe { msg_send![&*font, pointSize] };
    assert!((size - 21.0).abs() < 0.001);

    widgets::text::set_text_alignment(handle, 2);
    let paragraph = attribute(field, "NSParagraphStyle").expect("line height keeps a paragraph");
    let alignment: i64 = unsafe { msg_send![&*paragraph, alignment] };
    assert_eq!(alignment, 2);

    perry_ui_text_set_letter_spacing(handle, 0.0);
    perry_ui_text_set_line_height(handle, 1.0);
    assert_eq!(number_attribute(field, "NSKern"), None);
    assert_eq!(line_height(field), None);
    println!("PASS native text spacing: attributes survive text and color updates");
}

#[cfg(target_os = "macos")]
fn attribute(
    field: &objc2_app_kit::NSTextField,
    key: &str,
) -> Option<objc2::rc::Retained<objc2::runtime::AnyObject>> {
    use objc2::msg_send;
    use objc2_foundation::NSString;
    let value = field.attributedStringValue();
    let key = NSString::from_str(key);
    unsafe {
        msg_send![
            &*value,
            attribute: &*key,
            atIndex: 0usize,
            effectiveRange: std::ptr::null_mut::<objc2_foundation::NSRange>()
        ]
    }
}

#[cfg(target_os = "macos")]
fn number_attribute(field: &objc2_app_kit::NSTextField, key: &str) -> Option<f64> {
    attribute(field, key).map(|value| unsafe { objc2::msg_send![&*value, doubleValue] })
}

#[cfg(target_os = "macos")]
fn line_height(field: &objc2_app_kit::NSTextField) -> Option<f64> {
    attribute(field, "NSParagraphStyle")
        .map(|value| unsafe { objc2::msg_send![&*value, lineHeightMultiple] })
}

#[cfg(not(target_os = "macos"))]
fn main() {}
