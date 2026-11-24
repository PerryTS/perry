// A Link's label carries the click recognizer that opens its URL, and the callback
// passes only web URLs on to the open call. AppKit requires the main thread, so this is
// a harness = false binary.

#[cfg(target_os = "macos")]
fn main() {
    use objc2::msg_send;
    use objc2_foundation::{MainThreadMarker, NSArray};
    use perry_ffi::JsThis;
    use perry_runtime as _; // links the runtime symbols
    use perry_ui_macos::widgets;
    use std::cell::RefCell;

    thread_local! {
        static OPENED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }
    extern "C" fn record(url_ptr: i64) {
        let url = unsafe { perry_ffi::copy_string_from_raw(url_ptr as *const u8) };
        OPENED.with(|opened| opened.borrow_mut().push(url));
    }
    extern "C" {
        fn js_closure_call0(closure: *const u8, this: JsThis) -> f64;
        fn js_nanbox_get_pointer(value: f64) -> i64;
    }

    if std::env::args().any(|arg| arg == "--list") {
        println!("native_link: test");
        return;
    }
    let _mtm = MainThreadMarker::new().expect("native link test runs on the main thread");

    let text = perry_ffi::alloc_string("Docs").as_raw() as i64;
    let href = perry_ffi::alloc_string("https://example.test/").as_raw() as i64;
    let handle = perry_ui_macos::perry_ui_link_create(text, href);
    let view = widgets::get_widget(handle).expect("Link widget is registered");
    let recognizers: objc2::rc::Retained<NSArray> =
        unsafe { msg_send![&*view, gestureRecognizers] };
    assert_eq!(recognizers.count(), 1, "a Link handles clicks");

    for href in [
        "https://example.test/",
        "mailto:a@b.test",
        "tel:+34600000000",
        "/about",
        "file:///etc/hosts",
        "C:\\Windows\\notepad.exe",
        "javascript:alert(1)",
        "",
    ] {
        let open = perry_ui::link::open_link_callback(href.to_string(), record);
        unsafe { js_closure_call0(js_nanbox_get_pointer(open) as *const u8, JsThis::UNDEFINED) };
    }
    assert_eq!(
        OPENED.take(),
        [
            "https://example.test/",
            "mailto:a@b.test",
            "tel:+34600000000"
        ]
    );

    let path = perry_ui_macos::perry_ui_router_current_path();
    assert_ne!(path, 0);
    assert_eq!(
        unsafe { perry_ffi::copy_string_from_raw(path as *const u8) },
        ""
    );
    println!("PASS native link: a Link opens its URL");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
