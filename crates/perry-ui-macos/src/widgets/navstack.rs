use objc2::msg_send;
use objc2::rc::Retained;
use objc2::MainThreadOnly;
use objc2_app_kit::{NSStackView, NSView};
use objc2_foundation::MainThreadMarker;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// Map from navstack handle to Vec of view handles (navigation stack)
    static NAV_STACKS: RefCell<HashMap<i64, Vec<i64>>> = RefCell::new(HashMap::new());
}

/// Create a navigation stack container. Returns widget handle.
/// `_title_ptr` is a StringHeader pointer for the initial title (reserved for future use).
/// `body_handle` is the handle of the initial body view.
pub fn create(_title_ptr: *const u8, body_handle: i64) -> i64 {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    unsafe {
        let stack: Retained<NSStackView> = msg_send![
            NSStackView::alloc(mtm), initWithFrame: objc2_core_foundation::CGRect::ZERO
        ];
        let _: () = msg_send![&*stack, setOrientation: 1i64]; // vertical
        let _: () = msg_send![&*stack, setSpacing: 0.0f64];

        let view: Retained<NSView> = Retained::cast_unchecked(stack);
        let handle = super::register_widget(view);

        // Add initial body
        if body_handle > 0 {
            super::add_child(handle, body_handle);
        }
        NAV_STACKS.with(|ns| {
            ns.borrow_mut().insert(handle, vec![body_handle]);
        });

        handle
    }
}

/// Push a new view onto the navigation stack.
/// Hides the current top view and shows the new one.
pub fn push(handle: i64, _title_ptr: *const u8, body_handle: i64) {
    NAV_STACKS.with(|ns| {
        let mut stacks = ns.borrow_mut();
        if let Some(stack) = stacks.get_mut(&handle) {
            // Hide current top view
            if let Some(&top) = stack.last() {
                super::set_hidden(top, true);
            }
            stack.push(body_handle);
            super::add_child(handle, body_handle);
        }
    });
}

/// Pop the top view from the navigation stack.
/// Hides the popped view and shows the previous one.
pub fn pop(handle: i64) {
    NAV_STACKS.with(|ns| {
        let mut stacks = ns.borrow_mut();
        if let Some(stack) = stacks.get_mut(&handle) {
            if stack.len() > 1 {
                let popped = stack.pop().unwrap();
                super::set_hidden(popped, true);
                // Show previous top
                if let Some(&top) = stack.last() {
                    super::set_hidden(top, false);
                }
            }
        }
    });
}
