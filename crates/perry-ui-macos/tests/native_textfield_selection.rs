// AppKit must run on the process main thread, so this test has no Rust harness.
#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn perry_ui_textfield_set_selection_range(handle: i64, start: f64, end: f64);
    fn perry_ui_textfield_get_selection_start(handle: i64) -> f64;
    fn perry_ui_textfield_get_selection_end(handle: i64) -> f64;
}

#[cfg(target_os = "macos")]
fn main() {
    use objc2::rc::Retained;
    use objc2::MainThreadOnly;
    use objc2_app_kit::{
        NSApplication, NSBackingStoreType, NSTextField, NSWindow, NSWindowStyleMask,
    };
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_foundation::{MainThreadMarker, NSDate, NSRunLoop};
    use perry_ui_macos::widgets;

    if std::env::args().any(|arg| arg == "--list") {
        println!("native_textfield_selection: test");
        return;
    }
    let mtm = MainThreadMarker::new().expect("TextField selection test runs on the main thread");
    let _app = NSApplication::sharedApplication(mtm);
    let empty = perry_runtime::string::js_string_from_bytes(b"".as_ptr(), 0);
    let handle = widgets::textfield::create(empty.cast(), 0.0);
    widgets::textfield::set_text_str(handle, "a💙bc");

    // JS indexes UTF-16 code units, so the cursor at the end is position 5.
    unsafe { perry_ui_textfield_set_selection_range(handle, 5.0, 5.0) };
    assert_eq!(
        unsafe { perry_ui_textfield_get_selection_start(handle) },
        5.0
    );
    assert_eq!(unsafe { perry_ui_textfield_get_selection_end(handle) }, 5.0);

    let frame = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(320.0, 80.0));
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            frame,
            NSWindowStyleMask::Titled,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    let view = widgets::get_widget(handle).unwrap();
    window.setContentView(Some(&view));
    window.setInitialFirstResponder(Some(&view));
    window.makeKeyAndOrderFront(None);
    NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.02));

    let field = unsafe { &*(Retained::as_ptr(&view) as *const NSTextField) };
    let editor = field
        .currentEditor()
        .expect("initial first responder has a field editor");
    assert_eq!(editor.selectedRange().location, 5);
    assert_eq!(editor.selectedRange().length, 0);
    widgets::textfield::focus(handle);

    unsafe { perry_ui_textfield_set_selection_range(handle, 1.0, 3.0) };
    assert_eq!(
        unsafe { perry_ui_textfield_get_selection_start(handle) },
        1.0
    );
    assert_eq!(unsafe { perry_ui_textfield_get_selection_end(handle) }, 3.0);
    assert_eq!(editor.selectedRange().length, 2);
    unsafe { perry_ui_textfield_set_selection_range(handle, 100.0, 100.0) };
    assert_eq!(
        unsafe { perry_ui_textfield_get_selection_start(handle) },
        5.0
    );
    assert_eq!(unsafe { perry_ui_textfield_get_selection_end(handle) }, 5.0);
    assert_eq!(field.stringValue().to_string(), "a💙bc");
    window.close();
    println!("PASS native TextField selection before and after focus");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
