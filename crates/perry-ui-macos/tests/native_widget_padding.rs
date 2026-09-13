#[cfg(target_os = "macos")]
fn main() {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSApplication, NSButton, NSButtonCell, NSTextField, NSView};
    use objc2_core_foundation::CGSize;
    use objc2_foundation::{MainThreadMarker, NSString};
    use perry_ui_macos::widgets;

    if std::env::args().any(|arg| arg == "--list") {
        println!("native_widget_padding: test");
        return;
    }
    let mtm = MainThreadMarker::new().expect("native widget test runs on the main thread");
    let _app = NSApplication::sharedApplication(mtm);
    let text = "Padding";
    let string = perry_runtime::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    let label = widgets::text::create(string.cast());
    let button = widgets::button::create(string.cast(), 0.0);
    let button_view = widgets::get_widget(button).unwrap();
    let native_button = unsafe { &*(Retained::as_ptr(&button_view) as *const NSButton) };
    let factory_button = unsafe {
        NSButton::buttonWithTitle_target_action(&NSString::from_str(text), None, None, mtm)
    };
    assert_eq!(
        native_button.intrinsicContentSize(),
        factory_button.intrinsicContentSize()
    );
    assert_eq!(native_button.isBordered(), factory_button.isBordered());
    assert_eq!(native_button.bezelStyle(), factory_button.bezelStyle());
    assert_eq!(native_button.title(), factory_button.title());
    let cell = native_button.cell().unwrap();
    let factory_cell = factory_button.cell().unwrap();
    let cell = unsafe { &*(Retained::as_ptr(&cell) as *const NSButtonCell) };
    let factory_cell = unsafe { &*(Retained::as_ptr(&factory_cell) as *const NSButtonCell) };
    assert_eq!(cell.highlightsBy(), factory_cell.highlightsBy());
    assert_eq!(cell.showsStateBy(), factory_cell.showsStateBy());
    let target: *mut objc2::runtime::AnyObject = unsafe { msg_send![native_button, target] };
    let action = native_button.action();
    assert!(!target.is_null() && action.is_some());
    let bordered_size = native_button.intrinsicContentSize();
    widgets::set_edge_insets(button, 3.0, 5.0, 7.0, 11.0);
    assert_eq!(
        native_button.intrinsicContentSize(),
        CGSize::new(bordered_size.width + 16.0, bordered_size.height + 10.0)
    );
    ink_bounds(
        native_button,
        native_button.intrinsicContentSize(),
        "bordered-padded",
    );
    widgets::set_edge_insets(button, 0.0, 0.0, 0.0, 0.0);
    assert_eq!(native_button.intrinsicContentSize(), bordered_size);
    assert_eq!(native_button.bezelStyle(), factory_button.bezelStyle());
    let label_view = widgets::get_widget(label).unwrap();
    let native_label = unsafe { &*(Retained::as_ptr(&label_view) as *const NSTextField) };
    let factory_label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    assert_eq!(
        native_label.intrinsicContentSize(),
        factory_label.intrinsicContentSize()
    );
    assert_eq!(native_label.stringValue(), factory_label.stringValue());
    assert_eq!(native_label.isEditable(), factory_label.isEditable());
    assert_eq!(native_label.isSelectable(), factory_label.isSelectable());
    assert_eq!(
        native_label.drawsBackground(),
        factory_label.drawsBackground()
    );
    let attributed = widgets::attributed_text::create();
    widgets::attributed_text::append(attributed, string.cast(), 1, 0, 1, 18.0, 0.0, 0.0, 0.0, 1.0);
    widgets::button::set_bordered(button, false);
    let mut failures = Vec::new();
    for (name, handle, layer) in [
        ("label", label, false),
        ("button", button, false),
        ("label-layer", label, true),
        ("button-layer", button, true),
        ("attributed", attributed, true),
    ] {
        let view = widgets::get_widget(handle).unwrap();
        view.setWantsLayer(layer);
        let before: CGSize = unsafe { msg_send![&*view, intrinsicContentSize] };
        let ink_before = ink_bounds(&view, before, &format!("{name}-before"));
        widgets::set_edge_insets(handle, 3.0, 5.0, 7.0, 11.0);
        let after: CGSize = unsafe { msg_send![&*view, intrinsicContentSize] };
        widgets::set_edge_insets(handle, 3.0, 5.0, 7.0, 11.0);
        assert_eq!(
            view.intrinsicContentSize(),
            after,
            "padding is replaced, never accumulated"
        );
        let ink_after = ink_bounds(&view, after, &format!("{name}-after"));
        println!("{name}: {before:?} -> {after:?}");
        println!(
            "{name} ink: {ink_before:?} -> {ink_after:?}, flipped={}",
            view.isFlipped()
        );
        for (actual, expected) in [
            (ink_after.0 - ink_before.0, 5.0),
            (ink_after.1 - ink_before.1, 3.0),
            (ink_after.2 - ink_before.2, 5.0),
            (ink_after.3 - ink_before.3, 3.0),
        ] {
            assert!(
                (actual - expected).abs() < 0.51,
                "{name}: content must move by top/left padding without being clipped"
            );
        }
        if (after.width - before.width - 16.0).abs() > 0.01
            || (after.height - before.height - 10.0).abs() > 0.01
        {
            failures.push(name);
        }
        widgets::set_edge_insets(handle, 0.0, 0.0, 0.0, 0.0);
        let reset: CGSize = unsafe { msg_send![&*view, intrinsicContentSize] };
        assert_eq!(reset, before, "resetting {name} padding restores its size");
    }
    assert!(failures.is_empty(), "padding ignored by {failures:?}");
    let target_after: *mut objc2::runtime::AnyObject = unsafe { msg_send![native_button, target] };
    assert_eq!(target_after, target);
    assert_eq!(native_button.action(), action);
    drag_drop::check(mtm);
    println!("PASS native widget padding");

    fn ink_bounds(view: &NSView, size: CGSize, name: &str) -> (f64, f64, f64, f64) {
        use objc2_app_kit::NSBitmapImageFileType;
        use objc2_foundation::NSDictionary;
        view.setFrameSize(CGSize::new(size.width.ceil(), size.height.ceil()));
        let rect = view.bounds();
        let bitmap = view.bitmapImageRepForCachingDisplayInRect(rect).unwrap();
        view.cacheDisplayInRect_toBitmapImageRep(rect, &bitmap);
        if let Ok(dir) = std::env::var("PERRY_PADDING_SNAPSHOTS") {
            std::fs::create_dir_all(&dir).unwrap();
            let png = unsafe {
                bitmap.representationUsingType_properties(
                    NSBitmapImageFileType::PNG,
                    &NSDictionary::new(),
                )
            }
            .unwrap();
            std::fs::write(
                std::path::Path::new(&dir).join(format!("{name}.png")),
                png.to_vec(),
            )
            .unwrap();
        }
        let mut bounds = (isize::MAX, isize::MAX, -1, -1);
        for y in 0..bitmap.pixelsHigh() {
            for x in 0..bitmap.pixelsWide() {
                if bitmap.colorAtX_y(x, y).unwrap().alphaComponent() > 0.1 {
                    bounds.0 = bounds.0.min(x);
                    bounds.1 = bounds.1.min(y);
                    bounds.2 = bounds.2.max(x);
                    bounds.3 = bounds.3.max(y);
                }
            }
        }
        assert!(bounds.2 >= 0, "{name}: rendered content must be visible");
        let scale_x = bitmap.pixelsWide() as f64 / rect.size.width;
        let scale_y = bitmap.pixelsHigh() as f64 / rect.size.height;
        (
            bounds.0 as f64 / scale_x,
            bounds.1 as f64 / scale_y,
            bounds.2 as f64 / scale_x,
            bounds.3 as f64 / scale_y,
        )
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {}
#[cfg(target_os = "macos")]
#[path = "native_widget_padding/drag_drop.rs"]
mod drag_drop;
