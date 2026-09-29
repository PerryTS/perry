// TextField and SecureField draw through Perry's inset cell. The cell must
// keep the behaviour of the cell that `textFieldWithString:` builds:
// - the idle text sits where the field editor draws it, for every font and
//   control size (#11661);
// - a long value stays on one line (#10155);
// - `setPadding` moves the text by the top and left insets (#9954);
// - `textfieldSetBackgroundColor` fills the whole field, padding included, as
//   a CSS background does.
//
// AppKit must run on the process main thread, so this test has no Rust harness.
#[cfg(target_os = "macos")]
fn main() {
    use objc2::rc::Retained;
    use objc2::{ClassType, MainThreadOnly};
    use objc2_app_kit::{
        NSAppearance, NSAppearanceCustomization, NSApplication, NSBackingStoreType, NSColor,
        NSControlSize, NSFont, NSTextField, NSTextView, NSView, NSWindow, NSWindowStyleMask,
    };
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_foundation::{MainThreadMarker, NSRange, NSString};
    use perry_ui_macos::widgets;

    /// How the test styles the field's border.
    #[derive(Debug, Clone, Copy)]
    enum Border {
        /// `textfieldSetBorderless(f, 1)`.
        Borderless,
        /// The bezel that `textFieldWithString:` builds.
        Default,
        /// `textfieldSetBorderless(f, 0)`.
        Bordered,
    }

    struct Case {
        secure: bool,
        border: Border,
        family: &'static str,
        size: f64,
        control_size: NSControlSize,
        text: &'static str,
        padding: Option<(f64, f64)>,
    }

    /// The ink bounds of the idle field and of the same field while editing.
    struct Drawn {
        idle: Ink,
        editing: Ink,
        line_height: f64,
    }

    #[derive(Debug, Clone, Copy)]
    struct Ink {
        top: f64,
        left: f64,
        bottom: f64,
        /// The size of one bitmap pixel, in points.
        pixel: f64,
    }

    if std::env::args().any(|arg| arg == "--list") {
        println!("native_textfield_cell: test");
        return;
    }
    let mtm = MainThreadMarker::new().expect("TextField cell test runs on the main thread");
    let _app = NSApplication::sharedApplication(mtm);

    let long = "Hxg assignee = currentUser() AND statusCategory != Done AND project = SU";
    let mut cases = Vec::new();
    for secure in [false, true] {
        for border in [Border::Borderless, Border::Default, Border::Bordered] {
            for (family, size, control_size) in [
                ("Helvetica", 16.0, NSControlSize::Regular),
                ("Helvetica", 28.0, NSControlSize::Regular),
                ("Helvetica", 10.0, NSControlSize::Regular),
                ("Menlo", 16.0, NSControlSize::Small),
                ("Times", 20.0, NSControlSize::Regular),
            ] {
                cases.push(Case {
                    secure,
                    border,
                    family,
                    size,
                    control_size,
                    text: "Hxg",
                    padding: None,
                });
            }
            cases.push(Case {
                secure,
                border,
                family: "Helvetica",
                size: 16.0,
                control_size: NSControlSize::Regular,
                text: long,
                padding: None,
            });
            cases.push(Case {
                secure,
                border,
                family: "Helvetica",
                size: 16.0,
                control_size: NSControlSize::Regular,
                text: "Hxg",
                padding: Some((6.0, 10.0)),
            });
        }
    }

    let mut failures = Vec::new();
    for case in &cases {
        let name = format!(
            "{} {:?} {} {} {:?} {:?}{}",
            if case.secure {
                "SecureField"
            } else {
                "TextField"
            },
            case.border,
            case.family,
            case.size,
            case.control_size,
            case.text,
            case.padding
                .map_or(String::new(), |p| format!(" padded {p:?}")),
        );
        let drawn = draw(case, mtm);
        println!("{name}: idle {:?}, editing {:?}", drawn.idle, drawn.editing);
        let tolerance = drawn.idle.pixel + 0.01;

        // The field editor scrolls a value wider than the field 2pt left, as
        // it does in a stock NSTextField, so only a value that fits keeps its
        // left edge.
        let fits = case.text != long;
        if (drawn.idle.top - drawn.editing.top).abs() > tolerance
            || fits && (drawn.idle.left - drawn.editing.left).abs() > tolerance
        {
            failures.push(format!("{name}: text moves when editing starts"));
        }
        for (state, ink) in [("idle", drawn.idle), ("editing", drawn.editing)] {
            if ink.bottom - ink.top > drawn.line_height {
                failures.push(format!("{name}: {state} text wraps past one line"));
            }
        }
        if let Some((top, left)) = case.padding {
            let unpadded = draw(
                &Case {
                    padding: None,
                    ..*case
                },
                mtm,
            );
            if (drawn.idle.top - unpadded.idle.top - top).abs() > tolerance
                || (drawn.idle.left - unpadded.idle.left - left).abs() > tolerance
            {
                failures.push(format!(
                    "{name}: padding moved the text by ({}, {})",
                    drawn.idle.top - unpadded.idle.top,
                    drawn.idle.left - unpadded.idle.left
                ));
            }
        }
    }
    for secure in [false, true] {
        let empty = perry_runtime::string::js_string_from_bytes(b"".as_ptr(), 0);
        let handle = if secure {
            widgets::securefield::create(empty.cast(), 0.0)
        } else {
            widgets::textfield::create(empty.cast(), 0.0)
        };
        widgets::set_edge_insets(handle, 6.0, 10.0, 0.0, 0.0);
        widgets::textfield::set_background_color(handle, 1.0, 1.0, 0.0, 1.0);
        let view = widgets::get_widget(handle).unwrap();
        let field = unsafe { &*(Retained::as_ptr(&view) as *const NSTextField) };
        // The cell's background would fill only the text area inside the
        // padding; the layer's fills the whole field.
        let layer_color: *const perry_ui_macos::srgb::CGColor = unsafe {
            let layer: *mut objc2::runtime::AnyObject = objc2::msg_send![field, layer];
            assert!(!layer.is_null(), "the field is layer-backed");
            objc2::msg_send![layer, backgroundColor]
        };
        let color: Option<Retained<NSColor>> =
            unsafe { objc2::msg_send![NSColor::class(), colorWithCGColor: layer_color] };
        let rgba = color
            .and_then(|c| c.colorUsingColorSpace(&objc2_app_kit::NSColorSpace::sRGBColorSpace()))
            .map(|c| {
                (
                    c.redComponent(),
                    c.greenComponent(),
                    c.blueComponent(),
                    c.alphaComponent(),
                )
            });
        if field.drawsBackground() || rgba != Some((1.0, 1.0, 0.0, 1.0)) {
            failures.push(format!(
                "{}: background drawsBackground={} layer={rgba:?}",
                if secure { "SecureField" } else { "TextField" },
                field.drawsBackground()
            ));
        }
    }

    assert!(failures.is_empty(), "{failures:#?}");
    println!("PASS native TextField cell");

    fn draw(case: &Case, mtm: MainThreadMarker) -> Drawn {
        let empty = perry_runtime::string::js_string_from_bytes(b"".as_ptr(), 0);
        let handle = if case.secure {
            widgets::securefield::create(empty.cast(), 0.0)
        } else {
            widgets::textfield::create(empty.cast(), 0.0)
        };
        match case.border {
            Border::Borderless => widgets::textfield::set_borderless(handle, 1.0),
            Border::Default => {}
            Border::Bordered => widgets::textfield::set_borderless(handle, 0.0),
        }
        widgets::textfield::set_text_str(handle, case.text);
        if let Some((top, left)) = case.padding {
            widgets::set_edge_insets(handle, top, left, 0.0, 0.0);
        }
        let view = widgets::get_widget(handle).unwrap();
        let field = unsafe { &*(Retained::as_ptr(&view) as *const NSTextField) };
        let font = NSFont::fontWithName_size(&NSString::from_str(case.family), case.size)
            .expect("the font is installed");
        field.setFont(Some(&font));
        field.setControlSize(case.control_size);
        field.setTextColor(Some(&NSColor::blackColor()));
        field.setDrawsBackground(true);
        field.setBackgroundColor(Some(&NSColor::whiteColor()));
        let field_frame = CGRect::new(CGPoint::new(20.0, 20.0), CGSize::new(200.0, 70.0));
        field.setFrame(field_frame);

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
        // A bezel draws its own background and ignores the white one set
        // above, so in dark mode it would read as ink across the whole field.
        let light = NSAppearance::appearanceNamed(&NSString::from_str("NSAppearanceNameAqua"));
        window.setAppearance(light.as_deref());
        let content = window.contentView().unwrap();
        content.addSubview(&view);

        let idle = ink(&content, field_frame);
        window.makeFirstResponder(Some(field));
        let editor = field
            .currentEditor()
            .expect("the focused field has an editor");
        let editor = unsafe { &*(Retained::as_ptr(&editor) as *const NSTextView) };
        editor.setSelectedRange(NSRange::new(0, 0));
        // A black caret would count as ink and move the editing bounds.
        editor.setInsertionPointColor(Some(&NSColor::whiteColor()));
        let editing = ink(&content, field_frame);
        window.close();

        Drawn {
            idle,
            editing,
            line_height: font.ascender() - font.descender() + font.leading() + 2.0,
        }
    }

    /// The bounds of the dark ink inside `rect`, in points from its top-left.
    fn ink(view: &NSView, rect: CGRect) -> Ink {
        let bitmap = view.bitmapImageRepForCachingDisplayInRect(rect).unwrap();
        view.cacheDisplayInRect_toBitmapImageRep(rect, &bitmap);
        let scale = bitmap.pixelsHigh() as f64 / rect.size.height;
        let mut bounds = (isize::MAX, isize::MAX, -1);
        for y in 0..bitmap.pixelsHigh() {
            for x in 0..bitmap.pixelsWide() {
                let color = bitmap.colorAtX_y(x, y).unwrap();
                if color.alphaComponent() > 0.5 && color.brightnessComponent() < 0.5 {
                    bounds.0 = bounds.0.min(y);
                    bounds.1 = bounds.1.min(x);
                    bounds.2 = bounds.2.max(y);
                }
            }
        }
        assert!(bounds.2 >= 0, "the field draws no text");
        Ink {
            top: bounds.0 as f64 / scale,
            left: bounds.1 as f64 / scale,
            bottom: bounds.2 as f64 / scale,
            pixel: 1.0 / scale,
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {}
