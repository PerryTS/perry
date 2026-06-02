//! iOS Tree view widget (issue #480).
//!
//! UIKit has no `NSOutlineView`, so we approximate one with
//! `UITableView` over a depth-flattened tree. Tree topology mirrors
//! the macOS impl in `perry-ui-macos`: standalone
//! `TreeNode(id, label)` + `treeNodeAddChild(parent, child)` calls
//! build the tree; `TreeView(rootNode, onSelect)` mounts the result.
//!
//! Each visible row is rendered as a `UITableViewCell` whose
//! `imageView` (the leading slot) holds a chevron button toggling
//! expand/collapse for parent nodes, and whose `textLabel` shows the
//! node label. `indentationLevel` is set proportional to depth so the
//! built-in left padding handles the visual hierarchy.
//!
//! Selection (`tableView:didSelectRowAtIndexPath:`) fires `on_select`
//! with the node id as a NaN-boxed string.
//!
//! Out of scope: drag-and-drop, lazy loading, multi-select, inline
//! rename, icons. Filed back into #480 for follow-up.

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, AnyThread, DefinedClass};
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use objc2_ui_kit::UIView;
use std::cell::{Cell, RefCell};

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
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

struct TreeNode {
    id: String,
    label: String,
    children: Vec<i64>,
}

/// A row in the depth-flattened render list.
#[derive(Clone, Copy)]
struct FlatRow {
    node_id: i64,
    depth: i32,
}

struct TreeEntry {
    /// The widget handle of the UITableView itself.
    handle: i64,
    /// Root node id (1-based into NODES).
    root_node: i64,
    on_select: f64,
    /// Per-node expanded state — absent = collapsed.
    expanded: std::collections::HashSet<i64>,
    /// Flattened list cached for the data source.
    flat: Vec<FlatRow>,
    /// Last selected node id (0 = none).
    selected_node: i64,
    /// Strong reference to the delegate; UITableView holds it weakly.
    _delegate: Retained<PerryTreeDelegate>,
}

thread_local! {
    static NODES: RefCell<Vec<TreeNode>> = const { RefCell::new(Vec::new()) };
    static TREES: RefCell<Vec<TreeEntry>> = const { RefCell::new(Vec::new()) };
}

fn rebuild_flat(entry_idx: usize) {
    let (root, expanded_snapshot) = TREES.with(|t| {
        let trees = t.borrow();
        let Some(e) = trees.get(entry_idx) else {
            return (0, std::collections::HashSet::new());
        };
        (e.root_node, e.expanded.clone())
    });
    if root == 0 {
        return;
    }
    let mut out: Vec<FlatRow> = Vec::new();
    NODES.with(|n| {
        let nodes = n.borrow();
        fn walk(
            nodes: &[TreeNode],
            expanded: &std::collections::HashSet<i64>,
            node_id: i64,
            depth: i32,
            out: &mut Vec<FlatRow>,
        ) {
            let Some(node) = nodes.get((node_id - 1) as usize) else {
                return;
            };
            out.push(FlatRow { node_id, depth });
            if expanded.contains(&node_id) {
                for &child in &node.children {
                    walk(nodes, expanded, child, depth + 1, out);
                }
            }
        }
        walk(&nodes, &expanded_snapshot, root, 0, &mut out);
    });
    TREES.with(|t| {
        if let Some(e) = t.borrow_mut().get_mut(entry_idx) {
            e.flat = out;
        }
    });
}

fn reload_table(entry_idx: usize) {
    let handle = TREES.with(|t| t.borrow().get(entry_idx).map(|e| e.handle).unwrap_or(0));
    if handle == 0 {
        return;
    }
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let _: () = msg_send![&*view, reloadData];
        }
    }
}

fn has_children(node_id: i64) -> bool {
    NODES.with(|n| {
        n.borrow()
            .get((node_id - 1) as usize)
            .map(|node| !node.children.is_empty())
            .unwrap_or(false)
    })
}

fn toggle_expanded(entry_idx: usize, node_id: i64) {
    TREES.with(|t| {
        if let Some(e) = t.borrow_mut().get_mut(entry_idx) {
            if e.expanded.contains(&node_id) {
                e.expanded.remove(&node_id);
            } else {
                e.expanded.insert(node_id);
            }
        }
    });
    rebuild_flat(entry_idx);
    reload_table(entry_idx);
}

// ===========================================================================
// PerryTreeDelegate — UITableViewDataSource + UITableViewDelegate + the
// disclosure-button target action. One object plays all three roles.
// ===========================================================================

pub struct PerryTreeDelegateIvars {
    entry_idx: Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryTreeDelegateVisionOS"]
    #[ivars = PerryTreeDelegateIvars]
    pub struct PerryTreeDelegate;

    impl PerryTreeDelegate {
        // UITableViewDataSource: section count (single section).
        #[unsafe(method(numberOfSectionsInTableView:))]
        fn number_of_sections(&self, _tv: &AnyObject) -> i64 {
            1
        }

