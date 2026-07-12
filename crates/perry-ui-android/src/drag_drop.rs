//! Android drag & drop (issue #4773).
//!
//! Widget-level drag/drop setters that attach behavior to an existing widget
//! handle, mirroring the macOS/UIKit backends at the JS boundary. Android's
//! drag/drop is built on `View.setOnDragListener` (drop destination) and
//! `View.startDragAndDrop` (drag source); both require a Java/Kotlin
//! `View.OnDragListener` instance, so — exactly like buttons
//! (`widgets/button.rs`) and continuous pointer events (`pointer.rs`) — the
//! native side delegates the listener wiring to the host `PerryBridge` Java
//! class and exchanges per-widget closure *keys* (see [`crate::callback`])
//! across the JNI boundary. The closures themselves stay on the native side
//! and are invoked from the UI thread via the `nativeInvoke*` entry points
//! below, which pump the promise microtask queue afterwards just like every
//! other callback in this crate.
//!
//! Drop destination (`widgetOnDrop`): we register the callback under a key and
//! ask `PerryBridge.setOnDropCallback(view, key)` to install an
//! `OnDragListener` that advertises a copy operation and, on `ACTION_DROP`,
//! reads the `DragEvent`'s `ClipData`: each item's `getText()` becomes `text`,
//! each `getUri()` is sorted into `files` (a `content://`/`file://` URI) or
//! `urls` (an `http`/`https` web URL). The Java side passes those back through
//! [`Java_com_perry_app_PerryBridge_nativeInvokeDropCallback`], which builds the
//! `{ text?, files?, urls? }` object and invokes the callback closure.
//!
//! Drag source (`widgetSetDrag*`): each representation registers its provider
//! closure under a key; `PerryBridge.setDragSource(view, textKey, fileKey,
//! urlKey)` installs a long-press handler that, when a drag begins, calls back
//! into native via [`Java_com_perry_app_PerryBridge_nativeInvokeDragProvider`]
//! for each non-zero key to obtain that representation's payload string, builds
//! a single `ClipData` (newPlainText for text, a `ClipData.Item` with a `Uri`
//! for file/url), and calls `view.startDragAndDrop(clip, shadowBuilder, …)`.
//! Multiple `widgetSetDrag*` may be set on one widget; they share one
//! `ClipData` with one item per registered representation. `0` is used as the
//! "no provider" sentinel key (real keys from `callback::register` start at 1).
//!
//! Java-side glue still required (NOT yet present in this crate's companion
//! Kotlin sources — a reviewer/integrator must add it to `PerryBridge`):
//!   - `static void setOnDropCallback(View view, long key)` — installs an
//!     `OnDragListener`; on `ACTION_DRAG_STARTED`/`ACTION_DRAG_ENTERED` return
//!     `true` to accept, on `ACTION_DROP` parse `event.getClipData()` and call
//!     `nativeInvokeDropCallback(key, text, files[], urls[])`.
//!   - `static void setDragSource(View view, long textKey, long fileKey, long
//!     urlKey)` — installs an `OnLongClickListener` (or equivalent) that, on a
//!     drag gesture, calls `nativeInvokeDragProvider(key)` for each non-zero
//!     key, assembles a `ClipData`, and calls
//!     `view.startDragAndDrop(clip, new View.DragShadowBuilder(view), null,
//!     0)` (`startDrag` pre-API-24).
//! The three `nativeInvoke*` symbols below are the native counterparts those
//! Java methods must call.
//!
//! NOTE: like all `perry-ui-android` code, this module is **compile-unverified
//! in CI** — PR CI does not build the cross-host UI crates (no Android NDK
//! linker is available), so it needs on-device testing. The JNI calls and
//! signatures here mirror the existing `button.rs` / `pointer.rs` /
//! `image_picker.rs` bridges, but the new `PerryBridge` Java methods above must
//! be implemented before drag/drop does anything at runtime.

use crate::app::str_from_header;
use crate::callback;
use crate::jni_bridge;
use jni::objects::JValue;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;

extern "C" {
    // Signatures copied from the existing in-crate declarations so they match
    // the rest of the static library:
    //   js_object_alloc / js_object_set_field_by_name / js_string_from_bytes →
    //     `widgets/canvas.rs`
    //   js_array_alloc / js_array_push_f64 / js_nanbox_* / js_closure_call0 →
    //     `callback.rs`
    fn js_object_alloc(class_id: u32, field_count: u32) -> *mut c_void;
    fn js_object_set_field_by_name(obj: *mut c_void, key: *const c_void, value: f64);
    fn js_string_from_bytes(data: *const u8, len: u32) -> *mut c_void;
    fn js_array_alloc(capacity: u32) -> *mut c_void;
    fn js_array_push_f64(arr: *mut c_void, value: f64) -> *mut c_void;
    fn js_nanbox_pointer(ptr: i64) -> f64;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    // Converts an arbitrary JS value to a runtime `StringHeader` (typed here as
    // `*const u8` so it feeds `str_from_header`, the same way `clipboard.rs`
    // treats runtime string pointers). Defined in
    // `perry-runtime/src/value/to_string.rs`.
    fn js_jsvalue_to_string(value: f64) -> *const u8;
    fn js_promise_run_microtasks() -> i32;
}

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

