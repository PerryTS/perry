// A DatePicker that matches its stack's width fills that width exactly, so a
// layer border or background drawn on its frame stays inside the stack
// (#11966). At the regular control size, AppKit gives a stock NSDatePicker
// alignment rect insets of top 4 and right 3, so if the picker kept them, its
// frame would overhang the stack.
//
// The picker still draws its field, stepper, and focus ring where a stock
// picker aligned to the same rect draws them, so the visible control lines up
// with the column.
//
// AppKit must run on the process main thread, so this test has no Rust harness.
#[cfg(target_os = "macos")]
fn main() {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::{msg_send, MainThreadOnly};
    use objc2_app_kit::{
        NSAppearance, NSAppearanceCustomization, NSApplication, NSBackingStoreType,
        NSBitmapImageRep, NSView, NSWindow, NSWindowStyleMask,
    };
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_foundation::{MainThreadMarker, NSString};
    use perry_runtime as _;
    use perry_ui_macos::widgets;

    if std::env::args().any(|arg| arg == "--list") {
        println!("native_date_picker_fill_width: test");
        return;
    }
    let mtm = MainThreadMarker::new().expect("date picker test runs on the main thread");
    let _app = NSApplication::sharedApplication(mtm);
    let mut failures = Vec::new();

    let stock =
        stock_picker(widgets::get_widget(widgets::date_picker::create(2026, 1, 0.0)).unwrap());
    let stock_size = stock.alignmentRectForFrame(stock.frame()).size;

    let host = NSView::new(mtm);
    host.setFrameSize(CGSize::new(400.0, 400.0));
    for decorated in [false, true] {
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

        let picker = widgets::date_picker::create(2026, 1, 0.0);
        if decorated {
            widgets::set_background_color(picker, 1.0, 0.85, 0.4, 1.0);
            widgets::set_border_color(picker, 0.8, 0.0, 0.0, 1.0);
            widgets::set_border_width(picker, 1.0);
        }
        widgets::add_child(column, picker);
        widgets::match_parent_width(picker);
        host.layoutSubtreeIfNeeded();

        let frame = widgets::get_widget(picker).unwrap().frame();
        let column_frame = column_view.frame();
        let name = format!("decorated={decorated}");
        println!("{name}: picker {frame:?} in column {column_frame:?}");
        let expected = CGRect::new(
            CGPoint::new(0.0, 0.0),
            CGSize::new(300.0, stock_size.height),
        );
        if frame != expected {
            failures.push(format!(
                "{name}: expected frame {expected:?}, got {frame:?}"
            ));
        }
        if frame.size.height != column_frame.size.height {
            failures.push(format!(
                "{name}: picker is {} high in a column {} high",
                frame.size.height, column_frame.size.height
            ));
        }
        column_view.removeFromSuperview();
    }

    let perry = widgets::get_widget(widgets::date_picker::create(2026, 1, 0.0)).unwrap();
    let perry_size = perry.intrinsicContentSize();
    if perry_size != stock_size {
        failures.push(format!(
            "intrinsic size {perry_size:?}, a stock picker's alignment rect is {stock_size:?}"
        ));
    }
    for (width, height) in [
        (stock_size.width, stock_size.height),
        (300.0, stock_size.height),
        (300.0, 72.0),
    ] {
        for drawing in [Drawing::Control, Drawing::FocusRingMask] {
            let aligned = CGRect::new(CGPoint::new(20.0, 20.0), CGSize::new(width, height));
            let drawn = (
                render(&perry, aligned, drawing, mtm),
                render(&stock, aligned, drawing, mtm),
            );
            let differing = drawn.0.iter().zip(&drawn.1).filter(|(a, b)| a != b).count();
            println!(
                "{drawing:?} {width}x{height}: {differing} of {} pixels differ from a stock picker",
                drawn.0.len()
            );
            if drawn.0 != drawn.1 {
                failures.push(format!(
                    "{drawing:?} {width}x{height}: {differing} pixels differ from a stock picker aligned to the same rect"
                ));
            }
            if drawn.1.iter().all(|pixel| pixel[3] == 0) {
                failures.push(format!(
                    "{drawing:?} {width}x{height}: a stock picker draws nothing, so the check proves nothing"
                ));
            }
        }
    }

    // Each case selects a field element, then clicks the stepper. Points are
    // (from the aligned rect's right edge, from its top).
    let initial =
        date_of(&widgets::get_widget(widgets::date_picker::create(2026, 1, 0.0)).unwrap());
    let mut stock_changed = false;
    for clicks in [
        [(65.0, 11.0), (6.0, 5.0)],
        [(65.0, 11.0), (6.0, 17.0)],
        [(65.0, 11.0), (1.5, 5.0)],
        [(40.0, 11.0), (11.0, 17.0)],
    ] {
        let aligned = CGRect::new(CGPoint::new(20.0, 20.0), stock_size);
        let points = clicks.map(|(from_right, from_top)| {
            CGPoint::new(
                aligned.origin.x + aligned.size.width - from_right,
                aligned.origin.y + aligned.size.height - from_top,
            )
        });
        let perry = widgets::get_widget(widgets::date_picker::create(2026, 1, 0.0)).unwrap();
        let stock = stock_picker(perry.clone());
        let dates = (
            click(&stock, aligned, &points, mtm),
            click(&perry, aligned, &points, mtm),
        );
        println!("clicks {clicks:?}: {dates:?} (stock, Perry)");
        stock_changed |= dates.0 != initial;
        if dates.0 != dates.1 {
            failures.push(format!(
                "clicks {clicks:?}: a stock picker shows {}, Perry {}",
                dates.0, dates.1
            ));
        }
    }
    if !stock_changed {
        failures.push(
            "no click changed a stock picker's date, so the click check proves nothing".to_owned(),
        );
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
    println!("PASS native DatePicker fills the stack width and draws as a stock picker");

    /// A stock NSDatePicker configured as `perry`, the Perry date picker.
    fn stock_picker(perry: Retained<NSView>) -> Retained<NSView> {
        unsafe {
            let cls = AnyClass::get(c"NSDatePicker").unwrap();
            let stock: Retained<NSView> = msg_send![cls, new];
            let style: u64 = msg_send![&*perry, datePickerStyle];
            let elements: u64 = msg_send![&*perry, datePickerElements];
            let date: *mut AnyObject = msg_send![&*perry, dateValue];
            let _: () = msg_send![&*stock, setDatePickerStyle: style];
            let _: () = msg_send![&*stock, setDatePickerElements: elements];
            let _: () = msg_send![&*stock, setDateValue: date];
            let size = stock.fittingSize();
            stock.setFrameSize(size);
            stock
        }
    }

    fn date_of(picker: &NSView) -> String {
        let date: Retained<AnyObject> = unsafe { msg_send![picker, dateValue] };
        let description: Retained<NSString> = unsafe { msg_send![&*date, description] };
        description.to_string()
    }

    /// Aligns `picker` to `aligned` in a key window, clicks each of `points`
    /// in the window, and returns the date that the picker then shows.
    fn click(
        picker: &NSView,
        aligned: CGRect,
        points: &[CGPoint],
        mtm: MainThreadMarker,
    ) -> String {
        use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(360.0, 140.0)),
                NSWindowStyleMask::Titled,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        let content = window.contentView().unwrap();
        picker.setTranslatesAutoresizingMaskIntoConstraints(true);
        content.addSubview(picker);
        picker.setFrame(picker.frameForAlignmentRect(aligned));
        window.makeKeyAndOrderFront(None);
        // The test creates pickers with no onChange closure, so nothing may
        // call one.
        unsafe {
            let _: () = msg_send![picker, setTarget: std::ptr::null::<AnyObject>()];
            let _: () = msg_send![picker, setAction: None::<objc2::runtime::Sel>];
        }

        let event = |kind, point| {
            NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                kind,
                point,
                NSEventModifierFlags::empty(),
                0.0,
                window.windowNumber(),
                None,
                0,
                1,
                1.0,
            )
            .unwrap()
        };
        let app = NSApplication::sharedApplication(mtm);
        for &point in points {
            app.postEvent_atStart(&event(NSEventType::LeftMouseUp, point), false);
            picker.mouseDown(&event(NSEventType::LeftMouseDown, point));
        }

        let date = date_of(picker);
        picker.removeFromSuperview();
        window.close();
        date
    }

    #[derive(Clone, Copy, Debug)]
    enum Drawing {
        /// The window content, picker included, as it displays.
        Control,
        /// The mask that AppKit outlines when the picker has keyboard focus.
        FocusRingMask,
    }

    /// Aligns `picker` to `aligned` in a window and returns the RGBA pixels of
    /// `drawing` over that rect grown by 8pt on every side, so drawing past
    /// the rect counts.
    fn render(
        picker: &NSView,
        aligned: CGRect,
        drawing: Drawing,
        mtm: MainThreadMarker,
    ) -> Vec<[u8; 4]> {
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(360.0, 140.0)),
                NSWindowStyleMask::Titled,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        let light = NSAppearance::appearanceNamed(&NSString::from_str("NSAppearanceNameAqua"));
        window.setAppearance(light.as_deref());
        let content = window.contentView().unwrap();
        picker.setTranslatesAutoresizingMaskIntoConstraints(true);
        content.addSubview(picker);
        picker.setFrame(picker.frameForAlignmentRect(aligned));

        let rect = CGRect::new(
            CGPoint::new(aligned.origin.x - 8.0, aligned.origin.y - 8.0),
            CGSize::new(aligned.size.width + 16.0, aligned.size.height + 16.0),
        );
        let bitmap = content.bitmapImageRepForCachingDisplayInRect(rect).unwrap();
        match drawing {
            Drawing::Control => content.cacheDisplayInRect_toBitmapImageRep(rect, &bitmap),
            Drawing::FocusRingMask => draw_focus_ring_mask(picker, rect, &bitmap),
        }
        let mut pixels = Vec::new();
        for y in 0..bitmap.pixelsHigh() {
            for x in 0..bitmap.pixelsWide() {
                let color = bitmap.colorAtX_y(x, y).unwrap();
                pixels.push(
                    [
                        color.redComponent(),
                        color.greenComponent(),
                        color.blueComponent(),
                        color.alphaComponent(),
                    ]
                    .map(|component| (component * 255.0).round() as u8),
                );
            }
        }
        picker.removeFromSuperview();
        window.close();
        pixels
    }

    /// Draws the focus ring mask of `picker` into `bitmap`, which covers
    /// `rect` of the picker's superview. The mask is opaque where the ring
    /// goes and clear everywhere else.
    fn draw_focus_ring_mask(picker: &NSView, rect: CGRect, bitmap: &NSBitmapImageRep) {
        use objc2_app_kit::{
            NSAffineTransformNSAppKitAdditions, NSColor, NSCompositingOperation, NSGraphicsContext,
            NSRectFillUsingOperation,
        };
        use objc2_foundation::NSAffineTransform;
        let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(bitmap).unwrap();
        NSGraphicsContext::saveGraphicsState_class();
        NSGraphicsContext::setCurrentContext(Some(&context));
        NSColor::clearColor().set();
        NSRectFillUsingOperation(
            CGRect::new(CGPoint::new(0.0, 0.0), rect.size),
            NSCompositingOperation::Copy,
        );
        let frame = picker.frame();
        let to_picker = NSAffineTransform::transform();
        to_picker.translateXBy_yBy(
            frame.origin.x - rect.origin.x,
            frame.origin.y - rect.origin.y,
        );
        to_picker.concat();
        NSColor::blackColor().set();
        picker.drawFocusRingMask();
        NSGraphicsContext::restoreGraphicsState_class();
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {}
