//! NavigationStack — FrameLayout with page stack (show/hide)

use crate::jni_bridge;
use jni::objects::JValue;
use std::cell::RefCell;
use std::collections::HashMap;

fn str_from_header(ptr: *const u8) -> &'static str {
    crate::app::str_from_header(ptr)
}

struct NavState {
    pages: Vec<i64>, // widget handles for each page
}

thread_local! {
    static NAV_STATES: RefCell<HashMap<i64, NavState>> = RefCell::new(HashMap::new());
}

pub fn create(title_ptr: *const u8, body_handle: i64) -> i64 {
    let _title = str_from_header(title_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);

    let activity = super::get_activity(&mut env);
    let frame_layout = env
        .new_object(
            "android/widget/FrameLayout",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("Failed to create FrameLayout");

    let global = env
        .new_global_ref(frame_layout)
        .expect("Failed to create global ref");
    let handle = super::register_widget(global);

    // Add the initial page
    super::add_child(handle, body_handle);

    NAV_STATES.with(|s| {
        s.borrow_mut().insert(
            handle,
            NavState {
                pages: vec![body_handle],
            },
        );
    });

    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    handle
}

pub fn push(handle: i64, title_ptr: *const u8, body_handle: i64) {
    let _title = str_from_header(title_ptr);

    // Hide current top page
    NAV_STATES.with(|s| {
        let mut states = s.borrow_mut();
        if let Some(state) = states.get_mut(&handle) {
            if let Some(&top) = state.pages.last() {
                super::set_hidden(top, true);
            }
            state.pages.push(body_handle);
        }
    });

    // Add and show new page
    super::add_child(handle, body_handle);
}

pub fn pop(handle: i64) {
    NAV_STATES.with(|s| {
        let mut states = s.borrow_mut();
        if let Some(state) = states.get_mut(&handle) {
            if state.pages.len() > 1 {
                let removed = state.pages.pop().unwrap();
                super::set_hidden(removed, true);

                // Show the new top
                if let Some(&top) = state.pages.last() {
                    super::set_hidden(top, false);
                }
            }
        }
    });
}
