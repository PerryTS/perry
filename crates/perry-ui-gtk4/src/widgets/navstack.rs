use gtk4::prelude::*;
use gtk4::Stack;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static NAV_STACKS: RefCell<HashMap<i64, NavStackState>> = RefCell::new(HashMap::new());
}

struct NavStackState {
    stack: Stack,
    page_count: usize,
}

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

/// Create a NavigationStack with an initial page.
pub fn create(title_ptr: *const u8, body_handle: i64) -> i64 {
    crate::app::ensure_gtk_init();
    let _title = str_from_header(title_ptr);
    let stack = Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::SlideLeftRight);
    stack.set_transition_duration(250);
    stack.set_vexpand(true);
    stack.set_hexpand(true);

    if let Some(child) = super::get_widget(body_handle) {
        stack.add_named(&child, Some("page_0"));
        stack.set_visible_child_name("page_0");
    }

    let handle = super::register_widget(stack.clone().upcast());
    NAV_STACKS.with(|n| {
        n.borrow_mut().insert(
            handle,
            NavStackState {
                stack,
                page_count: 1,
            },
        );
    });
    handle
}

/// Push a new page onto the navigation stack.
pub fn push(handle: i64, title_ptr: *const u8, body_handle: i64) {
    let _title = str_from_header(title_ptr);
    NAV_STACKS.with(|n| {
        let mut stacks = n.borrow_mut();
        if let Some(state) = stacks.get_mut(&handle) {
            if let Some(child) = super::get_widget(body_handle) {
                let name = format!("page_{}", state.page_count);
                state.stack.add_named(&child, Some(&name));
                state.stack.set_visible_child_name(&name);
                state.page_count += 1;
            }
        }
    });
}

/// Pop the top page from the navigation stack.
pub fn pop(handle: i64) {
    NAV_STACKS.with(|n| {
        let mut stacks = n.borrow_mut();
        if let Some(state) = stacks.get_mut(&handle) {
            if state.page_count > 1 {
                state.page_count -= 1;
                let prev_name = format!("page_{}", state.page_count - 1);
                state.stack.set_visible_child_name(&prev_name);
                // Remove the popped page
                let popped_name = format!("page_{}", state.page_count);
                if let Some(child) = state.stack.child_by_name(&popped_name) {
                    state.stack.remove(&child);
                }
            }
        }
    });
}
