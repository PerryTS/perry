// #11092 — a main window must leave its explicit appearance unset so it
// inherits NSApp.effectiveAppearance. This lets AppKit follow the system by
// default and honor NSRequiresAquaSystemAppearance from the app bundle.

#[cfg(target_os = "macos")]
fn main() {
    use objc2::{msg_send, runtime::AnyObject};
    use objc2_app_kit::NSApplication;
    use objc2_foundation::MainThreadMarker;
    use perry_runtime as _;

    if std::env::args().any(|arg| arg == "--list") {
        println!("native_window_appearance: test");
        return;
    }

    let mtm = MainThreadMarker::new().expect("native window test runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    let windows_before = app.windows().len();

    perry_ui_macos::app::app_create(std::ptr::null(), 320.0, 200.0);

    let windows = app.windows();
    assert_eq!(
        windows.len(),
        windows_before + 1,
        "app_create must register one NSWindow"
    );
    let window = windows
        .iter()
        .last()
        .expect("app_create registered its NSWindow");
    let appearance: *mut AnyObject = unsafe { msg_send![&**window, appearance] };
    assert!(
        appearance.is_null(),
        "the main window must inherit NSApp.effectiveAppearance"
    );

    println!("PASS native window appearance inherits NSApp");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
