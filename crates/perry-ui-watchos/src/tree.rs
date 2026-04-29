//! Data-driven widget tree for watchOS.
//!
//! Stores UI nodes as plain data. The fixed PerryWatchApp.swift queries this
//! tree via FFI and renders it as SwiftUI views.

use std::cell::{Cell, RefCell};
use std::ffi::CString;

/// Widget types matching the Swift-side `NodeView` switch cases.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NodeKind {
    Text = 0,
    Button = 1,
    VStack = 2,
    HStack = 3,
    ZStack = 4,
    Spacer = 5,
    Divider = 6,
    Toggle = 7,
    Slider = 8,
    Image = 9,
    ScrollView = 10,
    ProgressView = 11,
    Picker = 12,
    List = 13,
    NavigationStack = 14,
    TextArea = 15,
}

/// A single node in the UI tree.
pub struct NodeData {
    pub kind: NodeKind,
    pub text: Option<CString>,
    pub children: Vec<i64>,
    /// NaN-boxed closure pointer for button taps, toggle changes, etc.
    pub action_closure: Option<f64>,
    /// NaN-boxed closure for on_change events (slider, picker, etc.)
    pub on_change_closure: Option<f64>,
    // Style properties
    pub font_size: Option<f64>,
    pub font_weight: Option<f64>,
    pub color: Option<(f64, f64, f64, f64)>,
    pub bg_color: Option<(f64, f64, f64, f64)>,
    pub padding: Option<f64>,
    pub frame_width: Option<f64>,
    pub frame_height: Option<f64>,
    pub corner_radius: Option<f64>,
    pub opacity: f64,
    pub hidden: bool,
    pub enabled: bool,
    pub spacing: f64,
    // Slider-specific
    pub slider_value: f64,
    pub slider_min: f64,
    pub slider_max: f64,
    // Toggle-specific
    pub toggle_on: bool,
    // ProgressView-specific
    pub progress_value: f64,
    // Picker-specific
    pub picker_items: Vec<CString>,
    pub picker_selected: i64,
    // Image
    pub image_system_name: Option<CString>,
    pub image_width: Option<f64>,
    pub image_height: Option<f64>,
    pub image_tint: Option<(f64, f64, f64, f64)>,
    // Border
    pub border_color: Option<(f64, f64, f64, f64)>,
    pub border_width: Option<f64>,
    // Drop shadow: (r, g, b, a, blur, offset_x, offset_y).
    // Issue #185 Phase B; populated for tree-introspection clients
    // even though the watchOS UIKit subset can't render shadows
    // directly (SwiftUI host applies them when present).
    pub shadow: Option<(f64, f64, f64, f64, f64, f64, f64)>,
    // Edge insets (padding per-side)
    pub edge_insets: Option<(f64, f64, f64, f64)>,
    // Font family
    pub font_family: Option<CString>,
    // Text wrapping
    pub text_wraps: bool,
    // Text decoration (issue #185 Phase B): 0=none, 1=underline, 2=strikethrough.
    // SwiftUI host applies `.underline()` / `.strikethrough()` modifier when
    // rendering.
    pub text_decoration: i64,
}

impl NodeData {
    pub fn new(kind: NodeKind) -> Self {
        Self {
            kind,
            text: None,
            children: Vec::new(),
            action_closure: None,
            on_change_closure: None,
            font_size: None,
            font_weight: None,
            color: None,
            bg_color: None,
            padding: None,
            frame_width: None,
            frame_height: None,
            corner_radius: None,
            opacity: 1.0,
            hidden: false,
            enabled: true,
            spacing: 0.0,
            slider_value: 0.0,
            slider_min: 0.0,
            slider_max: 1.0,
            toggle_on: false,
            progress_value: 0.0,
            picker_items: Vec::new(),
            picker_selected: 0,
            image_system_name: None,
            image_width: None,
            image_height: None,
            image_tint: None,
            border_color: None,
            border_width: None,
            shadow: None,
            edge_insets: None,
            font_family: None,
            text_wraps: false,
            text_decoration: 0,
        }
    }
}