        // UITableViewDataSource: row count.
        #[unsafe(method(tableView:numberOfRowsInSection:))]
        fn number_of_rows(&self, _tv: &AnyObject, _section: i64) -> i64 {
            let idx = self.ivars().entry_idx.get();
            TREES.with(|t| t.borrow().get(idx).map(|e| e.flat.len() as i64).unwrap_or(0))
        }

        // UITableViewDataSource: cellForRow — build a fresh cell each
        // time (we're not aiming for scroll perf in v1; the tree is
        // typically small).
        #[unsafe(method(tableView:cellForRowAtIndexPath:))]
        fn cell_for_row(
            &self,
            tv: &AnyObject,
            index_path: &AnyObject,
        ) -> *mut AnyObject {
            let idx = self.ivars().entry_idx.get();
            let row: i64 = unsafe { msg_send![index_path, row] };
            let flat_row = TREES.with(|t| {
                t.borrow()
                    .get(idx)
                    .and_then(|e| e.flat.get(row as usize).copied())
            });
            let Some(flat_row) = flat_row else {
                return std::ptr::null_mut();
            };
            let (label, parent) = NODES.with(|n| {
                n.borrow()
                    .get((flat_row.node_id - 1) as usize)
                    .map(|node| (node.label.clone(), !node.children.is_empty()))
                    .unwrap_or((String::new(), false))
            });
            let is_expanded = TREES.with(|t| {
                t.borrow()
                    .get(idx)
                    .map(|e| e.expanded.contains(&flat_row.node_id))
                    .unwrap_or(false)
            });
            // Try dequeue, otherwise create. We use a constant reuse
            // identifier — UIKit auto-creates cells via the class
            // registration below.
            unsafe {
                let reuse_id = NSString::from_str("perry_tree_cell");
                let cell_cls = AnyClass::get(c"UITableViewCell").unwrap();
                let alloc: *mut AnyObject = msg_send![cell_cls, alloc];
                // UITableViewCellStyleDefault = 0
                let cell: *mut AnyObject = msg_send![
                    alloc,
                    initWithStyle: 0i64,
                    reuseIdentifier: &*reuse_id
                ];

                // Indentation per depth — UITableViewCell honours this
                // via its built-in `indentationLevel`/`indentationWidth`.
                let _: () = msg_send![cell, setIndentationLevel: flat_row.depth as i64];
                let _: () = msg_send![cell, setIndentationWidth: 20.0_f64];

                let text_label: *mut AnyObject = msg_send![cell, textLabel];
                if !text_label.is_null() {
                    let ns = NSString::from_str(&label);
                    let _: () = msg_send![text_label, setText: &*ns];
                }

                // Disclosure chevron for parents — `accessoryView` =
                // UIButton whose action toggles expand/collapse on the
                // node. Leaf rows get a transparent placeholder so the
                // label alignment stays consistent.
                if parent {
                    let btn_cls = AnyClass::get(c"UIButton").unwrap();
                    // UIButtonType.system = 1
                    let chevron: *mut AnyObject = msg_send![btn_cls, buttonWithType: 1i64];
                    let title = if is_expanded { "▾" } else { "▸" };
                    let ns_title = NSString::from_str(title);
                    // UIControlState.normal = 0
                    let _: () = msg_send![chevron, setTitle: &*ns_title, forState: 0i64];
                    let frame = objc2_core_foundation::CGRect::new(
                        objc2_core_foundation::CGPoint::new(0.0, 0.0),
                        objc2_core_foundation::CGSize::new(28.0, 28.0),
                    );
                    let _: () = msg_send![chevron, setFrame: frame];
                    // Tag = node id so the handler can recover it from
                    // the button without a per-cell delegate.
                    let _: () = msg_send![chevron, setTag: flat_row.node_id];
                    let sel = Sel::register(c"handleChevronTap:");
                    // UIControlEventTouchUpInside = 1 << 6 = 64
                    let _: () =
                        msg_send![chevron, addTarget: self, action: sel, forControlEvents: 64u64];
                    let _: () = msg_send![cell, setAccessoryView: chevron];
                } else {
                    let _: () = msg_send![cell, setAccessoryView: std::ptr::null::<AnyObject>()];
                }

                let _ = tv;
                cell
            }
        }

        // UITableViewDelegate: selection — fire on_select(node.id).
        #[unsafe(method(tableView:didSelectRowAtIndexPath:))]
        fn did_select_row(&self, _tv: &AnyObject, index_path: &AnyObject) {
            let idx = self.ivars().entry_idx.get();
            let row: i64 = unsafe { msg_send![index_path, row] };
            let node_id = TREES.with(|t| {
                t.borrow()
                    .get(idx)
                    .and_then(|e| e.flat.get(row as usize).map(|r| r.node_id))
                    .unwrap_or(0)
            });
            if node_id == 0 {
                return;
            }
            TREES.with(|t| {
                if let Some(e) = t.borrow_mut().get_mut(idx) {
                    e.selected_node = node_id;
                }
            });
            let (id, on_select) = NODES.with(|n| {
                let nodes = n.borrow();
                let id = nodes
                    .get((node_id - 1) as usize)
                    .map(|node| node.id.clone())
                    .unwrap_or_default();
                let on_select = TREES.with(|t| {
                    t.borrow().get(idx).map(|e| e.on_select).unwrap_or(0.0)
                });
                (id, on_select)
            });
            if on_select == 0.0 || id.is_empty() {
                return;
            }
            crate::catch_callback_panic(
                "tree select",
                std::panic::AssertUnwindSafe(|| unsafe {
                    let bytes = id.as_bytes();
                    let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
                    let arg = js_nanbox_string(header as i64);
                    let closure_ptr = js_nanbox_get_pointer(on_select) as *const u8;
                    js_closure_call1(closure_ptr, arg);
                }),
            );
        }