thread_local! {
    /// Per-widget drag-source provider keys `(textKey, fileKey, urlKey)`. A key
    /// of `0` means "no provider for this representation". Kept so that setting
    /// a second representation on the same widget re-uses the existing
    /// `OnLongClickListener` and only fills in the missing slot — the same
    /// approach `pointer.rs` uses for its `(down, move, up)` key triple.
    static DRAG_KEYS: RefCell<HashMap<i64, (i64, i64, i64)>> = RefCell::new(HashMap::new());
}

/// Allocate a NaN-boxed Perry string key for a JS object field name.
fn js_key(name: &[u8]) -> *const c_void {
    unsafe { js_string_from_bytes(name.as_ptr(), name.len() as u32) as *const c_void }
}

/// Allocate a NaN-boxed Perry string from a Rust string.
unsafe fn nanbox_str(s: &str) -> f64 {
    let bytes = s.as_bytes();
    let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    js_nanbox_string(ptr as i64)
}

/// Drain the promise microtask queue so any `.then`/`await` continuations
/// scheduled by the drop/drag callback run before control returns to Java.
/// Mirrors `callback::pump_microtasks`.
fn pump_microtasks() {
    unsafe { while js_promise_run_microtasks() > 0 {} }
}

// --- drag-source key bookkeeping --------------------------------------------

/// Fetch (creating if needed) the `(textKey, fileKey, urlKey)` triple for a
/// widget. Newly-created slots start at `0` (no provider); the caller fills in
/// whichever slot it owns and then (re-)installs the Java drag source so the
/// listener sees the updated key set.
fn ensure_keys(handle: i64) -> (i64, i64, i64) {
    DRAG_KEYS.with(|m| *m.borrow_mut().entry(handle).or_insert((0, 0, 0)))
}

fn store_keys(handle: i64, keys: (i64, i64, i64)) {
    DRAG_KEYS.with(|m| {
        m.borrow_mut().insert(handle, keys);
    });
}

/// (Re-)install the Java-side drag source for `handle` with the current key
/// triple via `PerryBridge.setDragSource(View, J, J, J)`.
fn install_drag_source(handle: i64, keys: (i64, i64, i64)) {
    let Some(view_ref) = crate::widgets::get_widget(handle) else {
        return;
    };
    let (text_key, file_key, url_key) = keys;
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(
        bridge_cls,
        "setDragSource",
        "(Landroid/view/View;JJJ)V",
        &[
            JValue::Object(view_ref.as_obj()),
            JValue::Long(text_key),
            JValue::Long(file_key),
            JValue::Long(url_key),
        ],
    );
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
}

// --- FFI: drop destination ---------------------------------------------------

/// Register `widget` as a drop destination. `callback` (a NaN-boxed closure) is
/// invoked with a `{ text?, files?, urls? }` object describing the payload when
/// text, files, or URLs are dropped onto the widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_on_drop(widget: i64, callback: f64) {
    let Some(view_ref) = crate::widgets::get_widget(widget) else {
        return;
    };
    let key = callback::register(callback);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(
        bridge_cls,
        "setOnDropCallback",
        "(Landroid/view/View;J)V",
        &[JValue::Object(view_ref.as_obj()), JValue::Long(key)],
    );
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
}

// --- FFI: drag source --------------------------------------------------------

/// Register `widget` as a drag source offering plain text. `provider` (a
/// NaN-boxed closure) returns the text payload when a drag begins.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_drag_text(widget: i64, provider: f64) {
    let key = callback::register(provider);
    let (_, file, url) = ensure_keys(widget);
    let keys = (key, file, url);
    store_keys(widget, keys);
    install_drag_source(widget, keys);
}

/// Register `widget` as a drag source offering a file. `provider` returns the
/// absolute path of the file to carry.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_drag_file(widget: i64, provider: f64) {
    let key = callback::register(provider);
    let (text, _, url) = ensure_keys(widget);
    let keys = (text, key, url);
    store_keys(widget, keys);
    install_drag_source(widget, keys);
}

