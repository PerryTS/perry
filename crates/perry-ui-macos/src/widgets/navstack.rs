use objc2::msg_send;
use objc2::rc::Retained;
use objc2::MainThreadOnly;
use objc2_app_kit::NSView;
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
///
/// Implementation: plain `NSView` registered in `ZSTACK_HANDLES` so the
/// shared `add_child` routes through `zstack::add_child` (pin-to-fill).
/// State-driven `NavStack(state, routes)` adds N route bodies up front
/// and toggles visibility via setHidden; with NSStackView vertical the
/// inner VStack(Spacer, Text, Spacer) bodies have intrinsic height ≈ 0
/// and the visible route stayed collapsed (#612). z-stack pinning gives
/// every route body the navstack's full bounds; hidden routes simply
/// don't paint.
pub fn create(_title_ptr: *const u8, body_handle: i64) -> i64 {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    unsafe {
        let view: Retained<NSView> = msg_send![
            NSView::alloc(mtm), initWithFrame: objc2_core_foundation::CGRect::ZERO
        ];
        let handle = super::register_widget(view);
        super::zstack::register_as_zstack(handle);

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
