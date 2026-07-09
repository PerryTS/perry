use jni::objects::JValue;
use std::cell::RefCell;

use crate::jni_bridge;
use crate::widgets;

extern "C" {
    fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
    fn js_stdlib_process_pending();
}

thread_local! {
    static PENDING_CONFIG: RefCell<Option<AppConfig>> = RefCell::new(None);
    static PENDING_BODY: RefCell<Option<i64>> = RefCell::new(None);
}

struct AppConfig {
    _title: String,
    _width: f64,
    _height: f64,
}

/// Extract a &str from a *const StringHeader pointer (Perry runtime string format).
pub fn str_from_header(ptr: *const u8) -> &'static str {
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

/// Create an app. Stores config for deferred creation. Returns app handle (i64).
pub fn app_create(title_ptr: *const u8, width: f64, height: f64) -> i64 {
    let title = if title_ptr.is_null() {
        "Perry App".to_string()
    } else {
        str_from_header(title_ptr).to_string()
    };

    let w = if width > 0.0 { width } else { 400.0 };
    let h = if height > 0.0 { height } else { 300.0 };

    PENDING_CONFIG.with(|c| {
        *c.borrow_mut() = Some(AppConfig {
            _title: title,
            _width: w,
            _height: h,
        });
    });

    1 // Single app handle
}

/// Global body handle — set from any thread, read from any thread.
/// Workaround: Perry's App() intrinsic extracts the body handle using
/// js_nanbox_get_pointer, which returns 0 for NaN-boxed integers (widget handles
/// are small integers, not heap pointers). We store the last widget added via
/// widgetAddChild as a fallback.
static GLOBAL_BODY: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// Whether app_set_body has been called (initial build complete).
/// Widget handle truncation in clear_children is only safe AFTER the initial build,
/// because during init, clearChildren may be called on non-root containers (e.g.
/// refreshing a list) while sibling widgets haven't been created yet.
static APP_INITIALIZED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Set the root widget (body) of the app.
pub fn app_set_body(_app_handle: i64, root_handle: i64) {
    unsafe {
        __android_log_print(
            3,
            b"PerryApp\0".as_ptr(),
            b"app_set_body: root_handle=%lld\0".as_ptr(),
            root_handle,
        );
    }
    if root_handle > 0 {
        GLOBAL_BODY.store(root_handle, std::sync::atomic::Ordering::Relaxed);
    }
    APP_INITIALIZED.store(true, std::sync::atomic::Ordering::Relaxed);
    PENDING_BODY.with(|b| {
        *b.borrow_mut() = Some(root_handle);
    });
}