/// Register `widget` as a drag source offering a web URL. `provider` returns
/// the URL string to carry.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_drag_url(widget: i64, provider: f64) {
    let key = callback::register(provider);
    let (text, file, _) = ensure_keys(widget);
    let keys = (text, file, key);
    store_keys(widget, keys);
    install_drag_source(widget, keys);
}

// --- JNI entry points --------------------------------------------------------

/// Called from `PerryBridge` on `ACTION_DROP`. Builds the `{ text?, files?,
/// urls? }` object from whichever Java arguments are non-null/non-empty and
/// invokes the registered drop callback. `text` may be null; `files` and
/// `urls` may be null or empty arrays. Runs on the UI thread, so it pumps
/// microtasks afterwards (matching every other callback in this crate).
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeInvokeDropCallback(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    key: jni::sys::jlong,
    text: jni::objects::JString,
    files: jni::objects::JObjectArray,
    urls: jni::objects::JObjectArray,
) {
    let Some(closure_f64) = callback::get(key as i64) else {
        return;
    };
    let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) } as *const u8;
    if closure_ptr.is_null() {
        return;
    }

    // Read the optional `text` representation.
    let text_str: Option<String> = if text.is_null() {
        None
    } else {
        env.get_string(&text).map(|s| s.into()).ok()
    };

    let files_vec = read_string_array(&mut env, &files);
    let urls_vec = read_string_array(&mut env, &urls);

    unsafe {
        // Up to three fields: text, files, urls.
        let obj = js_object_alloc(0, 3);
        if obj.is_null() {
            return;
        }
        if let Some(t) = text_str {
            js_object_set_field_by_name(obj, js_key(b"text"), nanbox_str(&t));
        }
        if !files_vec.is_empty() {
            let arr = build_string_array(&files_vec);
            js_object_set_field_by_name(obj, js_key(b"files"), js_nanbox_pointer(arr as i64));
        }
        if !urls_vec.is_empty() {
            let arr = build_string_array(&urls_vec);
            js_object_set_field_by_name(obj, js_key(b"urls"), js_nanbox_pointer(arr as i64));
        }
        let payload = js_nanbox_pointer(obj as i64);
        js_closure_call1(closure_ptr, payload);
    }
    pump_microtasks();
}

/// Called from `PerryBridge` when a drag is starting, once per registered
/// representation. `key` is the provider key handed to `setDragSource`. Invokes
/// the provider closure (0 args), converts its return value to a string, and
/// returns it to Java as the payload for that representation. Returns an empty
/// string if the provider is missing or yields a non-string. Runs on the UI
/// thread, so it pumps microtasks afterwards.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeInvokeDragProvider<'local>(
    mut env: jni::JNIEnv<'local>,
    _class: jni::objects::JClass<'local>,
    key: jni::sys::jlong,
) -> jni::objects::JString<'local> {
    let payload = drag_provider_payload(key as i64).unwrap_or_default();
    pump_microtasks();
    env.new_string(payload).unwrap_or_default()
}

/// Invoke the provider closure registered under `key` and convert its return
/// value to a Rust string. `None` if the closure is gone, is the sentinel `0`,
/// or returned `undefined`/`null`.
fn drag_provider_payload(key: i64) -> Option<String> {
    let closure_f64 = callback::get(key)?;
    let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) } as *const u8;
    if closure_ptr.is_null() {
        return None;
    }
    unsafe {
        let ret = js_closure_call0(closure_ptr);
        if ret.to_bits() == TAG_UNDEFINED {
            return None;
        }
        let sh = js_jsvalue_to_string(ret);
        if sh.is_null() {
            None
        } else {
            Some(str_from_header(sh).to_string())
        }
    }
}

// --- helpers -----------------------------------------------------------------

/// Read a `String[]` JNI array into a `Vec<String>`, skipping null elements.
fn read_string_array(env: &mut jni::JNIEnv, arr: &jni::objects::JObjectArray) -> Vec<String> {
    let mut out = Vec::new();
    if arr.is_null() {
        return out;
    }
    if let Ok(len) = env.get_array_length(arr) {
        for i in 0..len {
            if let Ok(item) = env.get_object_array_element(arr, i) {
                if item.is_null() {
                    continue;
                }
                let jstr: jni::objects::JString = item.into();
                let s: Result<String, _> = env.get_string(&jstr).map(Into::into);
                if let Ok(s) = s {
                    out.push(s);
                }
            }
        }
    }
    out
}

/// Build a NaN-boxed Perry array of NaN-boxed Perry strings.
unsafe fn build_string_array(items: &[String]) -> *mut c_void {
    let mut arr = js_array_alloc(items.len() as u32);
    for s in items {
        arr = js_array_push_f64(arr, nanbox_str(s));
    }
    arr
}
