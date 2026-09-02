//! Multi-window — Dialog-based windows on Android

use crate::jni_bridge;
use crate::jni_bridge::GlobalRef;
use jni::JValue;
use std::cell::RefCell;
use std::collections::HashMap;

use perry_ffi::copy_string_from_raw as str_from_header;

struct WindowState {
    title: String,
    _width: f64,
    _height: f64,
    body_handle: Option<i64>,
    dialog_ref: Option<GlobalRef>,
}

thread_local! {
    static WINDOWS: RefCell<HashMap<i64, WindowState>> = RefCell::new(HashMap::new());
    static NEXT_WINDOW_ID: RefCell<i64> = RefCell::new(1);
}

pub fn create(title_ptr: *const u8, width: f64, height: f64) -> i64 {
    let title = unsafe { str_from_header(title_ptr) }.to_string();
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
                _width: width,
                _height: height,
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
            jni_bridge::with_env(|env| {
                let _ = jni_bridge::push_local_frame(env, 32);

                let activity = crate::widgets::get_activity(env);

                let dialog = env
                    .new_object(
                        jni::jni_str!("android/app/Dialog"),
                        jni::jni_sig!("(Landroid/content/Context;)V"),
                        &[JValue::Object(&activity)],
                    )
                    .expect("Failed to create Dialog");

                // Set title
                if !title.is_empty() {
                    let jtitle = env.new_string(&title).expect("Failed to create JNI string");
                    let _ = env.call_method(
                        &dialog,
                        jni::jni_str!("setTitle"),
                        jni::jni_sig!("(Ljava/lang/CharSequence;)V"),
                        &[JValue::Object(&jtitle)],
                    );
                }

                // Set content
                let _ = env.call_method(
                    &dialog,
                    jni::jni_str!("setContentView"),
                    jni::jni_sig!("(Landroid/view/View;)V"),
                    &[JValue::Object(view_ref.as_obj())],
                );

                let _ = env.call_method(&dialog, jni::jni_str!("show"), jni::jni_sig!("()V"), &[]);

                let global =
                    jni_bridge::new_global_ref(env, dialog).expect("Failed to create global ref");
                WINDOWS.with(|w| {
                    let mut windows = w.borrow_mut();
                    if let Some(state) = windows.get_mut(&window_handle) {
                        state.dialog_ref = Some(global);
                    }
                });

                unsafe {
                    let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
                }
            })
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
        jni_bridge::with_env(|env| {
            let _ = jni_bridge::push_local_frame(env, 8);
            let _ = env.call_method(
                dialog_ref.as_obj(),
                jni::jni_str!("dismiss"),
                jni::jni_sig!("()V"),
                &[],
            );
            unsafe {
                let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
            }
        })
    }
}
