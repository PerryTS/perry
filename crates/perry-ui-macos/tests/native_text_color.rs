// #10856 — textSetColor must color a Text() label. install_label_cell used to
// restore the text with setAttributedStringValue:, baking the default
// labelColor into the field; an NSTextField holding an attributed string
// ignores setTextColor:, so the color was silently overridden. This drives the
// real create + set_color path (the same entry points textSetColor lowers to)
// and asserts the label's rendered foreground color is the requested red.
//
// AppKit requires the main thread, so this is a harness = false binary whose
// main() is the process main thread — the pattern the other native_widget_*
// tests use. The default #[test] harness runs on a worker thread and cannot.

#[cfg(target_os = "macos")]
fn main() {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSColorSpace, NSTextField};
    use objc2_foundation::{MainThreadMarker, NSString};
    use perry_ui_macos::widgets;

    if std::env::args().any(|arg| arg == "--list") {
        println!("native_text_color: test");
        return;
    }
    let _mtm = MainThreadMarker::new().expect("native text-color test runs on the main thread");

    let text = "hi";
    let string = perry_runtime::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    let handle = widgets::text::create(string.cast());
    widgets::text::set_color(handle, 1.0, 0.0, 0.0, 1.0);

    let view = widgets::get_widget(handle).expect("Text widget is registered");
    let field = unsafe { &*(Retained::as_ptr(&view) as *const NSTextField) };

    // A plain-string field renders in its textColor; attributedStringValue
    // synthesizes that color at index 0. The old bug left a baked labelColor
    // here instead of the red just set.
    let attributed = field.attributedStringValue();
    let key = NSString::from_str("NSColor");
    let color: Option<Retained<objc2_app_kit::NSColor>> = unsafe {
        msg_send![
            &*attributed,
            attribute: &*key,
            atIndex: 0usize,
            effectiveRange: std::ptr::null_mut::<objc2_foundation::NSRange>()
        ]
    };
    let color = color.expect("label carries a foreground color at index 0");
    let srgb = color
        .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
        .expect("foreground color converts to sRGB");
    let (r, g, b) = (
        srgb.redComponent(),
        srgb.greenComponent(),
        srgb.blueComponent(),
    );
    assert!(
        r > 0.9 && g < 0.1 && b < 0.1,
        "label rendered ({r:.3}, {g:.3}, {b:.3}), not red — setTextColor was overridden (#10856)"
    );
    println!("PASS native text-color: textSetColor colors a Text label");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
