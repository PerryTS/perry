//! GTK4 Tree / outline view — `gtk4::TreeView` + `gtk4::TreeStore`
//! (issue #480 / Linux parity work).
//!
//! GTK 4.10+ recommends `GtkColumnView` + `GtkTreeListModel` over
//! the legacy TreeView/TreeStore stack, but TreeView is still
//! functional through gtk4-rs 0.9 + the `v4_6` feature gate this
//! crate uses, and the legacy API gives us a 1:1 mapping for the
//! `TreeNode` + `treeNodeAddChild` + `TreeView(root, onSelect)` API
//! shape with substantially less ceremony than ColumnView's
//! property-binding plumbing. Migration to ColumnView when the v4_6
//! gate is bumped past 4.10 is a follow-up.

use gtk4::glib::Type as GType;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

struct TreeNode {
    id: String,
    label: String,
    children: Vec<i64>,
}

thread_local! {
    static NODES: RefCell<Vec<TreeNode>> = const { RefCell::new(Vec::new()) };
    /// Per-tree-handle map back to (tree_view, model, last_selected_id_owner).
    /// We need TreeView to query selection later via `get_selected_id`.
    static TREES: RefCell<HashMap<i64, gtk4::TreeView>> = RefCell::new(HashMap::new());
}

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

pub fn node_create(id_ptr: *const u8, label_ptr: *const u8) -> i64 {
    let id = str_from_header(id_ptr).to_string();
    let label = str_from_header(label_ptr).to_string();
    NODES.with(|n| {
        let mut nodes = n.borrow_mut();
        nodes.push(TreeNode {
            id,
            label,
            children: Vec::new(),
        });
        nodes.len() as i64
    })
}

pub fn node_add_child(parent: i64, child: i64) {
    if parent <= 0 || child <= 0 {
        return;
    }
    NODES.with(|n| {
        let mut nodes = n.borrow_mut();
        if let Some(parent_node) = nodes.get_mut((parent - 1) as usize) {
            parent_node.children.push(child);
        }
    });
}

/// Recursively populate `store` rooted at `node_id`. Column 0 is the
/// node's label (display text); column 1 is the node's `id` string
/// (used by `get_selected_id`).
fn populate_store(store: &gtk4::TreeStore, parent_iter: Option<&gtk4::TreeIter>, node_id: i64) {
    let snapshot = NODES.with(|n| {
        n.borrow()
            .get((node_id - 1) as usize)
            .map(|node| (node.id.clone(), node.label.clone(), node.children.clone()))
    });
    let Some((id, label, children)) = snapshot else {
        return;
    };
    let iter = store.append(parent_iter);
    store.set_value(&iter, 0, &label.to_value());
    store.set_value(&iter, 1, &id.to_value());
    for child in children {
        populate_store(store, Some(&iter), child);
    }
}

pub fn create(root_node: i64, on_select: f64) -> i64 {
    crate::app::ensure_gtk_init();
    let store = gtk4::TreeStore::new(&[GType::STRING, GType::STRING]);
    if root_node > 0 {
        populate_store(&store, None, root_node);
    }

    let tree_view = gtk4::TreeView::with_model(&store);
    tree_view.set_headers_visible(false);

    let column = gtk4::TreeViewColumn::new();
    let cell = gtk4::CellRendererText::new();
    column.pack_start(&cell, true);
    column.add_attribute(&cell, "text", 0);
    tree_view.append_column(&column);

    if on_select != 0.0 {
        let on = on_select;
        let store_clone = store.clone();
        tree_view.selection().connect_changed(move |sel| {
            let Some((_model, iter)) = sel.selected() else {
                return;
            };
            let id = match store_clone.get_value(&iter, 1).get::<String>() {
                Ok(s) => s,
                Err(_) => return,
            };
            let bytes = id.as_bytes();
            unsafe {
                let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
                let arg = js_nanbox_string(header as i64);
                let closure_ptr = js_nanbox_get_pointer(on) as *const u8;
                js_closure_call1(closure_ptr, arg);
            }
        });
    }

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Automatic);
    scroll.set_child(Some(&tree_view));

    let handle = super::register_widget(scroll.upcast());
    TREES.with(|t| t.borrow_mut().insert(handle, tree_view));
    handle
}

pub fn expand_all(handle: i64) {
    if let Some(tv) = TREES.with(|t| t.borrow().get(&handle).cloned()) {
        tv.expand_all();
    }
}

pub fn collapse_all(handle: i64) {
    if let Some(tv) = TREES.with(|t| t.borrow().get(&handle).cloned()) {
        tv.collapse_all();
    }
}

pub fn get_selected_id(handle: i64) -> f64 {
    let tv = TREES.with(|t| t.borrow().get(&handle).cloned());
    let Some(tv) = tv else {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    };
    let Some((model, iter)) = tv.selection().selected() else {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    };
    let id = match model.get_value(&iter, 1).get::<String>() {
        Ok(s) => s,
        Err(_) => return f64::from_bits(0x7FFC_0000_0000_0001),
    };
    let bytes = id.as_bytes();
    unsafe {
        let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
        js_nanbox_string(header as i64)
    }
}