thread_local! {
    /// All nodes, indexed by handle-1
    static NODES: RefCell<Vec<NodeData>> = RefCell::new(Vec::new());
    /// Handle of the root widget
    static ROOT_NODE: Cell<i64> = Cell::new(0);
    /// Incremented on any tree mutation to signal SwiftUI to re-render
    static TREE_VERSION: Cell<u64> = Cell::new(0);
}

fn bump_version() {
    TREE_VERSION.with(|v| v.set(v.get().wrapping_add(1)));
}

/// Register a new node, returning its 1-based handle.
pub fn register_node(node: NodeData) -> i64 {
    NODES.with(|n| {
        let mut nodes = n.borrow_mut();
        nodes.push(node);
        let handle = nodes.len() as i64;
        bump_version();
        handle
    })
}

/// Get a reference to a node by handle (1-based).
pub fn with_node<F, R>(handle: i64, f: F) -> Option<R>
where
    F: FnOnce(&NodeData) -> R,
{
    NODES.with(|n| {
        let nodes = n.borrow();
        let idx = (handle - 1) as usize;
        nodes.get(idx).map(f)
    })
}

/// Mutate a node by handle (1-based).
pub fn with_node_mut<F, R>(handle: i64, f: F) -> Option<R>
where
    F: FnOnce(&mut NodeData) -> R,
{
    NODES.with(|n| {
        let mut nodes = n.borrow_mut();
        let idx = (handle - 1) as usize;
        let result = nodes.get_mut(idx).map(f);
        if result.is_some() {
            drop(nodes);
            bump_version();
        }
        result
    })
}

/// Add a child handle to a parent node.
pub fn add_child(parent_handle: i64, child_handle: i64) {
    with_node_mut(parent_handle, |parent| {
        parent.children.push(child_handle);
    });
}

/// Remove a child from a parent node.
pub fn remove_child(parent_handle: i64, child_handle: i64) {
    with_node_mut(parent_handle, |parent| {
        parent.children.retain(|&c| c != child_handle);
    });
}

/// Clear all children from a node.
pub fn clear_children(handle: i64) {
    with_node_mut(handle, |node| {
        node.children.clear();
    });
}

/// Set the root node handle.
pub fn set_root(handle: i64) {
    ROOT_NODE.with(|r| r.set(handle));
    bump_version();
}

/// Set a node's hidden state.
pub fn set_hidden(handle: i64, hidden: bool) {
    with_node_mut(handle, |node| {
        node.hidden = hidden;
    });
}

// =========================================================================
// FFI query functions — called from PerryWatchApp.swift
// =========================================================================

#[no_mangle]
pub extern "C" fn perry_watchos_root_node() -> i64 {
    ROOT_NODE.with(|r| r.get())
}

