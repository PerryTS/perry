// A TextField or SecureField that matches its stack's width fills that width
// exactly, whatever it draws. A borderless field with no cell background is
// shaped like a label to AppKit, and a label's alignment rect insets would
// push the frame, and the layer border and background drawn on it, 2pt past
// each edge of the stack (#11739).
//
// AppKit must run on the process main thread, so this test has no Rust harness.
#[cfg(target_os = "macos")]
fn main() {
    use objc2_app_kit::{NSApplication, NSView};
    use objc2_core_foundation::CGSize;
    use objc2_foundation::MainThreadMarker;
    use perry_ui_macos::widgets;

    if std::env::args().any(|arg| arg == "--list") {
        println!("native_textfield_width: test");
        return;
    }
    let mtm = MainThreadMarker::new().expect("TextField width test runs on the main thread");
    let _app = NSApplication::sharedApplication(mtm);
    let host = NSView::new(mtm);
    host.setFrameSize(CGSize::new(400.0, 400.0));

    let mut failures = Vec::new();
    for secure in [false, true] {
        for borderless in [true, false] {
            for background in [true, false] {
                let column = widgets::vstack::create_with_insets(6.0, 0.0, 0.0, 0.0, 0.0);
                let column_view = widgets::get_widget(column).unwrap();
                column_view.setTranslatesAutoresizingMaskIntoConstraints(false);
                host.addSubview(&column_view);
                column_view
                    .leadingAnchor()
                    .constraintEqualToAnchor(&host.leadingAnchor())
                    .setActive(true);
                column_view
                    .topAnchor()
                    .constraintEqualToAnchor(&host.topAnchor())
                    .setActive(true);
                widgets::set_width(column, 300.0);

                let empty = perry_runtime::string::js_string_from_bytes(b"".as_ptr(), 0);
                let field = if secure {
                    widgets::securefield::create(empty.cast(), 0.0)
                } else {
                    widgets::textfield::create(empty.cast(), 0.0)
                };
                widgets::textfield::set_borderless(field, if borderless { 1.0 } else { 0.0 });
                if background {
                    widgets::textfield::set_background_color(field, 1.0, 1.0, 1.0, 1.0);
                }
                widgets::set_border_color(field, 0.0, 0.0, 0.0, 1.0);
                widgets::set_border_width(field, 1.0);
                widgets::add_child(column, field);
                widgets::match_parent_width(field);
                widgets::set_height(field, 32.0);
                host.layoutSubtreeIfNeeded();

                let field_view = widgets::get_widget(field).unwrap();
                let frame = field_view.frame();
                let actual = (frame.origin.x, frame.size.width);
                let name = format!(
                    "{} borderless={borderless} background={background}",
                    if secure { "SecureField" } else { "TextField" },
                );
                println!("{name}: x {}, width {}", actual.0, actual.1);
                if actual != (0.0, 300.0) {
                    failures.push(format!("{name}: expected (0, 300), got {actual:?}"));
                }
                column_view.removeFromSuperview();
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    println!("PASS native TextField and SecureField fill the stack width");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
