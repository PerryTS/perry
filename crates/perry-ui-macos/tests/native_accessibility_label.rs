// widgetSetAccessibilityLabel and Label name the control VoiceOver reaches: the view
// itself, a Toggle's switch, a TextArea's text view. AppKit requires the main thread, so
// this is a harness = false binary.

#[cfg(target_os = "macos")]
fn main() {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::NSView;
    use objc2_foundation::{MainThreadMarker, NSArray, NSString};
    use perry_runtime as _; // links the runtime symbols
    use perry_ui_macos::{perry_ui_label_create, perry_ui_widget_set_accessibility_label, widgets};

    if std::env::args().any(|arg| arg == "--list") {
        println!("native_accessibility_label: test");
        return;
    }
    let _mtm = MainThreadMarker::new().expect("runs on the main thread");

    let string = |s: &str| perry_ffi::alloc_string(s).as_raw() as i64;
    let view = |handle| widgets::get_widget(handle).expect("widget is registered");
    let name = |view: &AnyObject| -> Option<String> {
        let label: Option<Retained<NSString>> = unsafe { msg_send![view, accessibilityLabel] };
        label.map(|label| label.to_string())
    };
    let text = |s: &str| string(s) as *const u8;

    let button = widgets::button::create(text("+"), 0.0);
    let field = widgets::textfield::create(text("placeholder"), 0.0);
    let toggle = widgets::toggle::create(text("Notify"), 0.0);
    let area = widgets::textarea::create(text("notes"), 0.0);
    for (handle, label) in [
        (button, "Add"),
        (field, "Phone"),
        (toggle, "Updates"),
        (area, "Notes"),
    ] {
        perry_ui_widget_set_accessibility_label(handle, string(label));
    }
    perry_ui_widget_set_accessibility_label(i64::MAX, string("no such widget"));

    assert_eq!(name(&view(button)).as_deref(), Some("Add"));
    assert_eq!(name(&view(field)).as_deref(), Some("Phone"));
    let children: Retained<NSArray<NSView>> = unsafe { msg_send![&*view(toggle), subviews] };
    let switch = children.lastObject().expect("a Toggle holds its switch");
    assert_eq!(name(&switch).as_deref(), Some("Updates"));
    let text_view: Retained<NSView> = unsafe { msg_send![&*view(area), documentView] };
    assert_eq!(name(&text_view).as_deref(), Some("Notes"));

    let caption = perry_ui_label_create(string("Phone number"), field);
    let shown: Retained<NSString> = unsafe { msg_send![&*view(caption), stringValue] };
    assert_eq!(shown.to_string(), "Phone number");
    assert_eq!(name(&view(field)).as_deref(), Some("Phone number"));
    println!("PASS native accessibility label");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
