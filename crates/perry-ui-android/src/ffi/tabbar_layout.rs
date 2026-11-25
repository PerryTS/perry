//! TabBar, button extras, scrollview refresh, layout sizing helpers,
//! stack alignment/distribution, text wrapping, textfield get/submit,
//! textarea, QR code, app-icon no-ops. Originally `lib.rs` lines
//! 1716-2064.

use crate::{catch_panic, catch_panic_void, jni_bridge, widgets};

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
pub extern "C" fn perry_ui_tabbar_add_tab(tabbar_handle: i64, label_ptr: i64, content: i64) {
    catch_panic_void("perry_ui_tabbar_add_tab", || {
        widgets::tabbar::add_tab(tabbar_handle, label_ptr as *const u8, content)
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
pub extern "C" fn perry_ui_button_set_image(handle: i64, name_ptr: i64, _point_size: f64) {
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
    // UIStackView distributions: 0 fill (default), 1 fillEqually; the others
    // (fillProportionally, equalSpacing, equalCentering) lay out as fill.
    // Recorded per stack in PerryBridge, whose relayout gives every child of a
    // fillEqually stack an equal share of the main axis, whatever order the
    // children are added in (setting LinearLayout weights here only reached
    // children that already existed, and relayout overwrote them).
    let d: i32 = if distribution as i64 == 1 { 1 } else { 0 };
    if let Some(view_ref) = widgets::get_widget(handle) {
        jni_bridge::with_env(|env| {
            let _ = jni_bridge::push_local_frame(env, 8);
            let bridge_class =
                jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
            let bridge_cls: &jni::objects::JClass = &bridge_class;
            let _ = env.call_static_method(
                bridge_cls,
                jni::jni_str!("setStackDistribution"),
                jni::jni_sig!("(Landroid/view/View;I)V"),
                &[jni::JValue::Object(view_ref.as_obj()), jni::JValue::Int(d)],
            );
            unsafe {
                let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
            }
        })
    }
}

#[no_mangle]
pub extern "C" fn perry_ui_stack_set_alignment(handle: i64, alignment: f64) {
    // Public constants are NSLayoutAttribute values (5 leading, 9 centerX,
    // 7 width, 3 top, 10 centerY, 4 bottom, ...), as on iOS. Normalised to
    // 0 fill, 1 leading/top, 3 center, 4 trailing/bottom; the children's
    // LayoutParams follow (PerryBridge.setStackAlignment).
    let align: i32 = match alignment as i64 {
        0 | 7 | 8 => 0,
        1 | 3 | 5 | 11 | 12 => 1,
        2 | 4 | 6 => 4,
        9 | 10 => 3,
        _ => return,
    };
    if let Some(view_ref) = widgets::get_widget(handle) {
        jni_bridge::with_env(|env| {
            let _ = jni_bridge::push_local_frame(env, 8);
            let bridge_class =
                jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
            let bridge_cls: &jni::objects::JClass = &bridge_class;
            let _ = env.call_static_method(
                bridge_cls,
                jni::jni_str!("setStackAlignment"),
                jni::jni_sig!("(Landroid/view/View;I)V"),
                &[
                    jni::JValue::Object(view_ref.as_obj()),
                    jni::JValue::Int(align),
                ],
            );
            unsafe {
                let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
            }
        })
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

#[no_mangle]
pub extern "C" fn perry_ui_app_set_quit_on_last_window_close(_app_handle: i64, _value: f64) {}

/// Issue #1280 — Android apps run in a single full-screen Activity. Stub.
#[no_mangle]
pub extern "C" fn perry_ui_app_set_window_state(_app_handle: i64, _value_ptr: i64) {}

/// Frame persistence only applies to repositionable desktop windows.
#[no_mangle]
pub extern "C" fn perry_ui_app_set_frame_autosave_name(_app_handle: i64, _value_ptr: i64) {}

/// Responsive max-width is not implemented on this backend yet.
/// Export the shared ABI so portable perry/ui code continues to compile.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_max_width(_handle: i64, _max_width: f64) {}
