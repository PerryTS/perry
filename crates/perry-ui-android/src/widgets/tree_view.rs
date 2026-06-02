//! Android TreeView — issue #480.
//!
//! Android's built-in `ExpandableListView` only supports two levels, so we
//! render the tree as a flat indented list on top of a plain
//! `android.widget.ListView`. The Rust side owns the node graph
//! (TREE_NODES) — `perry_ui_tree_node_create` / `_add_child` build the
//! graph, `perry_ui_tree_view_create` flattens the *visible* portion of
//! the tree and hands a `String[]` of rendered rows + a parallel
//! `String[]` of node IDs to PerryBridge, which renders them via an
//! ArrayAdapter with depth-based indentation and a chevron prefix that
//! toggles expand/collapse via `nativeInvokeCallbackWithString(key, id)`.

use crate::app::str_from_header;
use crate::callback;
use crate::jni_bridge;
use jni::objects::{JObject, JString, JValue};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

#[derive(Clone)]
struct TreeNode {
    id: String,
    label: String,
    children: Vec<i64>, // node handles, 1-based
}

#[derive(Clone)]
struct TreeViewState {
    root_node: i64,
    expanded: HashSet<i64>,
    callback_key: i64,
    selected_id: Option<String>,
}

thread_local! {
    static TREE_NODES: RefCell<HashMap<i64, TreeNode>> = RefCell::new(HashMap::new());
    static NEXT_NODE_ID: RefCell<i64> = const { RefCell::new(1) };
    static TREE_VIEWS: RefCell<HashMap<i64, TreeViewState>> = RefCell::new(HashMap::new());
}

/// Build a tree node (label + id). Returns an i64 node handle. These
/// handles are NOT widget handles — they live in a separate per-thread
/// map and only become a widget when `tree_view_create(root_node, ...)`
/// realizes them as a ListView.
pub fn node_create(id_ptr: *const u8, label_ptr: *const u8) -> i64 {
    let id = str_from_header(id_ptr).to_string();
    let label = str_from_header(label_ptr).to_string();
    NEXT_NODE_ID.with(|n| {
        let mut counter = n.borrow_mut();
        let handle = *counter;
        *counter += 1;
        TREE_NODES.with(|m| {
            m.borrow_mut().insert(
                handle,
                TreeNode {
                    id,
                    label,
                    children: Vec::new(),
                },
            );
        });
        handle
    })
}

pub fn node_add_child(parent: i64, child: i64) {
    TREE_NODES.with(|m| {
        if let Some(p) = m.borrow_mut().get_mut(&parent) {
            p.children.push(child);
        }
    });
}

/// Realize the tree as an Android ListView and register it as a widget.
pub fn create(root_node: i64, on_select: f64) -> i64 {
    let cb_key = if on_select != 0.0 {
        callback::register(on_select)
    } else {
        0
    };

    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let result = env.call_static_method(
        bridge_cls,
        "treeViewCreate",
        "(J)Landroid/widget/ListView;",
        &[JValue::Long(cb_key)],
    );
    let widget_handle = match result {
        Ok(jv) => match jv.l() {
            Ok(obj) if !obj.is_null() => {
                let g = env.new_global_ref(obj).expect("global-ref TreeView");
                super::register_widget(g)
            }
            _ => 0,
        },
        Err(_) => {
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_describe();
                let _ = env.exception_clear();
            }
            0
        }
    };
    unsafe {
        env.pop_local_frame(&JObject::null());
    }

    if widget_handle != 0 {
        TREE_VIEWS.with(|m| {
            m.borrow_mut().insert(
                widget_handle,
                TreeViewState {
                    root_node,
                    expanded: HashSet::new(),
                    callback_key: cb_key,
                    selected_id: None,
                },
            );
        });
        refresh(widget_handle);
    }
    widget_handle
}

pub fn expand_all(widget_handle: i64) {
    TREE_VIEWS.with(|m| {
        let root = m
            .borrow()
            .get(&widget_handle)
            .map(|s| s.root_node)
            .unwrap_or(0);
        if root == 0 {
            return;
        }
        let mut all = HashSet::new();
        collect_descendants(root, &mut all);
        if let Some(state) = m.borrow_mut().get_mut(&widget_handle) {
            state.expanded = all;
        }
    });
    refresh(widget_handle);
}

pub fn collapse_all(widget_handle: i64) {
    TREE_VIEWS.with(|m| {
        if let Some(state) = m.borrow_mut().get_mut(&widget_handle) {
            state.expanded.clear();
        }
    });
    refresh(widget_handle);
}

pub fn get_selected_id(widget_handle: i64) -> f64 {
    let selected = TREE_VIEWS.with(|m| {
        m.borrow()
            .get(&widget_handle)
            .and_then(|s| s.selected_id.clone())
    });
    match selected {
        Some(s) => {
            let bytes = s.as_bytes();
            unsafe {
                let p = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
                js_nanbox_string(p as i64)
            }
        }
        None => f64::from_bits(TAG_UNDEFINED),
    }
}

