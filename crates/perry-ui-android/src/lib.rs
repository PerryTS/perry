pub mod app;
pub mod audio;
pub mod callback;
pub mod camera;
pub mod clipboard;
pub mod dialog;
pub mod fetch;
pub mod file_dialog;
#[cfg(feature = "geisterhand")]
pub mod geisterhand_style;
pub mod jni_bridge;
pub mod json;
pub mod keychain;
pub mod location;
pub mod menu;
#[cfg(feature = "geisterhand")]
pub mod screenshot;
pub mod sheet;
pub mod state;
pub mod stdlib_stubs;
pub mod system;
pub mod toolbar;
pub mod widgets;
pub mod window;
pub mod ws;

// =============================================================================
// JNI lifecycle
// =============================================================================

extern "C" {
    fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
    fn mallopt(param: i32, value: i32) -> i32;
}

pub fn log_debug(msg: &str) {
    let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
    unsafe {
        __android_log_print(
            3,
            b"PerryDebug\0".as_ptr(),
            b"%s\0".as_ptr(),
            c_msg.as_ptr(),
        );
    }
}

/// Catch panics from widget functions, log them, and return 0 instead of aborting.
fn catch_panic(name: &str, f: impl FnOnce() -> i64 + std::panic::UnwindSafe) -> i64 {
    match std::panic::catch_unwind(f) {
        Ok(h) => h,
        Err(e) => {
            let detail = if let Some(s) = e.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else {
                "<unknown>".to_string()
            };
            let msg = format!("{} panicked: {}\0", name, detail);
            unsafe {
                __android_log_print(6, b"PerryJNI\0".as_ptr(), b"%s\0".as_ptr(), msg.as_ptr());
            }
            0
        }
    }
}

/// Catch panics from void widget functions, log them instead of aborting.
fn catch_panic_void(name: &str, f: impl FnOnce() + std::panic::UnwindSafe) {
    if let Err(e) = std::panic::catch_unwind(f) {
        let detail = if let Some(s) = e.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = e.downcast_ref::<String>() {
            s.clone()
        } else {
            "<unknown>".to_string()
        };
        let msg = format!("{} panicked: {}\0", name, detail);
        unsafe {
            __android_log_print(6, b"PerryJNI\0".as_ptr(), b"%s\0".as_ptr(), msg.as_ptr());
        }
    }
}

/// Called by the JVM when the native library is loaded via System.loadLibrary().
#[no_mangle]
pub extern "C" fn JNI_OnLoad(vm: jni::JavaVM, _reserved: *mut std::ffi::c_void) -> jni::sys::jint {
    unsafe {
        __android_log_print(
            3,
            b"PerryJNI\0".as_ptr(),
            b"JNI_OnLoad: starting\0".as_ptr(),
        );
    }

    // Disable MTE (Memory Tagging Extension) tagged addresses.
    // Perry's NaN-boxing uses 48-bit pointers (POINTER_MASK = 0x0000_FFFF_FFFF_FFFF).
    // Android's MTE puts a tag in the top byte, making pointers 56 bits.
    // When NaN-boxed pointers are extracted, the MTE tag is lost, causing crashes.
    // Disabling tagged addresses makes the allocator use standard 48-bit pointers.
    // Disable heap tagging (MTE/TBI) for the allocator.
    // Perry's NaN-boxing uses 48-bit pointers (POINTER_MASK = 0x0000_FFFF_FFFF_FFFF).
    // Android's scudo allocator tags pointers with a top byte (e.g., 0xb4...),
    // which breaks NaN-boxing when the tag is stripped.
    // mallopt(M_BIONIC_SET_HEAP_TAGGING_LEVEL, 0) disables tagging for NEW allocations
    // without breaking the JVM (which keeps its own tagged pointers).
    #[cfg(target_os = "android")]
    unsafe {
        // M_BIONIC_SET_HEAP_TAGGING_LEVEL = -204, level 0 = no tagging
        let ret = mallopt(-204, 0);
        __android_log_print(
            3,
            b"PerryJNI\0".as_ptr(),
            b"JNI_OnLoad: mallopt(-204, 0) returned %d\0".as_ptr(),
            ret,
        );
    }

    jni_bridge::init_vm(vm);
    unsafe {
        __android_log_print(3, b"PerryJNI\0".as_ptr(), b"JNI_OnLoad: done\0".as_ptr());
    }
    jni::sys::JNI_VERSION_1_6
}

/// Called from PerryActivity after the native library is loaded.
/// Initializes the JNI cache on the calling thread.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeInit(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
) {
    jni_bridge::init_cache(&mut env);
}

/// Called from PerryActivity when the Activity is being destroyed.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeShutdown(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
) {
    app::signal_shutdown();
}

#[cfg(not(test))]
extern "C" {
    fn main() -> i32;
}

// js_stdlib_init_dispatch and js_stdlib_process_pending — now provided by perry-runtime

/// Called from the native thread to run the compiled TypeScript entry point.
/// This wraps the compiler-generated `main()` function as a JNI method on PerryBridge,
/// so the Activity doesn't need its own native method (which would require package-specific JNI names).
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeMain(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
) {
    // Set CWD to the app's internal files directory so that relative paths
    // (e.g. SQLite databases like "mango.db") resolve to a writable location.
    {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);
        if let Ok(activity) = env.call_static_method(
            "com/perry/app/PerryBridge",
            "getActivity",
            "()Landroid/app/Activity;",
            &[],
        ) {
            if let Ok(act_obj) = activity.l() {
                if let Ok(files_dir) =
                    env.call_method(&act_obj, "getFilesDir", "()Ljava/io/File;", &[])
                {
                    if let Ok(fd_obj) = files_dir.l() {
                        if let Ok(abs_val) =
                            env.call_method(&fd_obj, "getAbsolutePath", "()Ljava/lang/String;", &[])
                        {
                            if let Ok(abs_obj) = abs_val.l() {
                                if let Ok(path_str) = env.get_string((&abs_obj).into()) {
                                    let path: String = path_str.into();
                                    let _ = std::fs::create_dir_all(&path);
                                    let _ = std::env::set_current_dir(&path);
                                }
                            }
                        }
                    }
                }
            }
        }
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }

    unsafe {
        __android_log_print(
            3,
            b"PerryJNI\0".as_ptr(),
            b"nativeMain: calling main()\0".as_ptr(),
        );
        main();
        __android_log_print(
            3,
            b"PerryJNI\0".as_ptr(),
            b"nativeMain: main() returned, parking thread\0".as_ptr(),
        );
    }

    // Park this thread forever — do NOT let it exit.
    // Module-level arrays/objects are allocated on this thread's arena.
    // If the thread exits, the arena's Drop frees all blocks, turning
    // every module-level pointer into a dangling reference. The UI thread's
    // pump ticks call into compiled functions (getLevelInfo etc.) that read
    // these pointers — segfault if the arena was freed.
    loop {
        std::thread::park();
    }
}

