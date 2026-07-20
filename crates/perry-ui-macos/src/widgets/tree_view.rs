//! macOS Tree / outline view widget (issue #480).
//!
//! Wraps `NSOutlineView` (an `NSTableView` subclass with hierarchical
//! disclosure). Tree topology is built TS-side via standalone
//! `TreeNode(id, label)` + `treeNodeAddChild(parent, child)` calls;
//! `TreeView(rootNode, onSelect)` then mounts the resulting topology in
//! an outline view.
//!
//! Selection fires `onSelect(id)` with the string id of the picked
//! node (NaN-boxed STRING). Expand/collapse is handled natively by the
//! outline view chevron and arrow keys.
//!
//! Out of scope this iteration: drag-and-drop, lazy loading, multi-
//! select, inline rename, icons. Filed back into #480 for follow-up.

use crate::ffi::js_string_from_bytes;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, AnyThread, DefinedClass};
use objc2_app_kit::NSView;
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_nanbox_string(ptr: i64) -> f64;
}

struct TreeNode {
    id: String,
    label: String,
    children: Vec<i64>,
}

struct TreeEntry {
    scroll_view: Retained<NSView>,
    outline_view: Retained<NSView>,
    handle: i64,
    root_node: i64,
    on_select: f64,
}

thread_local! {
    static NODES: RefCell<Vec<TreeNode>> = const { RefCell::new(Vec::new()) };
    static TREES: RefCell<Vec<TreeEntry>> = const { RefCell::new(Vec::new()) };
    static ITEMS: RefCell<HashMap<i64, Retained<PerryTreeItem>>> = RefCell::new(HashMap::new());
}

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const crate::string_header::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

// ===========================================================================
// PerryTreeItem — stable opaque NSObject identity for each tree node.
// NSOutlineView identifies rows by item pointer; we cache one per node id
// so `outlineView:child:ofItem:` always hands back the same instance.
// ===========================================================================

pub struct PerryTreeItemIvars {
    pub node_id: Cell<i64>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryTreeItem"]
    #[ivars = PerryTreeItemIvars]
    pub struct PerryTreeItem;
);

impl PerryTreeItem {
    fn new(node_id: i64) -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryTreeItemIvars {
            node_id: Cell::new(node_id),
        });
        unsafe { msg_send![super(this), init] }
    }
}

fn item_for_node(node_id: i64) -> Retained<PerryTreeItem> {
    ITEMS.with(|map| {
        let mut m = map.borrow_mut();
        if let Some(existing) = m.get(&node_id) {
            return existing.clone();
        }
        let new = PerryTreeItem::new(node_id);
        m.insert(node_id, new.clone());
        new
    })
}

fn node_id_from_item(item: *const AnyObject) -> i64 {
    if item.is_null() {
        return 0;
    }
    unsafe {
        let typed = item as *const PerryTreeItem;
        (*typed).ivars().node_id.get()
    }
}

// ===========================================================================
// Delegate / data source — single class implementing both protocols, like
// `widgets::table::PerryTableDelegate`.
// ===========================================================================

pub struct PerryTreeDelegateIvars {
    pub entry_idx: Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryTreeDelegate"]
    #[ivars = PerryTreeDelegateIvars]
    pub struct PerryTreeDelegate;

    impl PerryTreeDelegate {
        #[unsafe(method(outlineView:numberOfChildrenOfItem:))]
        fn number_of_children(
            &self,
            _outline: &AnyObject,
            item: *const AnyObject,
        ) -> i64 {
            let entry_idx = self.ivars().entry_idx.get();
            let target_node = if item.is_null() {
                TREES.with(|t| t.borrow().get(entry_idx).map(|e| e.root_node).unwrap_or(0))
            } else {
                node_id_from_item(item)
            };
            if target_node == 0 {
                return 0;
            }
            NODES.with(|n| {
                n.borrow()
                    .get((target_node - 1) as usize)
                    .map(|node| node.children.len() as i64)
                    .unwrap_or(0)
            })
        }