/// Called from widgetClearChildren to track which handle is the root.
/// The first widget that has clearChildren called on it is likely the root.
pub fn track_root_candidate(handle: i64) {
    // Only set if not already set by app_set_body
    let current = GLOBAL_BODY.load(std::sync::atomic::Ordering::Relaxed);
    if current == 0 {
        GLOBAL_BODY.store(handle, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Get the current root body handle (used by clear_children to decide whether to truncate).
pub fn get_root_handle() -> i64 {
    GLOBAL_BODY.load(std::sync::atomic::Ordering::Relaxed)
}

/// Whether the app has completed its initial build (app_set_body called).
pub fn is_initialized() -> bool {
    APP_INITIALIZED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Attach the root widget to the Activity's content view.
/// Called from the native init thread after all widgets are built.
/// Posts to the UI thread to add the root view to the FrameLayout.
fn attach_root_to_activity() {
    unsafe {
        __android_log_print(
            3,
            b"PerryApp\0".as_ptr(),
            b"attach_root_to_activity: called\0".as_ptr(),
        );
    }
    // Get root handle: prefer PENDING_BODY, fall back to GLOBAL_BODY
    let mut root_handle = PENDING_BODY.with(|b| b.borrow().unwrap_or(0));
    // If body handle is invalid (0), use the global fallback
    if root_handle <= 0 {
        root_handle = GLOBAL_BODY.load(std::sync::atomic::Ordering::Relaxed);
    }
    unsafe {
        __android_log_print(
            3,
            b"PerryApp\0".as_ptr(),
            b"attach_root_to_activity: final root_handle=%lld\0".as_ptr(),
            root_handle,
        );
    }
    if root_handle > 0 {
        if let Some(root_ref) = widgets::get_widget(root_handle) {
            let mut env = jni_bridge::get_env();
            let root_obj = root_ref.as_obj();
            let bridge_class = jni_bridge::with_cache(|c| {
                env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap()
            });
            let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
            let _ = env.call_static_method(
                bridge_cls,
                "setContentView",
                "(Landroid/view/View;)V",
                &[JValue::Object(root_obj)],
            );
            unsafe {
                __android_log_print(
                    3,
                    b"PerryApp\0".as_ptr(),
                    b"attach_root_to_activity: setContentView called\0".as_ptr(),
                );
            }
        }
    }
}

extern "C" {
    fn js_callback_timer_tick() -> i32;
    fn js_interval_timer_tick() -> i32;
    fn js_promise_run_microtasks() -> i32;
    fn js_frame_pump_default() -> i32;
    // perry-runtime's embedder GC stepper: spends up to `budget_us`
    // advancing an ACTIVE budgeted collection in bounded work units; a
    // cheap status probe when no cycle is active (out=null is allowed).
    fn js_gc_step_us(budget_us: u64, out: *mut u8) -> u32;
}

/// Start the timer pump that drives setInterval/setTimeout/Promise callbacks.
/// Uses PerryBridge.startPumpTimer to create a Handler-based repeating timer
/// on the UI thread. Timer state is now global (Mutex-protected, not TLS),
/// so timers registered on the perry-native thread are visible from the UI thread.
fn start_timer_pump() {
    unsafe {
        __android_log_print(
            3,
            b"PerryApp\0".as_ptr(),
            b"start_timer_pump: setting up 8ms pump\0".as_ptr(),
        );
    }

    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();

    // Register the pump timer — fires every 8ms, calls nativePumpTick on UI thread
    let _ = env.call_static_method(
        bridge_cls,
        "startPumpTimer",
        "(J)V",
        &[jni::objects::JValue::Long(8)], // 8ms interval
    );

    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
}

/// Counter for throttled pump tick logging
static PUMP_TICK_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Called from JNI on every pump tick (8ms). Drives the Perry runtime timers.
/// Timer state is global (Mutex-protected), so this works from the UI thread
/// even though timers were registered on the perry-native thread.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativePumpTick(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
) {
    let count = PUMP_TICK_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if count < 5 || count % 500 == 0 {
        unsafe {
            __android_log_print(
                3,
                b"PerryPump\0".as_ptr(),
                b"nativePumpTick count=%llu\0".as_ptr(),
                count,
            );
        }
    }
    unsafe {
        js_stdlib_process_pending();
        js_callback_timer_tick();
        js_interval_timer_tick();
        // Issue #1865: perry/ui `onFrame` display-link callbacks. Real
        // Choreographer.postFrameCallback wiring is a follow-up; for now
        // the same 8ms pump drives one-shot frame callbacks.
        js_frame_pump_default();
        js_promise_run_microtasks();
        // #6183 (2026-07-09 GC audit): spend a bounded idle budget on any
        // active budgeted GC cycle at the pump boundary — the JS stack is
        // fully unwound here, so this is a precise-root safepoint. A 2 ms
        // slice per 8 ms tick drains collection debt incrementally instead of
        // letting an alloc-point collection land unbounded on this (UI)
        // thread mid-interaction.
        js_gc_step_us(2_000, std::ptr::null_mut());
    }
    // perry/media (#351) — drive state polling on the UI thread so the
    // PLAYERS thread_local stays consistent with where create_player
    // stored entries. Internally throttled to ~10 Hz.
    crate::media_playback::pump_tick();
    #[cfg(feature = "geisterhand")]
    {
        extern "C" {
            fn perry_geisterhand_pump();
        }
        unsafe {
            perry_geisterhand_pump();
        }
    }
}

/// Run the app event loop.
/// On Android, the event loop is the Activity lifecycle managed by the system.
/// This just attaches the root widget to the Activity and starts the timer pump.
/// Unlike macOS/iOS, this does NOT block — the Activity keeps running.
pub fn app_run(_app_handle: i64) {
    unsafe {
        __android_log_print(3, b"PerryApp\0".as_ptr(), b"app_run: called\0".as_ptr());
    }

    // Phase 2 v3.3: register cross-platform showToast / setText handlers.
    // Forwards perry_arkts_show_toast → PerryBridge.showToast (UI-thread
    // Toast.makeText) and perry_arkts_set_text → TextView.setText via JNI.
    register_cross_platform_text_handlers();

    // Attach the root widget to the Activity
    attach_root_to_activity();

    // Start the timer pump for setInterval/setTimeout/Promise callbacks.
    // Timer state is now global (Mutex), so the UI thread pump can see
    // timers registered by the perry-native thread.
    start_timer_pump();

    // Register UI function pointers for geisterhand dispatch
    #[cfg(feature = "geisterhand")]
    {
        extern "C" {
            fn perry_geisterhand_register_state_set(f: extern "C" fn(i64, f64));
            fn perry_geisterhand_register_screenshot_capture(
                f: extern "C" fn(*mut usize) -> *mut u8,
            );
            fn perry_geisterhand_register_textfield_set_string(f: extern "C" fn(i64, i64));
            fn perry_geisterhand_register_apply_style(
                f: extern "C" fn(i64, u32, f64, f64, f64, f64),
            );
        }
        unsafe {
            perry_geisterhand_register_state_set(crate::perry_ui_state_set);
            perry_geisterhand_register_screenshot_capture(
                crate::screenshot::perry_ui_screenshot_capture,
            );
            perry_geisterhand_register_textfield_set_string(crate::perry_ui_textfield_set_string);
            perry_geisterhand_register_apply_style(crate::geisterhand_style::apply_style);
        }
    }

    // On Android we must NOT block — the Activity lifecycle IS the event loop.
}

// ============================================================================
// Phase 2 v3.3: cross-platform showToast / setText wiring.
// ============================================================================

extern "C" {
    fn js_register_show_toast_handler(f: extern "C" fn(msg_ptr: *const u8, msg_len: usize));
    fn js_register_set_text_handler(
        f: extern "C" fn(id_ptr: *const u8, id_len: usize, val_ptr: *const u8, val_len: usize),
    );
    fn js_register_text_id_handler(
        f: extern "C" fn(widget_handle: i64, id_ptr: *const u8, id_len: usize),
    );
    /// Issue #535 Layer 2 — `js_state_set` calls this for every NavStack
    /// route bound to the changed state's synth id. Defined in
    /// `perry-runtime/src/ui_text_registry.rs`'s `NAVSTACK_REGISTRY` block.
    fn js_register_widget_hidden_handler(f: extern "C" fn(widget_handle: i64, hidden: i32));
}

extern "C" fn navstack_set_widget_hidden(widget_handle: i64, hidden: i32) {
    crate::widgets::set_hidden(widget_handle, hidden != 0);
}

fn register_cross_platform_text_handlers() {
    unsafe {
        js_register_show_toast_handler(widgets::toast::show_toast_handler);
        js_register_set_text_handler(widgets::text_registry::set_text_handler);
        js_register_text_id_handler(widgets::text_registry::register_text_id_handler);
        js_register_widget_hidden_handler(navstack_set_widget_hidden);
    }
}

/// Called when the Activity is destroyed. No-op since App() doesn't block on Android.
pub fn signal_shutdown() {
    // Nothing to do — App() returns immediately on Android.
}

/// Set minimum window size (no-op on Android).
pub fn set_min_size(_app_handle: i64, _w: f64, _h: f64) {
    // No-op on Android
}

/// Set maximum window size (no-op on Android).
pub fn set_max_size(_app_handle: i64, _w: f64, _h: f64) {
    // No-op on Android
}

/// Add a keyboard shortcut.
/// On Android, this is handled via dispatchKeyEvent in the Activity.
/// For now, store the binding and the Activity will check against it.
pub fn add_keyboard_shortcut(_key_ptr: *const u8, _modifiers: f64, _callback: f64) {
    // Stub — Android hardware keyboard shortcuts are uncommon.
    // Could be implemented via onKeyDown in PerryActivity if needed.
}

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
}

thread_local! {
    static ON_ACTIVATE_CALLBACK: RefCell<Option<f64>> = RefCell::new(None);
    static ON_TERMINATE_CALLBACK: RefCell<Option<f64>> = RefCell::new(None);
}

/// Set a repeating timer via PerryBridge.setTimer.
pub fn set_timer(interval_ms: f64, callback: f64) {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let cb_key = crate::callback::register(callback);
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());

    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(
        bridge_cls,
        "setTimer",
        "(JJ)V",
        &[JValue::Long(cb_key), JValue::Long(interval_ms as i64)],
    );

    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
}

/// Register callback for app activation (resume).
pub fn on_activate(callback: f64) {
    ON_ACTIVATE_CALLBACK.with(|c| {
        *c.borrow_mut() = Some(callback);
    });
}

/// Register callback for app termination (destroy).
pub fn on_terminate(callback: f64) {
    ON_TERMINATE_CALLBACK.with(|c| {
        *c.borrow_mut() = Some(callback);
    });
}

/// Called from JNI when Activity resumes.
pub fn handle_activate() {
    ON_ACTIVATE_CALLBACK.with(|c| {
        if let Some(callback) = *c.borrow() {
            let ptr = callback.to_bits() as *const u8;
            unsafe {
                js_closure_call1(ptr, 0.0);
            }
        }
    });
}

/// Called from JNI when Activity is destroyed.
pub fn handle_terminate() {
    ON_TERMINATE_CALLBACK.with(|c| {
        if let Some(callback) = *c.borrow() {
            let ptr = callback.to_bits() as *const u8;
            unsafe {
                js_closure_call1(ptr, 0.0);
            }
        }
    });
}
