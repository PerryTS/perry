#[cfg(target_os = "macos")]
fn main() {
    use objc2::rc::Retained;
    use objc2_app_kit::{NSApplication, NSStackView, NSView};
    use objc2_core_foundation::CGSize;
    use objc2_foundation::MainThreadMarker;
    use perry_runtime as _;
    use perry_ui_macos::{perry_ui_widget_set_max_width, widgets};

    if std::env::args().any(|arg| arg == "--list") {
        println!("native_widget_max_width: test");
        return;
    }
    let mtm = MainThreadMarker::new().unwrap();
    let _app = NSApplication::sharedApplication(mtm);
    let host = NSView::new(mtm);
    host.setFrameSize(CGSize::new(1400.0, 400.0));

    for before_insertion in [true, false] {
        let parent = widgets::vstack::create_with_insets(0.0, 0.0, 20.0, 0.0, 40.0);
        let column = widgets::vstack::create_with_insets(0.0, 0.0, 32.0, 0.0, 32.0);
        let label_text = b"Content";
        let string = perry_runtime::string::js_string_from_bytes(
            label_text.as_ptr(),
            label_text.len() as u32,
        );
        let label = widgets::text::create(string.cast());
        widgets::add_child(column, label);
        let parent_view = widgets::get_widget(parent).unwrap();
        parent_view.setTranslatesAutoresizingMaskIntoConstraints(false);
        host.addSubview(&parent_view);
        let width = parent_view.widthAnchor().constraintEqualToConstant(1200.0);
        width.setActive(true);
        parent_view
            .heightAnchor()
            .constraintEqualToConstant(200.0)
            .setActive(true);
        parent_view
            .leadingAnchor()
            .constraintEqualToAnchor(&host.leadingAnchor())
            .setActive(true);
        parent_view
            .topAnchor()
            .constraintEqualToAnchor(&host.topAnchor())
            .setActive(true);

        let original = widgets::get_widget(column).unwrap();
        widgets::set_width(column, 900.0);
        if before_insertion {
            perry_ui_widget_set_max_width(column, 640.0);
        }
        widgets::add_child(parent, column);
        if !before_insertion {
            host.layoutSubtreeIfNeeded();
            perry_ui_widget_set_max_width(column, 640.0);
        }
        assert_eq!(
            Retained::as_ptr(&widgets::get_widget(column).unwrap()),
            Retained::as_ptr(&original)
        );

        let check = |parent_width: f64, cap: f64| {
            width.setConstant(parent_width);
            host.layoutSubtreeIfNeeded();
            let frame = original.convertRect_toView(original.bounds(), Some(&parent_view));
            let insets = parent_view
                .downcast_ref::<NSStackView>()
                .unwrap()
                .edgeInsets();
            let available = parent_width - insets.left - insets.right;
            near(frame.size.width, available.min(cap), "width");
            near(
                frame.origin.x,
                insets.left + (available - available.min(cap)) / 2.0,
                "center",
            );
            let label_view = widgets::get_widget(label).unwrap();
            let label_frame = label_view.convertRect_toView(label_view.bounds(), Some(&original));
            // NSTextField frames extend two points beyond their alignment rect.
            let aligned = label_view.alignmentRectForFrame(label_frame);
            near(aligned.origin.x, 32.0, "content padding");
        };
        for size in [1200.0, 700.0, 400.0, 1200.0] {
            check(size, 640.0);
        }
        perry_ui_widget_set_max_width(column, 480.0);
        perry_ui_widget_set_max_width(column, 480.0);
        check(1200.0, 480.0);
        perry_ui_widget_set_max_width(column, f64::NAN);
        perry_ui_widget_set_max_width(column, -1.0);
        check(400.0, 480.0);

        // Fill-aligned stacks must not force capped content to become full-width.
        widgets::set_alignment(parent, 7);
        check(1200.0, 480.0);
        widgets::set_hidden(column, true);
        host.layoutSubtreeIfNeeded();
        widgets::set_hidden(column, false);
        check(1200.0, 480.0);

        widgets::set_edge_insets(parent, 0.0, 30.0, 0.0, 50.0);
        check(400.0, 480.0);
        widgets::set_width(column, 900.0);
        host.layoutSubtreeIfNeeded();
        near(
            original.frame().size.width,
            900.0,
            "fixed width overrides cap",
        );
        perry_ui_widget_set_max_width(column, 480.0);
        check(400.0, 480.0);

        // Moving a capped widget retains the cap and binds to the new parent.
        let other = widgets::vstack::create(0.0);
        let other_view = widgets::get_widget(other).unwrap();
        other_view.setTranslatesAutoresizingMaskIntoConstraints(false);
        host.addSubview(&other_view);
        widgets::set_width(other, 300.0);
        widgets::set_height(other, 200.0);
        widgets::add_child(other, column);
        host.layoutSubtreeIfNeeded();
        near(original.frame().size.width, 300.0, "reparented width");
        widgets::add_child(parent, column);
        check(1200.0, 480.0);
        other_view.removeFromSuperview();

        widgets::clear_children(parent);
        widgets::add_child(parent, column);
        check(1200.0, 480.0);
        let stack = parent_view.downcast_ref::<NSStackView>().unwrap();
        assert_eq!(stack.arrangedSubviews().len(), 1);
        widgets::remove_child(parent, column);
        assert!(stack.arrangedSubviews().is_empty());
        parent_view.removeFromSuperview();
    }
    containers::check(mtm);
    println!(
        "PASS native max-width resizing, padding, updates, insertion, alignment and visibility"
    );

    fn near(actual: f64, expected: f64, label: &str) {
        assert!(
            (actual - expected).abs() < 0.51,
            "{label}: expected {expected}, got {actual}"
        );
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
#[path = "native_widget_max_width/containers.rs"]
mod containers;
