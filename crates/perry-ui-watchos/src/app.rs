//! App lifecycle for watchOS.
//!
//! Since watchOS uses SwiftUI's @main App, the actual app lifecycle is managed
//! by the fixed PerryWatchApp.swift. This module stores config and provides
//! the entry point that Swift calls to run the compiled TypeScript init code.

use std::cell::RefCell;

use crate::tree::{self, NodeData, NodeKind};

thread_local! {
    static PENDING_BODY: RefCell<Option<i64>> = RefCell::new(None);
}

pub fn app_create(_title_ptr: *const u8, _width: f64, _height: f64) -> i64 {
    // On watchOS, the app is created by the SwiftUI @main struct.
    // We just return a handle to satisfy the API contract.
    //
    // Ask for notification authorization once here (mirrors iOS, which prompts
    // from `PerryAppDelegate.didFinishLaunching`). This runs during
    // `perry_main_init` on the Swift main thread, before the WatchKit run loop
    // starts, so the permission dialog fires at launch rather than on the first
    // `notificationSend`. Keeping it Rust-side avoids editing the fixed
    // PerryWatchApp.swift template.
    crate::notifications::request_authorization();
    1
}

pub fn app_set_body(_app_handle: i64, root_handle: i64) {
    tree::set_root(root_handle);
    PENDING_BODY.with(|b| {
        *b.borrow_mut() = Some(root_handle);
    });
}

pub fn app_run(_app_handle: i64) {
    // On watchOS, the SwiftUI run loop is managed by PerryWatchApp.swift.
    // The compiled TypeScript calls perry_ui_app_run() at the end of init,
    // but on watchOS this is a no-op — the Swift @main struct drives the loop.
    //
    // perry_main_init() is called from Swift before the app body is rendered,
    // so by the time SwiftUI queries the tree, it's fully built. Flag the
    // event loop as host-driven so the generated entry's drain loop exits
    // instead of parking on live timers — otherwise perry_main_init never
    // returns and SwiftUI never renders (the shell drives timer ticks).
    unsafe { js_set_event_loop_host_driven(1) };
    register_cross_platform_text_handlers();
    install_test_mode_exit_timer();
}

extern "C" {
    /// Defined in `perry-runtime/src/event_pump.rs`. Tells the generated
    /// entry's event-drain loop that the Swift shell owns the run loop.
    fn js_set_event_loop_host_driven(v: i32);
    /// Defined in `perry-runtime/src/ui_text_registry.rs`. Stores the
    /// passed handler in an AtomicPtr that `perry_arkts_show_toast`
    /// consults on each call. No-op when `ohos-napi` is on.
    fn js_register_show_toast_handler(f: extern "C" fn(msg_ptr: *const u8, msg_len: usize));
    fn js_register_set_text_handler(
        f: extern "C" fn(id_ptr: *const u8, id_len: usize, val_ptr: *const u8, val_len: usize),
    );
    fn js_register_text_id_handler(
        f: extern "C" fn(widget_handle: i64, id_ptr: *const u8, id_len: usize),
    );
    /// Issue #535 Layer 2 — `js_state_set` calls this for every NavStack
    /// route bound to the changed state's synth id. Defined in
    /// `perry-runtime/src/ui_text_registry.rs`'s `NAVSTACK_REGISTRY` block.
    fn js_register_widget_hidden_handler(f: extern "C" fn(widget_handle: i64, hidden: i32));
}

extern "C" fn navstack_set_widget_hidden(widget_handle: i64, hidden: i32) {
    crate::tree::set_hidden(widget_handle, hidden != 0);
}

fn register_cross_platform_text_handlers() {
    unsafe {
        js_register_show_toast_handler(crate::widgets::toast::show_toast_handler);
        js_register_set_text_handler(crate::widgets::text_registry::set_text_handler);
        js_register_text_id_handler(crate::widgets::text_registry::register_text_id_handler);
        js_register_widget_hidden_handler(navstack_set_widget_hidden);
    }
}

/// If `PERRY_UI_TEST_MODE=1`, schedule a background thread that exits the
/// process cleanly after the configured delay. watchOS has no
/// screenshot-capable runtime yet (no `screenshot.rs` in this crate), so the
/// test-mode here is purely a "did the program launch without crashing?"
/// signal. Useful for CI smoke-checks against `--target watchos[-simulator]`
/// builds under `xcrun simctl launch --console`.
///
/// Uses a plain thread::sleep rather than NSTimer because the main runloop on
/// watchOS is Swift's, and scheduling onto it from Rust early-init is fragile.
fn install_test_mode_exit_timer() {
    if !perry_ui_testkit::is_test_mode() {
        return;
    }
    let delay_ms = perry_ui_testkit::exit_delay_ms();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(delay_ms as u64));
        use std::io::Write;
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
        std::process::exit(0);
    });
}