#[no_mangle]
pub extern "C" fn perry_watchos_tree_version() -> u64 {
    TREE_VERSION.with(|v| v.get())
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_kind(id: i64) -> i32 {
    with_node(id, |n| n.kind as i32).unwrap_or(-1)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_text(id: i64) -> *const std::ffi::c_char {
    NODES.with(|n| {
        let nodes = n.borrow();
        let idx = (id - 1) as usize;
        if let Some(node) = nodes.get(idx) {
            if let Some(ref text) = node.text {
                return text.as_ptr();
            }
        }
        std::ptr::null()
    })
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_child_count(id: i64) -> i32 {
    with_node(id, |n| n.children.len() as i32).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_child(id: i64, index: i32) -> i64 {
    with_node(id, |n| n.children.get(index as usize).copied().unwrap_or(0)).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_has_action(id: i64) -> bool {
    with_node(id, |n| n.action_closure.is_some()).unwrap_or(false)
}

#[no_mangle]
pub extern "C" fn perry_watchos_handle_action(id: i64) {
    let closure = with_node(id, |n| n.action_closure).flatten();
    if let Some(closure_f64) = closure {
        extern "C" {
            fn js_nanbox_get_pointer(value: f64) -> i64;
            fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
        }
        let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) } as *const u8;
        unsafe {
            js_closure_call1(closure_ptr, 0.0);
        }
    }
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_hidden(id: i64) -> bool {
    with_node(id, |n| n.hidden).unwrap_or(false)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_enabled(id: i64) -> bool {
    with_node(id, |n| n.enabled).unwrap_or(true)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_opacity(id: i64) -> f64 {
    with_node(id, |n| n.opacity).unwrap_or(1.0)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_spacing(id: i64) -> f64 {
    with_node(id, |n| n.spacing).unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_font_size(id: i64) -> f64 {
    with_node(id, |n| n.font_size.unwrap_or(-1.0)).unwrap_or(-1.0)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_font_weight(id: i64) -> f64 {
    with_node(id, |n| n.font_weight.unwrap_or(-1.0)).unwrap_or(-1.0)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_color(id: i64, component: i32) -> f64 {
    with_node(id, |n| {
        if let Some((r, g, b, a)) = n.color {
            match component {
                0 => r,
                1 => g,
                2 => b,
                3 => a,
                _ => -1.0,
            }
        } else {
            -1.0
        }
    })
    .unwrap_or(-1.0)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_has_color(id: i64) -> bool {
    with_node(id, |n| n.color.is_some()).unwrap_or(false)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_bg_color(id: i64, component: i32) -> f64 {
    with_node(id, |n| {
        if let Some((r, g, b, a)) = n.bg_color {
            match component {
                0 => r,
                1 => g,
                2 => b,
                3 => a,
                _ => -1.0,
            }
        } else {
            -1.0
        }
    })
    .unwrap_or(-1.0)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_has_bg_color(id: i64) -> bool {
    with_node(id, |n| n.bg_color.is_some()).unwrap_or(false)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_corner_radius(id: i64) -> f64 {
    with_node(id, |n| n.corner_radius.unwrap_or(-1.0)).unwrap_or(-1.0)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_frame_width(id: i64) -> f64 {
    with_node(id, |n| n.frame_width.unwrap_or(-1.0)).unwrap_or(-1.0)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_frame_height(id: i64) -> f64 {
    with_node(id, |n| n.frame_height.unwrap_or(-1.0)).unwrap_or(-1.0)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_padding(id: i64) -> f64 {
    with_node(id, |n| n.padding.unwrap_or(-1.0)).unwrap_or(-1.0)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_slider_value(id: i64) -> f64 {
    with_node(id, |n| n.slider_value).unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_slider_min(id: i64) -> f64 {
    with_node(id, |n| n.slider_min).unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_slider_max(id: i64) -> f64 {
    with_node(id, |n| n.slider_max).unwrap_or(1.0)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_toggle_on(id: i64) -> bool {
    with_node(id, |n| n.toggle_on).unwrap_or(false)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_progress_value(id: i64) -> f64 {
    with_node(id, |n| n.progress_value).unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_picker_count(id: i64) -> i32 {
    with_node(id, |n| n.picker_items.len() as i32).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_picker_item(id: i64, index: i32) -> *const std::ffi::c_char {
    NODES.with(|n| {
        let nodes = n.borrow();
        let idx = (id - 1) as usize;
        if let Some(node) = nodes.get(idx) {
            if let Some(item) = node.picker_items.get(index as usize) {
                return item.as_ptr();
            }
        }
        std::ptr::null()
    })
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_picker_selected(id: i64) -> i64 {
    with_node(id, |n| n.picker_selected).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_text_wraps(id: i64) -> bool {
    with_node(id, |n| n.text_wraps).unwrap_or(false)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_has_edge_insets(id: i64) -> bool {
    with_node(id, |n| n.edge_insets.is_some()).unwrap_or(false)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_edge_inset(id: i64, side: i32) -> f64 {
    with_node(id, |n| {
        if let Some((top, left, bottom, right)) = n.edge_insets {
            match side {
                0 => top,
                1 => left,
                2 => bottom,
                3 => right,
                _ => 0.0,
            }
        } else {
            0.0
        }
    })
    .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_image_width(id: i64) -> f64 {
    with_node(id, |n| n.image_width.unwrap_or(-1.0)).unwrap_or(-1.0)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_image_height(id: i64) -> f64 {
    with_node(id, |n| n.image_height.unwrap_or(-1.0)).unwrap_or(-1.0)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_has_image_tint(id: i64) -> bool {
    with_node(id, |n| n.image_tint.is_some()).unwrap_or(false)
}

#[no_mangle]
pub extern "C" fn perry_watchos_node_image_tint(id: i64, component: i32) -> f64 {
    with_node(id, |n| {
        if let Some((r, g, b, a)) = n.image_tint {
            match component {
                0 => r,
                1 => g,
                2 => b,
                3 => a,
                _ => -1.0,
            }
        } else {
            -1.0
        }
    })
    .unwrap_or(-1.0)
}

/// Called from Swift when slider value changes
#[no_mangle]
pub extern "C" fn perry_watchos_slider_changed(id: i64, value: f64) {
    let closure = with_node_mut(id, |n| {
        n.slider_value = value;
        n.on_change_closure
    })
    .flatten();
    if let Some(closure_f64) = closure {
        extern "C" {
            fn js_nanbox_get_pointer(value: f64) -> i64;
            fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
        }
        let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) } as *const u8;
        unsafe {
            js_closure_call1(closure_ptr, value);
        }
    }
}

/// Called from Swift when toggle state changes
#[no_mangle]
pub extern "C" fn perry_watchos_toggle_changed(id: i64, on: bool) {
    let closure = with_node_mut(id, |n| {
        n.toggle_on = on;
        n.on_change_closure
    })
    .flatten();
    if let Some(closure_f64) = closure {
        extern "C" {
            fn js_nanbox_get_pointer(value: f64) -> i64;
            fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
        }
        let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) } as *const u8;
        unsafe {
            js_closure_call1(closure_ptr, if on { 1.0 } else { 0.0 });
        }
    }
}

/// Called from Swift when picker selection changes
#[no_mangle]
pub extern "C" fn perry_watchos_picker_changed(id: i64, index: i64) {
    let closure = with_node_mut(id, |n| {
        n.picker_selected = index;
        n.on_change_closure
    })
    .flatten();
    if let Some(closure_f64) = closure {
        extern "C" {
            fn js_nanbox_get_pointer(value: f64) -> i64;
            fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
        }
        let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) } as *const u8;
        unsafe {
            js_closure_call1(closure_ptr, index as f64);
        }
    }
}

/// Called from Swift when textarea content changes.
#[no_mangle]
pub extern "C" fn perry_watchos_textarea_changed(id: i64, text_ptr: *const std::ffi::c_char) {
    extern "C" {
        fn js_nanbox_get_pointer(value: f64) -> i64;
        fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
        fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
        fn js_nanbox_string(ptr: i64) -> f64;
    }

    let new_text = if text_ptr.is_null() {
        CString::default()
    } else {
        unsafe { std::ffi::CStr::from_ptr(text_ptr) }.to_owned()
    };
    let bytes = new_text.as_bytes();

    let closure = with_node_mut(id, |n| {
        n.text = Some(new_text.clone());
        n.on_change_closure
    })
    .flatten();

    if let Some(closure_f64) = closure {
        let str_ptr = unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64) };
        let nanboxed = unsafe { js_nanbox_string(str_ptr as i64) };
        let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) } as *const u8;
        unsafe {
            js_closure_call1(closure_ptr, nanboxed);
        }
    }
}
