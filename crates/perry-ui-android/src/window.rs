//! Multi-window — Dialog-based windows on Android

use crate::jni_bridge;
use jni::objects::{GlobalRef, JValue};
use std::cell::RefCell;
use std::collections::HashMap;

fn str_from_header(ptr: *const u8) -> &'static str {
    crate::app::str_from_header(ptr)
}

struct WindowState {
    title: String,
    width: f64,
    height: f64,
    body_handle: Option<i64>,
    dialog_ref: Option<GlobalRef>,
}

thread_local! {
    static WINDOWS: RefCell<HashMap<i64, WindowState>> = RefCell::new(HashMap::new());
    static NEXT_WINDOW_ID: RefCell<i64> = RefCell::new(1);
}

pub fn create(title_ptr: *const u8, width: f64, height: f64) -> i64 {
    let title = str_from_header(title_ptr).to_string();
    let id = NEXT_WINDOW_ID.with(|n| {
        let mut n = n.borrow_mut();
        let id = *n;
        *n += 1;
        id
    });

    WINDOWS.with(|w| {
        w.borrow_mut().insert(
            id,
            WindowState {
                title,
                width,
                height,
                body_handle: None,
                dialog_ref: None,
            },
        );
    });

    id
}

pub fn set_body(window_handle: i64, widget_handle: i64) {
    WINDOWS.with(|w| {
        let mut windows = w.borrow_mut();
        if let Some(state) = windows.get_mut(&window_handle) {
            state.body_handle = Some(widget_handle);
        }
    });
}

pub fn show(window_handle: i64) {
    let (body, title) = WINDOWS.with(|w| {
        let windows = w.borrow();
        windows
            .get(&window_handle)
            .map(|st| (st.body_handle, st.title.clone()))
            .unwrap_or((None, String::new()))
    });

    if let Some(body_handle) = body {
        if let Some(view_ref) = crate::widgets::get_widget(body_handle) {
            let mut env = jni_bridge::get_env();
            let _ = env.push_local_frame(32);

            let activity = crate::widgets::get_activity(&mut env);

            let dialog = env
                .new_object(
                    "android/app/Dialog",
                    "(Landroid/content/Context;)V",
                    &[JValue::Object(&activity)],
                )
                .expect("Failed to create Dialog");

            // Set title
            if !title.is_empty() {
                let jtitle = env.new_string(&title).expect("Failed to create JNI string");
                let _ = env.call_method(
                    &dialog,
                    "setTitle",
                    "(Ljava/lang/CharSequence;)V",
                    &[JValue::Object(&jtitle)],
                );
            }

            // Set content
            let _ = env.call_method(
                &dialog,
                "setContentView",
                "(Landroid/view/View;)V",
                &[JValue::Object(view_ref.as_obj())],
            );

            let _ = env.call_method(&dialog, "show", "()V", &[]);

            let global = env
                .new_global_ref(dialog)
                .expect("Failed to create global ref");
            WINDOWS.with(|w| {
                let mut windows = w.borrow_mut();
                if let Some(state) = windows.get_mut(&window_handle) {
                    state.dialog_ref = Some(global);
                }
            });

            unsafe {
                env.pop_local_frame(&jni::objects::JObject::null());
            }
        }
    }
}

pub fn close(window_handle: i64) {
    let dialog = WINDOWS.with(|w| {
        let mut windows = w.borrow_mut();
        windows
            .get_mut(&window_handle)
            .and_then(|st| st.dialog_ref.take())
    });

    if let Some(dialog_ref) = dialog {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let _ = env.call_method(dialog_ref.as_obj(), "dismiss", "()V", &[]);
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}
