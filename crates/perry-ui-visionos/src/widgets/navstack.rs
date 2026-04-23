use objc2::rc::Retained;
use objc2::msg_send;
use objc2::runtime::AnyObject;
use objc2_ui_kit::UIView;
use objc2_foundation::MainThreadMarker;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static NAV_STACKS: RefCell<HashMap<i64, Vec<i64>>> = RefCell::new(HashMap::new());
}

pub fn create(_title_ptr: *const u8, body_handle: i64) -> i64 {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let stack_cls = objc2::runtime::AnyClass::get(c"UIStackView").unwrap();
        let obj: *mut AnyObject = msg_send![stack_cls, alloc];
        let obj: *mut AnyObject = msg_send![obj, init];
        let stack: Retained<UIView> = Retained::retain(obj as *mut UIView).unwrap();
        let _: () = msg_send![&*stack, setAxis: 1i64]; // vertical
        let handle = super::register_widget(stack);
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
