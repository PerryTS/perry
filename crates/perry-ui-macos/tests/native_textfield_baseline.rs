// A TextField must draw its text at the same height whether or not it is
// editing (#11661). The field editor draws on the font's own baseline, so the
// idle cell must too, for every font and control size.
//
// AppKit must run on the process main thread, so this test has no Rust harness.
#[cfg(target_os = "macos")]
fn main() {
    use objc2::rc::Retained;
    use objc2::MainThreadOnly;
    use objc2_app_kit::{
        NSApplication, NSBackingStoreType, NSColor, NSControlSize, NSFont, NSTextField, NSTextView,
        NSView, NSWindow, NSWindowStyleMask,
    };
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_foundation::{MainThreadMarker, NSRange, NSString};
    use perry_ui_macos::widgets;

    if std::env::args().any(|arg| arg == "--list") {
        println!("native_textfield_baseline: test");
        return;
    }
    let mtm = MainThreadMarker::new().expect("TextField baseline test runs on the main thread");
    let _app = NSApplication::sharedApplication(mtm);
    let empty = perry_runtime::string::js_string_from_bytes(b"".as_ptr(), 0);

    let cases = [
        ("Helvetica", 16.0, NSControlSize::Regular),
        ("Helvetica", 28.0, NSControlSize::Regular),
        ("Helvetica", 10.0, NSControlSize::Regular),
        ("Menlo", 16.0, NSControlSize::Small),
        ("Times", 20.0, NSControlSize::Regular),
    ];
    let mut failures = Vec::new();
    for secure in [false, true] {
        for (family, size, control_size) in cases {
            let handle = if secure {
                widgets::securefield::create(empty.cast(), 0.0)
            } else {
                widgets::textfield::create(empty.cast(), 0.0)
            };
            widgets::textfield::set_borderless(handle, 1.0);
            widgets::textfield::set_text_str(handle, "Hxg");
            let view = widgets::get_widget(handle).unwrap();
            let field = unsafe { &*(Retained::as_ptr(&view) as *const NSTextField) };
            let font = NSFont::fontWithName_size(&NSString::from_str(family), size)
                .expect("the font is installed");
            field.setFont(Some(&font));
            field.setControlSize(control_size);
            field.setTextColor(Some(&NSColor::blackColor()));
            field.setDrawsBackground(true);
            field.setBackgroundColor(Some(&NSColor::whiteColor()));
            let field_frame = CGRect::new(CGPoint::new(20.0, 20.0), CGSize::new(200.0, 60.0));
            field.setFrame(field_frame);

            let window = unsafe {
                NSWindow::initWithContentRect_styleMask_backing_defer(
                    NSWindow::alloc(mtm),
                    CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(240.0, 100.0)),
                    NSWindowStyleMask::Titled,
                    NSBackingStoreType::Buffered,
                    false,
                )
            };
            unsafe { window.setReleasedWhenClosed(false) };
            let content = window.contentView().unwrap();
            content.addSubview(&view);

            let idle = ink_top(&content, field_frame);
            window.makeFirstResponder(Some(field));
            let editor = field
                .currentEditor()
                .expect("the focused field has an editor");
            let editor = unsafe { &*(Retained::as_ptr(&editor) as *const NSTextView) };
            editor.setSelectedRange(NSRange::new(3, 0));
            editor.setInsertionPointColor(Some(&NSColor::whiteColor()));
            content.display();
            let editing = ink_top(&content, field_frame);
            window.close();

            let name = format!(
                "{} {family} {size} {control_size:?}",
                if secure { "SecureField" } else { "TextField" }
            );
            println!("{name}: idle top {idle}, editing top {editing}");
            if (idle - editing).abs() > 0.51 {
                failures.push(format!("{name}: idle {idle} vs editing {editing}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "text moves when editing starts: {failures:#?}"
    );
    println!("PASS native TextField baseline");

    // The top of the dark ink inside `rect`, in points below its top edge.
    fn ink_top(view: &NSView, rect: CGRect) -> f64 {
        let bitmap = view.bitmapImageRepForCachingDisplayInRect(rect).unwrap();
        view.cacheDisplayInRect_toBitmapImageRep(rect, &bitmap);
        let scale = bitmap.pixelsHigh() as f64 / rect.size.height;
        for y in 0..bitmap.pixelsHigh() {
            for x in 0..bitmap.pixelsWide() {
                let color = bitmap.colorAtX_y(x, y).unwrap();
                if color.alphaComponent() > 0.5 && color.brightnessComponent() < 0.5 {
                    return y as f64 / scale;
                }
            }
        }
        panic!("the field draws no text");
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {}