        #[unsafe(method(outlineView:child:ofItem:))]
        fn child(
            &self,
            _outline: &AnyObject,
            index: i64,
            item: *const AnyObject,
        ) -> *mut AnyObject {
            let entry_idx = self.ivars().entry_idx.get();
            let target_node = if item.is_null() {
                TREES.with(|t| t.borrow().get(entry_idx).map(|e| e.root_node).unwrap_or(0))
            } else {
                node_id_from_item(item)
            };
            if target_node == 0 {
                return std::ptr::null_mut();
            }
            let child_id = NODES.with(|n| {
                n.borrow()
                    .get((target_node - 1) as usize)
                    .and_then(|node| node.children.get(index as usize).copied())
            });
            let Some(child_id) = child_id else {
                return std::ptr::null_mut();
            };
            // Hand out the cached PerryTreeItem ptr — must outlive this
            // call. The cache holds a Retained, so the pointer is stable
            // for the lifetime of the tree.
            let item = item_for_node(child_id);
            Retained::as_ptr(&item) as *mut AnyObject
        }

        #[unsafe(method(outlineView:isItemExpandable:))]
        fn is_item_expandable(&self, _outline: &AnyObject, item: *const AnyObject) -> objc2::runtime::Bool {
            let target_node = node_id_from_item(item);
            if target_node == 0 {
                return objc2::runtime::Bool::NO;
            }
            let result = NODES.with(|n| {
                n.borrow()
                    .get((target_node - 1) as usize)
                    .map(|node| !node.children.is_empty())
                    .unwrap_or(false)
            });
            objc2::runtime::Bool::new(result)
        }

        #[unsafe(method(outlineView:objectValueForTableColumn:byItem:))]
        fn object_value_for_column(
            &self,
            _outline: &AnyObject,
            _column: &AnyObject,
            item: *const AnyObject,
        ) -> *mut AnyObject {
            let target_node = node_id_from_item(item);
            if target_node == 0 {
                return std::ptr::null_mut();
            }
            let label = NODES.with(|n| {
                n.borrow()
                    .get((target_node - 1) as usize)
                    .map(|node| node.label.clone())
            });
            let Some(label) = label else {
                return std::ptr::null_mut();
            };
            let ns: Retained<NSString> = NSString::from_str(&label);
            // Caller (NSOutlineView) takes ownership via objc autorelease
            // semantics; bump the retain so the Retained drop doesn't
            // free it under us.
            Retained::into_raw(ns) as *mut AnyObject
        }

        #[unsafe(method(outlineViewSelectionDidChange:))]
        fn selection_did_change(&self, note: &AnyObject) {
            let entry_idx = self.ivars().entry_idx.get();
            let on_select = TREES.with(|t| {
                t.borrow().get(entry_idx).map(|e| e.on_select).unwrap_or(0.0)
            });
            if on_select == 0.0 {
                return;
            }
            crate::catch_callback_panic("tree select", std::panic::AssertUnwindSafe(|| {
                unsafe {
                    let outline: *mut AnyObject = msg_send![note, object];
                    let row: i64 = msg_send![outline, selectedRow];
                    if row < 0 {
                        return;
                    }
                    let item: *mut AnyObject = msg_send![outline, itemAtRow: row];
                    let id_str = NODES.with(|n| {
                        let nid = node_id_from_item(item);
                        n.borrow()
                            .get((nid - 1) as usize)
                            .map(|node| node.id.clone())
                    });
                    let Some(id) = id_str else { return };
                    let bytes = id.as_bytes();
                    let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
                    let arg = js_nanbox_string(header as i64);
                    let closure = js_nanbox_get_pointer(on_select) as *const u8;
                    js_closure_call1(closure, arg);
                }
            }));
        }
    }
);

impl PerryTreeDelegate {
    fn new(entry_idx: usize) -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryTreeDelegateIvars {
            entry_idx: Cell::new(entry_idx),
        });
        unsafe { msg_send![super(this), init] }
    }
}

// ===========================================================================
// Public API
// ===========================================================================

/// Register a tree node with `id` and `label`. Returns 1-based handle.
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

/// Append `child` as the last child of `parent`.
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

