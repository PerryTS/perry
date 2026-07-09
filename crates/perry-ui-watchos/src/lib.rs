//! Perry UI runtime for watchOS.
//!
//! Uses a data-driven widget tree instead of UIKit views.
//! The fixed PerryWatchApp.swift queries this tree via FFI and renders
//! it as SwiftUI views reactively.

pub mod app;
pub mod audio;
pub mod audio_playback;
pub mod background;
pub mod drag_drop;
pub mod haptics;
pub mod media_playback;
pub mod notifications;
pub mod state;
pub mod system;
pub mod tree;
pub mod widgets;

use std::ffi::CString;
use tree::{NodeData, NodeKind};

/// Extract a &str from a *const StringHeader pointer.
fn str_from_header(ptr: *const u8) -> &'static str {
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

pub fn cstring_from_header(ptr: *const u8) -> Option<CString> {
    let s = str_from_header(ptr);
    CString::new(s).ok()
}

// =============================================================================
// Core app lifecycle
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

// =============================================================================
// Widget creation — supported on watchOS
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_text_create(text_ptr: i64) -> i64 {
    let mut node = NodeData::new(NodeKind::Text);
    node.text = cstring_from_header(text_ptr as *const u8);
    tree::register_node(node)
}

#[no_mangle]
pub extern "C" fn perry_ui_button_create(label_ptr: i64, on_press: f64) -> i64 {
    let mut node = NodeData::new(NodeKind::Button);
    node.text = cstring_from_header(label_ptr as *const u8);
    node.action_closure = Some(on_press);
    tree::register_node(node)
}

#[no_mangle]
pub extern "C" fn perry_ui_vstack_create(spacing: f64) -> i64 {
    let mut node = NodeData::new(NodeKind::VStack);
    node.spacing = spacing;
    tree::register_node(node)
}

#[no_mangle]
pub extern "C" fn perry_ui_hstack_create(spacing: f64) -> i64 {
    let mut node = NodeData::new(NodeKind::HStack);
    node.spacing = spacing;
    tree::register_node(node)
}

#[no_mangle]
pub extern "C" fn perry_ui_zstack_create() -> i64 {
    tree::register_node(NodeData::new(NodeKind::ZStack))
}

#[no_mangle]
pub extern "C" fn perry_ui_spacer_create() -> i64 {
    tree::register_node(NodeData::new(NodeKind::Spacer))
}

#[no_mangle]
pub extern "C" fn perry_ui_divider_create() -> i64 {
    tree::register_node(NodeData::new(NodeKind::Divider))
}

#[no_mangle]
pub extern "C" fn perry_ui_toggle_create(label_ptr: i64, on_change: f64) -> i64 {
    let mut node = NodeData::new(NodeKind::Toggle);
    node.text = cstring_from_header(label_ptr as *const u8);
    node.on_change_closure = Some(on_change);
    tree::register_node(node)
}

