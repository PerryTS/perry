use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::MainThreadMarker;
use objc2_ui_kit::UIView;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static NAV_STACKS: RefCell<HashMap<i64, Vec<i64>>> = RefCell::new(HashMap::new());
}

// Use a plain UIView (not UIStackView) so the standard `add_child` falls
// through to zstack-style "pin to fill parent" pinning. State-driven
// `NavStack(state, routes)` adds N route bodies up front and toggles
// visibility via setHidden; UIStackView with axis=vertical leaves a
// single visible child laid out at its intrinsic content size (small for
// VStack(Spacer, Text, Spacer) shapes), so the screen stayed blank
// (#612). With z-stack pinning, every route body fills the navstack
// area; hidden ones simply don't paint.
pub fn create(_title_ptr: *const u8, body_handle: i64) -> i64 {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let cls = objc2::runtime::AnyClass::get(c"UIView").unwrap();
        let obj: *mut AnyObject = msg_send![cls, alloc];
        let obj: *mut AnyObject = msg_send![obj, init];
        let view: Retained<UIView> = Retained::retain(obj as *mut UIView).unwrap();
        let handle = super::register_widget(view);
        if body_handle > 0 {
            super::add_child(handle, body_handle);
        }
        NAV_STACKS.with(|ns| {
            ns.borrow_mut().insert(handle, vec![body_handle]);
        });
        handle
    }
}

pub fn push(handle: i64, _title_ptr: *const u8, body_handle: i64) {
    NAV_STACKS.with(|ns| {
        let mut stacks = ns.borrow_mut();
        if let Some(stack) = stacks.get_mut(&handle) {
            if let Some(&top) = stack.last() {
                super::set_hidden(top, true);
            }
            stack.push(body_handle);
            super::add_child(handle, body_handle);
        }
    });
}

pub fn pop(handle: i64) {
    NAV_STACKS.with(|ns| {
        let mut stacks = ns.borrow_mut();
        if let Some(stack) = stacks.get_mut(&handle) {
            if stack.len() > 1 {
                let popped = stack.pop().unwrap();
                super::set_hidden(popped, true);
                if let Some(&top) = stack.last() {
                    super::set_hidden(top, false);
                }
            }
        }
    });
}