        // Disclosure-button target action — tag = node id.
        #[unsafe(method(handleChevronTap:))]
        fn handle_chevron(&self, sender: &AnyObject) {
            let node_id: i64 = unsafe { msg_send![sender, tag] };
            if node_id <= 0 {
                return;
            }
            toggle_expanded(self.ivars().entry_idx.get(), node_id);
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

/// Register a tree node with `id` and `label`. Returns the 1-based
/// node handle for use with `node_add_child` / `create`.
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

/// Mount `root_node` in a `UITableView`. Returns 1-based widget handle.
pub fn create(root_node: i64, on_select: f64) -> i64 {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let tv_cls = AnyClass::get(c"UITableView").unwrap();
        let tv_alloc: *mut AnyObject = msg_send![tv_cls, alloc];
        // UITableViewStylePlain = 0
        let frame = objc2_core_foundation::CGRect::new(
            objc2_core_foundation::CGPoint::new(0.0, 0.0),
            objc2_core_foundation::CGSize::new(240.0, 320.0),
        );
        let tv: *mut AnyObject = msg_send![tv_alloc, initWithFrame: frame, style: 0i64];
        let _: () = msg_send![tv, setTranslatesAutoresizingMaskIntoConstraints: false];

        let view: Retained<UIView> =
            Retained::retain(tv as *mut UIView).expect("UITableView retain");
        let handle = super::register_widget(view);

        // Allocate the entry first so the delegate can find its slot.
        let entry_idx = TREES.with(|t| t.borrow().len());
        let delegate = PerryTreeDelegate::new(entry_idx);
        let _: () = msg_send![tv, setDataSource: &*delegate];
        let _: () = msg_send![tv, setDelegate: &*delegate];

        // Root starts expanded — matches macOS NSOutlineView's
        // "root visible" semantics (you always see the root).
        let mut expanded: std::collections::HashSet<i64> = std::collections::HashSet::new();
        if has_children(root_node) {
            expanded.insert(root_node);
        }

        TREES.with(|t| {
            t.borrow_mut().push(TreeEntry {
                handle,
                root_node,
                on_select,
                expanded,
                flat: Vec::new(),
                selected_node: 0,
                _delegate: delegate,
            });
        });

        rebuild_flat(entry_idx);
        reload_table(entry_idx);

        handle
    }
}

fn entry_idx_for_handle(handle: i64) -> Option<usize> {
    TREES.with(|t| t.borrow().iter().position(|e| e.handle == handle))
}

/// Expand every parent node in the tree.
pub fn expand_all(handle: i64) {
    let Some(idx) = entry_idx_for_handle(handle) else {
        return;
    };
    let to_expand = NODES.with(|n| {
        n.borrow()
            .iter()
            .enumerate()
            .filter(|(_, node)| !node.children.is_empty())
            .map(|(i, _)| (i + 1) as i64)
            .collect::<Vec<i64>>()
    });
    TREES.with(|t| {
        if let Some(e) = t.borrow_mut().get_mut(idx) {
            for id in to_expand {
                e.expanded.insert(id);
            }
        }
    });
    rebuild_flat(idx);
    reload_table(idx);
}

/// Collapse every parent node in the tree. The root stays expanded so
/// at least the first level remains visible.
pub fn collapse_all(handle: i64) {
    let Some(idx) = entry_idx_for_handle(handle) else {
        return;
    };
    let root = TREES.with(|t| t.borrow().get(idx).map(|e| e.root_node).unwrap_or(0));
    TREES.with(|t| {
        if let Some(e) = t.borrow_mut().get_mut(idx) {
            e.expanded.clear();
            if root != 0 && has_children(root) {
                e.expanded.insert(root);
            }
        }
    });
    rebuild_flat(idx);
    reload_table(idx);
}

/// Get the id string of the currently-selected tree node, NaN-boxed
/// STRING. Returns undefined sentinel when nothing is selected.
pub fn get_selected_id(handle: i64) -> f64 {
    let Some(idx) = entry_idx_for_handle(handle) else {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    };
    let node_id = TREES.with(|t| t.borrow().get(idx).map(|e| e.selected_node).unwrap_or(0));
    if node_id == 0 {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    }
    let id = NODES.with(|n| {
        n.borrow()
            .get((node_id - 1) as usize)
            .map(|node| node.id.clone())
    });
    let Some(id) = id else {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    };
    unsafe {
        let bytes = id.as_bytes();
        let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
        js_nanbox_string(header as i64)
    }
}