#[no_mangle]
pub extern "C" fn perry_ui_slider_create(min: f64, max: f64, initial: f64, on_change: f64) -> i64 {
    let mut node = NodeData::new(NodeKind::Slider);
    node.slider_min = min;
    node.slider_max = max;
    node.slider_value = initial;
    node.on_change_closure = Some(on_change);
    tree::register_node(node)
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_create() -> i64 {
    tree::register_node(NodeData::new(NodeKind::ScrollView))
}

#[no_mangle]
pub extern "C" fn perry_ui_progressview_create() -> i64 {
    tree::register_node(NodeData::new(NodeKind::ProgressView))
}

#[no_mangle]
pub extern "C" fn perry_ui_image_create_symbol(name_ptr: i64) -> i64 {
    let mut node = NodeData::new(NodeKind::Image);
    node.image_system_name = cstring_from_header(name_ptr as *const u8);
    node.text = node.image_system_name.clone();
    tree::register_node(node)
}

#[no_mangle]
pub extern "C" fn perry_ui_image_create_file(_path_ptr: i64) -> i64 {
    // File-based images: limited support on watchOS, create placeholder
    let mut node = NodeData::new(NodeKind::Image);
    node.text = CString::new("photo").ok();
    tree::register_node(node)
}

/// #635 stub: remote URL images aren't fetched on watchOS yet —
/// register an empty Image node so layout still works.
#[no_mangle]
pub extern "C" fn perry_ui_image_create_url(_url_ptr: i64, _alt_ptr: i64) -> i64 {
    let mut node = NodeData::new(NodeKind::Image);
    node.text = CString::new("photo").ok();
    tree::register_node(node)
}

#[no_mangle]
pub extern "C" fn perry_ui_picker_create(on_change: f64) -> i64 {
    // Single `Closure` arg to match the dispatch-table ABI; a 3-arg
    // signature mis-binds `on_change` on the Windows x64 ABI (issue #5491).
    // No label is wired from TS, so the picker has no leading text.
    let mut node = NodeData::new(NodeKind::Picker);
    node.on_change_closure = Some(on_change);
    tree::register_node(node)
}

#[no_mangle]
pub extern "C" fn perry_ui_navstack_create() -> i64 {
    // Matches the 0-arg dispatch in perry-dispatch::PERRY_UI_TABLE.
    let node = NodeData::new(NodeKind::NavigationStack);
    tree::register_node(node)
}

#[no_mangle]
pub extern "C" fn perry_ui_form_create() -> i64 {
    // Form maps to List on watchOS
    tree::register_node(NodeData::new(NodeKind::List))
}

#[no_mangle]
pub extern "C" fn perry_ui_section_create(title_ptr: i64) -> i64 {
    // Section → VStack with a title text
    let mut node = NodeData::new(NodeKind::VStack);
    node.text = cstring_from_header(title_ptr as *const u8);
    tree::register_node(node)
}

// =============================================================================
// TextArea
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_textarea_create(placeholder: i64, on_change: f64) -> i64 {
    widgets::textarea::create(placeholder as *const u8, on_change)
}

#[no_mangle]
pub extern "C" fn perry_ui_textarea_set_string(handle: i64, text: i64) {
    widgets::textarea::set_string(handle, text as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_textarea_get_string(handle: i64) -> i64 {
    widgets::textarea::get_string(handle) as i64
}

// =============================================================================
// Widget manipulation
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_widget_add_child(parent_handle: i64, child_handle: i64) {
    tree::add_child(parent_handle, child_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_add_child_at(parent_handle: i64, child_handle: i64, index: f64) {
    tree::with_node_mut(parent_handle, |parent| {
        let idx = (index as usize).min(parent.children.len());
        parent.children.insert(idx, child_handle);
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_remove_child(parent_handle: i64, child_handle: i64) {
    tree::remove_child(parent_handle, child_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_clear_children(handle: i64) {
    tree::clear_children(handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_reorder_child(
    parent_handle: i64,
    from_index: f64,
    to_index: f64,
) {
    tree::with_node_mut(parent_handle, |parent| {
        let from = from_index as usize;
        let to = to_index as usize;
        if from < parent.children.len() && to < parent.children.len() {
            let child = parent.children.remove(from);
            parent.children.insert(to, child);
        }
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_child(scroll_handle: i64, child_handle: i64) {
    tree::with_node_mut(scroll_handle, |node| {
        node.children.clear();
        node.children.push(child_handle);
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_picker_add_item(handle: i64, title_ptr: i64) {
    if let Some(title) = cstring_from_header(title_ptr as *const u8) {
        tree::with_node_mut(handle, |node| {
            node.picker_items.push(title);
        });
    }
}

#[no_mangle]
pub extern "C" fn perry_ui_picker_set_selected(handle: i64, index: i64) {
    tree::with_node_mut(handle, |node| {
        node.picker_selected = index;
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_picker_get_selected(handle: i64) -> i64 {
    tree::with_node(handle, |n| n.picker_selected).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn perry_ui_progressview_set_value(handle: i64, value: f64) {
    tree::with_node_mut(handle, |node| {
        node.progress_value = value;
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_navstack_push(handle: i64, _title_ptr: i64, body_handle: i64) {
    tree::add_child(handle, body_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_navstack_pop(handle: i64) {
    tree::with_node_mut(handle, |node| {
        if node.children.len() > 1 {
            node.children.pop();
        }
    });
}

// =============================================================================
// Text styling
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_text_set_string(handle: i64, text_ptr: i64) {
    tree::with_node_mut(handle, |node| {
        node.text = cstring_from_header(text_ptr as *const u8);
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_text_set_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    tree::with_node_mut(handle, |node| {
        node.color = Some((r, g, b, a));
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_size(handle: i64, size: f64) {
    tree::with_node_mut(handle, |node| {
        node.font_size = Some(size);
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_weight(handle: i64, size: f64, weight: f64) {
    tree::with_node_mut(handle, |node| {
        node.font_size = Some(size);
        node.font_weight = Some(weight);
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_text_set_wraps(handle: i64, _max_width: f64) {
    tree::with_node_mut(handle, |node| {
        node.text_wraps = true;
    });
}

/// Text decoration (issue #185 Phase B). 0=none, 1=underline, 2=strikethrough.
/// Stored on `NodeData`; the SwiftUI host applies `.underline()` /
/// `.strikethrough()` modifiers at render time.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_decoration(handle: i64, decoration: i64) {
    tree::with_node_mut(handle, |node| {
        node.text_decoration = decoration;
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_text_set_selectable(_handle: i64, _selectable: f64) {}

/// Issue #707 — watchOS Text line cap. Persists on the tree NodeData;
/// SwiftUI host reads it via `perry_watchos_node_text_number_of_lines`
/// and applies `.lineLimit(n)` when > 0.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_number_of_lines(handle: i64, lines: i64) {
    tree::with_node_mut(handle, |node| {
        node.text_number_of_lines = lines.max(0);
    });
}
/// Issue #707 — watchOS Text truncation mode (0=word-wrap, 1=head,
/// 2=middle, 3=tail). SwiftUI host reads via
/// `perry_watchos_node_text_truncation_mode`.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_truncation_mode(handle: i64, mode: i64) {
    tree::with_node_mut(handle, |node| {
        node.text_truncation_mode = mode;
    });
}

/// Issue #3621 — watchOS Text horizontal alignment (0=left, 1=right,
/// 2=center, 3=justified, 4=natural). SwiftUI host reads via
/// `perry_watchos_node_text_alignment`.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_text_alignment(handle: i64, alignment: i64) {
    tree::with_node_mut(handle, |node| {
        node.text_alignment = alignment;
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_family(handle: i64, family_ptr: i64) {
    tree::with_node_mut(handle, |node| {
        node.font_family = cstring_from_header(family_ptr as *const u8);
    });
}

// =============================================================================
// Button styling
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_button_set_bordered(_handle: i64, _bordered: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_title(handle: i64, title_ptr: i64) {
    tree::with_node_mut(handle, |node| {
        node.text = cstring_from_header(title_ptr as *const u8);
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    tree::with_node_mut(handle, |node| {
        node.color = Some((r, g, b, a));
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_image(_handle: i64, _name_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_image_position(_handle: i64, _position: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_content_tint_color(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    tree::with_node_mut(handle, |node| {
        node.color = Some((r, g, b, a));
    });
}

// =============================================================================
// Widget common styling
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_width(handle: i64, width: f64) {
    tree::with_node_mut(handle, |node| {
        node.frame_width = Some(width);
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_height(handle: i64, height: f64) {
    tree::with_node_mut(handle, |node| {
        node.frame_height = Some(height);
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_background_color(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    tree::with_node_mut(handle, |node| {
        node.bg_color = Some((r, g, b, a));
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_background_gradient(
    _handle: i64,
    _r1: f64,
    _g1: f64,
    _b1: f64,
    _r2: f64,
    _g2: f64,
    _b2: f64,
    _vertical: f64,
) {
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_corner_radius(handle: i64, radius: f64) {
    tree::with_node_mut(handle, |node| {
        node.corner_radius = Some(radius);
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_opacity(handle: i64, alpha: f64) {
    tree::with_node_mut(handle, |node| {
        node.opacity = alpha;
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_set_widget_hidden(handle: i64, hidden: i64) {
    tree::set_hidden(handle, hidden != 0);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_enabled(handle: i64, enabled: i64) {
    tree::with_node_mut(handle, |node| {
        node.enabled = enabled != 0;
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_border_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    tree::with_node_mut(handle, |node| {
        node.border_color = Some((r, g, b, a));
    });
}

/// Set drop shadow on a widget node (issue #185 Phase B).
/// watchOS stores shadow in the introspection tree; the SwiftUI host
/// applies `.shadow(color:radius:x:y:)` modifier when rendering.
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
    tree::with_node_mut(handle, |node| {
        node.shadow = Some((r, g, b, a, blur, offset_x, offset_y));
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_border_width(handle: i64, width: f64) {
    tree::with_node_mut(handle, |node| {
        node.border_width = Some(width);
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
    tree::with_node_mut(handle, |node| {
        node.edge_insets = Some((top, left, bottom, right));
    });
}

// =============================================================================
// Image
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_image_set_size(handle: i64, width: f64, height: f64) {
    tree::with_node_mut(handle, |node| {
        node.image_width = Some(width);
        node.image_height = Some(height);
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_image_set_tint(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    tree::with_node_mut(handle, |node| {
        node.image_tint = Some((r, g, b, a));
    });
}

// =============================================================================
// State management
// =============================================================================

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
pub extern "C" fn perry_ui_state_bind_slider(state_handle: i64, slider_handle: i64) {
    state::bind_slider(state_handle, slider_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_state_bind_toggle(state_handle: i64, toggle_handle: i64) {
    state::bind_toggle(state_handle, toggle_handle);
}

/// Set an existing Toggle's on/off state (issue #5076). `on` is 0 for
/// off, non-zero for on. Updates the node's `toggle_on` flag so the
/// next host render reflects the new state.
#[no_mangle]
pub extern "C" fn perry_ui_toggle_set_state(handle: i64, on: i64) {
    tree::with_node_mut(handle, |node| {
        node.toggle_on = on != 0;
    });
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
pub extern "C" fn perry_ui_for_each_init(
    container_handle: i64,
    state_handle: i64,
    render_closure: f64,
) {
    state::for_each_init(container_handle, state_handle, render_closure);
}

#[no_mangle]
pub extern "C" fn perry_ui_state_on_change(state_handle: i64, callback: f64) {
    state::state_on_change(state_handle, callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_state_bind_textfield(_state_handle: i64, _textfield_handle: i64) {}

// =============================================================================
// VStack/HStack with insets
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_vstack_create_with_insets(
    spacing: f64,
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
) -> i64 {
    let mut node = NodeData::new(NodeKind::VStack);
    node.spacing = spacing;
    node.edge_insets = Some((top, left, bottom, right));
    tree::register_node(node)
}

#[no_mangle]
pub extern "C" fn perry_ui_hstack_create_with_insets(
    spacing: f64,
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
) -> i64 {
    let mut node = NodeData::new(NodeKind::HStack);
    node.spacing = spacing;
    node.edge_insets = Some((top, left, bottom, right));
    tree::register_node(node)
}

// =============================================================================
// Stubs — functions that exist in perry-ui-ios but are no-ops on watchOS
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_embed_nsview(_ptr: i64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_splitview_create(_left_width: f64) -> i64 {
    perry_ui_hstack_create(0.0)
}
#[no_mangle]
pub extern "C" fn perry_ui_splitview_add_child(p: i64, c: i64, _idx: f64) {
    tree::add_child(p, c);
}
#[no_mangle]
pub extern "C" fn perry_ui_vbox_create() -> i64 {
    perry_ui_vstack_create(0.0)
}
#[no_mangle]
pub extern "C" fn perry_ui_vbox_add_child(p: i64, c: i64, _slot: f64) {
    tree::add_child(p, c);
}
#[no_mangle]
pub extern "C" fn perry_ui_vbox_finalize(_handle: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_frame_split_create(_left_width: f64) -> i64 {
    perry_ui_hstack_create(0.0)
}
#[no_mangle]
pub extern "C" fn perry_ui_frame_split_add_child(p: i64, c: i64) {
    tree::add_child(p, c);
}
#[no_mangle]
pub extern "C" fn perry_ui_textfield_create(_placeholder: i64, _on_change: f64) -> i64 {
    perry_ui_text_create(0)
}
#[no_mangle]
pub extern "C" fn perry_ui_securefield_create(_placeholder: i64, _on_change: f64) -> i64 {
    perry_ui_text_create(0)
}
#[no_mangle]
pub extern "C" fn perry_ui_clipboard_read() -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn perry_ui_clipboard_write(_text: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_add_keyboard_shortcut(_key: i64, _mods: f64, _cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_register_global_hotkey(_key: i64, _mods: f64, _cb: f64) {}
// Keyboard event stubs (issue #1864) — watchOS has no hardware keyboard surface.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_key_down(_handle: i64, _cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_key_up(_handle: i64, _cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_app_set_on_key_down(_cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_app_set_on_key_up(_cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_focus_widget(_handle: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_blur_widget(_handle: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_is_key_down(_code: f64) -> i32 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_current_modifiers() -> i32 {
    0
}
#[no_mangle]
pub extern "C" fn perry_system_get_app_icon(_path: i64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_hugging(_handle: i64, _priority: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_widget_match_parent_width(_handle: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_widget_match_parent_height(_handle: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_stack_set_detaches_hidden(_handle: i64, _flag: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_stack_set_distribution(_handle: i64, _dist: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_stack_set_alignment(_handle: i64, _align: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_textfield_focus(_handle: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_string(_handle: i64, _text: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_textfield_get_string(_handle: i64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_on_submit(_handle: i64, _cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_on_focus(_handle: i64, _cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_textfield_blur_all() {}
#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_borderless(_handle: i64, _b: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_background_color(
    _h: i64,
    _r: f64,
    _g: f64,
    _b: f64,
    _a: f64,
) {
}
#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_font_size(_h: i64, _s: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_text_color(_h: i64, _r: f64, _g: f64, _b: f64, _a: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_scroll_to(_scroll: i64, _child: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_get_offset(_handle: i64) -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_offset(_handle: i64, _offset: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_refresh_control(_handle: i64, _cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_end_refreshing(_handle: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_menu_create() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_item(_menu: i64, _title: i64, _cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_context_menu(_widget: i64, _menu: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_item_with_shortcut(_m: i64, _t: i64, _s: i64, _cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_separator(_menu: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_submenu(_menu: i64, _title: i64, _sub: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_menubar_create() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_menubar_add_menu(_bar: i64, _title: i64, _menu: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_menubar_attach(_bar: i64) {}

// Tray icon (issue #490) — no-op on watchOS (no system tray concept).
#[no_mangle]
pub extern "C" fn perry_ui_tray_create(_icon_path_ptr: i64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_tray_set_icon(_tray_handle: i64, _icon_path_ptr: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_tray_set_tooltip(_tray_handle: i64, _tooltip_ptr: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_tray_attach_menu(_tray_handle: i64, _menu_handle: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_tray_on_click(_tray_handle: i64, _callback: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_tray_destroy(_tray_handle: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_menu_clear(_menu: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_standard_action(_m: i64, _t: i64, _sel: i64, _s: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_open_file_dialog(_cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_app_set_min_size(_app: i64, _w: f64, _h: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_app_set_max_size(_app: i64, _w: f64, _h: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_app_set_timer(_app_handle: i64, _interval_ms: f64, _cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_create(_w: f64, _h: f64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_clear(_h: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_begin_path(_h: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_move_to(_h: i64, _x: f64, _y: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_line_to(_h: i64, _x: f64, _y: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_stroke(_h: i64, _r: f64, _g: f64, _b: f64, _a: f64, _w: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_fill_gradient(
    _h: i64,
    _r1: f64,
    _g1: f64,
    _b1: f64,
    _r2: f64,
    _g2: f64,
    _b2: f64,
    _x1: f64,
    _y1: f64,
    _x2: f64,
    _y2: f64,
    _close: i64,
) {
}
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_tooltip(_h: i64, _t: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_rich_tooltip(_h: i64, _content: i64, _delay_ms: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_combobox_create(initial: i64, on_change: f64) -> i64 {
    perry_ui_textfield_create(initial, on_change)
}
#[no_mangle]
pub extern "C" fn perry_ui_combobox_add_item(_h: i64, _v: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_combobox_set_value(_h: i64, _v: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_combobox_get_value(_h: i64) -> f64 {
    f64::from_bits(0x7FFC_0000_0000_0001)
}

// Issue #478 — Rich text editor stubs.
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_create(_w: f64, _h: f64, _cb: f64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_set_string(_h: i64, _t: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_get_string(_h: i64) -> f64 {
    f64::from_bits(0x7FFC_0000_0000_0001)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_set_html(_h: i64, _html: i64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_get_html(_h: i64) -> f64 {
    f64::from_bits(0x7FFC_0000_0000_0001)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_toggle_bold(_h: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_toggle_italic(_h: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_toggle_underline(_h: i64) {}

// Issue #516 — PdfView stubs (watchOS not a realistic PDF target).
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_create(_w: f64, _h: f64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_load_file(_h: i64, _p: i64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_get_page_count(_h: i64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_go_to_page(_h: i64, _i: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_get_current_page(_h: i64) -> i64 {
    -1
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_set_scale(_h: i64, _s: f64) {}

// Issue #517 — MapView via SwiftUI Map(coordinateRegion:annotationItems:)
// in PerryWatchApp.swift's NodeView. Backed by the data-driven tree model
// (NodeKind::MapView + map_lat/lon/lat_span/lon_span/type/pins fields).
#[no_mangle]
pub extern "C" fn perry_ui_map_view_create(_w: f64, _h: f64) -> i64 {
    let mut node = NodeData::new(NodeKind::MapView);
    // Sensible defaults: zoomed all the way out until set_region is called.
    node.map_lat_span = 180.0;
    node.map_lon_span = 360.0;
    tree::register_node(node)
}

#[no_mangle]
pub extern "C" fn perry_ui_map_view_set_region(
    handle: i64,
    lat: f64,
    lon: f64,
    lat_span: f64,
    lon_span: f64,
) {
    tree::with_node_mut(handle, |node| {
        node.map_lat = lat;
        node.map_lon = lon;
        node.map_lat_span = lat_span;
        node.map_lon_span = lon_span;
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_map_view_add_pin(handle: i64, lat: f64, lon: f64, title_ptr: i64) {
    let title = cstring_from_header(title_ptr as *const u8)
        .unwrap_or_else(|| std::ffi::CString::new("").unwrap());
    tree::with_node_mut(handle, |node| {
        node.map_pins.push((lat, lon, title));
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_map_view_clear_pins(handle: i64) {
    tree::with_node_mut(handle, |node| {
        node.map_pins.clear();
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_map_view_set_map_type(handle: i64, style: i64) {
    tree::with_node_mut(handle, |node| {
        node.map_type = style;
    });
}

// Issue #477 — Command palette stubs.
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_register(_id: i64, _l: i64, _s: i64, _cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_unregister(_id: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_clear() {}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_show() {}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_hide() {}

// Issue #474 — Chart widget stubs.
#[no_mangle]
pub extern "C" fn perry_ui_chart_create(_kind: i64, _w: f64, _h: f64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_chart_add_data_point(_h: i64, _l: i64, _v: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_chart_clear_data(_h: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_chart_set_title(_h: i64, _t: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_chart_reload(_h: i64) {}

// Issue #481 — Calendar widget stubs (watchOS screen real estate
// makes a month grid impractical).
#[no_mangle]
pub extern "C" fn perry_ui_calendar_create(_y: i64, _m: i64, _cb: f64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_calendar_set_date(_h: i64, _y: i64, _m: i64, _d: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_calendar_get_selected_date(_h: i64) -> f64 {
    f64::from_bits(0x7FFC_0000_0000_0001)
}

// Issue #4772 — DatePicker widget stubs (watchOS screen real estate
// makes an inline date field impractical).
#[no_mangle]
pub extern "C" fn perry_ui_date_picker_create(_y: i64, _m: i64, _cb: f64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_date_picker_set_date(_h: i64, _y: i64, _m: i64, _d: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_date_picker_get_selected_date(_h: i64) -> f64 {
    f64::from_bits(0x7FFC_0000_0000_0001)
}

// Issue #473 — table sort/filter/multi-select stubs.
#[no_mangle]
pub extern "C" fn perry_ui_table_set_on_sort_change(_h: i64, _cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_allows_multiple_selection(_h: i64, _a: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_get_selected_rows_count(_h: i64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_table_get_selected_row_at(_h: i64, _n: i64) -> i64 {
    -1
}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_filter_text(_h: i64, _t: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_get_filter_text(_h: i64) -> f64 {
    f64::from_bits(0x7FFC_0000_0000_0001)
}

// TreeView (#480) — watchOS screen real estate makes hierarchical
// outline impractical; stubs only.
#[no_mangle]
pub extern "C" fn perry_ui_tree_node_create(_id: i64, _label: i64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_node_add_child(_p: i64, _c: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_create(_root: i64, _on_select: f64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_expand_all(_h: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_collapse_all(_h: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_get_selected_id(_h: i64) -> f64 {
    f64::from_bits(0x7FFC_0000_0000_0001)
}
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_control_size(_h: i64, _s: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_hover(_h: i64, _cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_click(_h: i64, _cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_double_click(_h: i64, _cb: f64) {}
// Continuous pointer events (issue #1868). watchOS doesn't expose raw
// touch coordinates through WKInterfaceObject — only WKTapGestureRecognizer
// fires, with no x/y. Symbols exported as no-op so cross-platform code
// links; the matrix entry stays Wired (symbol present) until the watchOS
// gesture-with-location API surfaces in a future watchOS release.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_mouse_down(_h: i64, _cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_mouse_up(_h: i64, _cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_mouse_move(_h: i64, _cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_widget_animate_opacity(_h: i64, _t: f64, _d: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_widget_animate_position(_h: i64, _dx: f64, _dy: f64, _d: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_tabbar_create(_on_change: f64) -> i64 {
    perry_ui_vstack_create(0.0)
}
#[no_mangle]
pub extern "C" fn perry_ui_tabbar_add_tab(h: i64, _label: i64) {
    let _ = h;
}
#[no_mangle]
pub extern "C" fn perry_ui_tabbar_set_selected(_h: i64, _idx: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_qrcode_create(_data: i64, _size: f64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_qrcode_set_data(_h: i64, _data: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_open_folder_dialog(_cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_save_file_dialog(_cb: f64, _name: i64, _types: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_poll_open_file() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_widget_add_overlay(_parent: i64, _child: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_overlay_frame(_h: i64, _x: f64, _y: f64, _w: f64, _h2: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_alert(_title: i64, _msg: i64, _btns: f64, _cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_alert_simple(_title: i64, _msg: i64) {}
#[no_mangle]
// #1033: signature aligned with the perry-dispatch row
// `[Widget, F64, F64]` and the TS surface `sheetCreate(body, w, h)`.
pub extern "C" fn perry_ui_sheet_create(_body: i64, _w: f64, _h: f64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_sheet_present(_sheet: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_sheet_dismiss(_sheet: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_app_on_terminate(_cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_app_on_activate(_cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_app_set_icon(_path: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_app_set_size(_app: i64, _w: f64, _h: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_app_set_frameless(_app: i64, _val: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_app_set_level(_app: i64, _ptr: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_app_set_transparent(_app: i64, _val: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_app_set_vibrancy(_app: i64, _ptr: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_app_set_activation_policy(_app: i64, _ptr: i64) {}
/// Issue #1280 — watchOS apps don't have user-resizable windows. Stub.
#[no_mangle]
pub extern "C" fn perry_ui_app_set_window_state(_app: i64, _ptr: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_toolbar_create() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_toolbar_add_item(_tb: i64, _label: i64, _icon: i64, _cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_toolbar_attach(_tb: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_window_create(_title: i64, _w: f64, _h: f64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_window_set_body(_window: i64, _widget: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_window_show(_window: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_window_close(_window: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_window_hide(_window: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_window_set_size(_window: i64, _w: f64, _h: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_window_on_focus_lost(_window: i64, _callback: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_create(_count: i64, _render: f64) -> i64 {
    perry_ui_vstack_create(0.0)
}
#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_update(_handle: i64, _count: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_create(_rows: f64, _cols: f64, _render: f64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_column_header(_h: i64, _col: i64, _title: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_column_width(_h: i64, _col: i64, _w: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_update_row_count(_h: i64, _count: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_on_row_select(_h: i64, _cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_get_selected_row(_h: i64) -> i64 {
    -1
}

// =============================================================================
// System functions — stubs for watchOS
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_system_open_url(_url: i64) {}
/// #917 — system share sheet stub on watchOS. WatchKit doesn't
/// expose a public share API; AirDrop / Messages share flows are
/// system-driven. Stub + first-call warning.
#[no_mangle]
pub extern "C" fn perry_system_share_text(_text_ptr: i64, _title_ptr: i64) {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_share_text",
        "watchOS does not expose a programmatic share sheet (#917)",
        Some("#917"),
    );
}
#[no_mangle]
pub extern "C" fn perry_system_share_url(_url_ptr: i64, _title_ptr: i64) {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_share_url",
        "watchOS does not expose a programmatic share sheet (#917)",
        Some("#917"),
    );
}

// #675 — App Group stubs on watchOS. WatchKit *does* support
// `UserDefaults(suiteName:)` against the same App Group entitlement
// as the iOS host, but the wiring is more complex; tracked as
// #675 follow-up.
#[no_mangle]
pub extern "C" fn perry_system_app_group_set(_key_ptr: i64, _value_ptr: i64) {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_app_group_set",
        "watchOS App Group not implemented (#675 follow-up)",
        Some("#675"),
    );
}
#[no_mangle]
pub extern "C" fn perry_system_app_group_get(_key_ptr: i64) -> i64 {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_app_group_get",
        "watchOS App Group not implemented (#675 follow-up)",
        Some("#675"),
    );
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    }
    unsafe { js_string_from_bytes(std::ptr::null(), 0) }
}
#[no_mangle]
pub extern "C" fn perry_system_app_group_delete(_key_ptr: i64) {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_app_group_delete",
        "watchOS App Group not implemented (#675 follow-up)",
        Some("#675"),
    );
}
#[no_mangle]
pub extern "C" fn perry_system_request_location(_cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_system_audio_start() -> f64 {
    audio::start() as f64
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
pub extern "C" fn perry_system_audio_get_waveform(_count: f64) -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn perry_system_get_device_model() -> i64 {
    audio::get_device_model()
}
/// Bug-report-flow utility: stable OS-version string. watchOS stub.
#[no_mangle]
pub extern "C" fn perry_system_get_os_version() -> i64 {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_get_os_version",
        "watchOS getOSVersion not yet implemented",
        None,
    );
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    }
    unsafe { js_string_from_bytes(std::ptr::null(), 0) }
}
#[no_mangle]
pub extern "C" fn perry_system_audio_set_output_filename(filename_ptr: i64) {
    let filename = str_from_header(filename_ptr as *const u8);
    audio::set_output_filename(filename);
}
#[no_mangle]
pub extern "C" fn perry_system_audio_start_recording() {
    audio::start_recording();
}
#[no_mangle]
pub extern "C" fn perry_system_audio_stop_recording() {
    audio::stop_recording();
}
#[no_mangle]
pub extern "C" fn perry_system_is_dark_mode() -> i64 {
    0
}
// perry_system_preferences_* / perry_system_keychain_* /
// perry_system_get_locale live in `system.rs` (real NSUserDefaults /
// SecItem / NSLocale implementations, ported from iOS).
#[no_mangle]
pub extern "C" fn perry_system_notification_send(title: i64, body: i64) {
    notifications::send(title as *const u8, body as *const u8);
}
// Remote push (register/receive/background-receive) has no watchOS equivalent —
// there is no `UIApplication registerForRemoteNotifications` on watchOS — so
// these stay no-op stubs. Local notifications below are fully implemented.
#[no_mangle]
pub extern "C" fn perry_system_notification_register_remote(_callback: f64) {}
#[no_mangle]
pub extern "C" fn perry_system_notification_on_receive(_callback: f64) {}
#[no_mangle]
pub extern "C" fn perry_system_notification_on_background_receive(_callback: f64) {}
#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_interval(
    id_ptr: i64,
    title_ptr: i64,
    body_ptr: i64,
    seconds: f64,
    repeats: f64,
) {
    notifications::schedule_interval(
        id_ptr as *const u8,
        title_ptr as *const u8,
        body_ptr as *const u8,
        seconds,
        repeats,
    );
}
#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_calendar(
    id_ptr: i64,
    title_ptr: i64,
    body_ptr: i64,
    timestamp_ms: f64,
) {
    notifications::schedule_calendar(
        id_ptr as *const u8,
        title_ptr as *const u8,
        body_ptr as *const u8,
        timestamp_ms,
    );
}
// `UNLocationNotificationTrigger` (geofencing) is iOS-only — no CoreLocation
// region monitoring on watchOS — so location-triggered notifications stay a
// no-op stub.
#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_location(
    _id_ptr: i64,
    _title_ptr: i64,
    _body_ptr: i64,
    _lat: f64,
    _lon: f64,
    _radius: f64,
) {
}
#[no_mangle]
pub extern "C" fn perry_system_notification_cancel(id_ptr: i64) {
    notifications::cancel(id_ptr as *const u8);
}
#[no_mangle]
pub extern "C" fn perry_system_notification_on_tap(callback: f64) {
    notifications::set_on_tap(callback);
}
#[no_mangle]
pub extern "C" fn perry_get_screen_width() -> f64 {
    198.0
} // Apple Watch Ultra width
#[no_mangle]
pub extern "C" fn perry_get_screen_height() -> f64 {
    242.0
}
#[no_mangle]
pub extern "C" fn perry_get_scale_factor() -> f64 {
    2.0
}
#[no_mangle]
pub extern "C" fn perry_get_orientation() -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn perry_get_device_idiom() -> f64 {
    4.0
} // 4 = watch
#[no_mangle]
pub extern "C" fn perry_ui_camera_create() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_camera_start(_h: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_camera_stop(_h: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_camera_freeze(_h: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_camera_unfreeze(_h: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_camera_sample_color(_x: f64, _y: f64) -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn perry_ui_camera_set_on_tap(_h: i64, _cb: f64) {}

// WebSocket stubs (hone legacy API)
#[no_mangle]
pub extern "C" fn hone_get_documents_dir() -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn __wrapper_hone_get_documents_dir() -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn hone_ws_connect(_url: i64) -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn __wrapper_hone_ws_connect(_url: f64) -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn hone_ws_is_open(_h: f64) -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn __wrapper_hone_ws_is_open(_h: f64) -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn hone_ws_send(_h: f64, _msg: i64) {}
#[no_mangle]
pub extern "C" fn __wrapper_hone_ws_send(_h: f64, _msg: f64) {}
#[no_mangle]
pub extern "C" fn hone_ws_receive(_h: f64) -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn __wrapper_hone_ws_receive(_h: f64) -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn hone_ws_message_count(_h: f64) -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn __wrapper_hone_ws_message_count(_h: f64) -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn hone_ws_close(_h: f64) {}
#[no_mangle]
pub extern "C" fn __wrapper_hone_ws_close(_h: f64) {}

// =============================================================================
// perry/media — streaming media playback (issue #351). AVPlayer-backed.
// See `media_playback.rs` for the implementation; everything below is a
// thin FFI thunk that the codegen-emitted `perry_media_*` declarations
// resolve to at link time.
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_media_create_player(url_ptr: i64) -> i64 {
    media_playback::create_player(url_ptr as *const u8)
}

#[no_mangle]
pub extern "C" fn perry_media_play(handle: f64) {
    media_playback::play(handle);
}

#[no_mangle]
pub extern "C" fn perry_media_pause(handle: f64) {
    media_playback::pause(handle);
}

#[no_mangle]
pub extern "C" fn perry_media_stop(handle: f64) {
    media_playback::stop(handle);
}

#[no_mangle]
pub extern "C" fn perry_media_seek(handle: f64, seconds: f64) {
    media_playback::seek(handle, seconds);
}

#[no_mangle]
pub extern "C" fn perry_media_set_volume(handle: f64, volume: f64) {
    media_playback::set_volume(handle, volume);
}

#[no_mangle]
pub extern "C" fn perry_media_set_rate(handle: f64, rate: f64) {
    media_playback::set_rate(handle, rate);
}

#[no_mangle]
pub extern "C" fn perry_media_get_current_time(handle: f64) -> f64 {
    media_playback::get_current_time(handle)
}

#[no_mangle]
pub extern "C" fn perry_media_get_duration(handle: f64) -> f64 {
    media_playback::get_duration(handle)
}

#[no_mangle]
pub extern "C" fn perry_media_get_state(handle: f64) -> i64 {
    media_playback::get_state(handle)
}

#[no_mangle]
pub extern "C" fn perry_media_is_playing(handle: f64) -> f64 {
    media_playback::is_playing(handle)
}

#[no_mangle]
pub extern "C" fn perry_media_on_state_change(handle: f64, closure: f64) {
    media_playback::on_state_change(handle, closure);
}

#[no_mangle]
pub extern "C" fn perry_media_on_time_update(handle: f64, closure: f64) {
    media_playback::on_time_update(handle, closure);
}

#[no_mangle]
pub extern "C" fn perry_media_set_now_playing(
    handle: f64,
    title_ptr: i64,
    artist_ptr: i64,
    album_ptr: i64,
    artwork_ptr: i64,
) {
    media_playback::set_now_playing(
        handle,
        title_ptr as *const u8,
        artist_ptr as *const u8,
        album_ptr as *const u8,
        artwork_ptr as *const u8,
    );
}

#[no_mangle]
pub extern "C" fn perry_media_destroy(handle: f64) {
    media_playback::destroy(handle);
}

pub mod deeplinks_stub;
pub mod issue_552_stub;
pub mod network_stub;

// =============================================================================
// Issue #553 — BottomNavigation, pull-to-refresh on LazyVStack, onScrollEnd,
// ImageGallery. Stub block — these widgets aren't natively implemented on
// this platform yet; the symbols exist so cross-platform code compiles
// without conditional branching. Real macOS + iOS implementations live in
// perry-ui-macos and perry-ui-ios. Filling in the platform-specific
// equivalents (BottomNavigationView on Android, GtkBox+ToggleButton on
// GTK4, custom XAML-style strip on Windows, UIPageViewController flavors
// for tvOS/watchOS/visionOS) is tracked in the same issue.

#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_create(_on_select: f64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_add_item(_handle: i64, _icon_ptr: i64, _label_ptr: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_badge(_handle: i64, _index: i64, _badge_ptr: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_selected(_handle: i64, _index: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_tint_color(_h: i64, _r: f64, _g: f64, _b: f64, _a: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_unselected_tint_color(
    _h: i64,
    _r: f64,
    _g: f64,
    _b: f64,
    _a: f64,
) {
}

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_set_refresh_control(_handle: i64, _callback: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_end_refreshing(_handle: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_set_scroll_end_callback(
    _handle: i64,
    _callback: f64,
    _threshold_items: i64,
) {
}
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_scroll_end_callback(
    _handle: i64,
    _callback: f64,
    _threshold_px: f64,
) {
}

#[no_mangle]
pub extern "C" fn perry_ui_image_gallery_create(_on_index_change: f64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_image_gallery_add_image(_handle: i64, _url_ptr: i64, _alt_ptr: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_image_gallery_set_index(_handle: i64, _index: i64) {}

// ---- perry/background (issue #538) — WKApplication.scheduleBackgroundRefresh ----
#[no_mangle]
pub extern "C" fn perry_background_register_task(identifier_ptr: i64, handler: f64) {
    background::register_task(identifier_ptr as *const u8, handler);
}
#[no_mangle]
pub extern "C" fn perry_background_schedule(
    identifier_ptr: i64,
    kind_ptr: i64,
    earliest_start_ms: f64,
    requires_network: f64,
    requires_charging: f64,
) {
    background::schedule(
        identifier_ptr as *const u8,
        kind_ptr as *const u8,
        earliest_start_ms,
        requires_network,
        requires_charging,
    );
}
#[no_mangle]
pub extern "C" fn perry_background_cancel(identifier_ptr: i64) {
    background::cancel(identifier_ptr as *const u8);
}

// --- WebView (issue #658) — stub on this platform; link-stability shape
//     matching the macOS / iOS / visionOS surface. v1 returns a 0 handle
//     and the imperative ops are no-ops; user code that imports WebView
//     still compiles and runs but the widget is invisible. Real backend
//     deferred to a later phase per #658's roadmap.
#[no_mangle]
pub extern "C" fn perry_ui_webview_create(
    _url_ptr: i64,
    _width: f64,
    _height: f64,
    _ephemeral: f64,
) -> i64 {
    0
}

// BloomView (issue #2395) — stub on this platform, matching the WebView shape
// above; returns a 0 handle so apps that import BloomView still link and run.
#[no_mangle]
pub extern "C" fn perry_ui_bloomview_create(_width: f64, _height: f64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_bloomview_get_hwnd(_handle: i64) -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ui_webview_set_user_agent(_handle: i64, _ua_ptr: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_allowed_domains(_handle: i64, _arr_handle: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_ephemeral(_handle: i64, _ephemeral: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_on_should_navigate(_handle: i64, _closure: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_on_loaded(_handle: i64, _closure: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_on_error(_handle: i64, _closure: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_webview_load_url(_handle: i64, _url_ptr: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_webview_reload(_handle: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_webview_go_back(_handle: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_webview_go_forward(_handle: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_webview_can_go_back(_handle: i64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_evaluate_js(_handle: i64, _js_ptr: i64, _callback: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_webview_clear_cookies(_handle: i64) {}

// AttributedText (Issue #710) — watchOS tree node + run buffer.
// SwiftUI host iterates `perry_watchos_node_attr_run_*` and emits a
// concatenated `Text(run).bold().italic().underline().foregroundColor(...)`.
#[no_mangle]
pub extern "C" fn perry_ui_attributed_text_create() -> i64 {
    let node = tree::NodeData::new(tree::NodeKind::AttributedText);
    tree::register_node(node)
}
#[no_mangle]
pub extern "C" fn perry_ui_attributed_text_append(
    h: i64,
    t: i64,
    bold: i64,
    italic: i64,
    underline: i64,
    font_size: f64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    let text = match cstring_from_header(t as *const u8) {
        Some(s) => s,
        None => return,
    };
    tree::with_node_mut(h, |node| {
        node.attributed_runs.push((
            text,
            bold != 0,
            italic != 0,
            underline != 0,
            font_size,
            r,
            g,
            b,
            a,
        ));
    });
}
#[no_mangle]
pub extern "C" fn perry_ui_attributed_text_clear(h: i64) {
    tree::with_node_mut(h, |node| {
        node.attributed_runs.clear();
    });
}

// ---- In-app screen capture (issue #918) ----
/// watchOS has no screenshot-capable runtime today (no `screenshot.rs` in
/// this crate). Returns an empty string so callers can branch on length.
#[no_mangle]
pub extern "C" fn perry_system_take_screenshot() -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    }
    unsafe { js_string_from_bytes(std::ptr::null(), 0) }
}

/// #1475 — safe-area insets. watchOS layout uses the system-provided safe
/// area implicitly; no explicit inset query is exposed here, so report
/// all-zero. Keeps the symbol present so `getSafeAreaInsets()` links.
#[no_mangle]
pub extern "C" fn perry_system_get_safe_area_insets() -> f64 {
    extern "C" {
        fn perry_safe_area_insets_make(top: f64, right: f64, bottom: f64, left: f64) -> f64;
    }
    unsafe { perry_safe_area_insets_make(0.0, 0.0, 0.0, 0.0) }
}

/// Load an image asset for Canvas.drawImage. watchOS currently has no real
/// Canvas backend, so expose a proper rejected Promise rather than a bare 0.
#[no_mangle]
pub extern "C" fn perry_ui_load_image(_url_ptr: i64) -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: u32) -> *mut std::ffi::c_void;
        fn js_nanbox_string(ptr: i64) -> f64;
        fn js_promise_rejected(reason: f64) -> *mut std::ffi::c_void;
    }
    let message = b"Canvas image loading is not available on watchOS";
    unsafe {
        let str_ptr = js_string_from_bytes(message.as_ptr(), message.len() as u32);
        js_promise_rejected(js_nanbox_string(str_ptr as i64)) as i64
    }
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_draw_image(
    _h: i64,
    _image: i64,
    _sx: f64,
    _sy: f64,
    _sw: f64,
    _sh: f64,
    _dx: f64,
    _dy: f64,
    _dw: f64,
    _dh: f64,
) {
}
