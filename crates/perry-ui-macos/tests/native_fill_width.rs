// A TextField, SecureField or Text that matches its stack's width fills that
// width exactly, whatever it draws (#11739). AppKit gives a label-shaped text
// field 2pt alignment rect insets on each side, so if a widget kept them, its
// frame, and the layer border and background drawn on it, would overhang the
// stack by 2pt on each side.
//
// A Text draws its text where a stock label aligned to the same rect draws
// it, and hugs its text at the stock label's width.
//
// AppKit must run on the process main thread, so this test has no Rust harness.
#[cfg(target_os = "macos")]
fn main() {
    use objc2::rc::Retained;
    use objc2::MainThreadOnly;
    use objc2_app_kit::{
        NSAppearance, NSAppearanceCustomization, NSApplication, NSBackingStoreType, NSColor,
        NSFont, NSTextField, NSView, NSWindow, NSWindowStyleMask,
    };
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_foundation::{MainThreadMarker, NSString};
    use perry_ui_macos::widgets;

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Kind {
        TextField,
        SecureField,
        Text,
    }

    if std::env::args().any(|arg| arg == "--list") {
        println!("native_fill_width: test");
        return;
    }
    let mtm = MainThreadMarker::new().expect("fill width test runs on the main thread");
    let _app = NSApplication::sharedApplication(mtm);
    let host = NSView::new(mtm);
    host.setFrameSize(CGSize::new(400.0, 400.0));

    let mut failures = Vec::new();
    for kind in [Kind::TextField, Kind::SecureField, Kind::Text] {
        for borderless in [true, false] {
            if kind == Kind::Text && !borderless {
                continue;
            }
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

                let text = perry_runtime::string::js_string_from_bytes(b"Hxg".as_ptr(), 3);
                let widget = match kind {
                    Kind::TextField => widgets::textfield::create(text.cast(), 0.0),
                    Kind::SecureField => widgets::securefield::create(text.cast(), 0.0),
                    Kind::Text => widgets::text::create(text.cast()),
                };
                if kind != Kind::Text {
                    widgets::textfield::set_borderless(widget, if borderless { 1.0 } else { 0.0 });
                }
                if background {
                    match kind {
                        Kind::Text => widgets::set_background_color(widget, 1.0, 1.0, 1.0, 1.0),
                        _ => widgets::textfield::set_background_color(widget, 1.0, 1.0, 1.0, 1.0),
                    }
                }
                widgets::set_border_color(widget, 0.0, 0.0, 0.0, 1.0);
                widgets::set_border_width(widget, 1.0);
                widgets::add_child(column, widget);
                widgets::match_parent_width(widget);
                widgets::set_height(widget, 32.0);
                host.layoutSubtreeIfNeeded();

                let frame = widgets::get_widget(widget).unwrap().frame();
                let actual = (frame.origin.x, frame.size.width);
                let name = format!("{kind:?} borderless={borderless} background={background}");
                println!("{name}: x {}, width {}", actual.0, actual.1);
                if actual != (0.0, 300.0) {
                    failures.push(format!("{name}: expected (0, 300), got {actual:?}"));
                }
                column_view.removeFromSuperview();
            }
        }
    }

    for (family, size) in [("Helvetica", 16.0), ("Helvetica", 40.0), ("Menlo", 9.0)] {
        for selectable in [false, true] {
            let name = format!("Text {family} {size} selectable={selectable}");
            let text = perry_runtime::string::js_string_from_bytes(b"Hxg".as_ptr(), 3);
            let handle = widgets::text::create(text.cast());
            widgets::text::set_selectable(handle, selectable);
            let perry_view = widgets::get_widget(handle).unwrap();
            let perry = unsafe { &*(Retained::as_ptr(&perry_view) as *const NSTextField) };
            let stock = NSTextField::labelWithString(&NSString::from_str("Hxg"), mtm);
            stock.setSelectable(selectable);
            let font = NSFont::fontWithName_size(&NSString::from_str(family), size)
                .expect("the font is installed");
            for label in [perry, &*stock] {
                label.setFont(Some(&font));
                label.setTextColor(Some(&NSColor::blackColor()));
            }

            let widths = (
                perry.intrinsicContentSize().width,
                stock.intrinsicContentSize().width,
            );
            if widths.0 != widths.1 {
                failures.push(format!("{name}: intrinsic width {widths:?} (Perry, stock)"));
            }

            let drawn = (
                ink_left(perry, selectable, mtm),
                ink_left(&stock, selectable, mtm),
            );
            println!("{name}: ink left {drawn:?} (Perry, stock)");
            for ((perry, stock), state) in [(drawn.0 .0, drawn.1 .0), (drawn.0 .1, drawn.1 .1)]
                .into_iter()
                .zip(["idle", "selected"])
            {
                if (perry - stock).abs() > 0.01 {
                    failures.push(format!(
                        "{name}: {state} text at {perry}, a stock label's at {stock}"
                    ));
                }
            }
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
    println!("PASS native TextField, SecureField and Text fill the stack width");

    /// Aligns `label` to the same rect in a window and returns the left edge
    /// of its ink, in points from that rect, idle and after it is selected.
    fn ink_left(label: &NSTextField, selectable: bool, mtm: MainThreadMarker) -> (f64, f64) {
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(240.0, 110.0)),
                NSWindowStyleMask::Titled,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        let light = NSAppearance::appearanceNamed(&NSString::from_str("NSAppearanceNameAqua"));
        window.setAppearance(light.as_deref());
        let content = window.contentView().unwrap();
        content.addSubview(label);
        let aligned = CGRect::new(CGPoint::new(20.0, 20.0), CGSize::new(200.0, 70.0));
        label.setFrame(label.frameForAlignmentRect(aligned));

        let idle = ink(&content, aligned);
        let selected = if selectable {
            window.makeKeyAndOrderFront(None);
            unsafe { label.selectText(None) };
            label
                .currentEditor()
                .expect("the selected label has an editor")
                .setSelectedRange(objc2_foundation::NSRange::new(0, 0));
            ink(&content, aligned)
        } else {
            idle
        };
        label.removeFromSuperview();
        window.close();
        (idle, selected)
    }

    /// The left edge of the dark ink inside `rect`, in points from its left.
    fn ink(view: &NSView, rect: CGRect) -> f64 {
        let bitmap = view.bitmapImageRepForCachingDisplayInRect(rect).unwrap();
        view.cacheDisplayInRect_toBitmapImageRep(rect, &bitmap);
        let scale = bitmap.pixelsHigh() as f64 / rect.size.height;
        let left = (0..bitmap.pixelsHigh())
            .flat_map(|y| (0..bitmap.pixelsWide()).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let color = bitmap.colorAtX_y(x, y).unwrap();
                color.alphaComponent() > 0.5 && color.brightnessComponent() < 0.5
            })
            .map(|(x, _)| x)
            .min()
            .expect("the label draws its text");
        left as f64 / scale
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {}
