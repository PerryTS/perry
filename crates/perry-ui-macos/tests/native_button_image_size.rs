// AppKit must run on the process main thread, so this uses a harness-free binary.
#[cfg(target_os = "macos")]
fn main() {
    use objc2::rc::Retained;
    use objc2_app_kit::{NSApplication, NSButton};
    use objc2_core_foundation::CGSize;
    use objc2_foundation::MainThreadMarker;
    use perry_ui_macos::{perry_ui_button_set_image, widgets};

    if std::env::args().any(|arg| arg == "--list") {
        println!("native_button_image_size: test");
        return;
    }

    let mtm = MainThreadMarker::new().expect("native button test runs on the main thread");
    let _app = NSApplication::sharedApplication(mtm);
    let label = b"Copy";
    let label = perry_runtime::string::js_string_from_bytes(label.as_ptr(), label.len() as u32);
    let button = widgets::button::create(label.cast(), 0.0);
    widgets::button::set_bordered(button, false);
    let symbol = b"doc.on.doc";
    let symbol = perry_runtime::string::js_string_from_bytes(symbol.as_ptr(), symbol.len() as u32);
    let view = widgets::get_widget(button).expect("button is registered");
    let native_button = unsafe { &*(Retained::as_ptr(&view) as *const NSButton) };

    perry_ui_button_set_image(button, symbol as i64, 0.0);
    let large: CGSize = unsafe { objc2::msg_send![native_button, fittingSize] };
    perry_ui_button_set_image(button, symbol as i64, 14.0);
    let compact: CGSize = unsafe { objc2::msg_send![native_button, fittingSize] };

    assert!(
        compact.height < large.height && compact.height <= 32.0,
        "14pt image must fit a 32pt control: large={large:?}, compact={compact:?}"
    );
    println!("PASS native button image size: {large:?} -> {compact:?}");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