// =============================================================================
// FFI exports — identical signatures to perry-ui-macos and perry-ui-ios
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_app_create(title_ptr: i64, width: f64, height: f64) -> i64 {
    app::app_create(title_ptr as *const u8, width, height)
}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_body(app_handle: i64, root_handle: i64) {
    app::app_set_body(app_handle, root_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_app_run(app_handle: i64) {
    app::app_run(app_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_text_create(text_ptr: i64) -> i64 {
    catch_panic("perry_ui_text_create", || {
        widgets::text::create(text_ptr as *const u8)
    })
}

#[no_mangle]
pub extern "C" fn perry_ui_button_create(label_ptr: i64, on_press: f64) -> i64 {
    catch_panic("perry_ui_button_create", || {
        widgets::button::create(label_ptr as *const u8, on_press)
    })
}

#[no_mangle]
pub extern "C" fn perry_ui_vstack_create(spacing: f64) -> i64 {
    catch_panic("perry_ui_vstack_create", || {
        widgets::vstack::create(spacing)
    })
}

#[no_mangle]
pub extern "C" fn perry_ui_hstack_create(spacing: f64) -> i64 {
    catch_panic("perry_ui_hstack_create", || {
        widgets::hstack::create(spacing)
    })
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_add_child(parent_handle: i64, child_handle: i64) {
    catch_panic_void("perry_ui_widget_add_child", || {
        widgets::add_child(parent_handle, child_handle)
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_state_create(initial: f64) -> i64 {
    state::state_create(initial)
}

#[no_mangle]
pub extern "C" fn perry_ui_state_get(state_handle: i64) -> f64 {
    state::state_get(state_handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_state_set(state_handle: i64, value: f64) {
    state::state_set(state_handle, value);
}

#[no_mangle]
pub extern "C" fn perry_ui_state_bind_text_numeric(
    state_handle: i64,
    text_handle: i64,
    prefix_ptr: i64,
    suffix_ptr: i64,
) {
    state::bind_text_numeric(
        state_handle,
        text_handle,
        prefix_ptr as *const u8,
        suffix_ptr as *const u8,
    );
}

#[no_mangle]
pub extern "C" fn perry_ui_spacer_create() -> i64 {
    widgets::spacer::create()
}

#[no_mangle]
pub extern "C" fn perry_ui_divider_create() -> i64 {
    widgets::divider::create()
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_create(placeholder_ptr: i64, on_change: f64) -> i64 {
    widgets::textfield::create(placeholder_ptr as *const u8, on_change)
}

#[no_mangle]
pub extern "C" fn perry_ui_toggle_create(label_ptr: i64, on_change: f64) -> i64 {
    widgets::toggle::create(label_ptr as *const u8, on_change)
}

#[no_mangle]
pub extern "C" fn perry_ui_slider_create(min: f64, max: f64, on_change: f64) -> i64 {
    // Codegen emits 3-arg `Slider(min, max, onChange)`; default initial=min.
    widgets::slider::create(min, max, min, on_change)
}

// =============================================================================
// Phase 4: Advanced Reactive UI
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_state_bind_slider(state_handle: i64, slider_handle: i64) {
    state::bind_slider(state_handle, slider_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_state_bind_toggle(state_handle: i64, toggle_handle: i64) {
    state::bind_toggle(state_handle, toggle_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_state_bind_text_template(
    text_handle: i64,
    num_parts: i32,
    types_ptr: i64,
    values_ptr: i64,
) {
    state::bind_text_template(
        text_handle,
        num_parts,
        types_ptr as *const i32,
        values_ptr as *const i64,
    );
}

#[no_mangle]
pub extern "C" fn perry_ui_state_bind_visibility(
    state_handle: i64,
    show_handle: i64,
    hide_handle: i64,
) {
    state::bind_visibility(state_handle, show_handle, hide_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_set_widget_hidden(handle: i64, hidden: i64) {
    widgets::set_hidden(handle, hidden != 0);
}

#[no_mangle]
pub extern "C" fn perry_ui_for_each_init(
    container_handle: i64,
    state_handle: i64,
    render_closure: f64,
) {
    state::for_each_init(container_handle, state_handle, render_closure);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_clear_children(handle: i64) {
    widgets::clear_children(handle);
}

// =============================================================================
// Phase A.1: Text Mutation & Layout Control
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_text_set_string(handle: i64, text_ptr: i64) {
    widgets::text::set_string(handle, text_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_vstack_create_with_insets(
    spacing: f64,
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
) -> i64 {
    widgets::vstack::create_with_insets(spacing, top, left, bottom, right)
}

#[no_mangle]
pub extern "C" fn perry_ui_hstack_create_with_insets(
    spacing: f64,
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
) -> i64 {
    widgets::hstack::create_with_insets(spacing, top, left, bottom, right)
}

// =============================================================================
// Phase A.2: ScrollView, Clipboard & Keyboard Shortcuts
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_create() -> i64 {
    unsafe {
        __android_log_print(
            3,
            b"PerryJNI\0".as_ptr(),
            b"perry_ui_scrollview_create: called\0".as_ptr(),
        );
    }
    let h = widgets::scrollview::create();
    unsafe {
        __android_log_print(
            3,
            b"PerryJNI\0".as_ptr(),
            b"perry_ui_scrollview_create: returned handle=%lld\0".as_ptr(),
            h,
        );
    }
    h
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_child(scroll_handle: i64, child_handle: i64) {
    widgets::scrollview::set_child(scroll_handle, child_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_clipboard_read() -> f64 {
    clipboard::read()
}

#[no_mangle]
pub extern "C" fn perry_ui_clipboard_write(text_ptr: i64) {
    clipboard::write(text_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_add_keyboard_shortcut(key_ptr: i64, modifiers: f64, callback: f64) {
    app::add_keyboard_shortcut(key_ptr as *const u8, modifiers, callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_register_global_hotkey(_key: i64, _mods: f64, _cb: f64) {}

#[no_mangle]
pub extern "C" fn perry_system_get_app_icon(_path: i64) -> i64 {
    0
}

// =============================================================================
// Phase A.3: Text Styling & Button Styling
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_text_set_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::text::set_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_size(handle: i64, size: f64) {
    widgets::text::set_font_size(handle, size);
}

#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_weight(handle: i64, size: f64, weight: f64) {
    widgets::text::set_font_weight(handle, size, weight);
}

#[no_mangle]
pub extern "C" fn perry_ui_text_set_selectable(handle: i64, selectable: f64) {
    widgets::text::set_selectable(handle, selectable != 0.0);
}

/// Text decoration (issue #185 Phase B). 0=none, 1=underline, 2=strikethrough.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_decoration(handle: i64, decoration: i64) {
    widgets::text::set_decoration(handle, decoration);
}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_bordered(handle: i64, bordered: f64) {
    widgets::button::set_bordered(handle, bordered != 0.0);
}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_title(handle: i64, title_ptr: i64) {
    widgets::button::set_title(handle, title_ptr as *const u8);
}

// =============================================================================
// Phase A.4: Focus & Scroll-To
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_textfield_focus(handle: i64) {
    widgets::textfield::focus(handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_scroll_to(scroll_handle: i64, child_handle: i64) {
    widgets::scrollview::scroll_to(scroll_handle, child_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_get_offset(scroll_handle: i64) -> f64 {
    widgets::scrollview::get_offset(scroll_handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_offset(scroll_handle: i64, offset: f64) {
    widgets::scrollview::set_offset(scroll_handle, offset);
}

// =============================================================================
// Phase A.5: Context Menus, File Dialog & Window Sizing
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_menu_create() -> i64 {
    menu::create()
}

#[no_mangle]
pub extern "C" fn perry_ui_menu_add_item(menu_handle: i64, title_ptr: i64, callback: f64) {
    menu::add_item(menu_handle, title_ptr as *const u8, callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_context_menu(widget_handle: i64, menu_handle: i64) {
    menu::set_context_menu(widget_handle, menu_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_menu_add_item_with_shortcut(
    _menu_handle: i64,
    _title_ptr: i64,
    _shortcut_ptr: i64,
    _callback: f64,
) {
    // No-op on Android — no menu bar on mobile
}

#[no_mangle]
pub extern "C" fn perry_ui_menu_add_separator(_menu_handle: i64) {
    // No-op on Android
}

#[no_mangle]
pub extern "C" fn perry_ui_menu_add_submenu(
    _menu_handle: i64,
    _title_ptr: i64,
    _submenu_handle: i64,
) {
    // No-op on Android
}

#[no_mangle]
pub extern "C" fn perry_ui_menubar_create() -> i64 {
    0 // Stub — no menu bar on Android
}

#[no_mangle]
pub extern "C" fn perry_ui_menubar_add_menu(_bar_handle: i64, _title_ptr: i64, _menu_handle: i64) {
    // No-op on Android
}

#[no_mangle]
pub extern "C" fn perry_ui_menubar_attach(_bar_handle: i64) {
    // No-op on Android
}

/// Remove all items from a menu (no-op on Android).
#[no_mangle]
pub extern "C" fn perry_ui_menu_clear(_menu_handle: i64) {
    // No-op on Android
}

/// Add a menu item with a standard action (no-op on Android — macOS responder chain concept).
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_standard_action(
    _menu_handle: i64,
    _title_ptr: i64,
    _selector_ptr: i64,
    _shortcut_ptr: i64,
) {
    // No-op on Android
}

#[no_mangle]
pub extern "C" fn perry_ui_open_file_dialog(callback: f64) {
    file_dialog::open_dialog(callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_min_size(app_handle: i64, w: f64, h: f64) {
    app::set_min_size(app_handle, w, h);
}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_max_size(app_handle: i64, w: f64, h: f64) {
    app::set_max_size(app_handle, w, h);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_string(handle: i64, text_ptr: i64) {
    widgets::textfield::set_string_value(handle, text_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_add_child_at(parent_handle: i64, child_handle: i64, index: f64) {
    widgets::add_child_at(parent_handle, child_handle, index as i64);
}

// =============================================================================
// App Lifecycle (new)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_app_on_activate(callback: f64) {
    app::on_activate(callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_app_on_terminate(callback: f64) {
    app::on_terminate(callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_timer(_app_handle: i64, interval_ms: f64, callback: f64) {
    app::set_timer(interval_ms, callback);
}

// =============================================================================
// State Bindings (new)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_state_on_change(state_handle: i64, callback: f64) {
    state::on_change(state_handle, callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_state_bind_textfield(state_handle: i64, textfield_handle: i64) {
    state::bind_textfield(state_handle, textfield_handle);
}

// =============================================================================
// Text Styling (new)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_family(handle: i64, family_ptr: i64) {
    widgets::text::set_font_family(handle, family_ptr as *const u8);
}

// =============================================================================
// Widget Creation (new)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_securefield_create(placeholder_ptr: i64, on_change: f64) -> i64 {
    widgets::securefield::create(placeholder_ptr as *const u8, on_change)
}

#[no_mangle]
pub extern "C" fn perry_ui_progressview_create() -> i64 {
    widgets::progressview::create()
}

#[no_mangle]
pub extern "C" fn perry_ui_progressview_set_value(handle: i64, value: f64) {
    widgets::progressview::set_value(handle, value);
}

#[no_mangle]
pub extern "C" fn perry_ui_form_create() -> i64 {
    widgets::form::create()
}

#[no_mangle]
pub extern "C" fn perry_ui_section_create(title_ptr: i64) -> i64 {
    widgets::form::section_create(title_ptr as *const u8)
}

#[no_mangle]
pub extern "C" fn perry_ui_zstack_create() -> i64 {
    widgets::zstack::create()
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_create(width: f64, height: f64) -> i64 {
    widgets::canvas::create(width, height)
}

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_create(count: f64, render_closure: f64) -> i64 {
    widgets::lazyvstack::create(count, render_closure)
}

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_update(handle: i64, count: i64) {
    widgets::lazyvstack::update(handle, count);
}

// Table (stub — not yet implemented on Android)
#[no_mangle]
pub extern "C" fn perry_ui_table_create(_row_count: f64, _col_count: f64, _render: f64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_column_header(_handle: i64, _col: i64, _title_ptr: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_column_width(_handle: i64, _col: i64, _width: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_update_row_count(_handle: i64, _count: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_on_row_select(_handle: i64, _callback: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_get_selected_row(_handle: i64) -> i64 {
    -1
}

// =============================================================================
// Canvas
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_canvas_clear(handle: i64) {
    widgets::canvas::clear(handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_begin_path(handle: i64) {
    widgets::canvas::begin_path(handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_move_to(handle: i64, x: f64, y: f64) {
    widgets::canvas::move_to(handle, x, y);
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_line_to(handle: i64, x: f64, y: f64) {
    widgets::canvas::line_to(handle, x, y);
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_stroke(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
    line_width: f64,
) {
    widgets::canvas::stroke(handle, r, g, b, a, line_width);
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_fill_gradient(
    handle: i64,
    r1: f64,
    g1: f64,
    b1: f64,
    a1: f64,
    r2: f64,
    g2: f64,
    b2: f64,
    a2: f64,
    direction: f64,
) {
    widgets::canvas::fill_gradient(handle, r1, g1, b1, a1, r2, g2, b2, a2, direction);
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_set_fill_color(_h: i64, _r: f64, _g: f64, _b: f64, _a: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_set_stroke_color(_h: i64, _r: f64, _g: f64, _b: f64, _a: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_set_line_width(_h: i64, _w: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_fill_rect(_h: i64, _x: f64, _y: f64, _w: f64, _ht: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_stroke_rect(_h: i64, _x: f64, _y: f64, _w: f64, _ht: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_clear_rect(h: i64, _x: f64, _y: f64, _w: f64, _ht: f64) {
    widgets::canvas::clear(h);
}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_arc(_h: i64, _x: f64, _y: f64, _r: f64, _sa: f64, _ea: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_close_path(_h: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_fill(_h: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_stroke_path(_h: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_fill_text(_h: i64, _ptr: i64, _x: f64, _y: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_set_font(_h: i64, _ptr: i64) {}

// =============================================================================
// Picker
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_picker_create(label_ptr: i64, on_change: f64, style: i64) -> i64 {
    widgets::picker::create(label_ptr as *const u8, on_change, style)
}

#[no_mangle]
pub extern "C" fn perry_ui_picker_add_item(handle: i64, title_ptr: i64) {
    widgets::picker::add_item(handle, title_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_picker_set_selected(handle: i64, index: i64) {
    widgets::picker::set_selected(handle, index);
}

#[no_mangle]
pub extern "C" fn perry_ui_picker_get_selected(handle: i64) -> i64 {
    widgets::picker::get_selected(handle)
}

// =============================================================================
// Image
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_image_create_file(path_ptr: i64) -> i64 {
    widgets::image::create_file(path_ptr as *const u8)
}

#[no_mangle]
pub extern "C" fn perry_ui_image_create_symbol(name_ptr: i64) -> i64 {
    widgets::image::create_symbol(name_ptr as *const u8)
}

#[no_mangle]
pub extern "C" fn perry_ui_image_set_size(handle: i64, width: f64, height: f64) {
    widgets::image::set_size(handle, width, height);
}

#[no_mangle]
pub extern "C" fn perry_ui_image_set_tint(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::image::set_tint(handle, r, g, b, a);
}

// =============================================================================
// Navigation
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_navstack_create() -> i64 {
    // Matches the 0-arg dispatch in perry-dispatch::PERRY_UI_TABLE.
    widgets::navstack::create(std::ptr::null(), 0)
}

#[no_mangle]
pub extern "C" fn perry_ui_navstack_push(handle: i64, title_ptr: i64, body_handle: i64) {
    widgets::navstack::push(handle, title_ptr as *const u8, body_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_navstack_pop(handle: i64) {
    widgets::navstack::pop(handle);
}

// =============================================================================
// Styling (new)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_enabled(handle: i64, enabled: i64) {
    widgets::set_enabled(handle, enabled != 0);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_tooltip(handle: i64, text_ptr: i64) {
    widgets::set_tooltip(handle, text_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_control_size(handle: i64, size: i64) {
    widgets::set_control_size(handle, size);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_corner_radius(handle: i64, radius: f64) {
    widgets::set_corner_radius(handle, radius);
}

/// Set drop shadow via Material `setElevation` + (API 28+)
/// `setOutlineSpotShadowColor` / `setOutlineAmbientShadowColor`. See
/// `widgets::set_shadow` for the full mapping rationale (issue #185 Phase B).
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_shadow(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
    blur: f64,
    offset_x: f64,
    offset_y: f64,
) {
    widgets::set_shadow(handle, r, g, b, a, blur, offset_x, offset_y);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_background_color(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    widgets::set_background_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_background_gradient(
    handle: i64,
    r1: f64,
    g1: f64,
    b1: f64,
    a1: f64,
    r2: f64,
    g2: f64,
    b2: f64,
    a2: f64,
    direction: f64,
) {
    widgets::set_background_gradient(handle, r1, g1, b1, a1, r2, g2, b2, a2, direction);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_border_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    catch_panic_void("perry_ui_widget_set_border_color", || {
        widgets::set_border_color(handle, r, g, b, a)
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_border_width(handle: i64, width: f64) {
    catch_panic_void("perry_ui_widget_set_border_width", || {
        widgets::set_border_width(handle, width)
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_edge_insets(
    handle: i64,
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
) {
    catch_panic_void("perry_ui_widget_set_edge_insets", || {
        widgets::set_edge_insets(handle, top, left, bottom, right)
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_opacity(handle: i64, alpha: f64) {
    catch_panic_void("perry_ui_widget_set_opacity", || {
        widgets::set_opacity(handle, alpha)
    });
}

// =============================================================================
// Events (new)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_hover(handle: i64, callback: f64) {
    widgets::set_on_hover(handle, callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_double_click(handle: i64, callback: f64) {
    widgets::set_on_double_click(handle, callback);
}

// =============================================================================
// Animation (new)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_widget_animate_opacity(handle: i64, target: f64, duration_secs: f64) {
    widgets::animate_opacity(handle, target, duration_secs);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_animate_position(
    handle: i64,
    dx: f64,
    dy: f64,
    duration_secs: f64,
) {
    widgets::animate_position(handle, dx, dy, duration_secs);
}

// =============================================================================
// Dialog (new)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_save_file_dialog(
    callback: f64,
    default_name_ptr: i64,
    allowed_types_ptr: i64,
) {
    dialog::save_file_dialog(
        callback,
        default_name_ptr as *const u8,
        allowed_types_ptr as *const u8,
    );
}

#[no_mangle]
pub extern "C" fn perry_ui_alert(
    title_ptr: i64,
    message_ptr: i64,
    buttons_ptr: i64,
    callback: f64,
) {
    dialog::alert(
        title_ptr as *const u8,
        message_ptr as *const u8,
        buttons_ptr as *const u8,
        callback,
    );
}

// =============================================================================
// Sheet (new)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_sheet_create(width: f64, height: f64, title_val: f64) -> i64 {
    sheet::create(width, height, title_val)
}

#[no_mangle]
pub extern "C" fn perry_ui_sheet_present(sheet_handle: i64) {
    sheet::present(sheet_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_sheet_dismiss(sheet_handle: i64) {
    sheet::dismiss(sheet_handle);
}

// =============================================================================
// Multi-Window (new)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_window_create(title_ptr: i64, width: f64, height: f64) -> i64 {
    window::create(title_ptr as *const u8, width, height)
}

#[no_mangle]
pub extern "C" fn perry_ui_window_set_body(window_handle: i64, widget_handle: i64) {
    window::set_body(window_handle, widget_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_window_show(window_handle: i64) {
    window::show(window_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_window_close(window_handle: i64) {
    window::close(window_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_window_hide(_window: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_window_set_size(_window: i64, _w: f64, _h: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_window_on_focus_lost(_window: i64, _callback: f64) {}

// =============================================================================
// Toolbar (new)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_toolbar_create() -> i64 {
    toolbar::create()
}

#[no_mangle]
pub extern "C" fn perry_ui_toolbar_add_item(
    toolbar_handle: i64,
    label_ptr: i64,
    icon_ptr: i64,
    callback: f64,
) {
    toolbar::add_item(
        toolbar_handle,
        label_ptr as *const u8,
        icon_ptr as *const u8,
        callback,
    );
}

#[no_mangle]
pub extern "C" fn perry_ui_toolbar_attach(toolbar_handle: i64) {
    toolbar::attach(toolbar_handle);
}

// =============================================================================
// System API (new)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_system_open_url(url_ptr: i64) {
    system::open_url(url_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_system_is_dark_mode() -> i64 {
    system::is_dark_mode()
}

#[no_mangle]
pub extern "C" fn perry_system_preferences_set(key_ptr: i64, value: f64) {
    system::preferences_set(key_ptr as *const u8, value);
}

#[no_mangle]
pub extern "C" fn perry_system_preferences_get(key_ptr: i64) -> f64 {
    system::preferences_get(key_ptr as *const u8)
}

#[no_mangle]
pub extern "C" fn perry_system_keychain_save(key_ptr: i64, value_ptr: i64) {
    keychain::save(key_ptr as *const u8, value_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_system_keychain_get(key_ptr: i64) -> f64 {
    keychain::get(key_ptr as *const u8)
}

#[no_mangle]
pub extern "C" fn perry_system_keychain_delete(key_ptr: i64) {
    keychain::delete(key_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_system_notification_send(title_ptr: i64, body_ptr: i64) {
    system::notification_send(title_ptr as *const u8, body_ptr as *const u8);
}

/// Real impl (#95): kick off FCM token fetch + register the JS closure that
/// fires when FCM hands us a registration token. Requires a real
/// `google-services.json` to actually work — the placeholder bundled with
/// the template lets the build succeed but the SDK rejects it at runtime.
#[no_mangle]
pub extern "C" fn perry_system_notification_register_remote(callback: f64) {
    system::notification_register_remote(callback);
}

/// Real impl (#95): register the JS closure that fires for foreground FCM
/// payloads. `PerryFirebaseMessagingService.onMessageReceived` forwards
/// the JSON-serialized RemoteMessage to native via JNI.
#[no_mangle]
pub extern "C" fn perry_system_notification_on_receive(callback: f64) {
    system::notification_on_receive(callback);
}

/// Real impl (#98): register the JS closure that fires for background FCM
/// payloads. Routes through the same `PerryFirebaseMessagingService`
/// pipeline as foreground delivery — Android doesn't split the two at the
/// service layer — so the callback fires for every payload that reaches
/// `nativeNotificationBackgroundReceive`. See system.rs for the v1
/// trade-offs around Promise gating and cold-start.
#[no_mangle]
pub extern "C" fn perry_system_notification_on_background_receive(callback: f64) {
    system::notification_on_background_receive(callback);
}

/// Schedule a fire-after-N-seconds notification via AlarmManager (#96).
#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_interval(
    id_ptr: i64,
    title_ptr: i64,
    body_ptr: i64,
    seconds: f64,
    repeats: f64,
) {
    system::notification_schedule_interval(
        id_ptr as *const u8,
        title_ptr as *const u8,
        body_ptr as *const u8,
        seconds,
        repeats,
    );
}

/// Schedule a fire-at-wallclock-ms notification via AlarmManager (#96).
#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_calendar(
    id_ptr: i64,
    title_ptr: i64,
    body_ptr: i64,
    timestamp_ms: f64,
) {
    system::notification_schedule_calendar(
        id_ptr as *const u8,
        title_ptr as *const u8,
        body_ptr as *const u8,
        timestamp_ms,
    );
}

/// Logged no-op — Geofencing API requires `FUSED_LOCATION_PROVIDER` + a
/// runtime `ACCESS_FINE_LOCATION` grant. Deferred to #96 follow-up.
#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_location(
    id_ptr: i64,
    title_ptr: i64,
    body_ptr: i64,
    lat: f64,
    lon: f64,
    radius: f64,
) {
    system::notification_schedule_location(
        id_ptr as *const u8,
        title_ptr as *const u8,
        body_ptr as *const u8,
        lat,
        lon,
        radius,
    );
}

/// Cancel a scheduled or already-displayed notification by id (#96).
#[no_mangle]
pub extern "C" fn perry_system_notification_cancel(id_ptr: i64) {
    system::notification_cancel(id_ptr as *const u8);
}

/// Real impl (#97): register the tap callback so `PerryNotificationReceiver`
/// can dispatch back to it when the user taps a delivered notification.
#[no_mangle]
pub extern "C" fn perry_system_notification_on_tap(callback: f64) {
    system::notification_on_tap(callback);
}

#[no_mangle]
pub extern "C" fn perry_system_request_location(callback: f64) {
    location::request_location(callback);
}

#[no_mangle]
pub extern "C" fn perry_system_get_locale() -> i64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    let locale_class = env.find_class("java/util/Locale").expect("Locale class");
    let default_locale = env
        .call_static_method(locale_class, "getDefault", "()Ljava/util/Locale;", &[])
        .expect("getDefault")
        .l()
        .expect("locale obj");
    let lang = env
        .call_method(&default_locale, "getLanguage", "()Ljava/lang/String;", &[])
        .expect("getLanguage")
        .l()
        .expect("lang string");
    let jstr: jni::objects::JString = lang.into();
    let s: String = env.get_string(&jstr).expect("get string").into();
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    let bytes = s.as_bytes();
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    }
    unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64) as i64 }
}

// =============================================================================
// TabBar
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_tabbar_create(on_select: f64) -> i64 {
    catch_panic("perry_ui_tabbar_create", || {
        widgets::tabbar::create(on_select)
    })
}

#[no_mangle]
pub extern "C" fn perry_ui_tabbar_add_tab(tabbar_handle: i64, label_ptr: i64) {
    catch_panic_void("perry_ui_tabbar_add_tab", || {
        widgets::tabbar::add_tab(tabbar_handle, label_ptr as *const u8)
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_tabbar_set_selected(tabbar_handle: i64, index: i64) {
    catch_panic_void("perry_ui_tabbar_set_selected", || {
        widgets::tabbar::set_selected(tabbar_handle, index)
    });
}

// =============================================================================
// Additional widget functions
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_button_set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    catch_panic_void("perry_ui_button_set_text_color", || {
        widgets::button::set_text_color(handle, r, g, b, a)
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_image(handle: i64, name_ptr: i64) {
    catch_panic_void("perry_ui_button_set_image", || {
        widgets::button::set_image(handle, name_ptr as *const u8)
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_image_position(handle: i64, position: i64) {
    widgets::button::set_image_position(handle, position);
}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_content_tint_color(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    catch_panic_void("perry_ui_button_set_content_tint_color", || {
        widgets::button::set_content_tint_color(handle, r, g, b, a)
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_refresh_control(scroll_handle: i64, callback: f64) {
    catch_panic_void("perry_ui_scrollview_set_refresh_control", || {
        widgets::scrollview::set_refresh_control(scroll_handle, callback)
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_end_refreshing(scroll_handle: i64) {
    catch_panic_void("perry_ui_scrollview_end_refreshing", || {
        widgets::scrollview::end_refreshing(scroll_handle)
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_click(handle: i64, callback: f64) {
    catch_panic_void("perry_ui_widget_set_on_click", || {
        widgets::set_on_click(handle, callback)
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_hugging(handle: i64, priority: f64) {
    catch_panic_void("perry_ui_widget_set_hugging", || {
        widgets::set_hugging(handle, priority)
    });
}

// =============================================================================
// Layout functions (parity with iOS)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_width(handle: i64, width: f64) {
    catch_panic_void("perry_ui_widget_set_width", || {
        widgets::set_width(handle, width)
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_height(handle: i64, height: f64) {
    catch_panic_void("perry_ui_widget_set_height", || {
        widgets::set_height(handle, height)
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_remove_child(parent_handle: i64, child_handle: i64) {
    catch_panic_void("perry_ui_widget_remove_child", || {
        widgets::remove_child(parent_handle, child_handle)
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_reorder_child(
    parent_handle: i64,
    from_index: f64,
    to_index: f64,
) {
    catch_panic_void("perry_ui_widget_reorder_child", || {
        widgets::reorder_child(parent_handle, from_index as i64, to_index as i64)
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_match_parent_width(handle: i64) {
    catch_panic_void("perry_ui_widget_match_parent_width", || {
        widgets::match_parent_width(handle)
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_match_parent_height(handle: i64) {
    catch_panic_void("perry_ui_widget_match_parent_height", || {
        widgets::match_parent_height(handle)
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_stack_set_detaches_hidden(handle: i64, flag: i64) {
    widgets::set_detaches_hidden_views(handle, flag != 0);
}

#[no_mangle]
pub extern "C" fn perry_ui_stack_set_distribution(handle: i64, distribution: f64) {
    // On Android LinearLayout, distribution maps to weight distribution.
    // 0=Fill (default), 1=FillEqually — set all children to equal weight.
    // Other values are no-ops since Android doesn't have direct equivalents.
    if distribution as i64 == 1 {
        // FillEqually: set all children to weight=1
        if let Some(view_ref) = widgets::get_widget(handle) {
            let mut env = jni_bridge::get_env();
            let _ = env.push_local_frame(32);
            let child_count = env
                .call_method(view_ref.as_obj(), "getChildCount", "()I", &[])
                .map(|v| v.i().unwrap_or(0))
                .unwrap_or(0);
            for i in 0..child_count {
                let child = env.call_method(
                    view_ref.as_obj(),
                    "getChildAt",
                    "(I)Landroid/view/View;",
                    &[jni::objects::JValue::Int(i)],
                );
                if let Ok(child_val) = child {
                    if let Ok(child_obj) = child_val.l() {
                        if !child_obj.is_null() {
                            if let Ok(lp) = env.call_method(
                                &child_obj,
                                "getLayoutParams",
                                "()Landroid/view/ViewGroup$LayoutParams;",
                                &[],
                            ) {
                                if let Ok(lp_obj) = lp.l() {
                                    if !lp_obj.is_null() {
                                        if env
                                            .is_instance_of(
                                                &lp_obj,
                                                "android/widget/LinearLayout$LayoutParams",
                                            )
                                            .unwrap_or(false)
                                        {
                                            let _ = env.set_field(
                                                &lp_obj,
                                                "weight",
                                                "F",
                                                jni::objects::JValue::Float(1.0),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            unsafe {
                env.pop_local_frame(&jni::objects::JObject::null());
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn perry_ui_stack_set_alignment(handle: i64, alignment: f64) {
    // On Android LinearLayout, alignment maps to gravity on the cross-axis.
    // iOS/macOS alignment values: 0=Fill, 1=Leading, 3=Center, 4=Trailing
    // For HStack (horizontal), cross-axis is vertical: TOP=48, CENTER_VERTICAL=16, BOTTOM=80
    // For VStack (vertical), cross-axis is horizontal: LEFT=3, CENTER_HORIZONTAL=1, RIGHT=5
    // Fill (0) means children stretch to fill the cross-axis — we don't set gravity
    // so that MATCH_PARENT on children takes effect.
    if let Some(view_ref) = widgets::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);

        // Determine orientation: 0=HORIZONTAL (HStack), 1=VERTICAL (VStack)
        let orientation = env
            .call_method(view_ref.as_obj(), "getOrientation", "()I", &[])
            .map(|v| v.i().unwrap_or(0))
            .unwrap_or(0);

        let align = alignment as i64;
        let gravity = if orientation == 0 {
            // HStack: cross-axis is vertical
            match align {
                0 => -1, // Fill — no gravity override (let children use MATCH_PARENT height)
                1 => 48, // Leading → TOP
                3 => 16, // Center → CENTER_VERTICAL
                4 => 80, // Trailing → BOTTOM
                _ => -1,
            }
        } else {
            // VStack: cross-axis is horizontal
            match align {
                0 => -1, // Fill — no gravity override
                1 => 3,  // Leading → LEFT
                3 => 1,  // Center → CENTER_HORIZONTAL
                4 => 5,  // Trailing → RIGHT
                _ => -1,
            }
        };

        if gravity >= 0 {
            let _ = env.call_method(
                view_ref.as_obj(),
                "setGravity",
                "(I)V",
                &[jni::objects::JValue::Int(gravity)],
            );
        }
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}

// =============================================================================
// Text wrapping (parity with iOS)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_text_set_wraps(handle: i64, max_width: f64) {
    catch_panic_void("perry_ui_text_set_wraps", || {
        widgets::text::set_wraps(handle, max_width)
    });
}

// =============================================================================
// TextField get/submit (parity with iOS)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_textfield_get_string(handle: i64) -> i64 {
    widgets::textfield::get_string_value(handle) as usize as i64
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_on_submit(handle: i64, on_submit: f64) {
    catch_panic_void("perry_ui_textfield_set_on_submit", || {
        widgets::textfield::set_on_submit(handle, on_submit)
    });
}

// =============================================================================
// TextArea (multi-line EditText)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_textarea_create(placeholder_ptr: i64, on_change: f64) -> i64 {
    catch_panic("perry_ui_textarea_create", || {
        widgets::textarea::create(placeholder_ptr as *const u8, on_change)
    })
}

#[no_mangle]
pub extern "C" fn perry_ui_textarea_set_string(handle: i64, text_ptr: i64) {
    catch_panic_void("perry_ui_textarea_set_string", || {
        widgets::textfield::set_string_value(handle, text_ptr as *const u8)
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_textarea_get_string(handle: i64) -> i64 {
    widgets::textfield::get_string_value(handle) as usize as i64
}

// =============================================================================
// QR Code (parity with iOS)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_qrcode_create(data_ptr: i64, size: f64) -> i64 {
    catch_panic("perry_ui_qrcode_create", || {
        widgets::qrcode::create(data_ptr as *const u8, size)
    })
}

#[no_mangle]
pub extern "C" fn perry_ui_qrcode_set_data(handle: i64, data_ptr: i64) {
    catch_panic_void("perry_ui_qrcode_set_data", || {
        widgets::qrcode::set_data(handle, data_ptr as *const u8)
    });
}

// =============================================================================
// App icon (no-op on Android — icons are set via AndroidManifest.xml)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_app_set_icon(_path_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_size(_app: i64, _w: f64, _h: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_frameless(_app_handle: i64, _value: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_level(_app_handle: i64, _value_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_transparent(_app_handle: i64, _value: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_vibrancy(_app_handle: i64, _value_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_activation_policy(_app_handle: i64, _value_ptr: i64) {}

// =============================================================================
// Folder Dialog (parity with iOS)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_open_folder_dialog(callback: f64) {
    // On Android, use the same file dialog (SAF) for folder picking
    file_dialog::open_dialog(callback);
}

// =============================================================================
// Embed Native View (parity with iOS embed_nsview)
// =============================================================================

/// Register an external Android View (from a native library) as a Perry widget.
/// The pointer must be a JNI GlobalRef to an Android View object.
#[no_mangle]
pub extern "C" fn perry_ui_embed_nsview(view_ptr: i64) -> i64 {
    if view_ptr == 0 {
        return 0;
    }
    // On Android, the native view pointer is a raw JNI object pointer.
    // Convert it to a GlobalRef and register as a widget.
    let env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    let obj = unsafe { jni::objects::JObject::from_raw(view_ptr as jni::sys::jobject) };
    let global = match env.new_global_ref(obj) {
        Ok(g) => g,
        Err(_) => {
            unsafe {
                env.pop_local_frame(&jni::objects::JObject::null());
            }
            return 0;
        }
    };
    let handle = widgets::register_widget(global);
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    handle
}

// =============================================================================
// Missing stubs — platform functions not yet implemented on Android
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_frame_split_create(_left_width: f64) -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ui_frame_split_add_child(_parent: i64, _child: i64) {}

/// Query display metrics from the Android system.
/// Returns (widthDp, heightDp, density).
fn query_display_metrics() -> (f64, f64, f64) {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    // Get Application context: ActivityThread.currentApplication()
    let result = (|| -> Option<(f64, f64, f64)> {
        let app = env
            .call_static_method(
                "android/app/ActivityThread",
                "currentApplication",
                "()Landroid/app/Application;",
                &[],
            )
            .ok()?
            .l()
            .ok()?;
        if app.is_null() {
            return None;
        }

        // Get Resources
        let res = env
            .call_method(
                &app,
                "getResources",
                "()Landroid/content/res/Resources;",
                &[],
            )
            .ok()?
            .l()
            .ok()?;
        // Get DisplayMetrics
        let dm = env
            .call_method(
                &res,
                "getDisplayMetrics",
                "()Landroid/util/DisplayMetrics;",
                &[],
            )
            .ok()?
            .l()
            .ok()?;

        let width_px = env.get_field(&dm, "widthPixels", "I").ok()?.i().ok()? as f64;
        let height_px = env.get_field(&dm, "heightPixels", "I").ok()?.i().ok()? as f64;
        let density = env.get_field(&dm, "density", "F").ok()?.f().ok()? as f64;

        if density > 0.0 {
            Some((width_px / density, height_px / density, density))
        } else {
            None
        }
    })();

    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    result.unwrap_or((412.0, 915.0, 2.625))
}

#[no_mangle]
pub extern "C" fn perry_get_screen_width() -> f64 {
    query_display_metrics().0
}

#[no_mangle]
pub extern "C" fn perry_get_screen_height() -> f64 {
    query_display_metrics().1
}

#[no_mangle]
pub extern "C" fn perry_get_scale_factor() -> f64 {
    query_display_metrics().2
}

#[no_mangle]
pub extern "C" fn perry_get_device_idiom() -> i64 {
    0
} // 0 = phone

// Audio capture (AudioRecord via JNI)
#[no_mangle]
pub extern "C" fn perry_system_audio_start() -> i64 {
    audio::start()
}
#[no_mangle]
pub extern "C" fn perry_system_audio_stop() {
    audio::stop()
}
#[no_mangle]
pub extern "C" fn perry_system_audio_get_level() -> f64 {
    audio::get_level()
}
#[no_mangle]
pub extern "C" fn perry_system_audio_get_peak() -> f64 {
    audio::get_peak()
}
#[no_mangle]
pub extern "C" fn perry_system_audio_get_waveform(count: f64) -> f64 {
    audio::get_waveform(count)
}
#[no_mangle]
pub extern "C" fn perry_system_get_device_model() -> i64 {
    audio::get_device_model()
}

// Camera (Camera2 API via JNI)
#[no_mangle]
pub extern "C" fn perry_ui_camera_create() -> i64 {
    camera::create()
}
#[no_mangle]
pub extern "C" fn perry_ui_camera_start(handle: i64) {
    camera::start(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_camera_stop(handle: i64) {
    camera::stop(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_camera_freeze(handle: i64) {
    camera::freeze(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_camera_unfreeze(handle: i64) {
    camera::unfreeze(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_camera_sample_color(x: f64, y: f64) -> f64 {
    camera::sample_color(x, y)
}
#[no_mangle]
pub extern "C" fn perry_ui_camera_set_on_tap(handle: i64, callback: f64) {
    camera::set_on_tap(handle, callback)
}

// Geisterhand screenshot stub: only when the geisterhand feature is OFF.
// When the feature is ON, `screenshot::perry_ui_screenshot_capture` is the
// real implementation and providing this stub here would be a duplicate
// `#[no_mangle]` symbol.
#[cfg(not(feature = "geisterhand"))]
#[no_mangle]
pub extern "C" fn perry_ui_screenshot_capture(_out_len: *mut usize) -> *mut u8 {
    std::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn perry_on_layout_change(_callback: f64) {}

#[no_mangle]
pub extern "C" fn __wrapper_perry_on_layout_change(_callback: f64) {}

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
}

extern "C" {
    fn js_nanbox_string(ptr: *const u8) -> f64;
}

fn get_app_files_dir_string() -> f64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);
    let result = (|| -> Option<f64> {
        let activity = env
            .call_static_method(
                "com/perry/app/PerryBridge",
                "getActivity",
                "()Landroid/app/Activity;",
                &[],
            )
            .ok()?
            .l()
            .ok()?;
        if activity.is_null() {
            return None;
        }
        let files_dir = env
            .call_method(&activity, "getFilesDir", "()Ljava/io/File;", &[])
            .ok()?
            .l()
            .ok()?;
        if files_dir.is_null() {
            return None;
        }
        let abs_path = env
            .call_method(&files_dir, "getAbsolutePath", "()Ljava/lang/String;", &[])
            .ok()?
            .l()
            .ok()?;
        let rust_str = env.get_string((&abs_path).into()).ok()?;
        let bytes = rust_str.to_str().unwrap_or("").as_bytes();
        if bytes.is_empty() {
            return None;
        }
        // Append /workspace to the files dir
        let mut path = String::from_utf8_lossy(bytes).to_string();
        path.push_str("/workspace");
        crate::log_debug(&format!("get_app_files_dir: path={}", path));
        let path_bytes = path.as_bytes();
        let str_ptr = unsafe { js_string_from_bytes(path_bytes.as_ptr(), path_bytes.len() as i64) };
        // NaN-box the string pointer so Perry can use it as a string value
        let nanboxed = unsafe { js_nanbox_string(str_ptr) };
        Some(nanboxed)
    })();
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    // Return empty string NaN-boxed (not 0, which is integer 0)
    result.unwrap_or_else(|| unsafe { js_nanbox_string(std::ptr::null()) })
}

#[no_mangle]
pub extern "C" fn hone_get_app_files_dir() -> f64 {
    get_app_files_dir_string()
}

#[no_mangle]
pub extern "C" fn __wrapper_hone_get_app_files_dir() -> f64 {
    get_app_files_dir_string()
}

#[no_mangle]
pub extern "C" fn hone_get_documents_dir() -> f64 {
    get_app_files_dir_string()
}

#[no_mangle]
pub extern "C" fn __wrapper_hone_get_documents_dir() -> f64 {
    get_app_files_dir_string()
}

// =============================================================================
// Stubs for UI functions not yet implemented on Android
// =============================================================================

/// perry_ui_poll_open_file() — macOS "Open With" not applicable on Android
#[no_mangle]
pub extern "C" fn perry_ui_poll_open_file() -> i64 {
    0 // null (no file)
}

/// perry_ui_textfield_blur_all() — dismiss all keyboard focus
#[no_mangle]
pub extern "C" fn perry_ui_textfield_blur_all() {
    // TODO: hide soft keyboard via InputMethodManager
}

/// perry_ui_textfield_set_on_focus(handle, callback) — on-focus callback for textfield
#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_on_focus(_handle: f64, _callback: f64) {
    // TODO: wire OnFocusChangeListener
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_next_key_view(_handle: i64, _next_handle: i64) {
    // Android handles tab/next navigation automatically
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_borderless(handle: i64, borderless: f64) {
    widgets::textfield::set_borderless(handle, borderless);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_background_color(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    widgets::textfield::set_background_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_font_size(handle: i64, size: f64) {
    widgets::textfield::set_font_size(handle, size);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::textfield::set_text_color(handle, r, g, b, a);
}

/// perry_ui_widget_add_overlay(parent, child) — add overlay view
#[no_mangle]
pub extern "C" fn perry_ui_widget_add_overlay(_parent: f64, _child: f64) {
    // TODO: add child as overlay in FrameLayout
}

/// perry_ui_widget_set_overlay_frame(child, x, y, w, h) — position overlay
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_overlay_frame(
    _child: f64,
    _x: f64,
    _y: f64,
    _w: f64,
    _h: f64,
) {
    // TODO: set FrameLayout.LayoutParams with margins
}
