//! Tree view — Win32 `SysTreeView32` (TreeView control).
//!
//! Topology built TS-side via standalone `TreeNode(id, label)` +
//! `treeNodeAddChild`; `TreeView(root, onSelect)` recursively inserts
//! the topology via `TVM_INSERTITEMW`. Each node's `lParam` carries
//! its perry-side handle so `TVN_SELCHANGED` can resolve back to the
//! `id` string without keeping a parallel map.

use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{alloc_control_id, register_widget, WidgetKind};

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

struct TreeNodeData {
    id: String,
    label: String,
    children: Vec<i64>,
}

thread_local! {
    static NODES: RefCell<Vec<TreeNodeData>> = const { RefCell::new(Vec::new()) };
    static CALLBACKS: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
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

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// TreeView constants (from CommCtrl.h).
#[cfg(target_os = "windows")]
const TV_FIRST: u32 = 0x1100;
#[cfg(target_os = "windows")]
const TVM_INSERTITEMW: u32 = TV_FIRST + 50;
#[cfg(target_os = "windows")]
const TVM_DELETEITEM: u32 = TV_FIRST + 1;
#[cfg(target_os = "windows")]
const TVM_GETNEXTITEM: u32 = TV_FIRST + 10;
#[cfg(target_os = "windows")]
const TVM_GETITEMW: u32 = TV_FIRST + 62;
#[cfg(target_os = "windows")]
const TVM_EXPAND: u32 = TV_FIRST + 2;
#[cfg(target_os = "windows")]
const TVI_ROOT: isize = -0x10000;
#[cfg(target_os = "windows")]
const TVI_LAST: isize = -0xFFFE; // alternative: 0xFFFFFFFE_isize
#[cfg(target_os = "windows")]
const TVGN_CARET: u32 = 9;
#[cfg(target_os = "windows")]
const TVIF_TEXT: u32 = 0x0001;
#[cfg(target_os = "windows")]
const TVIF_PARAM: u32 = 0x0004;
#[cfg(target_os = "windows")]
const TVE_EXPAND: u32 = 2;
#[cfg(target_os = "windows")]
const TVE_COLLAPSE: u32 = 1;
#[cfg(target_os = "windows")]
const TVS_HASLINES: u32 = 0x0002;
#[cfg(target_os = "windows")]
const TVS_LINESATROOT: u32 = 0x0004;
#[cfg(target_os = "windows")]
const TVS_HASBUTTONS: u32 = 0x0001;
#[cfg(target_os = "windows")]
const TVS_SHOWSELALWAYS: u32 = 0x0020;

#[cfg(target_os = "windows")]
#[repr(C)]
struct TvItemW {
    mask: u32,
    h_item: isize,
    state: u32,
    state_mask: u32,
    psz_text: *mut u16,
    cch_text_max: i32,
    i_image: i32,
    i_selected_image: i32,
    c_children: i32,
    l_param: isize,
}

#[cfg(target_os = "windows")]
#[repr(C)]
struct TvInsertStructW {
    h_parent: isize,
    h_insert_after: isize,
    item: TvItemW,
}

pub fn node_create(id_ptr: *const u8, label_ptr: *const u8) -> i64 {
    let id = str_from_header(id_ptr).to_string();
    let label = str_from_header(label_ptr).to_string();
    NODES.with(|n| {
        let mut nodes = n.borrow_mut();
        nodes.push(TreeNodeData {
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
        if let Some(p) = nodes.get_mut((parent - 1) as usize) {
            p.children.push(child);
        }
    });
}

#[cfg(target_os = "windows")]
unsafe fn insert_node(hwnd: HWND, h_parent: isize, node_id: i64) -> isize {
    let snapshot = NODES.with(|n| {
        n.borrow()
            .get((node_id - 1) as usize)
            .map(|node| (node.label.clone(), node.children.clone()))
    });
    let Some((label, children)) = snapshot else {
        return 0;
    };
    let mut wide = to_wide(&label);
    let item = TvItemW {
        mask: TVIF_TEXT | TVIF_PARAM,
        h_item: 0,
        state: 0,
        state_mask: 0,
        psz_text: wide.as_mut_ptr(),
        cch_text_max: 0,
        i_image: 0,
        i_selected_image: 0,
        c_children: 0,
        l_param: node_id as isize,
    };
    let ins = TvInsertStructW {
        h_parent,
        h_insert_after: TVI_LAST,
        item,
    };
    let h: isize = SendMessageW(
        hwnd,
        TVM_INSERTITEMW,
        Some(WPARAM(0)),
        Some(LPARAM(&ins as *const _ as isize)),
    )
    .0;
    for child in children {
        insert_node(hwnd, h, child);
    }
    h
}

pub fn create(root_node: i64, on_select: f64) -> i64 {
    let control_id = alloc_control_id();

    #[cfg(target_os = "windows")]
    {
        let class_name = to_wide("SysTreeView32");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let style = WINDOW_STYLE(
                WS_CHILD.0
                    | WS_VISIBLE.0
                    | WS_TABSTOP.0
                    | TVS_HASLINES
                    | TVS_HASBUTTONS
                    | TVS_LINESATROOT
                    | TVS_SHOWSELALWAYS,
            );
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(std::ptr::null()),
                style,
                0,
                0,
                240,
                320,
                Some(super::get_parking_hwnd()),
                Some(HMENU(control_id as *mut _)),
                Some(HINSTANCE::from(hinstance)),
                None,
            );
            let Ok(hwnd) = hwnd else {
                return register_widget(
                    HWND(std::ptr::null_mut()),
                    WidgetKind::TreeView,
                    control_id,
                );
            };

            let handle = register_widget(hwnd, WidgetKind::TreeView, control_id);
            CALLBACKS.with(|m| {
                m.borrow_mut().insert(handle, on_select);
            });

            if root_node > 0 {
                insert_node(hwnd, TVI_ROOT, root_node);
            }
            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (root_node, on_select);
        register_widget(0, WidgetKind::TreeView, control_id)
    }
}

#[cfg(target_os = "windows")]
unsafe fn walk_and_apply_expand(hwnd: HWND, h_item: isize, code: u32) {
    if h_item == 0 {
        return;
    }
    SendMessageW(
        hwnd,
        TVM_EXPAND,
        Some(WPARAM(code as usize)),
        Some(LPARAM(h_item)),
    );
    // Walk children — TVGN_CHILD = 4.
    let child = SendMessageW(hwnd, TVM_GETNEXTITEM, Some(WPARAM(4)), Some(LPARAM(h_item))).0;
    walk_and_apply_expand(hwnd, child, code);
    // Walk siblings — TVGN_NEXT = 1.
    let sibling = SendMessageW(hwnd, TVM_GETNEXTITEM, Some(WPARAM(1)), Some(LPARAM(h_item))).0;
    walk_and_apply_expand(hwnd, sibling, code);
}

pub fn expand_all(handle: i64) {
    #[cfg(target_os = "windows")]
    {
        let Some(hwnd) = super::get_hwnd(handle) else {
            return;
        };
        unsafe {
            // TVGN_ROOT = 0
            let root = SendMessageW(hwnd, TVM_GETNEXTITEM, Some(WPARAM(0)), Some(LPARAM(0))).0;
            walk_and_apply_expand(hwnd, root, TVE_EXPAND);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}

pub fn collapse_all(handle: i64) {
    #[cfg(target_os = "windows")]
    {
        let Some(hwnd) = super::get_hwnd(handle) else {
            return;
        };
        unsafe {
            let root = SendMessageW(hwnd, TVM_GETNEXTITEM, Some(WPARAM(0)), Some(LPARAM(0))).0;
            walk_and_apply_expand(hwnd, root, TVE_COLLAPSE);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}

pub fn get_selected_id(handle: i64) -> f64 {
    let undefined = f64::from_bits(0x7FFC_0000_0000_0001);
    #[cfg(target_os = "windows")]
    {
        let Some(hwnd) = super::get_hwnd(handle) else {
            return undefined;
        };
        unsafe {
            let h_item = SendMessageW(
                hwnd,
                TVM_GETNEXTITEM,
                Some(WPARAM(TVGN_CARET as usize)),
                Some(LPARAM(0)),
            )
            .0;
            if h_item == 0 {
                return undefined;
            }
            let id_str = read_node_id_from_h_item(hwnd, h_item);
            let Some(id) = id_str else { return undefined };
            let bytes = id.as_bytes();
            let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
            js_nanbox_string(header as i64)
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
        undefined
    }
}

#[cfg(target_os = "windows")]
unsafe fn read_node_id_from_h_item(hwnd: HWND, h_item: isize) -> Option<String> {
    let mut item = TvItemW {
        mask: TVIF_PARAM,
        h_item,
        state: 0,
        state_mask: 0,
        psz_text: std::ptr::null_mut(),
        cch_text_max: 0,
        i_image: 0,
        i_selected_image: 0,
        c_children: 0,
        l_param: 0,
    };
    let ok = SendMessageW(
        hwnd,
        TVM_GETITEMW,
        Some(WPARAM(0)),
        Some(LPARAM(&mut item as *mut _ as isize)),
    )
    .0;
    if ok == 0 {
        return None;
    }
    let node_id = item.l_param as i64;
    NODES.with(|n| {
        n.borrow()
            .get((node_id - 1) as usize)
            .map(|node| node.id.clone())
    })
}

/// Called by the WM_NOTIFY router when TVN_SELCHANGEDW (-411) arrives.
#[cfg(target_os = "windows")]
pub fn handle_selection_change(handle: i64) {
    let on = CALLBACKS.with(|m| m.borrow().get(&handle).copied().unwrap_or(0.0));
    if on == 0.0 {
        return;
    }
    let Some(hwnd) = super::get_hwnd(handle) else {
        return;
    };
    unsafe {
        let h_item = SendMessageW(
            hwnd,
            TVM_GETNEXTITEM,
            Some(WPARAM(TVGN_CARET as usize)),
            Some(LPARAM(0)),
        )
        .0;
        if h_item == 0 {
            return;
        }
        let Some(id) = read_node_id_from_h_item(hwnd, h_item) else {
            return;
        };
        let bytes = id.as_bytes();
        let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
        let arg = js_nanbox_string(header as i64);
        let closure_ptr = js_nanbox_get_pointer(on) as *const u8;
        js_closure_call1(closure_ptr, arg);
    }
}

/// Suppress unused-import warning when the module is built for a
/// non-Windows target — the cfg-gated extern fns aren't reachable.
#[cfg(not(target_os = "windows"))]
pub fn _unused() {
    let _ = (
        js_closure_call1 as usize,
        js_nanbox_get_pointer as usize,
        js_string_from_bytes as usize,
        js_nanbox_string as usize,
    );
}
