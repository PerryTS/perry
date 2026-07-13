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
pub extern "C" fn perry_ui_button_set_text_color(handle: f64, r: f64, g: f64, b: f64, a: f64) {
    catch_panic_void("perry_ui_button_set_text_color", || {
        let h = widgets::decode_js_handle_f64(handle);
        widgets::button::set_text_color(h, r, g, b, a)
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
pub extern "C" fn perry_ui_widget_set_width(handle: f64, width: f64) {
    catch_panic_void("perry_ui_widget_set_width", || {
        let h = widgets::decode_js_handle_f64(handle);
        widgets::set_width(h, width)
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_height(handle: f64, height: f64) {
    catch_panic_void("perry_ui_widget_set_height", || {
        let h = widgets::decode_js_handle_f64(handle);
        widgets::set_height(h, height)
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

/// Issue #1280 — Android apps run in a single full-screen Activity. Stub.
#[no_mangle]
pub extern "C" fn perry_ui_app_set_window_state(_app_handle: i64, _value_ptr: i64) {}