/// Mount `root_node` in an `NSOutlineView`. Returns 1-based widget
/// handle for the wrapping scroll view (suitable for layout containers).
pub fn create(root_node: i64, on_select: f64) -> i64 {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let ov_cls = AnyClass::get(c"NSOutlineView").unwrap();
        let outline_obj: Retained<AnyObject> = msg_send![ov_cls, new];

        let tc_cls = AnyClass::get(c"NSTableColumn").unwrap();
        let col_obj: Retained<AnyObject> = msg_send![tc_cls, new];
        let id_str = NSString::from_str("perry_tree_main");
        let _: () = msg_send![&*col_obj, setIdentifier: &*id_str];
        let _: () = msg_send![&*col_obj, setWidth: 240.0_f64];
        let _: () = msg_send![&*outline_obj, addTableColumn: &*col_obj];
        let _: () = msg_send![&*outline_obj, setOutlineTableColumn: &*col_obj];
        // Hide the header — outline trees are usually rendered without one.
        let _: () = msg_send![&*outline_obj, setHeaderView: std::ptr::null::<AnyObject>()];

        let scroll_cls = AnyClass::get(c"NSScrollView").unwrap();
        let scroll_obj: Retained<AnyObject> = msg_send![scroll_cls, new];
        let _: () = msg_send![&*scroll_obj, setHasVerticalScroller: true];
        let _: () = msg_send![&*scroll_obj, setHasHorizontalScroller: false];
        let _: () = msg_send![&*scroll_obj, setDocumentView: &*outline_obj];

        let outline_view: Retained<NSView> = Retained::cast_unchecked(outline_obj);
        let scroll_view: Retained<NSView> = Retained::cast_unchecked(scroll_obj);
        let handle = super::register_widget(scroll_view.clone());

        let entry_idx = TREES.with(|t| t.borrow().len());
        let delegate = PerryTreeDelegate::new(entry_idx);

        let _: () = msg_send![&*outline_view, setDataSource: &*delegate];
        let _: () = msg_send![&*outline_view, setDelegate: &*delegate];

        // Selection change notification — NSOutlineView posts via the
        // NotificationCenter and the delegate method is called
        // automatically when set as `setDelegate:`. The objc method
        // `outlineViewSelectionDidChange:` matches the protocol's
        // optional method signature.
        let _ = Sel::register(c"outlineViewSelectionDidChange:");

        std::mem::forget(delegate);

        TREES.with(|t| {
            t.borrow_mut().push(TreeEntry {
                scroll_view,
                outline_view,
                handle,
                root_node,
                on_select,
            });
        });

        let outline_for_reload = TREES.with(|t| {
            t.borrow()
                .get(entry_idx)
                .map(|e| Retained::as_ptr(&e.outline_view) as usize)
                .unwrap_or(0)
        });
        if outline_for_reload != 0 {
            let _: () = msg_send![outline_for_reload as *const AnyObject, reloadData];
        }

        handle
    }
}

fn outline_ptr(handle: i64) -> usize {
    TREES.with(|t| {
        t.borrow()
            .iter()
            .find(|e| e.handle == handle)
            .map(|e| Retained::as_ptr(&e.outline_view) as usize)
            .unwrap_or(0)
    })
}

pub fn expand_all(handle: i64) {
    let p = outline_ptr(handle);
    if p == 0 {
        return;
    }
    unsafe {
        let _: () = msg_send![p as *const AnyObject, expandItem: std::ptr::null::<AnyObject>(), expandChildren: true];
    }
}

pub fn collapse_all(handle: i64) {
    let p = outline_ptr(handle);
    if p == 0 {
        return;
    }
    unsafe {
        let _: () = msg_send![p as *const AnyObject, collapseItem: std::ptr::null::<AnyObject>(), collapseChildren: true];
    }
}

/// Get the id string of the currently-selected tree node, NaN-boxed
/// STRING. Returns undefined sentinel when nothing is selected.
pub fn get_selected_id(handle: i64) -> f64 {
    let p = outline_ptr(handle);
    if p == 0 {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    }
    unsafe {
        let row: i64 = msg_send![p as *const AnyObject, selectedRow];
        if row < 0 {
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }
        let item: *mut AnyObject = msg_send![p as *const AnyObject, itemAtRow: row];
        let nid = node_id_from_item(item);
        let id_opt = NODES.with(|n| {
            n.borrow()
                .get((nid - 1) as usize)
                .map(|node| node.id.clone())
        });
        let Some(id) = id_opt else {
            return f64::from_bits(0x7FFC_0000_0000_0001);
        };
        let bytes = id.as_bytes();
        let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        js_nanbox_string(header as i64)
    }
}
