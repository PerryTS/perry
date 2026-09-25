// AppKit must run on the process main thread (harness = false).
#[cfg(target_os = "macos")]
fn main() {
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::{msg_send, ClassType};
    use objc2_app_kit::{
        NSAppearance, NSAppearanceCustomization, NSApplication, NSButton, NSColor, NSColorSpace,
        NSTextField, NSView,
    };
    use objc2_foundation::{MainThreadMarker, NSString};
    use perry_ui_macos::*;
    if std::env::args().any(|arg| arg == "--list") {
        println!("native_dynamic_colors: test");
        return;
    }
    let mtm = MainThreadMarker::new().unwrap();
    let _app = NSApplication::sharedApplication(mtm);
    let stack = widgets::vstack::create(0.0);
    let parent = widgets::get_widget(stack).unwrap();
    let string = perry_runtime::string::js_string_from_bytes(b"hello".as_ptr(), 5);
    let text = widgets::text::create(string.cast());
    let view = widgets::get_widget(text).unwrap();
    widgets::add_child(stack, text);
    let button = widgets::button::create(string.cast(), 0.0);
    let button_view = widgets::get_widget(button).unwrap();
    widgets::add_child(stack, button);
    let light = [1.0, 0.0, 0.0, 1.0];
    let dark = [0.0, 0.0, 1.0, 0.5];
    // Exercise the actual ABI endpoints, including all eight color arguments.
    perry_ui_widget_set_dynamic_background_color(text, 1., 0., 0., 1., 0., 0., 1., 0.5);
    perry_ui_widget_set_dynamic_border_color(text, 1., 0., 0., 1., 0., 0., 1., 0.5);
    perry_ui_text_set_dynamic_color(text, 1., 0., 0., 1., 0., 0., 1., 0.5);
    perry_ui_text_set_dynamic_color(button, 1., 0., 0., 1., 0., 0., 1., 0.5);
    perry_ui_button_set_dynamic_content_tint_color(button, 1., 0., 0., 1., 0., 0., 1., 0.5);
    // Spacing rebuilds attributed text; it must retain the dynamic foreground.
    widgets::text::set_letter_spacing(text, 1.5);
    let children = view.subviews().len();
    widgets::dynamic_color::set_background(text, light, dark);
    assert_eq!(children, view.subviews().len(), "one observer per widget");

    let check = |appearance_name: &str, expected: [f64; 4]| {
        parent.setAppearance(Some(&appearance(appearance_name)));
        check_text(&view, expected);
        button_view
            .effectiveAppearance()
            .performAsCurrentDrawingAppearance(&RcBlock::new(|| {
                let button = button_view.downcast_ref::<NSButton>().unwrap();
                assert_color(&button.contentTintColor().unwrap(), expected);
                assert_foreground(&button.attributedTitle(), expected);
            }));
    };
    check("NSAppearanceNameAqua", light);
    check("NSAppearanceNameDarkAqua", dark);
    check("NSAppearanceNameAccessibilityHighContrastAqua", light);
    check("NSAppearanceNameAccessibilityHighContrastDarkAqua", dark);
    check("NSAppearanceNameAqua", light);

    // A widget override wins over its parent's appearance.
    view.setAppearance(Some(&appearance("NSAppearanceNameDarkAqua")));
    check_text(&view, dark);
    view.setAppearance(None);
    check_text(&view, light);

    // A palette installed while already dark resolves immediately.
    parent.setAppearance(Some(&appearance("NSAppearanceNameDarkAqua")));
    widgets::dynamic_color::set_background(text, light, dark);
    widgets::set_hidden(text, true);
    widgets::set_hidden(text, false);
    check_text(&view, dark);
    // Removing arranged children must not remove a stack's color observer.
    let container = widgets::vstack::create(0.0);
    widgets::dynamic_color::set_background(container, light, dark);
    let child = widgets::text::create(string.cast());
    widgets::add_child(container, child);
    let container_view = widgets::get_widget(container).unwrap();
    let count = container_view.subviews().len();
    widgets::clear_children(container);
    assert_eq!(container_view.subviews().len(), count - 1);

    // Static setters permanently replace their corresponding dynamic color.
    widgets::set_background_color(text, 1., 1., 1., 1.);
    widgets::set_border_color(text, 1., 1., 1., 1.);
    widgets::text::set_color(text, 1., 1., 1., 1.);
    widgets::button::set_text_color(button, 1., 1., 1., 1.);
    widgets::button::set_content_tint_color(button, 1., 1., 1., 1.);
    check("NSAppearanceNameAqua", [1.; 4]);
    check("NSAppearanceNameDarkAqua", [1.; 4]);

    // Invalid handles and unsupported foreground widget kinds are harmless.
    perry_ui_widget_set_dynamic_background_color(0, 1., 0., 0., 1., 0., 0., 1., 1.);
    let plain = widgets::register_widget(NSView::new(mtm));
    perry_ui_text_set_dynamic_color(plain, 1., 0., 0., 1., 0., 0., 1., 1.);
    perry_ui_button_set_dynamic_content_tint_color(plain, 1., 0., 0., 1., 0., 0., 1., 1.);
    println!("PASS native dynamic background, border, text and tint colors");

    fn appearance(name: &str) -> Retained<NSAppearance> {
        NSAppearance::appearanceNamed(&NSString::from_str(name)).unwrap()
    }
    fn check_text(view: &NSView, expected: [f64; 4]) {
        // Resolve lazy inherited appearance without calling the implementation's
        // refresh function or appearance callback directly.
        let appearance = view.effectiveAppearance();
        appearance.performAsCurrentDrawingAppearance(&RcBlock::new(|| unsafe {
            let layer: *mut objc2::runtime::AnyObject = msg_send![view, layer];
            for border in [false, true] {
                let cg: *const std::ffi::c_void = if border {
                    msg_send![layer, borderColor]
                } else {
                    msg_send![layer, backgroundColor]
                };
                let color: Retained<NSColor> = msg_send![NSColor::class(), colorWithCGColor: cg];
                assert_color(&color, expected);
            }
            let field = view.downcast_ref::<NSTextField>().unwrap();
            assert_foreground(&field.attributedStringValue(), expected);
        }));
    }
    fn assert_foreground(text: &objc2_foundation::NSAttributedString, expected: [f64; 4]) {
        let key = NSString::from_str("NSColor");
        let color: Retained<NSColor> = unsafe {
            msg_send![text, attribute: &*key, atIndex: 0usize,
                effectiveRange: std::ptr::null_mut::<objc2_foundation::NSRange>()]
        };
        assert_color(&color, expected);
    }
    fn assert_color(color: &NSColor, expected: [f64; 4]) {
        let rgb = color
            .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
            .unwrap();
        let actual = [
            rgb.redComponent(),
            rgb.greenComponent(),
            rgb.blueComponent(),
            rgb.alphaComponent(),
        ];
        for (a, e) in actual.into_iter().zip(expected) {
            assert!(
                (a - e).abs() < 0.02,
                "actual {actual:?}, expected {expected:?}"
            );
        }
    }
}
#[cfg(not(target_os = "macos"))]
fn main() {}