/// JNI-facing entry point. Called by PerryBridge when the user taps a row.
/// `id` is the tree node id (string), looked up against the most-recent
/// flatten and used both to (a) toggle expand/collapse if it's an interior
/// node and (b) fire the on_select callback.
pub fn handle_row_tap(widget_handle: i64, id: &str) {
    let (callback_key, has_children) = TREE_VIEWS.with(|m| {
        let mut tv = m.borrow_mut();
        let Some(state) = tv.get_mut(&widget_handle) else {
            return (0i64, false);
        };
        state.selected_id = Some(id.to_string());
        // Resolve id -> node handle, toggle expand if it has children.
        let node = find_node_by_id(state.root_node, id);
        let has_kids = node
            .and_then(|n| {
                TREE_NODES.with(|nm| nm.borrow().get(&n).map(|tn| !tn.children.is_empty()))
            })
            .unwrap_or(false);
        if let Some(n) = node {
            if has_kids {
                if state.expanded.contains(&n) {
                    state.expanded.remove(&n);
                } else {
                    state.expanded.insert(n);
                }
            }
        }
        (state.callback_key, has_kids)
    });
    if has_children {
        refresh(widget_handle);
    }
    if callback_key != 0 {
        let bytes = id.as_bytes();
        unsafe {
            let p = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
            let nb = js_nanbox_string(p as i64);
            callback::invoke1(callback_key, nb);
        }
    }
}

// =========================== internal helpers ===========================

fn collect_descendants(handle: i64, acc: &mut HashSet<i64>) {
    acc.insert(handle);
    let children: Vec<i64> = TREE_NODES.with(|m| {
        m.borrow()
            .get(&handle)
            .map(|n| n.children.clone())
            .unwrap_or_default()
    });
    for c in children {
        collect_descendants(c, acc);
    }
}

fn find_node_by_id(root: i64, id: &str) -> Option<i64> {
    TREE_NODES.with(|m| {
        let nodes = m.borrow();
        let mut stack = vec![root];
        while let Some(h) = stack.pop() {
            let Some(n) = nodes.get(&h) else { continue };
            if n.id == id {
                return Some(h);
            }
            for c in n.children.iter().rev() {
                stack.push(*c);
            }
        }
        None
    })
}

fn flatten_visible(widget_handle: i64) -> (Vec<String>, Vec<String>) {
    let mut rows: Vec<String> = Vec::new();
    let mut ids: Vec<String> = Vec::new();
    let (root, expanded) = TREE_VIEWS.with(|m| {
        m.borrow()
            .get(&widget_handle)
            .map(|s| (s.root_node, s.expanded.clone()))
            .unwrap_or((0, HashSet::new()))
    });
    if root == 0 {
        return (rows, ids);
    }
    TREE_NODES.with(|m| {
        let nodes = m.borrow();
        // Render the root and its descendants. Each row is
        // "  " * depth + chevron + label.
        fn walk(
            handle: i64,
            depth: usize,
            nodes: &HashMap<i64, TreeNode>,
            expanded: &HashSet<i64>,
            rows: &mut Vec<String>,
            ids: &mut Vec<String>,
        ) {
            let Some(node) = nodes.get(&handle) else {
                return;
            };
            let indent = "    ".repeat(depth);
            let chevron = if node.children.is_empty() {
                "  "
            } else if expanded.contains(&handle) {
                "▾ "
            } else {
                "▸ "
            };
            rows.push(format!("{}{}{}", indent, chevron, node.label));
            ids.push(node.id.clone());
            if expanded.contains(&handle) {
                for c in &node.children {
                    walk(*c, depth + 1, nodes, expanded, rows, ids);
                }
            }
        }
        walk(root, 0, &nodes, &expanded, &mut rows, &mut ids);
    });
    (rows, ids)
}

fn refresh(widget_handle: i64) {
    let Some(view) = super::get_widget(widget_handle) else {
        return;
    };
    let (rows, ids) = flatten_visible(widget_handle);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(64);

    let str_class = env.find_class("java/lang/String").expect("String class");
    let rows_arr = env
        .new_object_array(rows.len() as i32, &str_class, &JObject::null())
        .expect("rows array");
    for (i, row) in rows.iter().enumerate() {
        let js = env.new_string(row).expect("row string");
        let _ = env.set_object_array_element(&rows_arr, i as i32, &js);
    }
    let ids_arr = env
        .new_object_array(ids.len() as i32, &str_class, &JObject::null())
        .expect("ids array");
    for (i, id) in ids.iter().enumerate() {
        let js = env.new_string(id).expect("id string");
        let _ = env.set_object_array_element(&ids_arr, i as i32, &js);
    }

    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(
        bridge_cls,
        "treeViewRefresh",
        "(JLandroid/widget/ListView;[Ljava/lang/String;[Ljava/lang/String;)V",
        &[
            JValue::Long(widget_handle),
            JValue::Object(view.as_obj()),
            JValue::Object(&rows_arr),
            JValue::Object(&ids_arr),
        ],
    );
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    unsafe {
        env.pop_local_frame(&JObject::null());
    }
}

/// JNI entry point: PerryBridge.nativeTreeRowTapped(long widgetHandle, String id).
/// Runs on the UI thread (ListView OnItemClickListener).
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeTreeRowTapped(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    widget_handle: jni::sys::jlong,
    id: JString,
) {
    let s: String = env.get_string(&id).map(|j| j.into()).unwrap_or_default();
    handle_row_tap(widget_handle as i64, &s);
}
