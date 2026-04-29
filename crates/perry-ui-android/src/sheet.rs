//! Sheet — Modal dialog on Android

use crate::jni_bridge;
use jni::objects::{GlobalRef, JValue};
use std::cell::RefCell;
use std::collections::HashMap;

struct SheetState {
    width: f64,
    height: f64,
    body_handle: Option<i64>,
    dialog_ref: Option<GlobalRef>,
}

thread_local! {
    static SHEETS: RefCell<HashMap<i64, SheetState>> = RefCell::new(HashMap::new());
    static NEXT_SHEET_ID: RefCell<i64> = RefCell::new(1);
}

pub fn create(width: f64, height: f64, _title_val: f64) -> i64 {
    let id = NEXT_SHEET_ID.with(|n| {
        let mut n = n.borrow_mut();
        let id = *n;
        *n += 1;
        id
    });

    SHEETS.with(|s| {
        s.borrow_mut().insert(
            id,
            SheetState {
                width,
                height,
                body_handle: None,
                dialog_ref: None,
            },
        );
    });

    id
}

pub fn set_body(sheet_handle: i64, widget_handle: i64) {
    SHEETS.with(|s| {
        let mut sheets = s.borrow_mut();
        if let Some(state) = sheets.get_mut(&sheet_handle) {
            state.body_handle = Some(widget_handle);
        }
    });
}

pub fn present(sheet_handle: i64) {
    let body = SHEETS.with(|s| s.borrow().get(&sheet_handle).and_then(|st| st.body_handle));

    if let Some(body_handle) = body {
        if let Some(view_ref) = crate::widgets::get_widget(body_handle) {
            let mut env = jni_bridge::get_env();
            let _ = env.push_local_frame(32);

            let activity = crate::widgets::get_activity(&mut env);

            // Create Dialog
            let dialog = env
                .new_object(
                    "android/app/Dialog",
                    "(Landroid/content/Context;)V",
                    &[JValue::Object(&activity)],
                )
                .expect("Failed to create Dialog");

            // Set content view
            let _ = env.call_method(
                &dialog,
                "setContentView",
                "(Landroid/view/View;)V",
                &[JValue::Object(view_ref.as_obj())],
            );

            // Show
            let _ = env.call_method(&dialog, "show", "()V", &[]);

            let global = env
                .new_global_ref(dialog)
                .expect("Failed to create global ref");
            SHEETS.with(|s| {
                let mut sheets = s.borrow_mut();
                if let Some(state) = sheets.get_mut(&sheet_handle) {
                    state.dialog_ref = Some(global);
                }
            });

            unsafe {
                env.pop_local_frame(&jni::objects::JObject::null());
            }
        }
    }
}

pub fn dismiss(sheet_handle: i64) {
    let dialog = SHEETS.with(|s| {
        let mut sheets = s.borrow_mut();
        sheets
            .get_mut(&sheet_handle)
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
