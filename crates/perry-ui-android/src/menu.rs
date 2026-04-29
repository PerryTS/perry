use jni::objects::JValue;
use std::cell::RefCell;
use std::collections::HashMap;

use crate::app::str_from_header;
use crate::callback;
use crate::jni_bridge;

struct MenuItem {
    title: String,
    callback_key: i64,
}

struct MenuEntry {
    items: Vec<MenuItem>,
}

thread_local! {
    static MENUS: RefCell<Vec<MenuEntry>> = RefCell::new(Vec::new());
    /// Maps widget handle -> menu handle for context menus.
    static CONTEXT_MENUS: RefCell<HashMap<i64, i64>> = RefCell::new(HashMap::new());
}

/// Create a context menu. Returns a menu handle.
pub fn create() -> i64 {
    MENUS.with(|m| {
        let mut menus = m.borrow_mut();
        menus.push(MenuEntry { items: Vec::new() });
        menus.len() as i64
    })
}

/// Add an item to a context menu.
pub fn add_item(menu_handle: i64, title_ptr: *const u8, cb: f64) {
    let title = str_from_header(title_ptr).to_string();
    let cb_key = callback::register(cb);

    MENUS.with(|m| {
        let mut menus = m.borrow_mut();
        let idx = (menu_handle - 1) as usize;
        if idx < menus.len() {
            menus[idx].items.push(MenuItem {
                title,
                callback_key: cb_key,
            });
        }
    });
}

/// Set a context menu on a widget. On Android, this is triggered by long-press.
pub fn set_context_menu(widget_handle: i64, menu_handle: i64) {
    CONTEXT_MENUS.with(|cm| {
        cm.borrow_mut().insert(widget_handle, menu_handle);
    });

    // Set up long-click listener on the widget via PerryBridge
    if let Some(view_ref) = crate::widgets::get_widget(widget_handle) {
        let mut env = jni_bridge::get_env();
        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
        let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
        let _ = env.call_static_method(
            bridge_cls,
            "setContextMenu",
            "(Landroid/view/View;J)V",
            &[JValue::Object(view_ref.as_obj()), JValue::Long(menu_handle)],
        );
    }
}

/// JNI entry point: called from Java when a context menu needs its items.
/// Returns the number of menu items.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeGetMenuItemCount(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
    menu_handle: jni::sys::jlong,
) -> jni::sys::jint {
    MENUS.with(|m| {
        let menus = m.borrow();
        let idx = (menu_handle - 1) as usize;
        if idx < menus.len() {
            menus[idx].items.len() as i32
        } else {
            0
        }
    })
}

/// JNI entry point: called from Java to get a menu item title.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeGetMenuItemTitle(
    env: jni::JNIEnv,
    _class: jni::objects::JClass,
    menu_handle: jni::sys::jlong,
    index: jni::sys::jint,
) -> jni::sys::jstring {
    let title = MENUS.with(|m| {
        let menus = m.borrow();
        let idx = (menu_handle - 1) as usize;
        if idx < menus.len() {
            let item_idx = index as usize;
            if item_idx < menus[idx].items.len() {
                return menus[idx].items[item_idx].title.clone();
            }
        }
        String::new()
    });
    let jstr = env.new_string(&title).expect("Failed to create JNI string");
    jstr.into_raw()
}

/// JNI entry point: called from Java when a context menu item is selected.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeMenuItemSelected(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
    menu_handle: jni::sys::jlong,
    index: jni::sys::jint,
) {
    MENUS.with(|m| {
        let menus = m.borrow();
        let idx = (menu_handle - 1) as usize;
        if idx < menus.len() {
            let item_idx = index as usize;
            if item_idx < menus[idx].items.len() {
                let cb_key = menus[idx].items[item_idx].callback_key;
                callback::invoke0(cb_key);
            }
        }
    });
}
